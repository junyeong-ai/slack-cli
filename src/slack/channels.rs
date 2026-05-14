use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackChannel;

const PAGE_SIZE: u32 = 200;
const MEMBERS_PAGE_SIZE: u32 = 1000;

pub struct SlackChannelClient {
    pub(crate) core: Arc<SlackCore>,
}

impl SlackChannelClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn list(&self) -> Result<Vec<SlackChannel>> {
        let mut all_channels = Vec::new();
        let mut cursor: Option<String> = None;

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
                "limit": PAGE_SIZE,
                "types": types,
                "exclude_archived": false,
            });
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }

            let mut response = self.core.api_call("conversations.list", params).await?;

            let raw_channels = response
                .get_mut("channels")
                .and_then(|v| v.as_array_mut())
                .map(std::mem::take)
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing channels in conversations.list response")
                })?;

            let page_channels: Vec<SlackChannel> = raw_channels
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()?;

            all_channels.extend(page_channels);

            cursor = response["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(|c| c.to_string());

            if cursor.is_none() {
                break;
            }
        }

        Ok(all_channels)
    }

    pub async fn members(&self, channel: &str) -> Result<Vec<String>> {
        let mut all_members = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut params = json!({
                "channel": channel,
                "limit": MEMBERS_PAGE_SIZE,
            });
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
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
