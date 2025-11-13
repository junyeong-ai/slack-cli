use std::path::Path;
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::error::CacheResult;
use super::schema;

#[derive(Debug, Clone)]
pub struct SqliteCache {
    pub(super) pool: Pool<SqliteConnectionManager>,
    pub(super) instance_id: String,
}

impl SqliteCache {
    pub async fn new(path: impl AsRef<Path>) -> CacheResult<Self> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(path).with_init(|conn| {
            // Enable WAL mode for better concurrency
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = NORMAL;
                     PRAGMA foreign_keys = ON;
                     PRAGMA busy_timeout = 5000;
                     PRAGMA cache_size = -64000;", // 64MB cache
            )?;
            Ok(())
        });

        let pool = Pool::builder()
            .max_size(10)
            .min_idle(Some(2))
            .connection_timeout(Duration::from_secs(5))
            .build(manager)?;

        let instance_id = uuid::Uuid::new_v4().to_string();

        let cache = Self { pool, instance_id };

        schema::initialize_schema(&cache.pool).await?;
        Ok(cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_sqlite_cache_in_memory() {
        let result = SqliteCache::new(":memory:").await;
        assert!(result.is_ok());

        let cache = result.unwrap();
        assert!(!cache.instance_id.is_empty());
        assert!(uuid::Uuid::parse_str(&cache.instance_id).is_ok());
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
        assert!(tables.contains(&"locks".to_string()));
        assert!(tables.contains(&"metadata".to_string()));
    }

    #[tokio::test]
    async fn test_unique_instance_ids() {
        let cache1 = SqliteCache::new(":memory:").await.unwrap();
        let cache2 = SqliteCache::new(":memory:").await.unwrap();

        assert_ne!(cache1.instance_id, cache2.instance_id);
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
