use super::error::{CacheError, CacheResult};
use super::sqlite_cache::SqliteCache;
use crate::slack::types::SlackChannel;
use chrono::Utc;
use rusqlite::params;

#[allow(unused_imports)]
use rusqlite::OptionalExtension;

impl SqliteCache {
    pub async fn save_channels(&self, channels: Vec<SlackChannel>) -> CacheResult<()> {
        if channels.is_empty() {
            return Err(CacheError::InvalidInput("No channels to save".to_string()));
        }

        self.with_lock("channels_update", || self.save_channels_internal(channels))
            .await
    }

    pub(super) fn save_channels_internal(&self, channels: Vec<SlackChannel>) -> CacheResult<()> {
        if channels.is_empty() {
            return Err(CacheError::InvalidInput("No channels to save".to_string()));
        }

        let conn = self.pool.get()?;

        conn.execute(
            "CREATE TEMP TABLE IF NOT EXISTS channels_new (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at INTEGER DEFAULT (unixepoch())
            )",
            [],
        )?;

        conn.execute("DELETE FROM channels_new", [])?;

        let tx = conn.unchecked_transaction()?;
        let mut successful_count = 0;

        for channel in channels {
            if let Ok(json) = serde_json::to_string(&channel)
                && tx
                    .execute(
                        "INSERT INTO channels_new (id, data) VALUES (?, ?)",
                        params![&channel.id, json],
                    )
                    .is_ok()
            {
                successful_count += 1;
            }
        }

        if successful_count == 0 {
            return Err(CacheError::InvalidInput(
                "Failed to save any channels".to_string(),
            ));
        }

        tx.execute("DELETE FROM channels", [])?;
        tx.execute(
            "INSERT INTO channels (id, data, updated_at) SELECT id, data, updated_at FROM channels_new",
            [],
        )?;
        tx.execute("DELETE FROM channels_new", [])?;

        let now = Utc::now().timestamp();
        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('last_channel_sync', ?)",
            params![now.to_string()],
        )?;

        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn get_channels(&self) -> CacheResult<Vec<SlackChannel>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(
            "SELECT data FROM channels WHERE is_archived = 0 OR is_archived IS NULL ORDER BY name",
        )?;

        let channels = stmt
            .query_map([], |row| {
                let json: String = row.get(0)?;
                serde_json::from_str(&json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(channels)
    }

    pub fn search_channels(&self, query: &str, limit: usize) -> CacheResult<Vec<SlackChannel>> {
        let conn = self.pool.get()?;

        if query.trim().is_empty() {
            let mut stmt = conn.prepare_cached(
                "SELECT data FROM channels
                 WHERE (is_archived = 0 OR is_archived IS NULL)
                 ORDER BY name
                 LIMIT ?1",
            )?;

            let channels = stmt
                .query_map(params![limit], |row| {
                    let json: String = row.get(0)?;
                    serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            return Ok(channels);
        }

        let like_pattern = format!("%{query}%");
        let like_result = conn
            .prepare_cached(
                "SELECT data,
                CASE
                    WHEN lower(name) = lower(?1) THEN 0
                    ELSE 1
                END as priority
             FROM channels
             WHERE (is_archived = 0 OR is_archived IS NULL)
             AND name LIKE ?2
             ORDER BY priority, name
             LIMIT ?3",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![query, like_pattern, limit], |row| {
                    let json: String = row.get(0)?;
                    serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
            })?;

        if !like_result.is_empty() {
            return Ok(like_result);
        }

        let processed_query = self.process_fts_query(query);
        if processed_query.is_empty() {
            return Ok(vec![]);
        }

        let fts_result = conn
            .prepare_cached(
                "SELECT c.data
             FROM channels c
             JOIN channels_fts f ON c.rowid = f.rowid
             WHERE channels_fts MATCH ?1
             AND (c.is_archived = 0 OR c.is_archived IS NULL)
             ORDER BY rank
             LIMIT ?2",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![processed_query, limit], |row| {
                    let json: String = row.get(0)?;
                    serde_json::from_str(&json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
            })?;

        Ok(fts_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn create_test_channel(
        id: &str,
        name: &str,
        is_private: bool,
        is_archived: bool,
        is_im: bool,
        is_mpim: bool,
    ) -> SlackChannel {
        SlackChannel {
            id: id.to_string(),
            name: name.to_string(),
            is_channel: !is_im && !is_mpim,
            is_im,
            is_mpim,
            is_group: false,
            is_private,
            is_archived,
            is_general: name == "general",
            is_member: true,
            created: None,
            creator: None,
            topic: None,
            purpose: None,
            num_members: Some(10),
        }
    }

    async fn setup_cache() -> SqliteCache {
        SqliteCache::new(":memory:")
            .await
            .expect("Failed to create test cache")
    }

    #[tokio::test]
    async fn test_save_channels_empty_vec() {
        let cache = setup_cache().await;
        let result = cache.save_channels(vec![]).await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid input: No channels to save"
        );
    }

    #[tokio::test]
    async fn test_save_channels_single_channel() {
        let cache = setup_cache().await;
        let channel = create_test_channel("C123", "general", false, false, false, false);

        let result = cache.save_channels(vec![channel.clone()]).await;
        assert!(result.is_ok());

        let channels = cache.get_channels().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, "C123");
        assert_eq!(channels[0].name, "general");
    }

    #[tokio::test]
    async fn test_save_channels_multiple_channels() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("C456", "random", false, false, false, false),
            create_test_channel("G789", "private-team", true, false, false, false),
        ];

        let result = cache.save_channels(channels).await;
        assert!(result.is_ok());

        let all_channels = cache.get_channels().unwrap();
        assert_eq!(all_channels.len(), 3);
    }

    #[tokio::test]
    async fn test_save_channels_replaces_existing() {
        let cache = setup_cache().await;

        let channels_v1 = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("C456", "random", false, false, false, false),
        ];
        cache.save_channels(channels_v1).await.unwrap();

        let channels_v2 = vec![
            create_test_channel("C123", "general-updated", false, false, false, false),
            create_test_channel("C789", "announcements", false, false, false, false),
        ];
        cache.save_channels(channels_v2).await.unwrap();

        let all_channels = cache.get_channels().unwrap();
        assert_eq!(all_channels.len(), 2);

        let general = all_channels.iter().find(|c| c.id == "C123").unwrap();
        assert_eq!(general.name, "general-updated");

        assert!(all_channels.iter().all(|c| c.id != "C456"));
    }

    #[tokio::test]
    async fn test_get_channels_filters_archived() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("C456", "old-project", false, true, false, false),
            create_test_channel("C789", "active", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let active_channels = cache.get_channels().unwrap();
        assert_eq!(active_channels.len(), 2);
        assert!(active_channels.iter().all(|c| !c.is_archived));
    }

    #[tokio::test]
    async fn test_get_channels_sorted_by_name() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "zebra", false, false, false, false),
            create_test_channel("C456", "alpha", false, false, false, false),
            create_test_channel("C789", "beta", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let sorted_channels = cache.get_channels().unwrap();
        assert_eq!(sorted_channels.len(), 3);
        assert_eq!(sorted_channels[0].name, "alpha");
        assert_eq!(sorted_channels[1].name, "beta");
        assert_eq!(sorted_channels[2].name, "zebra");
    }

    #[tokio::test]
    async fn test_get_channels_includes_private() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "public", false, false, false, false),
            create_test_channel("G456", "private", true, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let all_channels = cache.get_channels().unwrap();
        assert_eq!(all_channels.len(), 2);
    }

    #[tokio::test]
    async fn test_get_channels_includes_dms() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("D456", "dm-alice", false, false, true, false),
            create_test_channel("G789", "mpdm-team", false, false, false, true),
        ];
        cache.save_channels(channels).await.unwrap();

        let all_channels = cache.get_channels().unwrap();
        assert_eq!(all_channels.len(), 3);
    }

    #[rstest]
    #[case("general", 1)]
    #[case("random", 1)]
    #[case("nonexistent", 0)]
    #[tokio::test]
    async fn test_search_channels_by_name(#[case] query: &str, #[case] expected_count: usize) {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("C456", "random", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels(query, 10).unwrap();
        assert_eq!(results.len(), expected_count);
    }

    #[tokio::test]
    async fn test_search_channels_empty_query() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("C456", "random", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_channels_with_limit() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "alpha", false, false, false, false),
            create_test_channel("C456", "beta", false, false, false, false),
            create_test_channel("C789", "gamma", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("", 2).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_channels_filters_archived() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "active", false, false, false, false),
            create_test_channel("C456", "archived-test", false, true, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("test", 10).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_search_channels_includes_private() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "public-channel", false, false, false, false),
            create_test_channel("G456", "private-channel", true, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("channel", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_channels_with_special_chars() {
        let cache = setup_cache().await;
        let channels = vec![create_test_channel(
            "C123", "general", false, false, false, false,
        )];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("general*@#$", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "general");
    }

    #[tokio::test]
    async fn test_search_channels_case_sensitivity() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "General", false, false, false, false),
            create_test_channel("C456", "RANDOM", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("general", 10).unwrap();
        assert_eq!(results.len(), 1);

        let results = cache.search_channels("random", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_save_channels() {
        let cache = setup_cache().await;

        let cache1 = cache.clone();
        let cache2 = cache.clone();

        let handle1 = tokio::spawn(async move {
            let channels = vec![create_test_channel(
                "C123", "general", false, false, false, false,
            )];
            cache1.save_channels(channels).await
        });

        let handle2 = tokio::spawn(async move {
            let channels = vec![create_test_channel(
                "C456", "random", false, false, false, false,
            )];
            cache2.save_channels(channels).await
        });

        let result1 = handle1.await.unwrap();
        let result2 = handle2.await.unwrap();

        let success_count = [&result1, &result2].iter().filter(|r| r.is_ok()).count();

        assert!(
            success_count >= 1,
            "At least one concurrent save should succeed. Results: {:?}, {:?}",
            result1,
            result2
        );

        let all_channels = cache.get_channels().unwrap();
        assert!(
            !all_channels.is_empty(),
            "Should have channels from successful save"
        );
    }

    #[tokio::test]
    async fn test_channel_types_preserved() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "public", false, false, false, false),
            create_test_channel("G456", "private", true, false, false, false),
            create_test_channel("D789", "dm", false, false, true, false),
            create_test_channel("G999", "mpdm", false, false, false, true),
        ];
        cache.save_channels(channels).await.unwrap();

        let all_channels = cache.get_channels().unwrap();
        assert_eq!(all_channels.len(), 4);

        let public = all_channels.iter().find(|c| c.id == "C123").unwrap();
        assert!(public.is_channel);
        assert!(!public.is_private);
        assert!(!public.is_im);
        assert!(!public.is_mpim);

        let private = all_channels.iter().find(|c| c.id == "G456").unwrap();
        assert!(private.is_private);
        assert!(private.is_channel);
        assert!(!private.is_im);
        assert!(!private.is_mpim);

        let dm = all_channels.iter().find(|c| c.id == "D789").unwrap();
        assert!(dm.is_im);
        assert!(!dm.is_channel);
        assert!(!dm.is_mpim);

        let mpdm = all_channels.iter().find(|c| c.id == "G999").unwrap();
        assert!(mpdm.is_mpim);
        assert!(!mpdm.is_channel);
        assert!(!mpdm.is_im);
    }

    #[tokio::test]
    async fn test_search_channels_exact_match_priority() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "general", false, false, false, false),
            create_test_channel("C456", "general-korea", false, false, false, false),
            create_test_channel("C789", "general-dev", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("general", 10).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].name, "general");
    }

    #[tokio::test]
    async fn test_search_channels_name_before_topic() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "dev-team", false, false, false, false),
            create_test_channel("C456", "dev-backend", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("dev", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|c| c.name == "dev-team"));
        assert!(results.iter().any(|c| c.name == "dev-backend"));
    }

    #[tokio::test]
    async fn test_search_channels_fallback_to_fts5() {
        let cache = setup_cache().await;
        let channels = vec![
            create_test_channel("C123", "alpha", false, false, false, false),
            create_test_channel("C456", "beta", false, false, false, false),
        ];
        cache.save_channels(channels).await.unwrap();

        let results = cache.search_channels("xyz", 10).unwrap();
        assert_eq!(results.len(), 0);
    }
}
