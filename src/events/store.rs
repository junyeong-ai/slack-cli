use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::db;
use super::envelope::Event;
use crate::config::{EventRetention, EventsConfig};

const SCHEMA_VERSION: i64 = 1;

/// How many events one over-budget pass deletes at a time, and how many such
/// passes it may make. Together they bound the work a single prune does.
const PRUNE_BATCH: usize = 500;
const MAX_PRUNE_BATCHES: usize = 64;

const STEPS: &[(i64, &str)] = &[(
    1,
    "CREATE TABLE event (
        seq         INTEGER PRIMARY KEY AUTOINCREMENT,
        dedupe_key  TEXT NOT NULL UNIQUE,
        id          TEXT NOT NULL,
        kind        TEXT NOT NULL,
        channel     TEXT,
        ts          TEXT,
        thread_ts   TEXT,
        author      TEXT,
        received_at INTEGER NOT NULL,
        payload     TEXT NOT NULL
     );
     CREATE INDEX idx_event_received ON event(received_at);
     CREATE INDEX idx_event_channel ON event(channel, ts);

     CREATE TABLE consumer (
        name       TEXT PRIMARY KEY,
        acked_seq  INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
     );",
)];

/// What a store can answer, which is what the commands built on it may assume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StoreCaps {
    /// Whether an event outlives the process that received it.
    pub durable: bool,
    /// Whether a consumer can ask for events it has not acknowledged.
    pub replayable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StoreStats {
    pub events: u64,
    pub bytes: u64,
    pub oldest: Option<i64>,
    pub newest: Option<i64>,
    pub consumers: Vec<ConsumerLag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsumerLag {
    pub name: String,
    pub acked_seq: i64,
    pub pending: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PruneOutcome {
    pub acknowledged: usize,
    pub expired: usize,
    pub over_budget: usize,
}

impl PruneOutcome {
    pub fn total(&self) -> usize {
        self.acknowledged + self.expired + self.over_budget
    }
}

/// The event log, in the two forms retention can take.
///
/// Everything upstream — the socket, the rule engine, the sinks — is written
/// against this and never against a mode. `events.mode = "stream"` is not a
/// second code path; it is `NullStore`.
pub trait EventStore: Send + Sync {
    fn caps(&self) -> StoreCaps;

    /// Assigns the event its position in the log, returning `None` when this
    /// delivery has already been recorded.
    fn append(&self, event: &Event) -> Result<Option<i64>>;

    /// Events after `from`, or after what `consumer` has acknowledged when
    /// `from` is `None`.
    ///
    /// A reader that is not acknowledging still has to move forward, or it
    /// would re-read the same batch for as long as it kept asking. `from` is
    /// how it carries that position itself without claiming to have handled
    /// anything.
    fn pull(&self, consumer: &str, from: Option<i64>, limit: usize) -> Result<Vec<Event>>;

    /// Moves a consumer's position forward. Refuses a position past anything
    /// the log has ever issued, which is a typo rather than an intention —
    /// and one that would silently swallow every event until the sequence
    /// caught up.
    fn ack(&self, consumer: &str, through: i64) -> Result<usize>;

    fn prune(&self) -> Result<PruneOutcome>;

    fn stats(&self) -> Result<StoreStats>;
}

/// Refused rather than answered wrongly: a command that needs to read events
/// back has no meaning against a store that keeps none.
fn unsupported(action: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "events.mode = \"stream\" keeps no event log, so {action} has nothing to read. \
         Set events.mode to \"spool\" (kept until acknowledged) or \"archive\" \
         (kept for events.retention_days) in config.toml"
    )
}

/// The store for `events.mode = "stream"`: positions are handed out, nothing
/// is written down.
///
/// The sequence restarts from zero with the process, which is honest for a
/// mode whose whole contract is that an event not delivered now is gone.
#[derive(Debug, Default)]
pub struct NullStore {
    next: AtomicI64,
}

impl NullStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventStore for NullStore {
    fn caps(&self) -> StoreCaps {
        StoreCaps {
            durable: false,
            replayable: false,
        }
    }

    fn append(&self, _event: &Event) -> Result<Option<i64>> {
        Ok(Some(self.next.fetch_add(1, Ordering::Relaxed) + 1))
    }

    fn pull(&self, _consumer: &str, _from: Option<i64>, _limit: usize) -> Result<Vec<Event>> {
        Err(unsupported("`events pull`"))
    }

    fn ack(&self, _consumer: &str, _through: i64) -> Result<usize> {
        Err(unsupported("`events ack`"))
    }

    fn prune(&self) -> Result<PruneOutcome> {
        Ok(PruneOutcome::default())
    }

    fn stats(&self) -> Result<StoreStats> {
        Ok(StoreStats::default())
    }
}

/// The durable event log. Content lives here and nowhere else, which is why
/// retention is enforced here and nowhere else.
pub struct SqliteStore {
    pool: Pool<SqliteConnectionManager>,
    mode: EventRetention,
    store_body: bool,
    retention_days: u64,
    max_bytes: u64,
}

impl SqliteStore {
    pub fn open(path: &Path, config: &EventsConfig) -> Result<Self> {
        let pool = db::open_pool(path)?;
        let mut conn = pool.get()?;
        db::migrate(&mut conn, "event log", SCHEMA_VERSION, STEPS)?;
        drop(conn);

        Ok(Self {
            pool,
            mode: config.mode,
            store_body: config.store_body,
            retention_days: config.retention_days,
            max_bytes: config.max_bytes,
        })
    }

    /// What the log actually occupies, with reclaimable pages discounted.
    ///
    /// `page_count` alone would keep counting pages a delete has already
    /// freed: SQLite returns them to a freelist and only hands them back to
    /// the filesystem when it vacuums. Measuring that way, a prune would see
    /// no progress after each batch and keep deleting until the log was
    /// empty — enforcing the size ceiling by discarding everything.
    fn byte_size(conn: &rusqlite::Connection) -> Result<u64> {
        let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let free: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
        let size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok(((pages - free).max(0) as u64) * (size.max(0) as u64))
    }
}

impl EventStore for SqliteStore {
    fn caps(&self) -> StoreCaps {
        StoreCaps {
            durable: true,
            replayable: true,
        }
    }

    fn append(&self, event: &Event) -> Result<Option<i64>> {
        // What is written is decided here, once: `store_body = false` means
        // the log holds a pointer to the message rather than a copy of it.
        let persisted = if self.store_body {
            event.clone()
        } else {
            event.without_body()
        };
        let payload = serde_json::to_string(&persisted).context("could not encode the event")?;

        let conn = self.pool.get()?;
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO event
                (dedupe_key, id, kind, channel, ts, thread_ts, author, received_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.dedupe_key(),
                event.id,
                event.kind.as_str(),
                event.channel,
                event.ts,
                event.thread_ts,
                event.user,
                event.received_at.timestamp(),
                payload,
            ],
        )?;

        if inserted == 0 {
            return Ok(None);
        }
        Ok(Some(conn.last_insert_rowid()))
    }

    fn pull(&self, consumer: &str, from: Option<i64>, limit: usize) -> Result<Vec<Event>> {
        let conn = self.pool.get()?;
        let after = match from {
            Some(seq) => seq,
            None => conn
                .query_row(
                    "SELECT acked_seq FROM consumer WHERE name = ?1",
                    params![consumer],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0),
        };

        let mut stmt =
            conn.prepare("SELECT seq, payload FROM event WHERE seq > ?1 ORDER BY seq LIMIT ?2")?;
        let events = stmt
            .query_map(params![after, limit as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(seq, payload)| {
                serde_json::from_str::<Event>(&payload)
                    .map(|mut event| {
                        // The row's position is authoritative: it is what an
                        // acknowledgement names, and a payload written before
                        // a crash may carry a stale one.
                        event.seq = seq;
                        event
                    })
                    .context("a stored event could not be decoded")
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(events)
    }

    fn ack(&self, consumer: &str, through: i64) -> Result<usize> {
        let conn = self.pool.get()?;

        // The high-water mark, which `sqlite_sequence` keeps even after the
        // rows themselves are pruned. Acknowledging past it would park the
        // consumer ahead of the log: every event until the sequence caught up
        // would be skipped without a word.
        let issued: i64 = conn
            .query_row(
                "SELECT IFNULL((SELECT seq FROM sqlite_sequence WHERE name = 'event'), 0)",
                [],
                |row| row.get(0),
            )
            .context("could not read how far the event log has been issued")?;
        if through > issued {
            anyhow::bail!(
                "cannot acknowledge through {through}: this log has only ever issued {issued}. \
                 Acknowledging past the end would skip every event up to it"
            );
        }

        // An acknowledgement only ever moves forward: a consumer replaying an
        // older batch must not rewind the position a later batch established.
        conn.execute(
            "INSERT INTO consumer (name, acked_seq, updated_at)
             VALUES (?1, ?2, unixepoch())
             ON CONFLICT(name) DO UPDATE SET
                acked_seq  = MAX(consumer.acked_seq, excluded.acked_seq),
                updated_at = unixepoch()",
            params![consumer, through],
        )?;

        let pending: i64 = conn.query_row(
            "SELECT count(*) FROM event
             WHERE seq > (SELECT acked_seq FROM consumer WHERE name = ?1)",
            params![consumer],
            |row| row.get(0),
        )?;
        Ok(pending as usize)
    }

    fn prune(&self) -> Result<PruneOutcome> {
        let conn = self.pool.get()?;
        let mut outcome = PruneOutcome::default();

        if self.mode == EventRetention::Spool {
            // Only what every consumer has acknowledged. With no consumer
            // registered nothing is dropped here, and the age cap below is
            // what keeps an abandoned spool from growing without end.
            outcome.acknowledged = conn.execute(
                "DELETE FROM event
                 WHERE (SELECT count(*) FROM consumer) > 0
                   AND seq <= (SELECT MIN(acked_seq) FROM consumer)",
                [],
            )?;
        }

        let horizon = (self.retention_days * 86_400) as i64;
        outcome.expired = conn.execute(
            "DELETE FROM event WHERE received_at < unixepoch() - ?1",
            params![horizon],
        )?;

        // The backstop. Retention is expressed in days, and a busy workspace
        // can outrun any number of them, so size has the last word. It bounds
        // the live data rather than the file: freed pages return to the
        // filesystem as vacuuming catches up, so the file trails this down
        // instead of tracking it. Bounded in both directions too — it stops
        // when nothing is left to delete, and after a fixed number of batches,
        // so a database that cannot shrink below the ceiling costs one pass
        // rather than the whole log.
        for _ in 0..MAX_PRUNE_BATCHES {
            if Self::byte_size(&conn)? <= self.max_bytes {
                break;
            }
            let removed = conn.execute(
                "DELETE FROM event WHERE seq IN
                    (SELECT seq FROM event ORDER BY seq LIMIT ?1)",
                params![PRUNE_BATCH as i64],
            )?;
            if removed == 0 {
                break;
            }
            outcome.over_budget += removed;
            conn.execute_batch("PRAGMA incremental_vacuum;")?;
        }

        if outcome.total() > 0 {
            conn.execute_batch("PRAGMA incremental_vacuum(1024);")?;
        }
        Ok(outcome)
    }

    fn stats(&self) -> Result<StoreStats> {
        let conn = self.pool.get()?;
        let (events, oldest, newest) = conn.query_row(
            "SELECT count(*), MIN(received_at), MAX(received_at) FROM event",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )?;

        let mut stmt = conn.prepare(
            "SELECT name, acked_seq,
                    (SELECT count(*) FROM event WHERE seq > consumer.acked_seq)
             FROM consumer ORDER BY name",
        )?;
        let consumers = stmt
            .query_map([], |row| {
                Ok(ConsumerLag {
                    name: row.get(0)?,
                    acked_seq: row.get(1)?,
                    pending: row.get::<_, i64>(2)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(StoreStats {
            events,
            bytes: Self::byte_size(&conn)?,
            oldest,
            newest,
            consumers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::envelope::{EVENT_SCHEMA, EventKind, EventSource};
    use chrono::{Duration, Utc};

    fn config(mode: EventRetention) -> EventsConfig {
        EventsConfig {
            mode,
            store_body: true,
            retention_days: 7,
            ..EventsConfig::default()
        }
    }

    fn event(ts: &str) -> Event {
        Event {
            schema: EVENT_SCHEMA.to_string(),
            id: format!("Ev{ts}"),
            seq: 0,
            kind: EventKind::Message,
            source: EventSource::Socket,
            team_id: Some("T01".into()),
            channel: Some("C01".into()),
            channel_type: Some("channel".into()),
            user: Some("U01".into()),
            bot_id: None,
            ts: Some(ts.to_string()),
            event_ts: None,
            thread_ts: None,
            subtype: None,
            text: Some("body".into()),
            reaction: None,
            item_user: None,
            received_at: Utc::now(),
            matched: vec!["mention".into()],
            raw: None,
        }
    }

    fn store(mode: EventRetention) -> (SqliteStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("events.db"), &config(mode)).unwrap();
        (store, dir)
    }

    #[test]
    fn the_null_store_hands_out_positions_and_keeps_nothing() {
        let store = NullStore::new();
        assert!(!store.caps().durable);
        assert_eq!(store.append(&event("1")).unwrap(), Some(1));
        assert_eq!(store.append(&event("2")).unwrap(), Some(2));
        assert_eq!(store.stats().unwrap().events, 0);
    }

    /// The commands that read events back must fail where someone can read
    /// the reason, not return an empty list that looks like a quiet workspace.
    #[test]
    fn reading_back_from_a_stream_store_is_refused_by_name() {
        let store = NullStore::new();
        let err = store.pull("agent", None, 10).unwrap_err();
        assert!(err.to_string().contains("events.mode"), "{err}");
        assert!(err.to_string().contains("spool"), "{err}");
        assert!(store.ack("agent", 1).is_err());
    }

    #[test]
    fn appending_the_same_delivery_twice_records_it_once() {
        let (store, _dir) = store(EventRetention::Spool);
        assert_eq!(store.append(&event("1700000000.000100")).unwrap(), Some(1));
        assert_eq!(store.append(&event("1700000000.000100")).unwrap(), None);
        assert_eq!(store.stats().unwrap().events, 1);
    }

    #[test]
    fn a_consumer_reads_forward_from_what_it_acknowledged() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..5 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }

        let first = store.pull("agent", None, 2).unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].seq, 1);

        store.ack("agent", first[1].seq).unwrap();
        let next = store.pull("agent", None, 2).unwrap();
        assert_eq!(next[0].seq, 3);

        // Another consumer has its own position and sees everything.
        assert_eq!(store.pull("other", None, 10).unwrap().len(), 5);
    }

    /// A reader that is not acknowledging still has to move forward. Without
    /// its own position it would re-print the same batch every time it asked,
    /// and — once the backlog filled a batch — would do so without pausing.
    #[test]
    fn a_reader_that_acknowledges_nothing_still_makes_progress() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..4 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }

        let first = store.pull("agent", None, 2).unwrap();
        assert_eq!(first.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2]);

        let next = store.pull("agent", Some(2), 2).unwrap();
        assert_eq!(next.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3, 4]);

        // And it acknowledged nothing, so a restart still replays everything.
        assert_eq!(store.pull("agent", None, 10).unwrap().len(), 4);
    }

    /// A mistyped `--through` would park the consumer past the end of the log,
    /// and every event until the sequence caught up would be skipped in
    /// silence. Refused instead.
    #[test]
    fn acknowledging_past_the_end_of_the_log_is_refused() {
        let (store, _dir) = store(EventRetention::Spool);
        store.append(&event("1700000000.000100")).unwrap();

        let err = store.ack("agent", 999_999).unwrap_err();
        assert!(err.to_string().contains("only ever issued"), "{err}");

        // The consumer is untouched, so nothing was skipped.
        assert_eq!(store.pull("agent", None, 10).unwrap().len(), 1);
        assert!(store.ack("agent", 1).is_ok());
    }

    /// The bound is what the log has ever issued, not what it still holds — a
    /// consumer that pulled a batch which was then pruned must still be able
    /// to acknowledge it.
    #[test]
    fn acknowledging_a_pruned_batch_still_works() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..3 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }
        store.ack("agent", 3).unwrap();
        store.prune().unwrap();
        assert_eq!(store.stats().unwrap().events, 0);

        assert!(store.ack("late", 3).is_ok());
    }

    #[test]
    fn an_acknowledgement_never_rewinds() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..3 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }
        store.ack("agent", 3).unwrap();
        store.ack("agent", 1).unwrap();
        assert!(store.pull("agent", None, 10).unwrap().is_empty());
    }

    #[test]
    fn spooled_events_are_dropped_once_every_consumer_has_them() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..3 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }

        // No consumer yet: nothing is acknowledged, so nothing is dropped.
        assert_eq!(store.prune().unwrap().acknowledged, 0);
        assert_eq!(store.stats().unwrap().events, 3);

        store.ack("agent", 2).unwrap();
        assert_eq!(store.prune().unwrap().acknowledged, 2);
        assert_eq!(store.stats().unwrap().events, 1);
    }

    /// Two consumers means the slower one decides. Dropping at the faster
    /// one's position would delete events the other has not read.
    #[test]
    fn the_slowest_consumer_holds_the_spool() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..4 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }
        store.ack("fast", 4).unwrap();
        store.ack("slow", 1).unwrap();

        store.prune().unwrap();
        assert_eq!(store.stats().unwrap().events, 3);
        assert_eq!(store.pull("slow", None, 10).unwrap().len(), 3);
    }

    #[test]
    fn an_archive_drops_what_has_aged_out() {
        let (store, _dir) = store(EventRetention::Archive);
        let mut old = event("1700000000.000100");
        old.received_at = Utc::now() - Duration::days(30);
        store.append(&old).unwrap();
        store.append(&event("1700000001.000100")).unwrap();

        let outcome = store.prune().unwrap();
        assert_eq!(outcome.expired, 1);
        assert_eq!(store.stats().unwrap().events, 1);
    }

    /// Retention is a duration and a busy workspace outruns any duration, so
    /// the byte ceiling has the last word.
    #[test]
    fn the_size_ceiling_outranks_the_retention_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(
            &dir.path().join("events.db"),
            &EventsConfig {
                mode: EventRetention::Archive,
                store_body: true,
                retention_days: 3650,
                max_bytes: 1024 * 1024,
                ..EventsConfig::default()
            },
        )
        .unwrap();

        for index in 0..6000 {
            let mut wide = event(&format!("17000{index:05}.000100"));
            wide.text = Some("x".repeat(512));
            store.append(&wide).unwrap();
        }

        let outcome = store.prune().unwrap();
        assert!(outcome.over_budget > 0, "nothing was dropped for size");
        assert!(store.stats().unwrap().events < 6000);
    }

    /// `store_body = false` is the setting that keeps other people's words off
    /// this disk while still recording that the conversation happened.
    #[test]
    fn a_reference_only_log_stores_no_message_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(
            &dir.path().join("events.db"),
            &EventsConfig {
                mode: EventRetention::Spool,
                store_body: false,
                ..EventsConfig::default()
            },
        )
        .unwrap();

        let mut secret = event("1700000000.000100");
        secret.text = Some("confidential".into());
        secret.raw = Some(serde_json::json!({ "text": "confidential" }));
        store.append(&secret).unwrap();

        let stored = store.pull("agent", None, 1).unwrap();
        assert!(stored[0].text.is_none());
        assert!(stored[0].raw.is_none());
        assert_eq!(stored[0].channel.as_deref(), Some("C01"));
        assert_eq!(stored[0].matched, vec!["mention".to_string()]);

        let dir_entries = std::fs::read_to_string(dir.path().join("events.db")).unwrap_or_default();
        assert!(!dir_entries.contains("confidential"));
    }

    #[test]
    fn statistics_report_each_consumer_backlog() {
        let (store, _dir) = store(EventRetention::Spool);
        for index in 0..3 {
            store
                .append(&event(&format!("170000000{index}.000100")))
                .unwrap();
        }
        store.ack("agent", 1).unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.events, 3);
        assert_eq!(stats.consumers.len(), 1);
        assert_eq!(stats.consumers[0].name, "agent");
        assert_eq!(stats.consumers[0].pending, 2);
        assert!(stats.bytes > 0);
    }
}
