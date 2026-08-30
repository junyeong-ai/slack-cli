use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::credential::{Credential, TokenSet};
use super::method::AuthMethod;
use super::oauth::client::OAuthClient;
use super::profile::{Profile, WorkspaceInfo};
use super::secret::{self, Secret};
use super::state::{AuthState, SCHEMA_VERSION};

/// Schema 1 stored each token as a bare string, with one flat scope list per
/// profile and only the client id of the app that issued them.
#[derive(Debug, Deserialize)]
struct V1State {
    #[serde(default)]
    active_profile: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, V1Profile>,
}

#[derive(Debug, Deserialize)]
struct V1Profile {
    method: AuthMethod,
    workspace: WorkspaceInfo,
    #[serde(default)]
    tokens: V1TokenSet,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    client_id: Option<String>,
    authorized_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize)]
struct V1TokenSet {
    #[serde(default, with = "secret::option")]
    user: Option<Secret>,
    #[serde(default, with = "secret::option")]
    bot: Option<Secret>,
}

pub fn from_v1(raw: &[u8]) -> Result<AuthState, serde_json::Error> {
    let legacy: V1State = serde_json::from_slice(raw)?;
    Ok(AuthState {
        version: SCHEMA_VERSION,
        active_profile: legacy.active_profile,
        profiles: legacy
            .profiles
            .into_iter()
            .map(|(name, profile)| (name, upgrade(profile)))
            .collect(),
    })
}

fn upgrade(profile: V1Profile) -> Profile {
    // Schema 1 recorded neither expiry nor refresh token, so every credential
    // it holds is one that was treated as long-lived. Its single scope list
    // described the user token — the only kind its browser flow could issue.
    let mut tokens = TokenSet::default();
    if let Some(user) = profile.tokens.user {
        tokens.user = Some(Credential::permanent(user, profile.scopes));
    }
    if let Some(bot) = profile.tokens.bot {
        tokens.bot = Some(Credential::permanent(bot, Vec::new()));
    }

    Profile {
        method: profile.method,
        workspace: profile.workspace,
        tokens,
        client: profile.client_id.map(OAuthClient::new),
        authorized_at: profile.authorized_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    const V1: &str = r#"{
        "version": 1,
        "active_profile": "acme",
        "profiles": {
            "acme": {
                "method": "pkce",
                "workspace": {"team_id": "T1", "team_name": "Acme", "user_id": "U1"},
                "tokens": {"user": "xoxp-user", "bot": "xoxb-bot"},
                "scopes": ["users:read", "chat:write"],
                "client_id": "123.456",
                "authorized_at": "2026-01-01T00:00:00Z"
            }
        }
    }"#;

    #[test]
    fn upgrades_every_field_of_a_schema_1_profile() {
        let state = from_v1(V1.as_bytes()).unwrap();
        assert_eq!(state.version, SCHEMA_VERSION);
        assert_eq!(state.active_profile.as_deref(), Some("acme"));

        let profile = &state.profiles["acme"];
        assert_eq!(profile.method, AuthMethod::Pkce);
        assert_eq!(profile.workspace.team_id, "T1");
        assert_eq!(profile.client.as_ref().unwrap().id, "123.456");

        let user = profile.tokens.user.as_ref().unwrap();
        assert_eq!(user.token.expose_secret(), "xoxp-user");
        assert_eq!(user.scopes, ["users:read", "chat:write"]);
        assert!(!user.expires());

        let bot = profile.tokens.bot.as_ref().unwrap();
        assert_eq!(bot.token.expose_secret(), "xoxb-bot");
        assert!(bot.scopes.is_empty());
    }

    #[test]
    fn upgrades_a_static_profile_without_a_client() {
        let raw = r#"{
            "version": 1,
            "profiles": {
                "acme": {
                    "method": "static",
                    "workspace": {"team_id": "T1", "team_name": "Acme"},
                    "tokens": {"bot": "xoxb-bot"},
                    "scopes": [],
                    "authorized_at": "2026-01-01T00:00:00Z"
                }
            }
        }"#;
        let state = from_v1(raw.as_bytes()).unwrap();
        let profile = &state.profiles["acme"];
        assert!(profile.client.is_none());
        assert!(profile.tokens.user.is_none());
        assert_eq!(
            profile.tokens.bot.as_ref().unwrap().token.expose_secret(),
            "xoxb-bot"
        );
    }

    #[test]
    fn upgrades_an_empty_store() {
        let state = from_v1(br#"{"version": 1, "profiles": {}}"#).unwrap();
        assert_eq!(state.version, SCHEMA_VERSION);
        assert!(state.profiles.is_empty());
        assert!(state.active_profile.is_none());
    }
}
