use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::Result;

use super::{
    Event, EventKind, EventQueue, EventState, EventStore, HANDLE_ATTEMPTS, HANDLE_BACKOFF,
    PipelineItem, RuleEngine, SinkSet, Tally,
};

/// The one consumer of the queue, and the only writer of state.
///
/// Being single means the read-then-commit around `seen` needs no lock, and
/// that ordering is what turns a crash into a redelivery instead of a hole.
pub(super) async fn process(
    queue: Arc<EventQueue<PipelineItem>>,
    state: Arc<EventState>,
    store: Arc<dyn EventStore>,
    engine: Arc<RuleEngine>,
    sinks: Arc<SinkSet>,
    tally: Arc<Tally>,
) -> Result<()> {
    while let Some(item) = queue.pop().await {
        let mut event = match item {
            PipelineItem::Event(event) => *event,
            PipelineItem::RecoveryComplete { generation, batch } => {
                state
                    .execute(move |state| {
                        state.finish_recovery(&generation, &batch.channels, &batch.threads)
                    })
                    .await?;
                continue;
            }
        };
        let mut pause = HANDLE_BACKOFF;
        for attempt in 1..=HANDLE_ATTEMPTS {
            match handle(&mut event, &state, &store, &engine, &sinks).await {
                // Counted here rather than inside, so a retry does not report
                // the same event twice.
                Ok(None) => break,
                Ok(Some(handled)) => {
                    commit_with_retry(&event, &state, handled.interesting).await?;
                    if handled.matched {
                        tally.matched.fetch_add(1, Ordering::Relaxed);
                    }
                    if handled.stored {
                        tally.stored.fetch_add(1, Ordering::Relaxed);
                    }
                    break;
                }
                // The event was acknowledged to Slack before it got here, so
                // nothing will send it again — and a reaction or an edit
                // cannot be read back from history at all. A busy database is
                // worth waiting out rather than losing it to.
                Err(err) if attempt < HANDLE_ATTEMPTS => {
                    tracing::warn!(
                        event = %event.id,
                        attempt,
                        "could not process an event, retrying: {err:#}"
                    );
                    tokio::time::sleep(pause).await;
                    pause *= 2;
                }
                Err(err) => {
                    tracing::error!(
                        event = %event.id,
                        attempts = HANDLE_ATTEMPTS,
                        "gave up on an event that Slack will not send again: {err:#}"
                    );
                    return Err(err);
                }
            }
        }
        tally.delivered.store(sinks.delivered(), Ordering::Relaxed);
        tally.failed.store(sinks.failed(), Ordering::Relaxed);
    }
    Ok(())
}

/// What one pass over an event did, for the counters that report it. Returned
/// rather than tallied inside, because the pass is retried and a retry must not
/// report the same event again.
#[derive(Debug, Default, Clone, Copy)]
struct Handled {
    matched: bool,
    stored: bool,
    interesting: bool,
}

async fn handle(
    event: &mut Event,
    state: &EventState,
    store: &Arc<dyn EventStore>,
    engine: &Arc<RuleEngine>,
    sinks: &SinkSet,
) -> Result<Option<Handled>> {
    let pending = event.clone();
    let engine = engine.clone();
    let store = store.clone();
    let prepared = state
        .execute(move |state| {
            let mut event = pending;
            if state.is_seen(&event)? {
                return Ok(None);
            }
            let outcome = engine.evaluate(&event, state)?;
            let mut handled = Handled {
                matched: outcome.matched(),
                stored: false,
                interesting: outcome.matched() || outcome.subscribed > 0,
            };
            if outcome.matched() {
                event.matched = outcome.rules.clone();
                if let Some(seq) = store.append(&event)? {
                    event.seq = seq;
                    handled.stored = true;
                }
            }
            Ok(Some((event, outcome, handled)))
        })
        .await?;
    let Some((prepared, outcome, handled)) = prepared else {
        return Ok(None);
    };
    *event = prepared;
    if outcome.matched() {
        sinks.deliver(event, &outcome.sinks).await;
    }

    Ok(Some(handled))
}

async fn commit_with_retry(event: &Event, state: &EventState, interesting: bool) -> Result<()> {
    // Delivery has completed; a failed commit must not replay the sinks.
    let mut pause = HANDLE_BACKOFF;
    for attempt in 1..=HANDLE_ATTEMPTS {
        let pending = event.clone();
        match state
            .execute(move |state| commit(&pending, state, interesting))
            .await
        {
            Ok(()) => return Ok(()),
            Err(err) if attempt < HANDLE_ATTEMPTS => {
                tracing::warn!(
                    event = %event.id,
                    attempt,
                    "could not record an event as handled, retrying: {err:#}"
                );
                tokio::time::sleep(pause).await;
                pause *= 2;
            }
            Err(err) => {
                tracing::error!(
                    event = %event.id,
                    "delivered an event but could not record it; it may be seen again: {err:#}"
                );
                return Err(err);
            }
        }
    }

    unreachable!("the final commit attempt returns")
}

/// Records that an event has been handled and how far its conversation has
/// been followed. The last thing to happen, and the only part of the pass that
/// is safe to repeat once the sinks have seen the event.
fn commit(event: &Event, state: &EventState, interesting: bool) -> Result<()> {
    state.mark_seen(event)?;

    if event.kind == EventKind::Message
        && let (Some(channel), Some(ts)) = (event.channel.as_deref(), event.ts.as_deref())
    {
        state.advance_cursor(channel, ts, interesting)?;
        // A subscribed thread keeps its own position, so recovery reads the
        // part of it nobody has seen rather than the whole conversation.
        if let Some(root) = event.thread_ts.as_deref() {
            state.advance_thread_cursor(channel, root, ts)?;
        }
    }

    Ok(())
}
