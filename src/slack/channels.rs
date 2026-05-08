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

    /// Fetch all channels from the workspace.
    ///
    /// Conversation types are driven by `cache.channel_types`. Prefers a user
    /// token (when present) so private channels the caller belongs to are
    /// included.
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

        let types = self
            .core
            .config
            .cache
            .channel_types
            .iter()
            .map(|t| t.as_api_str())
            .collect::<Vec<_>>()
            .join(",");

        loop {
            let mut params = json!({
                "limit": limit,
                "types": types,
                "exclude_archived": false,
            });

            if let Some(cursor_val) = &cursor {
                params["cursor"] = json!(cursor_val);
            }

            let mut response = self.core.api_call("conversations.list", params).await?;

            // Parse channels from response
            let raw_channels = response
                .get_mut("channels")
                .and_then(|v| v.as_array_mut())
                .map(std::mem::take)
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing channels in conversations.list response")
                })?;

            let page_channels = raw_channels
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<SlackChannel>, _>>()?;

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

    pub async fn list_members(&self, channel: &str) -> Result<Vec<String>> {
        let mut all_members = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut params = json!({
                "channel": channel,
                "limit": 1000,
            });

            if let Some(cursor) = &cursor {
                params["cursor"] = json!(cursor);
            }

            let response = self.core.api_call("conversations.members", params).await?;

            let members = response
                .get("members")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing members in conversations.members response")
                })?;

            all_members.extend(
                members
                    .iter()
                    .map(|m| {
                        m.as_str()
                            .map(ToOwned::to_owned)
                            .ok_or_else(|| anyhow::anyhow!("Invalid member id in response"))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );

            cursor = response["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(ToOwned::to_owned);

            if cursor.is_none() {
                break;
            }
        }

        Ok(all_members)
    }
}
