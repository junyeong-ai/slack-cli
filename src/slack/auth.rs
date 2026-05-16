use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

use super::core::SlackCore;

pub struct SlackAuthClient {
    core: Arc<SlackCore>,
}

#[derive(Debug, Clone)]
pub struct SlackAuthIdentity {
    pub team: String,
    pub team_id: String,
    pub user: String,
    pub user_id: String,
}

impl SlackAuthClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    /// Calls `auth.test` against Slack with an explicit token. Used during
    /// login (to validate a token before persisting it) and during
    /// `auth status --verify` (to check a specific stored profile).
    pub async fn test(&self, token: &str) -> Result<SlackAuthIdentity> {
        let response = self
            .core
            .api_call_with("auth.test", json!({}), token)
            .await?;
        decode_identity(response)
    }

    /// Calls `auth.revoke` against Slack with an explicit token.
    pub async fn revoke(&self, token: &str) -> Result<()> {
        self.core
            .api_call_with("auth.revoke", json!({}), token)
            .await?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct AuthTestPayload {
    team: String,
    team_id: String,
    user: String,
    user_id: String,
}

fn decode_identity(value: serde_json::Value) -> Result<SlackAuthIdentity> {
    let payload: AuthTestPayload =
        serde_json::from_value(value).context("auth.test response did not match expected shape")?;
    Ok(SlackAuthIdentity {
        team: payload.team,
        team_id: payload.team_id,
        user: payload.user,
        user_id: payload.user_id,
    })
}
