use std::path::Path;
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::error::CacheResult;
use super::schema;
use crate::blocking::BlockingExecutor;

const POOL_SIZE: u32 = 10;

/// Retry budget for the one-time WAL switch: SQLite returns SQLITE_BUSY
/// without consulting the busy handler on journal-mode transitions, so a
/// concurrent process creating the same file needs a brief retry loop.
const WAL_SWITCH_MAX_ATTEMPTS: u32 = 10;
const WAL_SWITCH_RETRY_DELAY_MS: u64 = 50;

#[derive(Debug, Clone)]
pub struct SqliteCache {
    pub(super) pool: Pool<SqliteConnectionManager>,
    executor: BlockingExecutor,
    pub(super) writer: BlockingExecutor,
}

fn is_busy(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _) if matches!(
            e.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        )
    )
}

impl SqliteCache {
    pub async fn new(path: impl AsRef<Path>) -> CacheResult<Self> {
        let path = path.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || Self::open(&path)).await?
    }

    fn open(path: &Path) -> CacheResult<Self> {
        // Ensure parent directory exists (skip for in-memory databases)
        let is_memory = path == Path::new(":memory:");
        if !is_memory && let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let instance_id = uuid::Uuid::new_v4().to_string();

        // For in-memory databases with connection pooling, use shared cache mode
        // with a unique name so all connections in the pool share the same database instance,
        // but different SqliteCache instances get separate databases
        let manager = if is_memory {
            SqliteConnectionManager::file(format!("file:{}?mode=memory&cache=shared", instance_id))
        } else {
            SqliteConnectionManager::file(path)
        }
        .with_init(|conn| {
            // Connection-local pragmas only. WAL is set once in `new` — the
            // journal-mode switch takes an exclusive lock that SQLite does
            // not route through the busy handler, so concurrent per-connection
            // switches race each other and the schema setup.
            conn.execute_batch(
                "PRAGMA busy_timeout = 5000;
                     PRAGMA synchronous = NORMAL;
                     PRAGMA foreign_keys = ON;
                     PRAGMA cache_size = -64000;", // 64MB cache
            )?;
            Ok(())
        });

        let pool = Pool::builder()
            .max_size(POOL_SIZE)
            .min_idle(Some(2))
            .connection_timeout(Duration::from_secs(5))
            .build(manager)?;

        let cache = Self {
            pool,
            executor: BlockingExecutor::new(POOL_SIZE),
            writer: BlockingExecutor::new(1),
        };

        // WAL mode persists in the database file: switch it once, before any
        // other database activity, instead of per connection (skip in-memory).
        if !is_memory {
            let conn = cache.pool.get()?;
            let mut attempts = 0u32;
            while let Err(err) = conn.execute_batch("PRAGMA journal_mode = WAL;") {
                if attempts >= WAL_SWITCH_MAX_ATTEMPTS || !is_busy(&err) {
                    return Err(err.into());
                }
                attempts += 1;
                std::thread::sleep(Duration::from_millis(WAL_SWITCH_RETRY_DELAY_MS));
            }
        }

        schema::initialize_schema(&cache.pool)?;
        Ok(cache)
    }

    /// Runs a synchronous cache operation without occupying a runtime worker.
    pub async fn execute<T, F>(&self, operation: F) -> CacheResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Self) -> CacheResult<T> + Send + 'static,
    {
        let cache = self.clone();
        self.executor.execute(move || operation(&cache)).await
    }

    #[cfg(test)]
    pub fn new_sync(path: impl AsRef<Path>) -> CacheResult<Self> {
        Self::open(path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_sqlite_cache_in_memory() {
        let result = SqliteCache::new(":memory:").await;
        assert!(result.is_ok());

        assert_eq!(result.unwrap().get_counts().unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn test_new_sqlite_cache_creates_schema() {
        let cache = SqliteCache::new(":memory:").await.unwrap();
        let conn = cache.pool.get().unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"channels".to_string()));
        assert!(tables.contains(&"metadata".to_string()));
    }

    #[tokio::test]
    async fn in_memory_caches_are_isolated() {
        let cache1 = SqliteCache::new(":memory:").await.unwrap();
        let cache2 = SqliteCache::new(":memory:").await.unwrap();

        cache1
            .pool
            .get()
            .unwrap()
            .execute(
                r#"INSERT INTO users (id, data) VALUES ('U1', '{"name":"alice"}')"#,
                [],
            )
            .unwrap();
        assert_eq!(cache1.get_counts().unwrap(), (1, 0));
        assert_eq!(cache2.get_counts().unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn test_pool_configuration() {
        let cache = SqliteCache::new(":memory:").await.unwrap();

        // Verify we can get multiple connections
        let conn1 = cache.pool.get();
        let conn2 = cache.pool.get();

        assert!(conn1.is_ok());
        assert!(conn2.is_ok());
    }
}
