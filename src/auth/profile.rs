use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::method::AuthMethod;
use super::secret::{self, Secret};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub method: AuthMethod,
    pub workspace: WorkspaceInfo,
    #[serde(default)]
    pub tokens: TokenSet,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub authorized_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenSet {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "secret::option"
    )]
    pub user: Option<Secret>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "secret::option"
    )]
    pub bot: Option<Secret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub team_id: String,
    pub team_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl Profile {
    pub fn label(&self) -> String {
        format!("{} ({})", self.workspace.team_name, self.workspace.team_id)
    }
}
