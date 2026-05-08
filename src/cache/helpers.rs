use super::error::{CacheError, CacheResult};
use super::sqlite_cache::SqliteCache;
use rusqlite::OptionalExtension;
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

    pub fn get_cache_status(
        &self,
        users_ttl_hours: u64,
        channels_ttl_hours: u64,
        threshold_percent: u64,
    ) -> CacheResult<CacheStatus> {
        let (users, channels) = self.get_counts()?;
        if users == 0 && channels == 0 {
            return Ok(CacheStatus::Empty);
        }

        if users == 0 || channels == 0 {
            return Ok(CacheStatus::NeedsRefresh);
        }

        let users_status =
            self.get_cache_entry_status("last_user_sync", users_ttl_hours, threshold_percent)?;
        let channels_status = self.get_cache_entry_status(
            "last_channel_sync",
            channels_ttl_hours,
            threshold_percent,
        )?;

        if users_status == CacheStatus::NeedsRefresh || channels_status == CacheStatus::NeedsRefresh
        {
            Ok(CacheStatus::NeedsRefresh)
        } else {
            Ok(CacheStatus::Fresh)
        }
    }

    fn get_cache_entry_status(
        &self,
        metadata_key: &str,
        ttl_hours: u64,
        threshold_percent: u64,
    ) -> CacheResult<CacheStatus> {
        let age_hours = self.get_cache_age_hours(metadata_key)?;
        let threshold_hours = (ttl_hours * threshold_percent / 100) as f64;

        if age_hours >= threshold_hours {
            Ok(CacheStatus::NeedsRefresh)
        } else {
            Ok(CacheStatus::Fresh)
        }
    }

    fn get_cache_age_hours(&self, metadata_key: &str) -> CacheResult<f64> {
        let conn = self.pool.get()?;

        let last_sync: Option<i64> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = ?",
                [metadata_key],
                |row| row.get(0),
            )
            .optional()?;

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
            assert_eq!(
                cache.get_cache_status(168, 168, 10).unwrap(),
                CacheStatus::Empty
            );
        }

        #[test]
        fn partial_cache_needs_refresh() {
            let cache = create_test_cache();
            let conn = cache.pool.get().unwrap();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            conn.execute(
                "INSERT INTO users (id, data) VALUES ('U1', json('{\"id\":\"U1\",\"name\":\"user\"}'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO metadata (key, value) VALUES ('last_user_sync', ?)",
                [now],
            )
            .unwrap();

            assert_eq!(
                cache.get_cache_status(168, 168, 10).unwrap(),
                CacheStatus::NeedsRefresh
            );
        }

        #[test]
        fn fresh_users_and_channels_status() {
            let cache = create_test_cache();
            let conn = cache.pool.get().unwrap();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            conn.execute(
                "INSERT INTO users (id, data) VALUES ('U1', json('{\"id\":\"U1\",\"name\":\"user\"}'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO channels (id, data) VALUES ('C1', json('{\"id\":\"C1\",\"name\":\"channel\"}'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO metadata (key, value) VALUES ('last_user_sync', ?)",
                [now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO metadata (key, value) VALUES ('last_channel_sync', ?)",
                [now],
            )
            .unwrap();

            assert_eq!(
                cache.get_cache_status(168, 168, 10).unwrap(),
                CacheStatus::Fresh
            );
        }

        #[test]
        fn stale_channel_status_needs_refresh() {
            let cache = create_test_cache();
            let conn = cache.pool.get().unwrap();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let stale = now - (24 * 3600);

            conn.execute(
                "INSERT INTO users (id, data) VALUES ('U1', json('{\"id\":\"U1\",\"name\":\"user\"}'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO channels (id, data) VALUES ('C1', json('{\"id\":\"C1\",\"name\":\"channel\"}'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO metadata (key, value) VALUES ('last_user_sync', ?)",
                [now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO metadata (key, value) VALUES ('last_channel_sync', ?)",
                [stale],
            )
            .unwrap();

            assert_eq!(
                cache.get_cache_status(168, 168, 10).unwrap(),
                CacheStatus::NeedsRefresh
            );
        }
    }
}
