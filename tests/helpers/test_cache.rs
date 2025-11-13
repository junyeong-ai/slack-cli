use mcp_slack::cache::SqliteCache;
use mcp_slack::slack::types::{SlackUser, SlackChannel};
use std::collections::HashMap;

/// Builder for creating test cache with in-memory SQLite database
pub struct TestCacheBuilder {
    seed_users: Vec<SlackUser>,
    seed_channels: Vec<SlackChannel>,
    seed_memberships: HashMap<String, Vec<String>>,
}

impl TestCacheBuilder {
    /// Create empty test cache builder
    pub fn new() -> Self {
        Self {
            seed_users: Vec::new(),
            seed_channels: Vec::new(),
            seed_memberships: HashMap::new(),
        }
    }

    /// Seed with users
    pub fn with_users(mut self, users: Vec<SlackUser>) -> Self {
        self.seed_users = users;
        self
    }

    /// Seed with channels
    pub fn with_channels(mut self, channels: Vec<SlackChannel>) -> Self {
        self.seed_channels = channels;
        self
    }

    /// Seed with channel memberships
    pub fn with_memberships(mut self, channel_id: impl Into<String>, user_ids: Vec<String>) -> Self {
        self.seed_memberships.insert(channel_id.into(), user_ids);
        self
    }

    /// Build cache with in-memory database
    pub async fn build(self) -> anyhow::Result<SqliteCache> {
        // Create in-memory SQLite database
        let cache = SqliteCache::new(":memory:").await?;

        // Seed users if provided
        if !self.seed_users.is_empty() {
            cache.save_users(&self.seed_users).await?;
        }

        // Seed channels if provided
        if !self.seed_channels.is_empty() {
            cache.save_channels(&self.seed_channels).await?;
        }

        // Note: Membership seeding would require save_memberships method
        // For now, this is a placeholder for when that method exists

        Ok(cache)
    }
}

impl Default for TestCacheBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{MockUserBuilder, MockChannelBuilder};

    #[tokio::test]
    async fn test_cache_builder_empty() {
        let cache = TestCacheBuilder::new().build().await.unwrap();
        // Cache should be created successfully
        assert!(cache.is_empty().await.unwrap());
    }

    #[tokio::test]
    async fn test_cache_builder_with_users() {
        let user = MockUserBuilder::new().id("U123").build();
        let cache = TestCacheBuilder::new()
            .with_users(vec![user.clone()])
            .build()
            .await
            .unwrap();

        let retrieved = cache.get_user("U123").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "U123");
    }

    #[tokio::test]
    async fn test_cache_builder_with_channels() {
        let channel = MockChannelBuilder::new().id("C123").name("test").build();
        let cache = TestCacheBuilder::new()
            .with_channels(vec![channel.clone()])
            .build()
            .await
            .unwrap();

        let retrieved = cache.get_channel("C123").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "C123");
    }
}
