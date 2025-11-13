use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

use super::core::SlackCore;
use crate::slack::SlackUser;

const SLACK_API_LIMIT: u32 = 200;

pub struct SlackUserClient {
    pub(crate) core: Arc<SlackCore>,
}

impl SlackUserClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    /// Fetch all users from the workspace
    pub async fn fetch_all_users(&self) -> Result<Vec<SlackUser>> {
        let mut all_users = Vec::new();

        self.fetch_all_users_streaming(|users| {
            all_users.extend(users);
            Ok(())
        })
        .await?;

        Ok(all_users)
    }

    /// Stream fetch users with callback for immediate processing of each page
    pub async fn fetch_all_users_streaming<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(Vec<SlackUser>) -> Result<()>,
    {
        let mut total_fetched = 0;
        let mut cursor: Option<String> = None;
        let limit = SLACK_API_LIMIT;

        loop {
            let mut params = json!({
                "limit": limit,
            });

            if let Some(cursor_val) = &cursor {
                params["cursor"] = json!(cursor_val);
            }

            let response = self
                .core
                .api_call("users.list", params, None, false)
                .await?;

            // Parse users from response
            let mut page_users = Vec::new();
            if let Some(members) = response["members"].as_array() {
                for member in members {
                    match serde_json::from_value::<SlackUser>(member.clone()) {
                        Ok(user) => {
                            if !user.deleted {
                                page_users.push(user);
                            }
                        }
                        Err(_) => {
                            // Skip malformed user
                        }
                    }
                }
            }

            // Process this page immediately via callback
            if !page_users.is_empty() {
                let page_count = page_users.len();
                callback(page_users)?;
                total_fetched += page_count;
            }

            // Check for pagination
            cursor = response["response_metadata"]["next_cursor"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(|c| c.to_string());

            if cursor.is_none() {
                break;
            }
        }

        Ok(total_fetched)
    }
}
