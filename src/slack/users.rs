use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackUser;

const PAGE_SIZE: u32 = 200;

pub struct SlackUserClient {
    pub(crate) core: Arc<SlackCore>,
}

impl SlackUserClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn list(&self) -> Result<Vec<SlackUser>> {
        let mut all_users = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut params = json!({ "limit": PAGE_SIZE });
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }

            let mut response = self.core.api_call("users.list", params).await?;

            let raw_members = response
                .get_mut("members")
                .and_then(|v| v.as_array_mut())
                .map(std::mem::take)
                .ok_or_else(|| anyhow::anyhow!("Missing members in users.list response"))?;

            let page_users: Vec<SlackUser> = raw_members
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|user: &SlackUser| !user.deleted)
                .collect();

            all_users.extend(page_users);

            cursor = response["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(|c| c.to_string());

            if cursor.is_none() {
                break;
            }
        }

        Ok(all_users)
    }
}
