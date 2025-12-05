use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub link: String,
    #[serde(rename = "type")]
    pub bookmark_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    pub date_created: i64,
    pub date_updated: i64,
}

pub struct SlackBookmarkClient {
    core: Arc<SlackCore>,
}

impl SlackBookmarkClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn add(
        &self,
        channel: &str,
        title: &str,
        url: &str,
        emoji: Option<&str>,
    ) -> Result<Bookmark> {
        let mut params = json!({
            "channel_id": channel,
            "title": title,
            "type": "link",
            "link": url,
        });

        if let Some(emoji) = emoji {
            params["emoji"] = json!(emoji);
        }

        let response = self
            .core
            .api_call("bookmarks.add", params, None, false)
            .await?;

        let bookmark = response
            .get("bookmark")
            .ok_or_else(|| anyhow::anyhow!("Missing bookmark in response"))?;

        Ok(Bookmark {
            id: bookmark["id"].as_str().unwrap_or("").to_string(),
            channel_id: bookmark["channel_id"]
                .as_str()
                .unwrap_or(channel)
                .to_string(),
            title: bookmark["title"].as_str().unwrap_or(title).to_string(),
            link: bookmark["link"].as_str().unwrap_or(url).to_string(),
            bookmark_type: bookmark["type"].as_str().unwrap_or("link").to_string(),
            emoji: bookmark["emoji"].as_str().map(String::from),
            date_created: bookmark["date_created"].as_i64().unwrap_or(0),
            date_updated: bookmark["date_updated"].as_i64().unwrap_or(0),
        })
    }

    pub async fn remove(&self, channel: &str, bookmark_id: &str) -> Result<()> {
        let params = json!({
            "channel_id": channel,
            "bookmark_id": bookmark_id,
        });

        self.core
            .api_call("bookmarks.remove", params, None, false)
            .await?;

        Ok(())
    }

    pub async fn list(&self, channel: &str) -> Result<Vec<Bookmark>> {
        let params = json!({
            "channel_id": channel,
        });

        let response = self
            .core
            .api_call("bookmarks.list", params, None, false)
            .await?;

        let bookmarks = response
            .get("bookmarks")
            .and_then(|b| b.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| {
                        Some(Bookmark {
                            id: b.get("id")?.as_str()?.to_string(),
                            channel_id: b
                                .get("channel_id")
                                .and_then(|c| c.as_str())
                                .unwrap_or(channel)
                                .to_string(),
                            title: b.get("title")?.as_str()?.to_string(),
                            link: b
                                .get("link")
                                .and_then(|l| l.as_str())
                                .unwrap_or("")
                                .to_string(),
                            bookmark_type: b
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("link")
                                .to_string(),
                            emoji: b.get("emoji").and_then(|e| e.as_str()).map(String::from),
                            date_created: b
                                .get("date_created")
                                .and_then(|d| d.as_i64())
                                .unwrap_or(0),
                            date_updated: b
                                .get("date_updated")
                                .and_then(|d| d.as_i64())
                                .unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(bookmarks)
    }
}
