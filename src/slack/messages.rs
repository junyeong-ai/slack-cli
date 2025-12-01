use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackMessage;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SendMessageResponse {
    pub ts: String,
}

pub struct SlackMessageClient {
    core: Arc<SlackCore>,
}

impl SlackMessageClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn send_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<SendMessageResponse> {
        let response_ts = self
            .post_message(channel, Some(text), None, thread_ts, false)
            .await?;
        Ok(SendMessageResponse { ts: response_ts })
    }

    pub async fn get_thread_messages(
        &self,
        channel: &str,
        thread_ts: &str,
        limit: usize,
    ) -> Result<Vec<SlackMessage>> {
        let (messages, _) = self.get_thread_replies(channel, thread_ts, limit).await?;
        Ok(messages)
    }

    pub async fn list_channel_members(&self, channel: &str) -> Result<Vec<String>> {
        let params = json!({
            "channel": channel,
        });

        let response = self
            .core
            .api_call("conversations.members", params, None, false)
            .await?;

        let members: Vec<String> = response["members"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|m| m.as_str().map(|s| s.to_string()))
            .collect();

        Ok(members)
    }

    /// Send a message to a channel
    pub async fn post_message(
        &self,
        channel: &str,
        text: Option<&str>,
        blocks: Option<&Vec<Value>>,
        thread_ts: Option<&str>,
        reply_broadcast: bool,
    ) -> Result<String> {
        let mut params = json!({
            "channel": channel,
            "reply_broadcast": reply_broadcast,
        });

        if let Some(text) = text {
            params["text"] = json!(text);
        }

        if let Some(blocks) = blocks {
            params["blocks"] = json!(blocks);
        }

        if let Some(ts) = thread_ts {
            params["thread_ts"] = json!(ts);
        }

        let response = self
            .core
            .api_call("chat.postMessage", params, None, false)
            .await?;

        let timestamp = response["ts"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing timestamp in response"))?;

        Ok(timestamp.to_string())
    }

    /// Get channel messages
    pub async fn get_channel_messages(
        &self,
        channel: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<(Vec<SlackMessage>, Option<String>)> {
        let mut params = json!({
            "channel": channel,
            "limit": limit,
        });

        if let Some(cursor) = cursor {
            params["cursor"] = json!(cursor);
        }

        let mut response = self
            .core
            .api_call("conversations.history", params, None, false)
            .await?;

        let messages: Vec<SlackMessage> = response
            .get_mut("messages")
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| serde_json::from_value(m).ok())
            .collect();

        let next_cursor = response["response_metadata"]["next_cursor"]
            .as_str()
            .filter(|c| !c.is_empty())
            .map(|c| c.to_string());

        Ok((messages, next_cursor))
    }

    /// Get thread replies
    pub async fn get_thread_replies(
        &self,
        channel: &str,
        thread_ts: &str,
        limit: usize,
    ) -> Result<(Vec<SlackMessage>, bool)> {
        let params = json!({
            "channel": channel,
            "ts": thread_ts,
            "limit": limit,
        });

        let mut response = self
            .core
            .api_call("conversations.replies", params, None, false)
            .await?;

        let messages: Vec<SlackMessage> = response
            .get_mut("messages")
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| serde_json::from_value(m).ok())
            .collect();

        let has_more = response["has_more"].as_bool().unwrap_or(false);

        Ok((messages, has_more))
    }

    /// Search messages
    pub async fn search_messages(
        &self,
        query: &str,
        channel: Option<&str>,
        from_user: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SlackMessage>> {
        let mut search_query = query.to_string();

        if let Some(channel) = channel {
            search_query.push_str(&format!(" in:{}", channel));
        }

        if let Some(user) = from_user {
            search_query.push_str(&format!(" from:{}", user));
        }

        let params = json!({
            "query": search_query,
            "count": limit,
        });

        let mut response = self
            .core
            .api_call("search.messages", params, None, true)
            .await?;

        let messages: Vec<SlackMessage> = response
            .get_mut("messages")
            .and_then(|v| v.get_mut("matches"))
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| serde_json::from_value(m).ok())
            .collect();

        Ok(messages)
    }
}
