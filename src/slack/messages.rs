use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::{MessageMetadata, SlackMessage};

const REPLIES_PAGE_SIZE: usize = 1000;

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct MessageResponse {
    pub channel: String,
    pub ts: String,
}

/// Content of a `chat.postMessage` or `chat.update` call.
///
/// `chat.postMessage` and `chat.update` share the same payload surface in
/// the Slack API. This type captures that surface in one place so callers
/// can build a payload once and route it through either endpoint.
#[derive(Debug, Default, Clone)]
pub struct MessagePayload {
    pub text: Option<String>,
    /// Standard-markdown message body, rendered by Slack itself. Slack
    /// rejects it alongside `text` or `blocks` (`markdown_text_conflict`).
    pub markdown_text: Option<String>,
    pub blocks: Option<Vec<Value>>,
    pub attachments: Option<Vec<Value>>,
    pub metadata: Option<MessageMetadata>,
}

impl MessagePayload {
    /// True when at least one of the content fields (`text`, `markdown_text`,
    /// `blocks`, `attachments`) is *provided*. Empty values count:
    /// `Some(vec![])` on `chat.update` is Slack's explicit "clear this field"
    /// intent. `metadata` alone is not a content field — it decorates content.
    pub fn has_content(&self) -> bool {
        self.text.is_some()
            || self.markdown_text.is_some()
            || self.blocks.is_some()
            || self.attachments.is_some()
    }

    pub fn validate(&self) -> Result<()> {
        if !self.has_content() {
            anyhow::bail!(
                "message payload must provide at least one of --text, --markdown-text, --blocks, --attachments"
            );
        }
        if self.markdown_text.is_some() && (self.text.is_some() || self.blocks.is_some()) {
            anyhow::bail!("--markdown-text cannot be combined with --text or --blocks");
        }
        Ok(())
    }

    fn into_fields(self) -> serde_json::Map<String, Value> {
        let mut map = serde_json::Map::new();
        if let Some(text) = self.text {
            map.insert("text".into(), Value::String(text));
        }
        if let Some(markdown_text) = self.markdown_text {
            map.insert("markdown_text".into(), Value::String(markdown_text));
        }
        if let Some(blocks) = self.blocks {
            map.insert("blocks".into(), Value::Array(blocks));
        }
        if let Some(attachments) = self.attachments {
            map.insert("attachments".into(), Value::Array(attachments));
        }
        if let Some(metadata) = self.metadata {
            map.insert(
                "metadata".into(),
                serde_json::to_value(metadata)
                    .expect("MessageMetadata serializes to JSON infallibly"),
            );
        }
        map
    }

    pub fn into_post_json(self, channel: &str, thread_ts: Option<&str>) -> Value {
        let mut map = self.into_fields();
        map.insert("channel".into(), Value::String(channel.to_string()));
        if let Some(ts) = thread_ts {
            map.insert("thread_ts".into(), Value::String(ts.to_string()));
        }
        Value::Object(map)
    }

    pub fn into_update_json(self, channel: &str, ts: &str) -> Value {
        let mut map = self.into_fields();
        map.insert("channel".into(), Value::String(channel.to_string()));
        map.insert("ts".into(), Value::String(ts.to_string()));
        Value::Object(map)
    }
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
        payload: MessagePayload,
        thread_ts: Option<&str>,
    ) -> Result<MessageResponse> {
        payload.validate()?;
        let params = payload.into_post_json(channel, thread_ts);
        let response = self.core.api_call("chat.postMessage", params).await?;

        let ts = response["ts"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing timestamp in response"))?;
        Ok(MessageResponse {
            channel: response["channel"].as_str().unwrap_or(channel).to_string(),
            ts: ts.to_string(),
        })
    }

    pub async fn update(
        &self,
        channel: &str,
        ts: &str,
        payload: MessagePayload,
    ) -> Result<MessageResponse> {
        payload.validate()?;
        let params = payload.into_update_json(channel, ts);
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

    pub async fn permalink(&self, channel: &str, message_ts: &str) -> Result<String> {
        let params = json!({
            "channel": channel,
            "message_ts": message_ts,
        });

        let response = self.core.api_call("chat.getPermalink", params).await?;

        response["permalink"]
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("Missing permalink in response"))
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
            "include_all_metadata": true,
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
            .ok_or_else(|| anyhow!("Missing messages in conversations.history response"))?;

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
                "include_all_metadata": true,
            });

            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }

            let mut response = self.core.api_call("conversations.replies", params).await?;

            let raw_messages = response
                .get_mut("messages")
                .and_then(|v| v.as_array_mut())
                .map(std::mem::take)
                .ok_or_else(|| anyhow!("Missing messages in conversations.replies response"))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata_fixture() -> MessageMetadata {
        MessageMetadata {
            event_type: "task_created".into(),
            event_payload: json!({ "id": "T-42", "owner": "alice" }),
        }
    }

    #[test]
    fn validate_rejects_default_payload() {
        assert!(MessagePayload::default().validate().is_err());
    }

    #[test]
    fn validate_rejects_metadata_only_payload() {
        // metadata alone is decoration; Slack requires ≥1 content field.
        let payload = MessagePayload {
            metadata: Some(metadata_fixture()),
            ..Default::default()
        };
        assert!(payload.validate().is_err());
    }

    #[test]
    fn validate_accepts_empty_blocks_as_clear_intent() {
        // `chat.update` accepts `blocks: []` to clear blocks. The library
        // must not block that.
        let payload = MessagePayload {
            blocks: Some(vec![]),
            ..Default::default()
        };
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn validate_accepts_empty_attachments_as_clear_intent() {
        let payload = MessagePayload {
            attachments: Some(vec![]),
            ..Default::default()
        };
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn validate_accepts_text_only() {
        let payload = MessagePayload {
            text: Some("hello".into()),
            ..Default::default()
        };
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn validate_accepts_blocks_only() {
        let payload = MessagePayload {
            blocks: Some(vec![json!({"type": "section"})]),
            ..Default::default()
        };
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn validate_accepts_markdown_text_only() {
        let payload = MessagePayload {
            markdown_text: Some("**hello**".into()),
            ..Default::default()
        };
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn validate_accepts_markdown_text_with_attachments() {
        let payload = MessagePayload {
            markdown_text: Some("**hello**".into()),
            attachments: Some(vec![]),
            ..Default::default()
        };
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn validate_rejects_markdown_text_with_text() {
        let payload = MessagePayload {
            text: Some("hi".into()),
            markdown_text: Some("**hi**".into()),
            ..Default::default()
        };
        assert!(payload.validate().is_err());
    }

    #[test]
    fn validate_rejects_markdown_text_with_blocks() {
        let payload = MessagePayload {
            markdown_text: Some("**hi**".into()),
            blocks: Some(vec![json!({"type": "section"})]),
            ..Default::default()
        };
        assert!(payload.validate().is_err());
    }

    #[test]
    fn post_json_includes_markdown_text() {
        let payload = MessagePayload {
            markdown_text: Some("**hi**".into()),
            ..Default::default()
        };
        let value = payload.into_post_json("C123", None);
        assert_eq!(value["markdown_text"], json!("**hi**"));
        assert!(value.get("text").is_none());
    }

    #[test]
    fn post_json_includes_thread_and_metadata() {
        let payload = MessagePayload {
            text: Some("hi".into()),
            blocks: Some(vec![json!({"type": "section"})]),
            metadata: Some(metadata_fixture()),
            ..Default::default()
        };
        let value = payload.into_post_json("C123", Some("1700000000.000100"));
        assert_eq!(value["channel"], json!("C123"));
        assert_eq!(value["text"], json!("hi"));
        assert_eq!(value["blocks"][0]["type"], json!("section"));
        assert_eq!(value["thread_ts"], json!("1700000000.000100"));
        assert_eq!(value["metadata"]["event_type"], json!("task_created"));
        assert_eq!(value["metadata"]["event_payload"]["id"], json!("T-42"));
    }

    #[test]
    fn post_json_omits_thread_when_absent() {
        let payload = MessagePayload {
            text: Some("hi".into()),
            ..Default::default()
        };
        let value = payload.into_post_json("C123", None);
        assert!(value.get("thread_ts").is_none());
    }

    #[test]
    fn update_json_omits_thread_even_if_caller_wanted_it() {
        let payload = MessagePayload {
            text: Some("hi".into()),
            ..Default::default()
        };
        let value = payload.into_update_json("C123", "1700000000.000100");
        assert!(value.get("thread_ts").is_none());
        assert_eq!(value["ts"], json!("1700000000.000100"));
    }
}
