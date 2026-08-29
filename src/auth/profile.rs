use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::credential::TokenSet;
use super::method::AuthMethod;
use super::oauth::client::OAuthClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub method: AuthMethod,
    pub workspace: WorkspaceInfo,

    #[serde(default)]
    pub tokens: TokenSet,

    /// The app the tokens were issued to. Present for profiles created by a
    /// browser flow, which is also what makes their tokens renewable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<OAuthClient>,

    pub authorized_at: DateTime<Utc>,
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
