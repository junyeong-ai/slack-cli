use anyhow::Result;
use chrono::{Duration, Utc};

use super::envelope::{EVENT_SCHEMA, Event, EventKind, EventSource};
use super::state::EventState;
use crate::config::EventsConfig;
use crate::slack::{SlackClient, SlackMessage};

/// How many messages one channel's recovery asks for per request. `SlackCore`
/// clamps this to whatever the app's distribution allows — 15 for a
/// non-Marketplace app — so the number here is the ceiling, not the promise.
const PAGE_SIZE: usize = 200;

/// How many replies one subscribed thread's recovery reads past its cursor.
/// Paging starts at the cursor, so this bounds the *missed* tail rather than
/// the head the daemon has already delivered.
const THREAD_LIMIT: usize = 50;

/// How many pages one channel's recovery may read.
///
/// `conversations.history` returns the newest messages in the range first, so
/// a single page of a deep gap would leave the middle of it unread while the
/// cursor moved past — a hole that never closes. Paging fixes that; bounding
/// the paging is what keeps recovery from spending an hour of a rationed
/// endpoint on one busy channel. When the bound is reached the gap is reported
/// rather than quietly abandoned.
const MAX_PAGES: usize = 5;

/// Closes the gap a disconnect left behind.
///
/// Socket Mode replays nothing, so this is the only recovery there is — and it
/// runs on `conversations.history`, which a non-Marketplace app may call once
/// a minute. That is why it is bounded in three directions at once: only
/// channels a rule cares about, only the most recent `backfill_max_channels`
/// of them, and only back as far as `backfill_max_age_hours`. A full-workspace
/// catch-up is not a slow option here; it is an impossible one.
pub async fn recover(
    slack: &SlackClient,
    state: &EventState,
    config: &EventsConfig,
    team_id: Option<&str>,
) -> Result<Vec<Event>> {
    if !config.backfill {
        return Ok(Vec::new());
    }

    // The furthest back any read will reach. A cursor older than this is not
    // a channel to skip — it is a channel to read from here, with the stretch
    // in between reported rather than dropped in silence.
    let horizon = format!(
        "{}.000000",
        (Utc::now() - Duration::hours(config.backfill_max_age_hours)).timestamp()
    );
    let gaps = state.gaps(config.backfill_max_channels)?;
    let watched = state.watched_count()?;
    if gaps.is_empty() && watched == 0 {
        return Ok(Vec::new());
    }

    tracing::info!(
        channels = gaps.len(),
        threads = watched,
        "recovering what the connection missed"
    );

    let mut recovered = Vec::new();

    // Subscribed threads first. `conversations.history` returns a channel's
    // top-level messages and never a thread's replies, so without this pass
    // the one flow the emoji rule exists for is the one a disconnect loses.
    // Bounded by the same budget as the channels, on the same rationed
    // endpoints.
    for thread in state.watched_threads(config.backfill_max_channels)? {
        match slack
            .messages
            .replies(
                &thread.channel,
                &thread.thread_ts,
                THREAD_LIMIT,
                Some(&thread.cursor_ts),
            )
            .await
        {
            Ok(messages) => {
                for message in messages {
                    // Slack returns a thread's parent alongside its replies and
                    // treats `oldest` inclusively, so the cursor is enforced
                    // here too. Without it every reconnect would re-deliver at
                    // least the message the thread hangs off, and once the
                    // deduplication layers lapsed, the whole thread with it.
                    if message.ts.as_str() <= thread.cursor_ts.as_str() {
                        continue;
                    }
                    recovered.push(from_history(&thread.channel, team_id, message));
                }
            }
            Err(err) => {
                tracing::warn!(
                    channel = %thread.channel,
                    thread = %thread.thread_ts,
                    "could not recover the thread: {err:#}"
                );
            }
        }
    }

    for gap in gaps {
        let mut cursor: Option<String> = None;
        let mut pages = 0;

        // Read from wherever the channel was left, unless that is further back
        // than recovery reaches — then from the horizon, and say what was
        // skipped. A daemon down over a weekend still recovers the window it
        // is allowed to; only the older stretch is lost, and visibly.
        let clamped = gap.last_ts.as_str() < horizon.as_str();
        let oldest = if clamped {
            horizon.as_str()
        } else {
            gap.last_ts.as_str()
        };
        let mut complete = true;
        let mut found = 0usize;

        loop {
            let page = slack
                .messages
                .history(
                    &gap.channel,
                    PAGE_SIZE,
                    cursor.as_deref(),
                    Some(oldest),
                    None,
                )
                .await;

            let (messages, next) = match page {
                Ok(page) => page,
                // One unreadable channel — archived, left, or scope-restricted
                // — must not stop the others from being recovered.
                Err(err) => {
                    tracing::warn!(channel = %gap.channel, "could not recover: {err:#}");
                    complete = false;
                    break;
                }
            };

            for message in messages {
                found += 1;
                recovered.push(from_history(&gap.channel, team_id, message));
            }

            pages += 1;
            cursor = next;
            if cursor.is_none() {
                break;
            }
            if pages >= MAX_PAGES {
                tracing::warn!(
                    channel = %gap.channel,
                    pages,
                    "the gap is deeper than recovery will read in one pass; the oldest of it \
                     will not be seen"
                );
                complete = false;
                break;
            }
        }

        // A clamped read that finished has covered the horizon to now, so the
        // cursor belongs at the horizon rather than weeks behind it. This does
        // not stop a perpetually quiet channel from clamping again — the
        // horizon moves forward with the clock, so it is always ahead of where
        // the last pass left the cursor. What it buys is that the position
        // tracks reality to within one reconnect instead of drifting stale,
        // which is what the next clamp reports and what an operator reads.
        //
        // Safe to advance before the events are processed: the position moves
        // only to where reading *began*, never past what was found, so a crash
        // re-reads exactly the same window.
        // Reported once the read is done, and loudly only when the skipped
        // stretch could have held something. A channel that is simply quiet
        // clamps on every reconnect by construction, and warning each time
        // would name a window the previous pass had already covered — a
        // recurring, slightly untrue warning is worse than none.
        if clamped {
            let skipped = "the gap reaches further back than \
                           events.backfill_max_age_hours; the older part of it was not read";
            if found > 0 || !complete {
                tracing::warn!(
                    channel = %gap.channel,
                    followed_to = %gap.last_ts,
                    read_from = %horizon,
                    recovered = found,
                    "{skipped}"
                );
            } else {
                tracing::debug!(
                    channel = %gap.channel,
                    followed_to = %gap.last_ts,
                    read_from = %horizon,
                    "{skipped}, and the window held nothing"
                );
            }
        }

        if clamped && complete {
            state.advance_cursor(&gap.channel, oldest, true)?;
        }
    }

    // Oldest first. `conversations.history` answers newest first, and the
    // cursor advances as each event is processed — so replaying in Slack's
    // order would move the cursor to the newest message before the older ones
    // had been handled, and a crash or an overflow in between would put them
    // permanently behind it.
    recovered.sort_by(|left, right| left.ts.cmp(&right.ts));

    Ok(recovered)
}

/// A history message in the shape the pipeline already knows.
///
/// It deliberately carries no synthetic identity: `dedupe_key` is derived from
/// the channel and timestamp, so a message the socket already delivered is
/// recognised as the same one and dropped here.
fn from_history(channel: &str, team_id: Option<&str>, message: SlackMessage) -> Event {
    Event {
        schema: EVENT_SCHEMA.to_string(),
        id: format!("backfill:{channel}:{}", message.ts),
        seq: 0,
        kind: EventKind::Message,
        source: EventSource::Backfill,
        team_id: team_id.map(ToOwned::to_owned),
        channel: Some(channel.to_string()),
        channel_type: None,
        user: message.user,
        bot_id: message.bot_id,
        ts: Some(message.ts),
        // A recovered message has no delivery of its own to name — which is
        // exactly what lets it collapse onto the socket's copy.
        event_ts: None,
        thread_ts: message.thread_ts,
        subtype: message.subtype,
        text: Some(message.text).filter(|text| !text.is_empty()),
        reaction: None,
        item_user: None,
        received_at: Utc::now(),
        matched: Vec::new(),
        raw: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::envelope::from_event_callback;
    use serde_json::json;

    fn history_message(ts: &str) -> SlackMessage {
        serde_json::from_value(json!({
            "ts": ts,
            "user": "U01",
            "text": "recovered",
            "thread_ts": "1700000000.000100",
        }))
        .unwrap()
    }

    #[test]
    fn a_recovered_message_looks_like_the_event_it_stands_in_for() {
        let event = from_history("C01", Some("T01"), history_message("1700000005.000100"));

        assert_eq!(event.kind, EventKind::Message);
        assert_eq!(event.source, EventSource::Backfill);
        assert_eq!(event.channel.as_deref(), Some("C01"));
        assert_eq!(event.team_id.as_deref(), Some("T01"));
        assert_eq!(event.text.as_deref(), Some("recovered"));
        assert_eq!(event.thread_root(), Some("1700000000.000100"));
    }

    /// The property that makes recovery safe to run on every reconnect: a
    /// message the socket already delivered is recognised, not duplicated.
    #[test]
    fn recovery_collides_with_the_live_delivery_of_the_same_message() {
        let live = from_event_callback(
            &json!({
                "team_id": "T01",
                "event_id": "Ev01",
                "event": {
                    "type": "message", "channel": "C01", "user": "U01",
                    "text": "recovered", "ts": "1700000005.000100",
                    "thread_ts": "1700000000.000100",
                },
            }),
            false,
        )
        .unwrap();

        let recovered = from_history("C01", Some("T01"), history_message("1700000005.000100"));

        assert_eq!(live.dedupe_key(), recovered.dedupe_key());
        assert_ne!(live.source, recovered.source);
    }

    /// The ordering that keeps a hole from becoming permanent: the cursor
    /// advances per processed event, so the oldest has to go first.
    #[test]
    fn recovered_messages_are_replayed_oldest_first() {
        let mut recovered: Vec<Event> = [
            "1700000003.000100",
            "1700000001.000100",
            "1700000002.000100",
        ]
        .into_iter()
        .map(|ts| from_history("C01", None, history_message(ts)))
        .collect();
        recovered.sort_by(|left, right| left.ts.cmp(&right.ts));

        let order: Vec<_> = recovered.iter().filter_map(|e| e.ts.as_deref()).collect();
        assert_eq!(
            order,
            vec![
                "1700000001.000100",
                "1700000002.000100",
                "1700000003.000100"
            ]
        );
    }

    #[test]
    fn an_empty_history_body_is_absent_rather_than_blank() {
        let mut message = history_message("1700000005.000100");
        message.text = String::new();
        assert!(from_history("C01", None, message).text.is_none());
    }
}
