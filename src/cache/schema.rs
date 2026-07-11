use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, types::Value};

use super::error::CacheResult;

pub const SCHEMA_VERSION: i32 = 2;

/// Every object the cache owns. `apply_schema` drops and recreates all of it
/// when the stored version differs from `SCHEMA_VERSION` — cache contents are
/// refetchable from Slack, so a rebuild is always safe and always correct.
const SCHEMA_DDL: &str = "
    -- Users table with JSON storage and indexed fields
    CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY,
        data JSON NOT NULL,
        name TEXT GENERATED ALWAYS AS (json_extract(data, '$.name')) STORED,
        display_name TEXT GENERATED ALWAYS AS (json_extract(data, '$.profile.display_name')) STORED,
        real_name TEXT GENERATED ALWAYS AS (json_extract(data, '$.profile.real_name')) STORED,
        email TEXT GENERATED ALWAYS AS (json_extract(data, '$.profile.email')) STORED,
        is_bot INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_bot')) STORED,
        updated_at INTEGER DEFAULT (unixepoch())
    );

    CREATE INDEX IF NOT EXISTS idx_users_name ON users(name);
    CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
    CREATE INDEX IF NOT EXISTS idx_users_is_bot ON users(is_bot);

    -- Channels table with JSON storage and indexed fields
    CREATE TABLE IF NOT EXISTS channels (
        id TEXT PRIMARY KEY,
        data JSON NOT NULL,
        name TEXT GENERATED ALWAYS AS (json_extract(data, '$.name')) STORED,
        topic TEXT GENERATED ALWAYS AS (json_extract(data, '$.topic.value')) STORED,
        purpose TEXT GENERATED ALWAYS AS (json_extract(data, '$.purpose.value')) STORED,
        is_private INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_private')) STORED,
        is_channel INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_channel')) STORED,
        is_group INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_group')) STORED,
        is_im INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_im')) STORED,
        is_mpim INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_mpim')) STORED,
        is_archived INTEGER GENERATED ALWAYS AS (json_extract(data, '$.is_archived')) STORED,
        updated_at INTEGER DEFAULT (unixepoch())
    );

    CREATE INDEX IF NOT EXISTS idx_channels_name ON channels(name);
    CREATE INDEX IF NOT EXISTS idx_channels_type ON channels(is_channel, is_group, is_im, is_mpim);
    CREATE INDEX IF NOT EXISTS idx_channels_archived ON channels(is_archived);

    -- FTS5 tables for fuzzy search
    CREATE VIRTUAL TABLE IF NOT EXISTS users_fts USING fts5(
        id UNINDEXED,
        name,
        display_name,
        real_name,
        email,
        content=users,
        content_rowid=rowid,
        tokenize='porter unicode61'
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS channels_fts USING fts5(
        id UNINDEXED,
        name,
        topic,
        purpose,
        content=channels,
        content_rowid=rowid,
        tokenize='porter unicode61'
    );

    -- Triggers to keep FTS in sync
    CREATE TRIGGER IF NOT EXISTS users_ai AFTER INSERT ON users BEGIN
        INSERT INTO users_fts(rowid, id, name, display_name, real_name, email)
        VALUES (new.rowid, new.id, new.name, new.display_name, new.real_name, new.email);
    END;

    CREATE TRIGGER IF NOT EXISTS users_ad AFTER DELETE ON users BEGIN
        DELETE FROM users_fts WHERE rowid = old.rowid;
    END;

    CREATE TRIGGER IF NOT EXISTS users_au AFTER UPDATE ON users BEGIN
        DELETE FROM users_fts WHERE rowid = old.rowid;
        INSERT INTO users_fts(rowid, id, name, display_name, real_name, email)
        VALUES (new.rowid, new.id, new.name, new.display_name, new.real_name, new.email);
    END;

    CREATE TRIGGER IF NOT EXISTS channels_ai AFTER INSERT ON channels BEGIN
        INSERT INTO channels_fts(rowid, id, name, topic, purpose)
        VALUES (new.rowid, new.id, new.name, new.topic, new.purpose);
    END;

    CREATE TRIGGER IF NOT EXISTS channels_ad AFTER DELETE ON channels BEGIN
        DELETE FROM channels_fts WHERE rowid = old.rowid;
    END;

    CREATE TRIGGER IF NOT EXISTS channels_au AFTER UPDATE ON channels BEGIN
        DELETE FROM channels_fts WHERE rowid = old.rowid;
        INSERT INTO channels_fts(rowid, id, name, topic, purpose)
        VALUES (new.rowid, new.id, new.name, new.topic, new.purpose);
    END;

    -- Metadata table
    CREATE TABLE IF NOT EXISTS metadata (
        key TEXT PRIMARY KEY,
        value JSON NOT NULL,
        updated_at INTEGER DEFAULT (unixepoch())
    );

    -- Distributed locks table for multi-instance coordination
    CREATE TABLE IF NOT EXISTS locks (
        key TEXT PRIMARY KEY,
        instance_id TEXT NOT NULL,
        acquired_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_locks_expires ON locks(expires_at);
";

/// FTS virtual tables first (they shadow the content tables), then the
/// content tables (their triggers and indexes drop with them).
const SCHEMA_TEARDOWN: &str = "
    DROP TABLE IF EXISTS users_fts;
    DROP TABLE IF EXISTS channels_fts;
    DROP TABLE IF EXISTS users;
    DROP TABLE IF EXISTS channels;
    DROP TABLE IF EXISTS metadata;
    DROP TABLE IF EXISTS locks;
";

pub async fn initialize_schema(pool: &Pool<SqliteConnectionManager>) -> CacheResult<()> {
    let mut conn = pool.get()?;
    apply_schema(&mut conn)
}

#[cfg(test)]
pub fn initialize_schema_sync(pool: &Pool<SqliteConnectionManager>) -> CacheResult<()> {
    let mut conn = pool.get()?;
    apply_schema(&mut conn)
}

fn apply_schema(conn: &mut Connection) -> CacheResult<()> {
    // The whole check-teardown-rebuild sequence runs in one IMMEDIATE
    // transaction: concurrent openers serialize on the write lock (via the
    // busy handler), and the version is re-read under that lock so the losers
    // of the race see the winner's rebuild and skip their own. busy_timeout is
    // set here rather than assumed from pool init so the serialization holds
    // for any connection this runs on.
    conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    if stored_schema_version(&tx)? != Some(SCHEMA_VERSION) {
        tx.execute_batch(SCHEMA_TEARDOWN)?;
        tx.execute_batch(SCHEMA_DDL)?;
        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', json(?))",
            params![SCHEMA_VERSION],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn stored_schema_version(conn: &Connection) -> CacheResult<Option<i32>> {
    let metadata_exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'metadata')",
        [],
        |row| row.get(0),
    )?;
    if !metadata_exists {
        return Ok(None);
    }

    // Anything other than an integer counts as "no valid version" and takes
    // the rebuild path — cache contents are refetchable, so rebuilding on a
    // corrupt value is strictly safer than failing the open.
    let version = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, Value>(0),
        )
        .optional()?;
    Ok(match version {
        Some(Value::Integer(v)) => i32::try_from(v).ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_connection() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn fresh_database_gets_current_version() {
        let mut conn = open_connection();
        apply_schema(&mut conn).unwrap();

        assert_eq!(stored_schema_version(&conn).unwrap(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn missing_metadata_table_reads_as_no_version() {
        let conn = open_connection();
        assert_eq!(stored_schema_version(&conn).unwrap(), None);
    }

    #[test]
    fn matching_version_preserves_cached_data() {
        let mut conn = open_connection();
        apply_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO users (id, data) VALUES ('U1', json('{\"name\":\"alice\"}'))",
            [],
        )
        .unwrap();

        apply_schema(&mut conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn outdated_version_rebuilds_from_scratch() {
        let mut conn = open_connection();
        apply_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO users (id, data) VALUES ('U1', json('{\"name\":\"alice\"}'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', json(?))",
            params![SCHEMA_VERSION - 1],
        )
        .unwrap();

        apply_schema(&mut conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(stored_schema_version(&conn).unwrap(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn non_integer_version_reads_as_no_version_and_rebuilds() {
        let mut conn = open_connection();
        apply_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', json('\"corrupt\"'))",
            [],
        )
        .unwrap();

        assert_eq!(stored_schema_version(&conn).unwrap(), None);

        apply_schema(&mut conn).unwrap();
        assert_eq!(stored_schema_version(&conn).unwrap(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn legacy_table_layout_is_replaced_by_current_ddl() {
        let mut conn = open_connection();
        conn.execute_batch(
            "CREATE TABLE users (id TEXT PRIMARY KEY, data JSON NOT NULL);
             CREATE TABLE metadata (key TEXT PRIMARY KEY, value JSON NOT NULL);
             INSERT INTO metadata (key, value) VALUES ('schema_version', json(1));",
        )
        .unwrap();

        apply_schema(&mut conn).unwrap();

        let generated_columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_xinfo('users') WHERE name = 'display_name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(generated_columns, 1);
        assert_eq!(stored_schema_version(&conn).unwrap(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn concurrent_opens_serialize_the_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            apply_schema(&mut conn).unwrap();
            conn.execute(
                "INSERT INTO users (id, data) VALUES ('U1', json('{\"name\":\"alice\"}'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', json(?))",
                params![SCHEMA_VERSION - 1],
            )
            .unwrap();
        }

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut conn = Connection::open(&path)?;
                    apply_schema(&mut conn)
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let conn = Connection::open(&path).unwrap();
        assert_eq!(stored_schema_version(&conn).unwrap(), Some(SCHEMA_VERSION));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
