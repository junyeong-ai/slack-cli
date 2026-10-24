use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::{ConversationType, SlackApiError, SlackChannel, is_user_id};

const PAGE_SIZE: u32 = 200;
const MEMBERS_PAGE_SIZE: u32 = 1000;

/// The channels of every configured conversation type the token may list, and
/// the types Slack refused. Each type needs its own scope, so a token without
/// `groups:read` still resolves public channels by name.
pub struct ChannelDirectory {
    pub channels: Vec<SlackChannel>,
    pub unreadable: Vec<ConversationType>,
}

pub struct SlackChannelClient {
    pub(crate) core: Arc<SlackCore>,
}

impl SlackChannelClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    /// Lists each configured type on its own. A type refused with
    /// `missing_scope` is reported instead of failing the others; when every
    /// type is refused, that refusal is the result.
    pub async fn list(&self) -> Result<ChannelDirectory> {
        let types = &self.core.config.cache.channel_types;
        let mut channels = Vec::new();
        let mut unreadable = Vec::new();
        let mut refusal = None;

        for &kind in types {
            match self.list_kind(kind).await {
                Ok(mut listed) => channels.append(&mut listed),
                Err(err) if is_missing_scope(&err) => {
                    unreadable.push(kind);
                    refusal.get_or_insert(err);
                }
                Err(err) => return Err(err),
            }
        }

        // A conversation converted between types while the lists were read
        // appears in both; the directory is keyed by ID.
        let mut seen = HashSet::new();
        channels.retain(|channel| seen.insert(channel.id.clone()));

        match refusal {
            Some(err) if unreadable.len() == types.len() => Err(err),
            _ => Ok(ChannelDirectory {
                channels,
                unreadable,
            }),
        }
    }

    async fn list_kind(&self, kind: ConversationType) -> Result<Vec<SlackChannel>> {
        let mut all_channels = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut params = json!({
                "limit": PAGE_SIZE,
                "types": kind.as_api_str(),
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

    /// The direct message with these users, opened when it does not exist
    /// yet: one user is a DM, two to eight are a group DM.
    pub async fn open(&self, users: &[String]) -> Result<String> {
        if !(1..=8).contains(&users.len()) {
            anyhow::bail!("a direct message takes 1 to 8 users, got {}", users.len());
        }
        if let Some(input) = users.iter().find(|user| !is_user_id(user)) {
            anyhow::bail!("not a user ID: {input}. Find one with `slack-cli users <name>`");
        }

        let response = self
            .core
            .api_call("conversations.open", json!({ "users": users.join(",") }))
            .await?;
        response
            .pointer("/channel/id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .context("Missing channel in conversations.open response")
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

            let response = self
                .core
                .api_call_in_conversation("conversations.members", params, "channel")
                .await?;

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

fn is_missing_scope(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<SlackApiError>(),
        Some(SlackApiError::Api { code, .. }) if code == "missing_scope"
    )
}
