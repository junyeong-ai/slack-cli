use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, TransactionBehavior};

/// Retry budget for the one-time WAL switch. SQLite answers SQLITE_BUSY on a
/// journal-mode transition without consulting the busy handler, so a sibling
/// process opening the same file needs a brief retry loop. Mirrors the cache.
const WAL_SWITCH_MAX_ATTEMPTS: u32 = 10;
const WAL_SWITCH_RETRY_DELAY_MS: u64 = 50;

/// Opens one of the daemon's databases.
///
/// Unlike the cache, these files are opened with `auto_vacuum = INCREMENTAL`,
/// which SQLite can only be told before the first table exists: a pruned event
/// log that never returns its pages to the filesystem would make every
/// retention setting a lie.
pub fn open_pool(path: &Path) -> Result<Pool<SqliteConnectionManager>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
        restrict(parent, 0o700)?;
    }

    let manager = SqliteConnectionManager::file(path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
    });

    let pool = Pool::builder()
        .max_size(4)
        .connection_timeout(Duration::from_secs(5))
        .build(manager)
        .with_context(|| format!("could not open {}", path.display()))?;

    // Before the first write, so the `-wal` and `-shm` files SQLite creates
    // alongside inherit the same mode. The event log holds other people's
    // messages; the default umask would leave that readable to anyone with an
    // account on the machine.
    restrict(path, 0o600)?;

    let conn = pool.get()?;
    // A `PRAGMA` that sets rather than reads returns no rows, so it is run as
    // a statement. SQLite only honours this one while the database is still
    // empty, which is why it comes before the schema: afterwards it is a
    // silent no-op, and the existing setting stands.
    conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")
        .context("could not request incremental vacuuming")?;
    switch_to_wal(&conn)?;

    Ok(pool)
}

/// Tightens permissions on a path the daemon owns.
///
/// Not inherited from anywhere: `auth.json` gets its 0700 root from the auth
/// store, but an installation driven entirely by `SLACK_USER_TOKEN` never
/// writes one, and `[events] data_path` can point somewhere else entirely.
#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        // Nothing there yet: SQLite creates the file, and the directory it
        // lands in has already been tightened.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("could not stat {}", path.display())),
    };

    let mut permissions = metadata.permissions();
    if permissions.mode() & 0o777 == mode {
        return Ok(());
    }
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions)
        .with_context(|| format!("could not restrict {}", path.display()))
}

/// A no-op off Unix, where the path inherits the ACL of the directory it is
/// created in — the same compromise the auth store makes.
#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn switch_to_wal(conn: &Connection) -> Result<()> {
    let mut attempt = 0;
    loop {
        match conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        }) {
            // SQLite answers with the mode actually in force. A filesystem that
            // cannot support write-ahead logging — some network mounts — keeps
            // the old one and reports it without erroring, and the daemon would
            // then block every `events pull` behind its own writes.
            Ok(mode) if mode.eq_ignore_ascii_case("wal") => return Ok(()),
            Ok(mode) => {
                anyhow::bail!(
                    "this filesystem refused write-ahead logging and stayed in {mode:?} mode. \
                     The daemon writes while `events pull` reads, which that cannot support — \
                     put the event store on a local filesystem with [events] data_path"
                );
            }
            Err(err) if attempt + 1 < WAL_SWITCH_MAX_ATTEMPTS => {
                attempt += 1;
                tracing::debug!("WAL switch busy ({err}); retrying");
                std::thread::sleep(Duration::from_millis(WAL_SWITCH_RETRY_DELAY_MS));
            }
            Err(err) => return Err(err).context("could not switch the database to WAL mode"),
        }
    }
}

/// Brings a database up to `target` by applying the steps it is missing.
///
/// This is deliberately not the cache's drop-and-rebuild. What the daemon
/// stores cannot be refetched — Socket Mode replays nothing and a backfill
/// reaches back hours, not weeks — so a schema change migrates the data or it
/// fails, and a file written by a *newer* build is refused rather than
/// destroyed by an older one.
pub fn migrate(
    conn: &mut Connection,
    label: &str,
    target: i64,
    steps: &[(i64, &str)],
) -> Result<()> {
    conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let current: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > target {
        anyhow::bail!(
            "the {label} database is at schema version {current}, but this build understands \
             {target}. It was written by a newer slack-cli; upgrade rather than downgrade, \
             because these records cannot be refetched from Slack"
        );
    }

    for (version, ddl) in steps {
        if current < *version {
            tx.execute_batch(ddl)
                .with_context(|| format!("{label} schema step {version} failed"))?;
        }
    }

    if current < target {
        // No parameter binding: SQLite refuses one on a pragma, and `target`
        // is a compile-time constant rather than anything a user supplies.
        tx.execute_batch(&format!("PRAGMA user_version = {target}"))?;
    }

    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEPS: &[(i64, &str)] = &[
        (1, "CREATE TABLE thing (id INTEGER PRIMARY KEY, note TEXT);"),
        (2, "ALTER TABLE thing ADD COLUMN extra TEXT;"),
    ];

    fn versioned(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_fresh_database_receives_every_step() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, "test", 2, STEPS).unwrap();
        assert_eq!(versioned(&conn), 2);
        conn.execute("INSERT INTO thing (note, extra) VALUES ('a', 'b')", [])
            .unwrap();
    }

    /// The property the cache does not have and this must: existing rows
    /// survive the upgrade, because Slack will not send them again.
    #[test]
    fn an_upgrade_preserves_what_is_already_stored() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, "test", 1, &STEPS[..1]).unwrap();
        conn.execute("INSERT INTO thing (note) VALUES ('keep me')", [])
            .unwrap();

        migrate(&mut conn, "test", 2, STEPS).unwrap();

        let note: String = conn
            .query_row("SELECT note FROM thing", [], |row| row.get(0))
            .unwrap();
        assert_eq!(note, "keep me");
        assert_eq!(versioned(&conn), 2);
    }

    #[test]
    fn migrating_twice_changes_nothing() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, "test", 2, STEPS).unwrap();
        migrate(&mut conn, "test", 2, STEPS).unwrap();
        assert_eq!(versioned(&conn), 2);
    }

    /// The event log holds other people's messages, so it must not be
    /// readable by every account on the machine.
    #[cfg(unix)]
    #[test]
    fn the_database_and_its_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("events.db");
        let pool = open_pool(&path).unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute_batch("CREATE TABLE t(a); INSERT INTO t VALUES ('x');")
                .unwrap();
        }

        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600, "the log itself must be private");
        assert_eq!(
            mode(path.parent().unwrap()),
            0o700,
            "and so must the directory holding it"
        );
    }

    /// An older build must not silently truncate a newer file. The cache can
    /// rebuild; this cannot.
    #[test]
    fn a_newer_database_is_refused_rather_than_rebuilt() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, "test", 2, STEPS).unwrap();

        let err = migrate(&mut conn, "test", 1, &STEPS[..1]).unwrap_err();
        assert!(err.to_string().contains("newer slack-cli"), "{err}");

        conn.query_row("SELECT count(*) FROM thing", [], |row| row.get::<_, i64>(0))
            .expect("the table must still be there");
    }
}
