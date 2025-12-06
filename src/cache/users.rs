use super::error::{CacheError, CacheResult};
use super::sqlite_cache::SqliteCache;
use crate::slack::types::SlackUser;
use chrono::Utc;
use rusqlite::params;

#[allow(unused_imports)]
use rusqlite::OptionalExtension;

impl SqliteCache {
    pub async fn save_users(&self, users: Vec<SlackUser>) -> CacheResult<()> {
        if users.is_empty() {
            return Err(CacheError::InvalidInput("No users to save".to_string()));
        }

        self.with_lock("users_update", || self.save_users_internal(users))
            .await
    }

    pub(super) fn save_users_internal(&self, users: Vec<SlackUser>) -> CacheResult<()> {
        if users.is_empty() {
            return Err(CacheError::InvalidInput("No users to save".to_string()));
        }

        let conn = self.pool.get()?;

        conn.execute(
            "CREATE TEMP TABLE IF NOT EXISTS users_new (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at INTEGER DEFAULT (unixepoch())
            )",
            [],
        )?;

        conn.execute("DELETE FROM users_new", [])?;

        let tx = conn.unchecked_transaction()?;
        let mut successful_count = 0;

        for user in users {
            if let Ok(json) = serde_json::to_string(&user)
                && tx
                    .execute(
                        "INSERT INTO users_new (id, data) VALUES (?, ?)",
                        params![&user.id, json],
                    )
                    .is_ok()
            {
                successful_count += 1;
            }
        }

        if successful_count == 0 {
            return Err(CacheError::InvalidInput(
                "Failed to save any users".to_string(),
            ));
        }

        tx.execute("DELETE FROM users", [])?;
        tx.execute(
            "INSERT INTO users (id, data, updated_at) SELECT id, data, updated_at FROM users_new",
            [],
        )?;
        tx.execute("DELETE FROM users_new", [])?;

        let now = Utc::now().timestamp();
        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('last_user_sync', ?)",
            params![now.to_string()],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn get_users(&self) -> CacheResult<Vec<SlackUser>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(
            "SELECT data FROM users WHERE is_bot = 0 OR is_bot IS NULL ORDER BY name",
        )?;

        let users = stmt
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

        Ok(users)
    }

    #[cfg(test)]
    pub fn get_user_by_id(&self, id: &str) -> CacheResult<Option<SlackUser>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached("SELECT data FROM users WHERE id = ?")?;

        let user = stmt
            .query_row([id], |row| {
                let json: String = row.get(0)?;
                serde_json::from_str(&json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .optional()?;

        Ok(user)
    }

    pub fn search_users(
        &self,
        query: &str,
        limit: usize,
        include_bots: bool,
    ) -> CacheResult<Vec<SlackUser>> {
        let conn = self.pool.get()?;

        if query.trim().is_empty() {
            let bot_filter = if include_bots {
                ""
            } else {
                "WHERE is_bot = 0 OR is_bot IS NULL"
            };
            let sql = format!(
                "SELECT data FROM users {} ORDER BY name LIMIT ?1",
                bot_filter
            );
            let mut stmt = conn.prepare_cached(&sql)?;

            let users = stmt
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

            return Ok(users);
        }

        let bot_filter = if include_bots {
            ""
        } else {
            "AND (is_bot = 0 OR is_bot IS NULL)"
        };

        let like_sql = format!(
            "SELECT data,
                CASE
                    WHEN lower(name) = lower(?1) THEN 0
                    WHEN lower(display_name) = lower(?1) THEN 1
                    WHEN lower(real_name) = lower(?1) THEN 2
                    ELSE 3
                END as priority
             FROM users
             WHERE 1=1 {}
             AND (name LIKE ?2 OR display_name LIKE ?2 OR real_name LIKE ?2 OR email LIKE ?2)
             ORDER BY priority, name
             LIMIT ?3",
            bot_filter
        );

        let like_pattern = format!("%{query}%");
        let like_result = conn.prepare_cached(&like_sql).and_then(|mut stmt| {
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

        let fts_sql = format!(
            "SELECT u.data
             FROM users u
             JOIN users_fts f ON u.rowid = f.rowid
             WHERE users_fts MATCH ?1
             {}
             ORDER BY rank
             LIMIT ?2",
            bot_filter
        );

        let fts_result = conn.prepare_cached(&fts_sql).and_then(|mut stmt| {
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
    use crate::slack::types::SlackUserProfile;
    use rstest::rstest;

    fn create_test_user(id: &str, name: &str, email: Option<&str>, is_bot: bool) -> SlackUser {
        SlackUser {
            id: id.to_string(),
            name: name.to_string(),
            is_bot,
            is_admin: false,
            deleted: false,
            profile: Some(SlackUserProfile {
                real_name: Some(format!("Real {name}")),
                display_name: Some(name.to_string()),
                email: email.map(std::string::ToString::to_string),
                status_text: None,
                status_emoji: None,
            }),
        }
    }

    async fn setup_cache() -> SqliteCache {
        SqliteCache::new(":memory:")
            .await
            .expect("Failed to create test cache")
    }

    #[tokio::test]
    async fn test_save_users_empty_vec() {
        let cache = setup_cache().await;
        let result = cache.save_users(vec![]).await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid input: No users to save"
        );
    }

    #[tokio::test]
    async fn test_save_users_single_user() {
        let cache = setup_cache().await;
        let user = create_test_user("U123", "alice", Some("alice@example.com"), false);

        let result = cache.save_users(vec![user.clone()]).await;
        assert!(result.is_ok());

        let retrieved = cache.get_user_by_id("U123").unwrap();
        assert!(retrieved.is_some());
        let retrieved_user = retrieved.unwrap();
        assert_eq!(retrieved_user.id, "U123");
        assert_eq!(retrieved_user.name, "alice");
    }

    #[tokio::test]
    async fn test_save_users_multiple_users() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "bob", Some("bob@example.com"), false),
            create_test_user("U789", "charlie", Some("charlie@example.com"), false),
        ];

        let result = cache.save_users(users).await;
        assert!(result.is_ok());

        let all_users = cache.get_users().unwrap();
        assert_eq!(all_users.len(), 3);
    }

    #[tokio::test]
    async fn test_save_users_replaces_existing() {
        let cache = setup_cache().await;

        let users_v1 = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "bob", Some("bob@example.com"), false),
        ];
        cache.save_users(users_v1).await.unwrap();

        let users_v2 = vec![
            create_test_user(
                "U123",
                "alice_updated",
                Some("alice.new@example.com"),
                false,
            ),
            create_test_user("U789", "charlie", Some("charlie@example.com"), false),
        ];
        cache.save_users(users_v2).await.unwrap();

        let all_users = cache.get_users().unwrap();
        assert_eq!(all_users.len(), 2);

        let alice = cache.get_user_by_id("U123").unwrap().unwrap();
        assert_eq!(alice.name, "alice_updated");

        let bob = cache.get_user_by_id("U456").unwrap();
        assert!(bob.is_none());
    }

    #[tokio::test]
    async fn test_get_users_filters_bots() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("B456", "slackbot", None, true),
            create_test_user("U789", "charlie", Some("charlie@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let human_users = cache.get_users().unwrap();
        assert_eq!(human_users.len(), 2);
        assert!(human_users.iter().all(|u| !u.is_bot));
    }

    #[tokio::test]
    async fn test_get_users_sorted_by_name() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "charlie", Some("charlie@example.com"), false),
            create_test_user("U456", "alice", Some("alice@example.com"), false),
            create_test_user("U789", "bob", Some("bob@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let sorted_users = cache.get_users().unwrap();
        assert_eq!(sorted_users.len(), 3);
        assert_eq!(sorted_users[0].name, "alice");
        assert_eq!(sorted_users[1].name, "bob");
        assert_eq!(sorted_users[2].name, "charlie");
    }

    #[tokio::test]
    async fn test_get_user_by_id_found() {
        let cache = setup_cache().await;
        let user = create_test_user("U123", "alice", Some("alice@example.com"), false);
        cache.save_users(vec![user]).await.unwrap();

        let result = cache.get_user_by_id("U123").unwrap();
        assert!(result.is_some());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.id, "U123");
        assert_eq!(retrieved.name, "alice");
    }

    #[tokio::test]
    async fn test_get_user_by_id_not_found() {
        let cache = setup_cache().await;
        let result = cache.get_user_by_id("U999").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_user_by_id_with_bot() {
        let cache = setup_cache().await;
        let bot = create_test_user("B123", "slackbot", None, true);
        cache.save_users(vec![bot]).await.unwrap();

        let result = cache.get_user_by_id("B123").unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().is_bot);
    }

    #[rstest]
    #[case("alice", 1)]
    #[case("bob", 1)]
    #[case("test", 0)]
    #[tokio::test]
    async fn test_search_users_by_name(#[case] query: &str, #[case] expected_count: usize) {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "bob", Some("bob@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users(query, 10, false).unwrap();
        assert_eq!(results.len(), expected_count);
    }

    #[tokio::test]
    async fn test_search_users_by_email() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "bob", Some("bob@company.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("example.com", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "alice");
    }

    #[tokio::test]
    async fn test_search_users_empty_query() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "bob", Some("bob@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("", 10, false).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_users_with_limit() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "bob", Some("bob@example.com"), false),
            create_test_user("U789", "charlie", Some("charlie@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("", 2, false).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_users_filters_bots() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("B456", "testbot", None, true),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("test", 10, false).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_search_users_fts5_with_special_chars() {
        let cache = setup_cache().await;
        let users = vec![create_test_user(
            "U123",
            "alice",
            Some("alice@example.com"),
            false,
        )];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("alice*@#$", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "alice");
    }

    #[tokio::test]
    async fn test_search_users_case_sensitivity() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "Alice", Some("alice@example.com"), false),
            create_test_user("U456", "BOB", Some("bob@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("alice", 10, false).unwrap();
        assert_eq!(results.len(), 1);

        let results = cache.search_users("bob", 10, false).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_search_users_exact_match_priority() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "john", Some("john@example.com"), false),
            create_test_user("U456", "john.smith", Some("john.smith@example.com"), false),
            create_test_user("U789", "johnny", Some("johnny@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("john", 10, false).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].name, "john");
    }

    #[tokio::test]
    async fn test_search_users_with_include_bots() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("B456", "testbot", None, true),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("test", 10, true).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "testbot");
        assert!(results[0].is_bot);
    }

    #[tokio::test]
    async fn test_search_users_substring_before_fuzzy() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "junyeong.eom", Some("junyeong@example.com"), false),
            create_test_user(
                "U456",
                "seungryoung.lee",
                Some("seungryoung@example.com"),
                false,
            ),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("junyeong", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "junyeong.eom");
    }

    #[tokio::test]
    async fn test_search_users_fallback_to_fts5() {
        let cache = setup_cache().await;
        let users = vec![
            create_test_user("U123", "alice", Some("alice@example.com"), false),
            create_test_user("U456", "alicia", Some("alicia@example.com"), false),
        ];
        cache.save_users(users).await.unwrap();

        let results = cache.search_users("xyz", 10, false).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_save_users() {
        let cache = setup_cache().await;

        let cache1 = cache.clone();
        let cache2 = cache.clone();

        let handle1 = tokio::spawn(async move {
            let users = vec![create_test_user(
                "U123",
                "alice",
                Some("alice@example.com"),
                false,
            )];
            cache1.save_users(users).await
        });

        let handle2 = tokio::spawn(async move {
            let users = vec![create_test_user(
                "U456",
                "bob",
                Some("bob@example.com"),
                false,
            )];
            cache2.save_users(users).await
        });

        let result1 = handle1.await.unwrap();
        let result2 = handle2.await.unwrap();

        assert!(result1.is_ok() || result2.is_ok());
    }
}
