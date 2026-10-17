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
                let appended = store.append(&event)?;
                event.id = appended.id;
                event.seq = appended.seq;
                handled.stored = appended.inserted;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EventsConfig, RuleConfig, SinkKind, default_sink};
    use crate::events::{EventSource, SqliteStore, StdoutFormat};
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn replay_uses_the_retained_identity_without_replacing_current_event_content() {
        let dir = tempfile::tempdir().unwrap();
        let state = EventState::open(&dir.path().join("state.db")).unwrap();
        let store: Arc<dyn EventStore> = Arc::new(
            SqliteStore::open(
                &dir.path().join("events.db"),
                &EventsConfig {
                    store_body: false,
                    ..EventsConfig::default()
                },
            )
            .unwrap(),
        );
        let original = super::super::envelope::from_event_callback(
            &json!({
                "event_id":"Ev-original", "event":{
                    "type":"message", "channel":"C01", "user":"U_OTHER",
                    "ts":"1700000000.000100", "text":"current body"
                }
            }),
            false,
        )
        .unwrap();
        let retained = store.append(&original).unwrap();
        let mut recovered = Event {
            id: "backfill:C01:1700000000.000100".into(),
            source: EventSource::Backfill,
            ..original
        };
        let server = MockServer::start().await;
        Mock::given(path("/sink"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let sinks = SinkSet::build(
            &[crate::config::SinkConfig {
                kind: SinkKind::Http,
                url: Some(format!("{}/sink", server.uri())),
                ..default_sink("consumer")
            }],
            StdoutFormat::Ndjson,
        )
        .unwrap();
        let rule: RuleConfig =
            serde_json::from_value(json!({"name":"channel", "channels":["C01"]})).unwrap();
        let engine = Arc::new(RuleEngine::new(
            vec![rule],
            sinks.names(),
            Some("U_ME".into()),
        ));
        let handled = handle(&mut recovered, &state, &store, &engine, &sinks)
            .await
            .unwrap()
            .unwrap();
        assert!(handled.matched);
        assert!(!handled.stored);
        let requests = server.received_requests().await.unwrap();
        let delivered: Event = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(delivered.id, retained.id);
        assert_eq!(delivered.seq, retained.seq);
        assert_eq!(delivered.source, EventSource::Backfill);
        assert_eq!(delivered.text.as_deref(), Some("current body"));
        assert_eq!(delivered.matched, vec!["channel"]);
        assert_eq!(store.stats().unwrap().events, 1);
        commit(&recovered, &state, handled.interesting).unwrap();
        assert!(state.is_seen(&recovered).unwrap());
        assert!(
            handle(&mut recovered, &state, &store, &engine, &sinks)
                .await
                .unwrap()
                .is_none()
        );
    }
}
