use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::core::SlackCore;

pub struct SlackAuthClient {
    core: Arc<SlackCore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackAuthIdentity {
    pub team: String,
    pub team_id: String,
    pub user: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_enterprise_install: Option<bool>,
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
        serde_json::from_value(response).context("auth.test response did not match expected shape")
    }

    /// Calls `auth.revoke` against Slack with an explicit token.
    pub async fn revoke(&self, token: &str) -> Result<()> {
        self.core
            .api_call_with("auth.revoke", json!({}), token)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_deserializes_personal_workspace() {
        let identity: SlackAuthIdentity = serde_json::from_value(json!({
            "team": "Acme",
            "team_id": "T01",
            "user": "alice",
            "user_id": "U01",
            "url": "https://acme.slack.com/",
        }))
        .unwrap();
        assert_eq!(identity.team_id, "T01");
        assert_eq!(identity.url.as_deref(), Some("https://acme.slack.com/"));
        assert!(identity.enterprise_id.is_none());
        assert!(identity.is_enterprise_install.is_none());
    }

    #[test]
    fn identity_deserializes_enterprise_install() {
        let identity: SlackAuthIdentity = serde_json::from_value(json!({
            "team": "Acme",
            "team_id": "T01",
            "user": "alice",
            "user_id": "U01",
            "bot_id": "B01",
            "enterprise_id": "E01",
            "enterprise_name": "Acme Corp",
            "is_enterprise_install": true,
        }))
        .unwrap();
        assert_eq!(identity.enterprise_id.as_deref(), Some("E01"));
        assert_eq!(identity.bot_id.as_deref(), Some("B01"));
        assert_eq!(identity.is_enterprise_install, Some(true));
    }

    #[test]
    fn identity_serializes_skipping_absent_optionals() {
        let identity = SlackAuthIdentity {
            team: "Acme".into(),
            team_id: "T01".into(),
            user: "alice".into(),
            user_id: "U01".into(),
            url: None,
            bot_id: None,
            enterprise_id: None,
            enterprise_name: None,
            is_enterprise_install: None,
        };
        let value = serde_json::to_value(&identity).unwrap();
        assert!(value.get("url").is_none());
        assert!(value.get("enterprise_id").is_none());
        assert!(value.get("is_enterprise_install").is_none());
    }
}
