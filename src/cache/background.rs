use super::sqlite_cache::SqliteCache;
use crate::slack::SlackClient;
use tracing::{debug, warn};

impl SqliteCache {
    pub async fn try_background_refresh(&self, slack: &SlackClient) {
        let _ = self.mark_refresh_attempted();

        if self.try_acquire_lock("users_update").unwrap_or(false) {
            match self.refresh_users_internal(slack).await {
                Ok(_) => debug!("Background user refresh completed"),
                Err(e) => warn!("Background user refresh failed: {}", e),
            }
            let _ = self.release_lock("users_update").await;
        }

        if self.try_acquire_lock("channels_update").unwrap_or(false) {
            match self.refresh_channels_internal(slack).await {
                Ok(_) => debug!("Background channel refresh completed"),
                Err(e) => warn!("Background channel refresh failed: {}", e),
            }
            let _ = self.release_lock("channels_update").await;
        }
    }

    async fn refresh_users_internal(&self, slack: &SlackClient) -> anyhow::Result<()> {
        let users = slack.users.fetch_all_users().await?;
        self.save_users_internal(users)?;
        Ok(())
    }

    async fn refresh_channels_internal(&self, slack: &SlackClient) -> anyhow::Result<()> {
        let channels = slack.channels.fetch_all_channels().await?;
        self.save_channels_internal(channels)?;
        Ok(())
    }
}
