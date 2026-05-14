use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackMessage;

const REPLIES_PAGE_SIZE: usize = 1000;

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

    pub async fn send(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<MessageResponse> {
        let mut params = json!({
            "channel": channel,
            "text": text,
        });
        if let Some(ts) = thread_ts {
            params["thread_ts"] = json!(ts);
        }

        let response = self.core.api_call("chat.postMessage", params).await?;

        let ts = response["ts"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing timestamp in response"))?;

        Ok(MessageResponse {
            channel: channel.to_string(),
            ts: ts.to_string(),
        })
    }

    pub async fn update(&self, channel: &str, ts: &str, text: &str) -> Result<MessageResponse> {
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

    pub async fn delete(&self, channel: &str, ts: &str) -> Result<MessageResponse> {
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

    pub async fn history(
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

    pub async fn replies(
        &self,
        channel: &str,
        thread_ts: &str,
        limit: usize,
    ) -> Result<Vec<SlackMessage>> {
        let mut all_messages = Vec::new();
        let mut cursor: Option<String> = None;
        let page_limit = limit.min(REPLIES_PAGE_SIZE);

        loop {
            let mut params = json!({
                "channel": channel,
                "ts": thread_ts,
                "limit": page_limit,
            });

            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
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
        Ok(all_messages)
    }
}
