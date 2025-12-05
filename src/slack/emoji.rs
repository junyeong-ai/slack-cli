use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use super::core::SlackCore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEmoji {
    pub name: String,
    pub url: String,
    pub is_alias: bool,
    pub alias_for: Option<String>,
}

pub struct SlackEmojiClient {
    core: Arc<SlackCore>,
}

impl SlackEmojiClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn list(&self) -> Result<Vec<CustomEmoji>> {
        let response = self
            .core
            .api_call("emoji.list", json!({}), None, false)
            .await?;

        let emoji_map: HashMap<String, String> = response
            .get("emoji")
            .and_then(|e| serde_json::from_value(e.clone()).ok())
            .unwrap_or_default();

        let emojis = emoji_map
            .into_iter()
            .map(|(name, url)| {
                if url.starts_with("alias:") {
                    let alias_for = url.strip_prefix("alias:").map(String::from);
                    CustomEmoji {
                        name,
                        url: String::new(),
                        is_alias: true,
                        alias_for,
                    }
                } else {
                    CustomEmoji {
                        name,
                        url,
                        is_alias: false,
                        alias_for: None,
                    }
                }
            })
            .collect();

        Ok(emojis)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<CustomEmoji>> {
        let all = self.list().await?;
        let query_lower = query.to_lowercase();

        Ok(all
            .into_iter()
            .filter(|e| e.name.to_lowercase().contains(&query_lower))
            .collect())
    }
}
