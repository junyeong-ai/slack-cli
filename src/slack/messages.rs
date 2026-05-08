use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackMessage;

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct MessageResponse {
    pub channel: String,
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
    ) -> Result<MessageResponse> {
        let ts = self
            .post_message(channel, Some(text), None, thread_ts, false)
            .await?;
        Ok(MessageResponse {
            channel: channel.to_string(),
            ts,
        })
    }

    pub async fn update_message(
        &self,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> Result<MessageResponse> {
        let params = json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });

        let response = self.core.api_call("chat.update", params).await?;

        Ok(MessageResponse {
            channel: response["channel"].as_str().unwrap_or(channel).to_string(),
            ts: response["ts"].as_str().unwrap_or(ts).to_string(),
        })
    }

    pub async fn delete_message(&self, channel: &str, ts: &str) -> Result<MessageResponse> {
        let params = json!({
            "channel": channel,
            "ts": ts,
        });

        let response = self.core.api_call("chat.delete", params).await?;

        Ok(MessageResponse {
            channel: response["channel"].as_str().unwrap_or(channel).to_string(),
            ts: response["ts"].as_str().unwrap_or(ts).to_string(),
        })
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

        let response = self.core.api_call("chat.postMessage", params).await?;

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
        oldest: Option<&str>,
        latest: Option<&str>,
    ) -> Result<(Vec<SlackMessage>, Option<String>)> {
        let mut params = json!({
            "channel": channel,
            "limit": limit,
        });

        if let Some(cursor) = cursor {
            params["cursor"] = json!(cursor);
        }

        if let Some(oldest) = oldest {
            params["oldest"] = json!(oldest);
        }

        if let Some(latest) = latest {
            params["latest"] = json!(latest);
        }

        let mut response = self.core.api_call("conversations.history", params).await?;

        let raw_messages = response
            .get_mut("messages")
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .ok_or_else(|| anyhow::anyhow!("Missing messages in conversations.history response"))?;

        let messages = raw_messages
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<SlackMessage>, _>>()?;

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
        let mut all_messages = Vec::new();
        let mut cursor: Option<String> = None;
        let page_limit = limit.min(1000);

        loop {
            let mut params = json!({
                "channel": channel,
                "ts": thread_ts,
                "limit": page_limit,
            });

            if let Some(cursor) = &cursor {
                params["cursor"] = json!(cursor);
            }

            let mut response = self.core.api_call("conversations.replies", params).await?;

            let raw_messages = response
                .get_mut("messages")
                .and_then(|v| v.as_array_mut())
                .map(std::mem::take)
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing messages in conversations.replies response")
                })?;

            let mut page_messages = raw_messages
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<SlackMessage>, _>>()?;

            all_messages.append(&mut page_messages);

            cursor = response["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(ToOwned::to_owned);

            if cursor.is_none() || all_messages.len() >= limit {
                break;
            }
        }

        all_messages.truncate(limit);
        let has_more = cursor.is_some();

        Ok((all_messages, has_more))
    }
}
