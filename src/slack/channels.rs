use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackChannel;

const SLACK_API_LIMIT: u32 = 200;

pub struct SlackChannelClient {
    pub(crate) core: Arc<SlackCore>,
}

impl SlackChannelClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    /// Fetch all channels from the workspace
    /// Uses user token when available to get private channels the user has access to
    pub async fn fetch_all_channels(&self) -> Result<Vec<SlackChannel>> {
        let mut all_channels = Vec::new();

        self.fetch_all_channels_streaming(|channels| {
            all_channels.extend(channels);
            Ok(())
        })
        .await?;

        Ok(all_channels)
    }

    /// Stream fetch channels with callback for immediate processing of each page
    pub async fn fetch_all_channels_streaming<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(Vec<SlackChannel>) -> Result<()>,
    {
        let mut total_fetched = 0;
        let mut cursor: Option<String> = None;
        let limit = SLACK_API_LIMIT;

        loop {
            let mut params = json!({
                "limit": limit,
                "types": "public_channel,private_channel",
                "exclude_archived": false,
            });

            if let Some(cursor_val) = &cursor {
                params["cursor"] = json!(cursor_val);
            }

            // Use user token preference to get private channels
            let mut response = self
                .core
                .api_call("conversations.list", params, None, true)
                .await?;

            // Parse channels from response
            let page_channels: Vec<SlackChannel> = response
                .get_mut("channels")
                .and_then(|v| v.as_array_mut())
                .map(std::mem::take)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|channel| serde_json::from_value::<SlackChannel>(channel).ok())
                .collect();

            // Process this page immediately via callback
            if !page_channels.is_empty() {
                let page_count = page_channels.len();
                callback(page_channels)?;
                total_fetched += page_count;
            }

            // Check for pagination
            cursor = response["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(|c| c.to_string());

            if cursor.is_none() {
                break;
            }
        }

        Ok(total_fetched)
    }
}
