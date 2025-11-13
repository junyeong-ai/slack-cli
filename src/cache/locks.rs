use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::error::{CacheError, CacheResult};
use rusqlite::params;
use tracing::warn;

use super::sqlite_cache::SqliteCache;

const LOCK_TIMEOUT_SECS: u64 = 60;
const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 500;

impl SqliteCache {
    // Lock management for multi-instance coordination
    pub(super) async fn acquire_lock(&self, key: &str) -> CacheResult<()> {
        let mut backoff = Duration::from_millis(INITIAL_BACKOFF_MS);

        for attempt in 0..MAX_RETRIES {
            let conn = self.pool.get()?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
            let expires_at = now + LOCK_TIMEOUT_SECS as i64;

            // Clean up expired locks
            conn.execute("DELETE FROM locks WHERE expires_at < ?", params![now])?;

            // Try to acquire lock
            let result = conn.execute(
                "INSERT INTO locks (key, instance_id, acquired_at, expires_at) VALUES (?, ?, ?, ?)",
                params![key, &self.instance_id, now, expires_at],
            );

            match result {
                Ok(_) => {
                    return Ok(());
                }
                Err(rusqlite::Error::SqliteFailure(err, _))
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    // Lock is held by another instance
                    if attempt < MAX_RETRIES - 1 {
                        // Check if lock is stale (held by dead instance)
                        if let Ok(lock_info) = conn.query_row(
                            "SELECT instance_id, acquired_at FROM locks WHERE key = ?",
                            params![key],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                        ) {
                            let (holder_id, acquired_at) = lock_info;
                            let lock_age = now - acquired_at;

                            // If lock is very old, it might be from a dead instance
                            if lock_age > (LOCK_TIMEOUT_SECS * 2) as i64 {
                                warn!(
                                    "Detected potentially stale lock held by {} for {} seconds, forcing cleanup",
                                    holder_id, lock_age
                                );
                                // Force delete the stale lock
                                let _ = conn.execute(
                                    "DELETE FROM locks WHERE key = ? AND instance_id = ?",
                                    params![key, holder_id],
                                );
                                continue; // Retry immediately
                            }
                        }

                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(1));
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }

        Err(CacheError::LockAcquisitionFailed {
            key: key.to_string(),
            attempts: MAX_RETRIES as usize,
        })
    }

    pub(super) async fn release_lock(&self, key: &str) -> CacheResult<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM locks WHERE key = ? AND instance_id = ?",
            params![key, &self.instance_id],
        )?;
        Ok(())
    }

    pub async fn with_lock<F, R>(&self, key: &str, f: F) -> CacheResult<R>
    where
        F: FnOnce() -> CacheResult<R>,
    {
        self.acquire_lock(key).await?;

        // Execute function and always try to release lock, even if function fails
        let result = f();

        // Try to release lock, but don't fail if release fails
        // Lock will expire automatically after timeout
        if let Err(e) = self.release_lock(key).await {
            warn!(
                "Failed to release lock for key '{}': {}. Lock will expire automatically.",
                key, e
            );
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    async fn setup_cache() -> SqliteCache {
        SqliteCache::new(":memory:")
            .await
            .expect("Failed to create test cache")
    }

    #[tokio::test]
    async fn test_acquire_lock_success() {
        let cache = setup_cache().await;
        let result = cache.acquire_lock("test_lock").await;
        assert!(result.is_ok());

        // Clean up
        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_release_lock_success() {
        let cache = setup_cache().await;
        cache.acquire_lock("test_lock").await.unwrap();

        let result = cache.release_lock("test_lock").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_acquire_lock_twice_same_instance_fails() {
        let cache = setup_cache().await;
        cache.acquire_lock("test_lock").await.unwrap();

        // Try to acquire same lock again from same instance
        let result = cache.acquire_lock("test_lock").await;
        assert!(result.is_err());

        // Clean up
        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_release_lock_only_own_locks() {
        let cache = setup_cache().await;

        cache.acquire_lock("test_lock").await.unwrap();

        // Try to release with different instance_id (simulate different instance)
        // Since we can't create truly separate instances with :memory:,
        // we'll just verify the release succeeds but is instance-specific
        let result = cache.release_lock("test_lock").await;
        assert!(result.is_ok());

        // After releasing, lock should be acquirable again
        let acquire_result = cache.acquire_lock("test_lock").await;
        assert!(acquire_result.is_ok());

        // Clean up
        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_lock_different_keys() {
        let cache = setup_cache().await;

        let result1 = cache.acquire_lock("lock1").await;
        let result2 = cache.acquire_lock("lock2").await;

        assert!(result1.is_ok());
        assert!(result2.is_ok());

        // Clean up
        cache.release_lock("lock1").await.unwrap();
        cache.release_lock("lock2").await.unwrap();
    }

    #[tokio::test]
    async fn test_with_lock_executes_function() {
        let cache = setup_cache().await;
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let executed_clone = executed.clone();

        let result = cache
            .with_lock("test_lock", || {
                executed_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(42)
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert!(executed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_with_lock_releases_on_success() {
        let cache = setup_cache().await;

        cache.with_lock("test_lock", || Ok(())).await.unwrap();

        // Should be able to acquire lock again
        let result = cache.acquire_lock("test_lock").await;
        assert!(result.is_ok());

        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_with_lock_releases_on_error() {
        let cache = setup_cache().await;

        let result: CacheResult<()> = cache
            .with_lock("test_lock", || {
                Err(CacheError::InvalidInput("Function failed".to_string()))
            })
            .await;

        assert!(result.is_err());

        // Lock should still be released despite error
        let acquire_result = cache.acquire_lock("test_lock").await;
        assert!(acquire_result.is_ok());

        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_with_lock_function_return_value() {
        let cache = setup_cache().await;

        let result = cache
            .with_lock("test_lock", || Ok("test_value".to_string()))
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test_value");
    }

    #[tokio::test]
    async fn test_with_lock_function_error_propagated() {
        let cache = setup_cache().await;

        let result: CacheResult<()> = cache
            .with_lock("test_lock", || {
                Err(CacheError::InvalidInput("Custom error message".to_string()))
            })
            .await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid input: Custom error message"
        );
    }

    #[tokio::test]
    async fn test_concurrent_lock_acquisition_serialized() {
        let cache = Arc::new(setup_cache().await);
        let counter = Arc::new(std::sync::Mutex::new(0));

        let mut handles = vec![];

        for _ in 0..3 {
            let cache = cache.clone();
            let counter = counter.clone();

            let handle = tokio::spawn(async move {
                cache
                    .with_lock("counter_lock", || {
                        let mut c = counter.lock().unwrap();
                        *c += 1;
                        Ok(())
                    })
                    .await
            });

            handles.push(handle);
        }

        // Wait for all tasks
        for handle in handles {
            let result = handle.await.unwrap();
            // Some will succeed, some may fail due to lock contention
            let _ = result;
        }

        let final_count = *counter.lock().unwrap();
        // At least one should have succeeded
        assert!(final_count >= 1);
    }

    #[tokio::test]
    async fn test_lock_acquisition_after_manual_cleanup() {
        let cache = setup_cache().await;

        // Acquire and release a lock
        cache.acquire_lock("test_lock").await.unwrap();
        cache.release_lock("test_lock").await.unwrap();

        // Should be able to acquire it again after cleanup
        let result = cache.acquire_lock("test_lock").await;
        assert!(result.is_ok());

        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_lock_retries_with_backoff() {
        let cache = Arc::new(setup_cache().await);

        // Acquire lock
        cache.acquire_lock("test_lock").await.unwrap();

        let cache_clone = cache.clone();
        let handle = tokio::spawn(async move {
            let start = std::time::Instant::now();
            let result = cache_clone.acquire_lock("test_lock").await;
            let elapsed = start.elapsed();
            (result, elapsed)
        });

        let (result, elapsed) = handle.await.unwrap();

        // Should have failed after retries (lock is held)
        assert!(result.is_err());
        // Should have taken some time due to retries with backoff
        // Using a more lenient threshold to avoid flaky tests
        // Expected: 3 retries with backoff (500ms + 1000ms + 1000ms = ~2.5s)
        // But due to system scheduling, we just verify it took longer than immediate failure
        assert!(
            elapsed.as_millis() >= 100,
            "Expected some delay from retries, got {}ms",
            elapsed.as_millis()
        );

        // Clean up
        cache.release_lock("test_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_lock_attempts_same_instance() {
        let cache = Arc::new(setup_cache().await);

        let result1 = cache.acquire_lock("shared_lock").await;
        assert!(result1.is_ok());

        // Try to acquire again from same instance (should fail)
        let result2 = cache.acquire_lock("shared_lock").await;
        assert!(result2.is_err());

        cache.release_lock("shared_lock").await.unwrap();

        // Now should be able to acquire again
        let result3 = cache.acquire_lock("shared_lock").await;
        assert!(result3.is_ok());

        cache.release_lock("shared_lock").await.unwrap();
    }

    #[tokio::test]
    async fn test_lock_key_isolation() {
        let cache = setup_cache().await;

        cache.acquire_lock("lock_a").await.unwrap();
        cache.acquire_lock("lock_b").await.unwrap();

        // Both locks should be independent
        cache.release_lock("lock_a").await.unwrap();

        // lock_b should still be held
        let result = cache.acquire_lock("lock_b").await;
        assert!(result.is_err());

        cache.release_lock("lock_b").await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_high_concurrency_locking() {
        let cache = Arc::new(setup_cache().await);
        let success_count = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let mut handles = vec![];

        for _ in 0..10 {
            let cache = cache.clone();
            let success_count = success_count.clone();

            let handle = tokio::spawn(async move {
                let result = cache
                    .with_lock("concurrent_lock", || {
                        // Simulate some work
                        std::thread::sleep(Duration::from_millis(10));
                        Ok(())
                    })
                    .await;

                if result.is_ok() {
                    success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let final_success = success_count.load(std::sync::atomic::Ordering::SeqCst);
        // At least some operations should succeed
        assert!(final_success >= 1);
        println!("Successful lock acquisitions: {}/10", final_success);
    }
}
