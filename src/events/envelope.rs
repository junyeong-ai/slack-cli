use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::EventKindConfig;

/// The contract between this daemon and whatever consumes its output.
///
/// Consumers key off it, so it is versioned and only ever grows: a new field
/// is additive and an old consumer ignores it. Removing or repurposing one is
/// a new major version, not an edit.
pub const EVENT_SCHEMA: &str = "slack-cli.event/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Message,
    ReactionAdded,
    ReactionRemoved,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::ReactionAdded => "reaction_added",
            Self::ReactionRemoved => "reaction_removed",
        }
    }

    /// The Slack event types this daemon understands. Anything else is
    /// acknowledged and dropped: an app may be subscribed to more than the
    /// rules can express, and an unknown type is not an error.
    fn from_slack(event_type: &str) -> Option<Self> {
        match event_type {
            "message" => Some(Self::Message),
            "reaction_added" => Some(Self::ReactionAdded),
            "reaction_removed" => Some(Self::ReactionRemoved),
            _ => None,
        }
    }

    pub fn matches_config(self, configured: EventKindConfig) -> bool {
        matches!(
            (self, configured),
            (Self::Message, EventKindConfig::Message)
                | (Self::ReactionAdded, EventKindConfig::ReactionAdded)
                | (Self::ReactionRemoved, EventKindConfig::ReactionRemoved)
        )
    }
}

/// How an event reached the daemon. A consumer that saw the live one and then
/// the recovered one needs to tell them apart, and the daemon itself reports
/// on how much recovery it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// Pushed over the Socket Mode connection.
    Socket,
    /// Read back from `conversations.history` to close a gap the connection
    /// left behind.
    Backfill,
}

impl EventSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Socket => "socket",
            Self::Backfill => "backfill",
        }
    }
}

/// One normalized Slack event, as it is handed to a rule, a store and a sink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub schema: String,
    /// Slack's own identifier for the delivery, kept for traceability. It is
    /// *not* the deduplication key — see `dedupe_key`.
    pub id: String,
    /// Monotonic position in this installation's log. Assigned on ingest, so
    /// it orders events as the daemon saw them, not as Slack timestamped them.
    pub seq: i64,
    pub kind: EventKind,
    pub source: EventSource,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    /// When Slack emitted the event, as opposed to when the message it refers
    /// to was posted. The two differ for an edit, a deletion and a reaction —
    /// and where they differ, this is the only thing telling two deliveries
    /// about the same message apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// `reaction_added` / `reaction_removed`: the emoji name, no colons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
    /// The author of the message a reaction was placed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_user: Option<String>,

    pub received_at: DateTime<Utc>,

    /// Names of the rules that matched. Empty until the rule engine has run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched: Vec<String>,

    /// The Slack payload as delivered, present only while the raw buffer is
    /// enabled. Rule authoring needs samples; steady-state operation does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

impl crate::events::queue::Evictable for Event {
    /// A live event may be discarded under load: Slack still has it, the drop
    /// is counted, and the alternative is making the acknowledgement path
    /// wait. A recovered one may not — it exists because Slack will not send
    /// it again, and the events queued behind it will move its channel's
    /// cursor past the gap it would leave.
    fn may_evict(&self) -> bool {
        matches!(self.source, EventSource::Socket)
    }
}

impl Event {
    /// Subtypes whose `ts` names a *different* message than the event itself.
    ///
    /// For these, channel and timestamp identify the message that was changed,
    /// not the change — so two edits of one message would share a key and the
    /// second would be discarded as a duplicate.
    const RESTATES_ANOTHER_MESSAGE: [&'static str; 2] = ["message_changed", "message_deleted"];

    /// Identity by content rather than by delivery.
    ///
    /// The same message can arrive twice as two different deliveries: Slack
    /// redelivers an unacknowledged envelope, and a backfill re-reads what the
    /// socket already brought. Keying on channel and timestamp collapses both,
    /// which keying on `event_id` would not.
    ///
    /// Where a delivery is *not* about a message's own existence — an edit, a
    /// deletion, a reaction — `event_ts` joins the key. Those shapes are the
    /// ones `conversations.history` can never return, so nothing that recovery
    /// produces stops collapsing, while two edits of one message and a
    /// re-added reaction stay distinct events instead of being swallowed as
    /// repeats of the first.
    pub fn dedupe_key(&self) -> String {
        let channel = self.channel.as_deref().unwrap_or("-");
        let ts = self.ts.as_deref().unwrap_or("-");
        let occurred = self.event_ts.as_deref().unwrap_or("-");

        match self.kind {
            EventKind::Message => {
                let subtype = self.subtype.as_deref().unwrap_or("");
                if Self::RESTATES_ANOTHER_MESSAGE.contains(&subtype) {
                    format!("message:{channel}:{ts}:{subtype}:{occurred}")
                } else {
                    format!("message:{channel}:{ts}:{subtype}")
                }
            }
            EventKind::ReactionAdded | EventKind::ReactionRemoved => format!(
                "{}:{}:{}:{channel}:{ts}:{occurred}",
                self.kind.as_str(),
                self.user.as_deref().unwrap_or("-"),
                self.reaction.as_deref().unwrap_or("-"),
            ),
        }
    }

    /// The thread this event belongs to, which for a top-level message is the
    /// message itself. What a thread subscription is keyed on.
    pub fn thread_root(&self) -> Option<&str> {
        self.thread_ts.as_deref().or(self.ts.as_deref())
    }

    /// Strips everything that is a copy of what was said, leaving the
    /// reference to it. What `store_body = false` persists.
    pub fn without_body(&self) -> Self {
        Self {
            text: None,
            raw: None,
            ..self.clone()
        }
    }

    pub fn to_ndjson(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// A Socket Mode envelope, before the payload inside it has been understood.
#[derive(Debug, Clone)]
pub struct SocketEnvelope {
    pub envelope_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub retry_attempt: u32,
}

impl SocketEnvelope {
    pub fn parse(raw: &Value) -> Self {
        Self {
            envelope_id: raw
                .get("envelope_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            kind: raw
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            payload: raw.get("payload").cloned().unwrap_or(Value::Null),
            retry_attempt: raw
                .get("retry_attempt")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
        }
    }
}

/// Turns the `event` object of an Events API callback into an `Event`.
///
/// `seq` is assigned later, by whichever store hands out positions, so it
/// starts at zero here: normalization knows the shape of an event, not its
/// place in the log.
pub fn from_event_callback(payload: &Value, keep_raw: bool) -> Option<Event> {
    let event = payload.get("event")?;
    let kind = EventKind::from_slack(event.get("type").and_then(Value::as_str)?)?;

    let id = payload
        .get("event_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let team_id = payload
        .get("team_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let mut normalized = Event {
        schema: EVENT_SCHEMA.to_string(),
        id,
        seq: 0,
        kind,
        source: EventSource::Socket,
        team_id,
        channel: None,
        channel_type: str_field(event, "channel_type"),
        user: str_field(event, "user"),
        bot_id: str_field(event, "bot_id"),
        ts: None,
        // Read from the raw event, before the message branch below replaces
        // `ts` with the timestamp of whatever message the event is about.
        event_ts: str_field(event, "event_ts").or_else(|| str_field(event, "ts")),
        thread_ts: None,
        subtype: str_field(event, "subtype"),
        text: None,
        reaction: str_field(event, "reaction"),
        item_user: str_field(event, "item_user"),
        received_at: Utc::now(),
        matched: Vec::new(),
        raw: keep_raw.then(|| event.clone()),
    };

    match kind {
        EventKind::Message => {
            normalized.channel = str_field(event, "channel");
            // An edit carries the edited message in a nested object: the
            // top-level `ts` is when the edit happened, and the text and
            // author of the message itself are only in there.
            let body = match normalized.subtype.as_deref() {
                Some("message_changed") => event.get("message").unwrap_or(event),
                Some("message_deleted") => event.get("previous_message").unwrap_or(event),
                _ => event,
            };
            // A deletion names its victim in `deleted_ts`; the top-level `ts`
            // is when the deletion happened. `previous_message` is not always
            // there, and reading the top level instead would file the event
            // under a message that does not exist.
            normalized.ts = match normalized.subtype.as_deref() {
                Some("message_deleted") => str_field(event, "deleted_ts")
                    .or_else(|| str_field(body, "ts"))
                    .or_else(|| str_field(event, "ts")),
                _ => str_field(body, "ts").or_else(|| str_field(event, "ts")),
            };
            normalized.thread_ts = str_field(body, "thread_ts");
            normalized.text = str_field(body, "text");
            normalized.user = normalized.user.or_else(|| str_field(body, "user"));
            normalized.bot_id = normalized.bot_id.or_else(|| str_field(body, "bot_id"));
        }
        EventKind::ReactionAdded | EventKind::ReactionRemoved => {
            let item = event.get("item");
            normalized.channel = item.and_then(|i| str_field(i, "channel"));
            normalized.ts = item.and_then(|i| str_field(i, "ts"));
        }
    }

    Some(normalized)
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn callback(event: Value) -> Value {
        json!({ "team_id": "T01", "event_id": "Ev01", "event": event })
    }

    #[test]
    fn a_channel_message_normalizes_to_its_identifying_fields() {
        let event = from_event_callback(
            &callback(json!({
                "type": "message",
                "channel": "C01",
                "channel_type": "channel",
                "user": "U01",
                "text": "hey <@U99>",
                "ts": "1700000000.000100",
                "thread_ts": "1699999999.000100",
            })),
            false,
        )
        .unwrap();

        assert_eq!(event.kind, EventKind::Message);
        assert_eq!(event.channel.as_deref(), Some("C01"));
        assert_eq!(event.user.as_deref(), Some("U01"));
        assert_eq!(event.text.as_deref(), Some("hey <@U99>"));
        assert_eq!(event.thread_root(), Some("1699999999.000100"));
        assert!(event.raw.is_none());
    }

    /// The text and author of an edited message live only in the nested
    /// object; reading the top level would produce an event with no author
    /// and no text, which every rule would then decline to match.
    #[test]
    fn an_edit_is_read_from_the_nested_message() {
        let event = from_event_callback(
            &callback(json!({
                "type": "message",
                "subtype": "message_changed",
                "channel": "C01",
                "ts": "1700000009.000000",
                "message": {
                    "type": "message",
                    "user": "U01",
                    "text": "corrected <@U99>",
                    "ts": "1700000000.000100",
                    "thread_ts": "1700000000.000100",
                },
            })),
            false,
        )
        .unwrap();

        assert_eq!(event.user.as_deref(), Some("U01"));
        assert_eq!(event.text.as_deref(), Some("corrected <@U99>"));
        assert_eq!(event.ts.as_deref(), Some("1700000000.000100"));
    }

    /// Slack's ordinary deletion payload carries no `previous_message`; the
    /// message that went away is named by `deleted_ts` alone, and the
    /// top-level `ts` is when the deletion happened.
    #[test]
    fn a_deletion_names_the_message_it_deleted() {
        let event = from_event_callback(
            &callback(json!({
                "type": "message",
                "subtype": "message_deleted",
                "channel": "C01",
                "ts": "1700000009.000000",
                "deleted_ts": "1700000000.000100",
                "event_ts": "1700000009.000000",
            })),
            false,
        )
        .unwrap();

        assert_eq!(event.ts.as_deref(), Some("1700000000.000100"));
        assert_eq!(event.event_ts.as_deref(), Some("1700000009.000000"));
    }

    #[test]
    fn a_reaction_takes_its_channel_and_ts_from_the_item() {
        let event = from_event_callback(
            &callback(json!({
                "type": "reaction_added",
                "user": "U01",
                "reaction": "eyes",
                "item": { "type": "message", "channel": "C01", "ts": "1700000000.000100" },
                "item_user": "U02",
            })),
            false,
        )
        .unwrap();

        assert_eq!(event.kind, EventKind::ReactionAdded);
        assert_eq!(event.channel.as_deref(), Some("C01"));
        assert_eq!(event.ts.as_deref(), Some("1700000000.000100"));
        assert_eq!(event.reaction.as_deref(), Some("eyes"));
        assert_eq!(event.item_user.as_deref(), Some("U02"));
    }

    #[test]
    fn an_unknown_event_type_is_not_an_event() {
        assert!(from_event_callback(&callback(json!({ "type": "team_join" })), false).is_none());
        assert!(from_event_callback(&json!({ "no": "event" }), false).is_none());
    }

    /// Redelivery and backfill both re-present the same message under a
    /// different delivery id, so identity has to come from the content.
    #[test]
    fn the_same_message_from_two_deliveries_shares_one_dedupe_key() {
        let live = from_event_callback(
            &callback(json!({
                "type": "message", "channel": "C01", "user": "U01",
                "text": "hi", "ts": "1700000000.000100",
            })),
            false,
        )
        .unwrap();

        let mut recovered = live.clone();
        recovered.id = "backfill-1".to_string();
        recovered.source = EventSource::Backfill;

        assert_ne!(live.id, recovered.id);
        assert_eq!(live.dedupe_key(), recovered.dedupe_key());
    }

    /// Two corrections to one message are two things that happened. Keying on
    /// the edited message alone would let the first through and silently drop
    /// the second for as long as the deduplication window lasts.
    #[test]
    fn two_edits_of_one_message_are_two_deliveries() {
        let edit = |event_ts: &str, text: &str| {
            from_event_callback(
                &callback(json!({
                    "type": "message", "subtype": "message_changed", "channel": "C01",
                    "ts": event_ts, "event_ts": event_ts,
                    "message": { "user": "U01", "text": text, "ts": "1700000000.000100" },
                })),
                false,
            )
            .unwrap()
        };

        let first = edit("1700000009.000000", "corrected once");
        let second = edit("1700000019.000000", "corrected twice");

        assert_eq!(first.ts, second.ts, "both name the same message");
        assert_ne!(first.dedupe_key(), second.dedupe_key());
    }

    /// Removing a reaction and putting it back is the subscribe toggle. If the
    /// second add collapsed onto the first, the thread could never be
    /// resubscribed within the deduplication window.
    #[test]
    fn re_adding_a_reaction_is_a_new_delivery() {
        let reacted = |event_ts: &str| {
            from_event_callback(
                &callback(json!({
                    "type": "reaction_added",
                    "user": "U01",
                    "reaction": "eyes",
                    "item": { "type": "message", "channel": "C01", "ts": "1700000000.000100" },
                    "event_ts": event_ts,
                })),
                false,
            )
            .unwrap()
        };

        assert_ne!(
            reacted("1700000100.000000").dedupe_key(),
            reacted("1700000200.000000").dedupe_key()
        );
    }

    /// Slack's own redelivery of an unacknowledged envelope repeats the event
    /// verbatim, `event_ts` included, so it still collapses.
    #[test]
    fn a_redelivered_reaction_still_collapses() {
        let payload = json!({
            "type": "reaction_added",
            "user": "U01",
            "reaction": "eyes",
            "item": { "type": "message", "channel": "C01", "ts": "1700000000.000100" },
            "event_ts": "1700000100.000000",
        });
        let first = from_event_callback(&callback(payload.clone()), false).unwrap();
        let redelivered = from_event_callback(&callback(payload), false).unwrap();

        assert_eq!(first.dedupe_key(), redelivered.dedupe_key());
    }

    #[test]
    fn an_edit_does_not_collide_with_the_message_it_edits() {
        let original = from_event_callback(
            &callback(json!({
                "type": "message", "channel": "C01", "user": "U01",
                "text": "a", "ts": "1700000000.000100",
            })),
            false,
        )
        .unwrap();
        let edited = from_event_callback(
            &callback(json!({
                "type": "message", "subtype": "message_changed", "channel": "C01",
                "ts": "1700000009.000000",
                "message": { "user": "U01", "text": "b", "ts": "1700000000.000100" },
            })),
            false,
        )
        .unwrap();

        assert_ne!(original.dedupe_key(), edited.dedupe_key());
    }

    #[test]
    fn dropping_the_body_keeps_the_reference() {
        let event = from_event_callback(
            &callback(json!({
                "type": "message", "channel": "C01", "user": "U01",
                "text": "secret", "ts": "1700000000.000100",
            })),
            true,
        )
        .unwrap();

        let reference = event.without_body();
        assert!(reference.text.is_none());
        assert!(reference.raw.is_none());
        assert_eq!(reference.channel.as_deref(), Some("C01"));
        assert_eq!(reference.ts, event.ts);
        assert_eq!(reference.dedupe_key(), event.dedupe_key());
    }

    #[test]
    fn an_envelope_parses_its_delivery_metadata() {
        let envelope = SocketEnvelope::parse(&json!({
            "envelope_id": "env-1",
            "type": "events_api",
            "retry_attempt": 2,
            "payload": { "event": { "type": "message" } },
        }));

        assert_eq!(envelope.envelope_id.as_deref(), Some("env-1"));
        assert_eq!(envelope.kind, "events_api");
        assert_eq!(envelope.retry_attempt, 2);
        assert!(envelope.payload.get("event").is_some());
    }
}
