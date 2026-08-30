use std::path::Path;

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};

use super::db;
use super::envelope::Event;

const SCHEMA_VERSION: i64 = 2;

const STEPS: &[(i64, &str)] = &[
    (
        1,
        "CREATE TABLE channel_cursor (
        channel     TEXT PRIMARY KEY,
        last_ts     TEXT NOT NULL,
        recoverable INTEGER NOT NULL DEFAULT 0,
        updated_at  INTEGER NOT NULL
     );
     CREATE INDEX idx_cursor_recoverable ON channel_cursor(recoverable, updated_at);

     CREATE TABLE watched_thread (
        channel    TEXT NOT NULL,
        thread_ts  TEXT NOT NULL,
        rule       TEXT NOT NULL,
        emoji      TEXT NOT NULL,
        watcher    TEXT,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (channel, thread_ts, rule)
     );

     CREATE TABLE seen (
        key     TEXT PRIMARY KEY,
        seen_at INTEGER NOT NULL
     );
     CREATE INDEX idx_seen_at ON seen(seen_at);

     CREATE TABLE daemon (
        id           INTEGER PRIMARY KEY CHECK (id = 1),
        pid          INTEGER NOT NULL,
        started_at   INTEGER NOT NULL,
        heartbeat_at INTEGER NOT NULL,
        connected    INTEGER NOT NULL,
        received     INTEGER NOT NULL,
        matched      INTEGER NOT NULL,
        stored       INTEGER NOT NULL,
        dropped      INTEGER NOT NULL,
        delivered    INTEGER NOT NULL,
        failed       INTEGER NOT NULL,
        reconnects   INTEGER NOT NULL,
        backfilled   INTEGER NOT NULL
     );",
    ),
    (
        2,
        // Where a subscribed thread has been followed to. Without it, recovery
        // re-reads the thread from its first message on every reconnect, and once
        // the deduplication layers lapse — the seen keys after a day, the log rows
        // as soon as a consumer acknowledges them — it re-delivers the whole
        // thread each time.
        "ALTER TABLE watched_thread ADD COLUMN cursor_ts TEXT;",
    ),
];

/// How long a deduplication key is remembered. Slack redelivers an
/// unacknowledged envelope within minutes and a backfill reaches back hours,
/// so a day covers both with room to spare, and the table stays small.
const SEEN_RETENTION_HOURS: i64 = 24;

/// The daemon's positions and subscriptions — never its content.
///
/// Everything here is a reference: which timestamp a channel was last seen at,
/// which threads are subscribed, which delivery keys have been handled. That
/// is what lets `events.mode = "stream"` keep no message text at all and still
/// recover from a disconnect, unsubscribe a thread, and refuse a duplicate.
#[derive(Clone)]
pub struct EventState {
    pool: Pool<SqliteConnectionManager>,
}

/// One channel the daemon may ask Slack to replay after a disconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub channel: String,
    pub last_ts: String,
}

/// One subscribed thread, which recovery reads separately from its channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedThread {
    pub channel: String,
    pub thread_ts: String,
    /// How far this thread has been followed. Recovery reads from here, never
    /// from the top of the thread.
    pub cursor_ts: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonStatus {
    pub pid: i64,
    pub started_at: i64,
    pub heartbeat_at: i64,
    pub connected: bool,
    pub counters: Counters,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub received: u64,
    pub matched: u64,
    pub stored: u64,
    pub dropped: u64,
    pub delivered: u64,
    pub failed: u64,
    pub reconnects: u64,
    pub backfilled: u64,
}

impl EventState {
    pub fn open(path: &Path) -> Result<Self> {
        let pool = db::open_pool(path)?;
        let mut conn = pool.get()?;
        db::migrate(&mut conn, "event state", SCHEMA_VERSION, STEPS)?;
        drop(conn);
        Ok(Self { pool })
    }

    /// Records how far a conversation has been followed.
    ///
    /// `recoverable` is sticky: once a channel has produced a match, or has
    /// been named by a rule, it stays a candidate for gap recovery even
    /// through quiet periods. It never turns off on its own, because the rule
    /// that made it interesting is still there.
    pub fn advance_cursor(&self, channel: &str, ts: &str, recoverable: bool) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO channel_cursor (channel, last_ts, recoverable, updated_at)
             VALUES (?1, ?2, ?3, unixepoch())
             ON CONFLICT(channel) DO UPDATE SET
                last_ts     = CASE WHEN excluded.last_ts > channel_cursor.last_ts
                                   THEN excluded.last_ts ELSE channel_cursor.last_ts END,
                recoverable = MAX(channel_cursor.recoverable, excluded.recoverable),
                updated_at  = unixepoch()",
            params![channel, ts, i64::from(recoverable)],
        )?;
        Ok(())
    }

    /// Marks a channel worth recovering even before it has produced a match —
    /// what a rule's channel allowlist declares at startup.
    pub fn mark_recoverable(&self, channel: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO channel_cursor (channel, last_ts, recoverable, updated_at)
             VALUES (?1, '0', 1, unixepoch())
             ON CONFLICT(channel) DO UPDATE SET recoverable = 1, updated_at = unixepoch()",
            params![channel],
        )?;
        Ok(())
    }

    /// The channels a reconnect may ask Slack to replay, most recently active
    /// first and never more than `limit`.
    ///
    /// Only channels a rule cares about are offered, and only those that have
    /// been followed to a real position — the `'0'` a rule's allowlist writes
    /// is a declaration of interest, not a place to read from. Recovery reads
    /// `conversations.history`, which a non-Marketplace app may call once a
    /// minute, so the list is bounded here and the *window* is bounded by the
    /// caller: a cursor older than the recovery horizon is read from the
    /// horizon, not dropped. Excluding the channel instead would lose
    /// everything inside the window as well, silently, which is exactly what
    /// the horizon is not for.
    pub fn gaps(&self, limit: usize) -> Result<Vec<Gap>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT channel, last_ts FROM channel_cursor
             WHERE recoverable = 1 AND last_ts > '0'
             ORDER BY updated_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Gap {
                    channel: row.get(0)?,
                    last_ts: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Subscribes a thread, following it from `from_ts` onwards.
    ///
    /// `from_ts` is when the subscription was made, not when the thread
    /// started: the live path only matches replies that arrive after the
    /// reaction, and recovery has to agree with it or a reconnect would
    /// deliver the whole conversation that preceded it.
    pub fn watch_thread(
        &self,
        channel: &str,
        thread_ts: &str,
        rule: &str,
        emoji: &str,
        watcher: Option<&str>,
        from_ts: Option<&str>,
    ) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO watched_thread
                (channel, thread_ts, rule, emoji, watcher, cursor_ts, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch())
             ON CONFLICT(channel, thread_ts, rule) DO UPDATE SET emoji = excluded.emoji",
            params![
                channel,
                thread_ts,
                rule,
                emoji,
                watcher,
                from_ts.unwrap_or(thread_ts)
            ],
        )?;
        Ok(())
    }

    /// Records how far a subscribed thread has been followed. Like a channel
    /// cursor it only moves forward, so an out-of-order delivery cannot make
    /// recovery skip what it has not seen.
    pub fn advance_thread_cursor(&self, channel: &str, thread_ts: &str, ts: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE watched_thread
                SET cursor_ts = ?3
              WHERE channel = ?1 AND thread_ts = ?2
                AND (cursor_ts IS NULL OR ?3 > cursor_ts)",
            params![channel, thread_ts, ts],
        )?;
        Ok(())
    }

    pub fn unwatch_thread(&self, channel: &str, thread_ts: &str, rule: &str) -> Result<bool> {
        let conn = self.pool.get()?;
        let removed = conn.execute(
            "DELETE FROM watched_thread WHERE channel = ?1 AND thread_ts = ?2 AND rule = ?3",
            params![channel, thread_ts, rule],
        )?;
        Ok(removed > 0)
    }

    pub fn is_watched(&self, channel: &str, thread_ts: &str, rule: &str) -> Result<bool> {
        let conn = self.pool.get()?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM watched_thread
                 WHERE channel = ?1 AND thread_ts = ?2 AND rule = ?3",
                params![channel, thread_ts, rule],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Every thread a rule is currently subscribed to, newest first.
    ///
    /// Recovery needs these by name: `conversations.history` returns a
    /// channel's top-level messages and never a thread's replies, so a reply
    /// that arrived during a disconnect is reachable only through
    /// `conversations.replies` on the thread it belongs to.
    pub fn watched_threads(&self, limit: usize) -> Result<Vec<WatchedThread>> {
        let conn = self.pool.get()?;
        // The furthest-followed cursor across the rules watching a thread: a
        // reply already delivered under one rule must not be read back for
        // another.
        let mut stmt = conn.prepare(
            "SELECT channel, thread_ts, MAX(IFNULL(cursor_ts, thread_ts)), MAX(created_at)
             FROM watched_thread
             GROUP BY channel, thread_ts
             ORDER BY MAX(created_at) DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(WatchedThread {
                    channel: row.get(0)?,
                    thread_ts: row.get(1)?,
                    cursor_ts: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn watched_count(&self) -> Result<u64> {
        let conn = self.pool.get()?;
        let count: i64 =
            conn.query_row("SELECT count(*) FROM watched_thread", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Whether this delivery has already been processed to completion.
    ///
    /// The gate is a read, and the write that closes it happens only after the
    /// event has been stored and delivered. A crash in between therefore
    /// leaves both the key and the channel cursor untouched, so the recovery
    /// pass finds the event again — at-least-once, rather than a silent hole.
    /// One process writes this table, so checking and then committing cannot
    /// race.
    pub fn is_seen(&self, event: &Event) -> Result<bool> {
        let conn = self.pool.get()?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM seen WHERE key = ?1",
                params![event.dedupe_key()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Records a delivery key, answering whether it was new.
    ///
    /// This is the filter that works in every retention mode, including the
    /// one that stores no events at all. Where an event log exists it is the
    /// cheap first pass, and the log's own unique key is the authority.
    pub fn mark_seen(&self, event: &Event) -> Result<bool> {
        let conn = self.pool.get()?;
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO seen (key, seen_at) VALUES (?1, unixepoch())",
            params![event.dedupe_key()],
        )?;
        Ok(inserted > 0)
    }

    /// Forgets keys older than the redelivery window, so the table tracks the
    /// window rather than the workspace's whole history.
    pub fn prune_seen(&self) -> Result<usize> {
        let conn = self.pool.get()?;
        let removed = conn.execute(
            "DELETE FROM seen WHERE seen_at < unixepoch() - ?1",
            params![SEEN_RETENTION_HOURS * 3600],
        )?;
        Ok(removed)
    }

    pub fn claim_daemon(&self, pid: u32) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO daemon (id, pid, started_at, heartbeat_at, connected,
                                 received, matched, stored, dropped, delivered,
                                 failed, reconnects, backfilled)
             VALUES (1, ?1, unixepoch(), unixepoch(), 0, 0, 0, 0, 0, 0, 0, 0, 0)
             ON CONFLICT(id) DO UPDATE SET
                pid = excluded.pid, started_at = excluded.started_at,
                heartbeat_at = excluded.heartbeat_at, connected = 0,
                received = 0, matched = 0, stored = 0, dropped = 0,
                delivered = 0, failed = 0, reconnects = 0, backfilled = 0",
            params![i64::from(pid)],
        )?;
        Ok(())
    }

    pub fn heartbeat(&self, connected: bool, counters: Counters) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE daemon SET heartbeat_at = unixepoch(), connected = ?1,
                    received = ?2, matched = ?3, stored = ?4, dropped = ?5,
                    delivered = ?6, failed = ?7, reconnects = ?8, backfilled = ?9
             WHERE id = 1",
            params![
                i64::from(connected),
                counters.received as i64,
                counters.matched as i64,
                counters.stored as i64,
                counters.dropped as i64,
                counters.delivered as i64,
                counters.failed as i64,
                counters.reconnects as i64,
                counters.backfilled as i64,
            ],
        )?;
        Ok(())
    }

    pub fn daemon_status(&self) -> Result<Option<DaemonStatus>> {
        let conn = self.pool.get()?;
        let status = conn
            .query_row(
                "SELECT pid, started_at, heartbeat_at, connected, received, matched,
                        stored, dropped, delivered, failed, reconnects, backfilled
                 FROM daemon WHERE id = 1",
                [],
                |row| {
                    Ok(DaemonStatus {
                        pid: row.get(0)?,
                        started_at: row.get(1)?,
                        heartbeat_at: row.get(2)?,
                        connected: row.get::<_, i64>(3)? != 0,
                        counters: Counters {
                            received: row.get::<_, i64>(4)? as u64,
                            matched: row.get::<_, i64>(5)? as u64,
                            stored: row.get::<_, i64>(6)? as u64,
                            dropped: row.get::<_, i64>(7)? as u64,
                            delivered: row.get::<_, i64>(8)? as u64,
                            failed: row.get::<_, i64>(9)? as u64,
                            reconnects: row.get::<_, i64>(10)? as u64,
                            backfilled: row.get::<_, i64>(11)? as u64,
                        },
                    })
                },
            )
            .optional()
            .context("could not read the daemon record")?;
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::envelope::{EVENT_SCHEMA, EventKind, EventSource};
    use chrono::Utc;

    fn state() -> (EventState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let state = EventState::open(&dir.path().join("state.db")).unwrap();
        (state, dir)
    }

    /// Cursor tests run against the recovery horizon, which is measured from
    /// now — so their timestamps have to be too.
    fn ts(offset_seconds: i64) -> String {
        format!("{}.000000", Utc::now().timestamp() + offset_seconds)
    }

    fn message(channel: &str, ts: &str) -> Event {
        Event {
            schema: EVENT_SCHEMA.to_string(),
            id: format!("Ev{ts}"),
            seq: 0,
            kind: EventKind::Message,
            source: EventSource::Socket,
            team_id: None,
            channel: Some(channel.to_string()),
            channel_type: None,
            user: Some("U01".to_string()),
            bot_id: None,
            ts: Some(ts.to_string()),
            event_ts: None,
            thread_ts: None,
            subtype: None,
            text: Some("hi".to_string()),
            reaction: None,
            item_user: None,
            received_at: Utc::now(),
            matched: Vec::new(),
            raw: None,
        }
    }

    #[test]
    fn a_cursor_only_moves_forward() {
        let (state, _dir) = state();
        let newer = ts(-10);
        state.advance_cursor("C01", &newer, true).unwrap();
        state.advance_cursor("C01", &ts(-600), false).unwrap();

        let gaps = state.gaps(10).unwrap();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].last_ts, newer);
    }

    /// Once a rule has cared about a channel it stays a recovery candidate,
    /// even though later traffic in it matches nothing.
    #[test]
    fn interest_in_a_channel_is_sticky() {
        let (state, _dir) = state();
        state.advance_cursor("C01", &ts(-20), true).unwrap();
        state.advance_cursor("C01", &ts(-10), false).unwrap();

        assert_eq!(state.gaps(10).unwrap().len(), 1);
    }

    #[test]
    fn a_channel_no_rule_cares_about_is_never_recovered() {
        let (state, _dir) = state();
        state.advance_cursor("C01", &ts(-10), false).unwrap();
        assert!(state.gaps(10).unwrap().is_empty());
    }

    /// Recovery reads a rate-limited endpoint, so the list it works from is
    /// bounded by how many channels it offers. It is deliberately *not*
    /// bounded by how old their cursors are: a channel followed to a point
    /// further back than recovery reaches is read from the horizon instead,
    /// and dropping it here would lose everything inside the window too.
    #[test]
    fn recovery_is_bounded_by_count_but_not_by_the_age_of_a_cursor() {
        let (state, _dir) = state();
        for index in 0..5 {
            state
                .advance_cursor(&format!("C{index:07}"), &ts(-60), true)
                .unwrap();
        }
        state
            .advance_cursor("C9999999", &ts(-90 * 3600), true)
            .unwrap();

        assert_eq!(state.gaps(2).unwrap().len(), 2, "the count still bounds it");

        let offered = state.gaps(50).unwrap();
        assert_eq!(offered.len(), 6);
        assert!(
            offered.iter().any(|gap| gap.channel == "C9999999"),
            "a long-stale channel is still offered, to be read from the horizon"
        );
    }

    /// A channel a rule merely names has no position to read from. Offering it
    /// would spend a rationed request replaying history nobody was following.
    #[test]
    fn a_channel_with_no_real_cursor_is_not_a_gap() {
        let (state, _dir) = state();
        state.mark_recoverable("C0000001").unwrap();
        assert!(state.gaps(50).unwrap().is_empty());

        state.advance_cursor("C0000001", &ts(-30), true).unwrap();
        assert_eq!(state.gaps(50).unwrap().len(), 1);
    }

    #[test]
    fn a_delivery_key_is_new_exactly_once() {
        let (state, _dir) = state();
        let event = message("C01", "1700000000.000100");
        assert!(state.mark_seen(&event).unwrap());
        assert!(!state.mark_seen(&event).unwrap());

        let mut redelivered = event.clone();
        redelivered.id = "Ev-different".to_string();
        redelivered.source = EventSource::Backfill;
        assert!(!state.mark_seen(&redelivered).unwrap());
    }

    #[test]
    fn a_thread_subscription_is_per_rule_and_reversible() {
        let (state, _dir) = state();
        state
            .watch_thread(
                "C01",
                "1700000000.000100",
                "watched",
                "eyes",
                Some("U01"),
                None,
            )
            .unwrap();

        assert!(
            state
                .is_watched("C01", "1700000000.000100", "watched")
                .unwrap()
        );
        assert!(
            !state
                .is_watched("C01", "1700000000.000100", "other")
                .unwrap()
        );
        assert_eq!(state.watched_count().unwrap(), 1);

        assert!(
            state
                .unwatch_thread("C01", "1700000000.000100", "watched")
                .unwrap()
        );
        assert!(
            !state
                .is_watched("C01", "1700000000.000100", "watched")
                .unwrap()
        );
        assert!(
            !state
                .unwatch_thread("C01", "1700000000.000100", "watched")
                .unwrap()
        );
    }

    /// Recovery reads a subscribed thread from its cursor. Without one it
    /// would re-read from the thread's first message on every reconnect, and
    /// once the deduplication layers lapsed it would re-deliver the lot.
    #[test]
    fn a_thread_cursor_starts_at_the_subscription_and_only_moves_forward() {
        let (state, _dir) = state();
        state
            .watch_thread(
                "C01",
                "1700000000.000100",
                "watched",
                "eyes",
                Some("U01"),
                Some("1700000500.000000"),
            )
            .unwrap();

        let followed = |state: &EventState| state.watched_threads(10).unwrap()[0].cursor_ts.clone();
        assert_eq!(followed(&state), "1700000500.000000");

        state
            .advance_thread_cursor("C01", "1700000000.000100", "1700000900.000100")
            .unwrap();
        assert_eq!(followed(&state), "1700000900.000100");

        // An out-of-order delivery must not make recovery skip backwards.
        state
            .advance_thread_cursor("C01", "1700000000.000100", "1700000600.000100")
            .unwrap();
        assert_eq!(followed(&state), "1700000900.000100");
    }

    /// Two rules watching one thread share a position: a reply delivered under
    /// one of them must not be read back for the other.
    #[test]
    fn rules_watching_one_thread_share_the_furthest_position() {
        let (state, _dir) = state();
        for rule in ["a", "b"] {
            state
                .watch_thread("C01", "1700000000.000100", rule, "eyes", None, None)
                .unwrap();
        }
        state
            .advance_thread_cursor("C01", "1700000000.000100", "1700000900.000100")
            .unwrap();

        let threads = state.watched_threads(10).unwrap();
        assert_eq!(threads.len(), 1, "one thread, however many rules watch it");
        assert_eq!(threads[0].cursor_ts, "1700000900.000100");
    }

    #[test]
    fn the_daemon_record_reports_what_the_last_heartbeat_saw() {
        let (state, _dir) = state();
        assert!(state.daemon_status().unwrap().is_none());

        state.claim_daemon(4242).unwrap();
        state
            .heartbeat(
                true,
                Counters {
                    received: 10,
                    matched: 2,
                    dropped: 1,
                    ..Counters::default()
                },
            )
            .unwrap();

        let status = state.daemon_status().unwrap().unwrap();
        assert_eq!(status.pid, 4242);
        assert!(status.connected);
        assert_eq!(status.counters.received, 10);
        assert_eq!(status.counters.dropped, 1);
    }

    /// State survives a restart: that is what makes a thread subscription and
    /// a recovery cursor useful at all.
    #[test]
    fn state_outlives_the_process_that_wrote_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        {
            let state = EventState::open(&path).unwrap();
            state
                .watch_thread("C01", "1700000000.000100", "r", "eyes", None, None)
                .unwrap();
            state.advance_cursor("C01", &ts(-30), true).unwrap();
        }

        let reopened = EventState::open(&path).unwrap();
        assert!(
            reopened
                .is_watched("C01", "1700000000.000100", "r")
                .unwrap()
        );
        assert_eq!(reopened.gaps(10).unwrap().len(), 1);
    }
}
