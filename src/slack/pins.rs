use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedMessage {
    pub channel: String,
    pub ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    pub created: i64,
    pub created_by: String,
}

pub struct SlackPinClient {
    core: Arc<SlackCore>,
}

impl SlackPinClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn add(&self, channel: &str, ts: &str) -> Result<()> {
        let params = json!({
            "channel": channel,
            "timestamp": ts,
        });

        self.core.api_call("pins.add", params).await?;

        Ok(())
    }

    pub async fn remove(&self, channel: &str, ts: &str) -> Result<()> {
        let params = json!({
            "channel": channel,
            "timestamp": ts,
        });

        self.core.api_call("pins.remove", params).await?;

        Ok(())
    }

    pub async fn list(&self, channel: &str) -> Result<Vec<PinnedMessage>> {
        let params = json!({
            "channel": channel,
        });

        let response = self.core.api_call("pins.list", params).await?;

        let items = response
            .get("items")
            .and_then(|items| items.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|item| {
                        let item_type = item
                            .get("type")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| anyhow::anyhow!("Missing pinned item type"))?;
                        if item_type != "message" {
                            return Ok(None);
                        }

                        let msg = item
                            .get("message")
                            .ok_or_else(|| anyhow::anyhow!("Missing pinned message payload"))?;
                        Ok(Some(PinnedMessage {
                            channel: item
                                .get("channel")
                                .and_then(|c| c.as_str())
                                .unwrap_or(channel)
                                .to_string(),
                            ts: msg
                                .get("ts")
                                .and_then(|v| v.as_str())
                                .ok_or_else(|| anyhow::anyhow!("Missing pinned message timestamp"))?
                                .to_string(),
                            text: msg.get("text").and_then(|t| t.as_str()).map(String::from),
                            user: msg.get("user").and_then(|u| u.as_str()).map(String::from),
                            created: item.get("created").and_then(|c| c.as_i64()).unwrap_or(0),
                            created_by: item
                                .get("created_by")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string(),
                        }))
                    })
                    .collect::<Result<Vec<_>>>()
                    .map(|items| items.into_iter().flatten().collect::<Vec<_>>())
            })
            .transpose()?
            .unwrap_or_default();

        Ok(items)
    }
}
