use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use super::error::CacheResult;

pub const SCHEMA_VERSION: i32 = 2;

pub async fn initialize_schema(pool: &Pool<SqliteConnectionManager>) -> CacheResult<()> {
    let conn = pool.get()?;

    conn.execute_batch(
        "
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

        -- Initialize schema version
        INSERT OR IGNORE INTO metadata (key, value) VALUES ('schema_version', json(?));
        "
    )?;

    // Set schema version
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', json(?))",
        params![SCHEMA_VERSION],
    )?;

    Ok(())
}
