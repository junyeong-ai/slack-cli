use super::constants::{MIN_REFRESH_INTERVAL_SECS, REFRESH_THRESHOLD_PERCENT};
use super::error::{CacheError, CacheResult};
use super::sqlite_cache::SqliteCache;
use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    Empty,
    Fresh,
    NeedsRefresh,
}

impl SqliteCache {
    pub(super) fn process_fts_query(&self, query: &str) -> String {
        let trimmed = query.trim();

        if trimmed.is_empty() || trimmed == "*" || trimmed == "%" {
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

    pub fn get_cache_status(&self, ttl_hours: u64) -> CacheResult<CacheStatus> {
        if self.is_cache_empty()? {
            return Ok(CacheStatus::Empty);
        }

        let age_hours = self.get_cache_age_hours()?;
        let threshold_hours = (ttl_hours * REFRESH_THRESHOLD_PERCENT / 100) as f64;

        if age_hours >= threshold_hours {
            Ok(CacheStatus::NeedsRefresh)
        } else {
            Ok(CacheStatus::Fresh)
        }
    }

    fn get_cache_age_hours(&self) -> CacheResult<f64> {
        let conn = self.pool.get()?;

        let last_sync: Option<i64> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'last_user_sync'",
                [],
                |row| row.get(0),
            )
            .ok();

        match last_sync {
            Some(ts) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(CacheError::SystemTimeError)?
                    .as_secs() as i64;
                Ok((now - ts) as f64 / 3600.0)
            }
            None => Ok(f64::MAX),
        }
    }

    pub fn is_within_refresh_cooldown(&self) -> CacheResult<bool> {
        let conn = self.pool.get()?;

        let last_attempt: Option<i64> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'last_refresh_attempt'",
                [],
                |row| row.get(0),
            )
            .ok();

        match last_attempt {
            Some(ts) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(CacheError::SystemTimeError)?
                    .as_secs() as i64;
                Ok(now - ts < MIN_REFRESH_INTERVAL_SECS)
            }
            None => Ok(false),
        }
    }

    pub fn mark_refresh_attempted(&self) -> CacheResult<()> {
        let conn = self.pool.get()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(CacheError::SystemTimeError)?
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('last_refresh_attempt', ?)",
            params![now.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_cache() -> SqliteCache {
        SqliteCache::new_sync(":memory:").unwrap()
    }

    mod process_fts_query_tests {
        use super::*;

        #[test]
        fn quotes_simple_query() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query("test"), "\"test\"");
        }

        #[test]
        fn trims_whitespace() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query("  test  "), "\"test\"");
        }

        #[test]
        fn returns_empty_for_empty_input() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query(""), "");
            assert_eq!(cache.process_fts_query("   "), "");
        }

        #[test]
        fn returns_empty_for_wildcard_only() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query("*"), "");
            assert_eq!(cache.process_fts_query("%"), "");
        }

        #[test]
        fn strips_wildcards() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query("test*"), "\"test\"");
            assert_eq!(cache.process_fts_query("*test*"), "\"test\"");
            assert_eq!(cache.process_fts_query("te%st"), "\"test\"");
        }

        #[test]
        fn escapes_quotes() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query("test\"query"), "\"test\"\"query\"");
            assert_eq!(cache.process_fts_query("\"quoted\""), "\"\"\"quoted\"\"\"",);
        }

        #[test]
        fn handles_special_chars() {
            let cache = create_test_cache();
            assert_eq!(
                cache.process_fts_query("test@email.com"),
                "\"test@email.com\""
            );
            assert_eq!(cache.process_fts_query("john.doe"), "\"john.doe\"");
        }

        #[test]
        fn returns_empty_if_only_wildcards() {
            let cache = create_test_cache();
            assert_eq!(cache.process_fts_query("***"), "");
            assert_eq!(cache.process_fts_query("%%%"), "");
            assert_eq!(cache.process_fts_query("* % *"), "");
        }
    }

    mod cache_status_tests {
        use super::*;

        #[test]
        fn empty_cache_reports_empty() {
            let cache = create_test_cache();
            assert!(cache.is_cache_empty().unwrap());
        }

        #[test]
        fn counts_are_zero_for_empty_cache() {
            let cache = create_test_cache();
            let (users, channels) = cache.get_counts().unwrap();
            assert_eq!(users, 0);
            assert_eq!(channels, 0);
        }

        #[test]
        fn empty_cache_status() {
            let cache = create_test_cache();
            assert_eq!(cache.get_cache_status(168).unwrap(), CacheStatus::Empty);
        }

        #[test]
        fn cooldown_false_when_never_attempted() {
            let cache = create_test_cache();
            assert!(!cache.is_within_refresh_cooldown().unwrap());
        }

        #[test]
        fn cooldown_true_after_recent_attempt() {
            let cache = create_test_cache();
            cache.mark_refresh_attempted().unwrap();
            assert!(cache.is_within_refresh_cooldown().unwrap());
        }
    }
}
