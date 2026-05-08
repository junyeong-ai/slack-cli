use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionInfo {
    pub name: String,
    pub users: Vec<String>,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReactions {
    pub channel: String,
    pub ts: String,
    pub reactions: Vec<ReactionInfo>,
}

pub struct SlackReactionClient {
    core: Arc<SlackCore>,
}

impl SlackReactionClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn add(&self, channel: &str, ts: &str, emoji: &str) -> Result<()> {
        let name = emoji.trim_matches(':');
        let params = json!({
            "channel": channel,
            "timestamp": ts,
            "name": name,
        });

        self.core.api_call("reactions.add", params).await?;

        Ok(())
    }

    pub async fn remove(&self, channel: &str, ts: &str, emoji: &str) -> Result<()> {
        let name = emoji.trim_matches(':');
        let params = json!({
            "channel": channel,
            "timestamp": ts,
            "name": name,
        });

        self.core.api_call("reactions.remove", params).await?;

        Ok(())
    }

    pub async fn get(&self, channel: &str, ts: &str) -> Result<MessageReactions> {
        let params = json!({
            "channel": channel,
            "timestamp": ts,
            "full": true,
        });

        let response = self.core.api_call("reactions.get", params).await?;

        let reactions = response
            .get("message")
            .and_then(|m| m.get("reactions"))
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .cloned()
                    .map(serde_json::from_value)
                    .collect::<Result<Vec<ReactionInfo>, _>>()
            })
            .transpose()?
            .unwrap_or_default();

        Ok(MessageReactions {
            channel: channel.to_string(),
            ts: ts.to_string(),
            reactions,
        })
    }
}
