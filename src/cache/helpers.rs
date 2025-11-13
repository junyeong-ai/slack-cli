use super::error::CacheResult;
use super::sqlite_cache::SqliteCache;
use std::time::{SystemTime, UNIX_EPOCH};

impl SqliteCache {
    pub(super) fn process_fts_query(&self, query: &str) -> String {
        let trimmed = query.trim();

        if trimmed.is_empty() {
            return String::new();
        }

        if trimmed == "*" || trimmed == "%" {
            return String::new();
        }

        let cleaned = trimmed
            .replace("\"", "\"\"")
            .replace("*", "")
            .replace("%", "")
            .trim()
            .to_string();

        if cleaned.is_empty() {
            return String::new();
        }

        format!("\"{}\"", cleaned)
    }

    pub fn get_counts(&self) -> CacheResult<(usize, usize)> {
        let conn = self.pool.get()?;

        let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;

        let channel_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM channels", [], |row| row.get(0))?;

        Ok((user_count as usize, channel_count as usize))
    }

    pub fn is_cache_empty(&self) -> CacheResult<bool> {
        let (users, channels) = self.get_counts()?;
        Ok(users == 0 && channels == 0)
    }

    pub fn is_cache_stale(&self, ttl_hours: u64) -> CacheResult<bool> {
        let conn = self.pool.get()?;

        let last_sync: Option<i64> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'last_user_sync'
                 ORDER BY value DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();

        if let Some(last_sync_ts) = last_sync {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(super::error::CacheError::SystemTimeError)?
                .as_secs() as i64;

            let age_hours = (now - last_sync_ts) / 3600;
            Ok(age_hours > ttl_hours as i64)
        } else {
            Ok(true)
        }
    }
}
