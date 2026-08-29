use chrono::{DateTime, Duration, Utc};
use secrecy::ExposeSecret;
use serde::Deserialize;

use crate::auth::credential::Credential;
use crate::auth::errors::OAuthError;
use crate::auth::secret::{self, Secret};

use super::client::OAuthClient;

const USER_TOKEN_TYPE: &str = "user";

/// The two grants Slack's `oauth.v2.access` accepts for an app installation.
pub enum Grant<'a> {
    AuthorizationCode {
        code: &'a str,
        redirect_uri: &'a str,
        code_verifier: &'a str,
    },
    RefreshToken {
        refresh_token: &'a Secret,
    },
}

pub struct TokenExchange {
    pub api_base_url: String,
    pub http: reqwest::Client,
}

#[derive(Debug)]
pub struct TokenResponse {
    pub team: Option<TeamIdentity>,
    pub user_id: Option<String>,
    pub user: Option<IssuedToken>,
    pub bot: Option<IssuedToken>,
}

#[derive(Debug, Clone)]
pub struct TeamIdentity {
    pub id: String,
    pub name: String,
}

#[derive(Debug)]
pub struct IssuedToken {
    pub token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_in: Option<i64>,
    pub scopes: Vec<String>,
}

impl IssuedToken {
    pub fn into_credential(self, now: DateTime<Utc>) -> Credential {
        Credential {
            token: self.token,
            refresh_token: self.refresh_token,
            expires_at: self
                .expires_in
                .map(|seconds| now + Duration::seconds(seconds)),
            scopes: self.scopes,
        }
    }
}

impl TokenResponse {
    pub fn team(&self) -> Result<&TeamIdentity, OAuthError> {
        self.team
            .as_ref()
            .ok_or(OAuthError::MissingField("team.id"))
    }
}

impl TokenExchange {
    pub async fn execute(
        &self,
        client: &OAuthClient,
        grant: Grant<'_>,
    ) -> Result<TokenResponse, OAuthError> {
        let mut form: Vec<(&str, &str)> = Vec::with_capacity(5);
        form.push(("client_id", &client.id));

        match &grant {
            Grant::AuthorizationCode {
                code,
                redirect_uri,
                code_verifier,
            } => {
                form.push(("code", code));
                form.push(("redirect_uri", redirect_uri));
                form.push(("code_verifier", code_verifier));
            }
            Grant::RefreshToken { refresh_token } => {
                form.push(("grant_type", "refresh_token"));
                form.push(("refresh_token", refresh_token.expose_secret()));
            }
        }

        let body = serde_urlencoded::to_string(&form)
            .map_err(|e| OAuthError::ExchangeFailed(format!("failed to encode form: {e}")))?;

        let endpoint = format!(
            "{}/oauth.v2.access",
            self.api_base_url.trim_end_matches('/')
        );
        let request = self
            .http
            .post(&endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body);

        let raw: RawResponse = request.send().await?.json().await?;
        raw.into_token_response()
    }
}

/// The union of every `oauth.v2.access` success shape.
///
/// An authorization-code exchange returns the bot token at the top level and
/// the user token nested under `authed_user`. A refresh returns a single token
/// at the top level, tagged by `token_type`.
#[derive(Debug, Deserialize)]
struct RawResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    team: Option<TeamPart>,
    #[serde(default)]
    authed_user: Option<AuthedUserPart>,
}

#[derive(Debug, Deserialize, Default)]
struct TeamPart {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AuthedUserPart {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

impl RawResponse {
    fn into_token_response(self) -> Result<TokenResponse, OAuthError> {
        if !self.ok {
            return Err(OAuthError::ExchangeFailed(
                self.error.unwrap_or_else(|| "unknown_error".into()),
            ));
        }

        let tags_a_user_token = self.token_type.as_deref() == Some(USER_TOKEN_TYPE);
        let authed_user = self.authed_user.unwrap_or_default();

        let top_level = self.access_token.map(|token| IssuedToken {
            token: secret::new(token),
            refresh_token: self.refresh_token.map(secret::new),
            expires_in: self.expires_in,
            scopes: split_scopes(self.scope),
        });

        let nested_user = authed_user.access_token.map(|token| IssuedToken {
            token: secret::new(token),
            refresh_token: authed_user.refresh_token.map(secret::new),
            expires_in: authed_user.expires_in,
            scopes: split_scopes(authed_user.scope),
        });

        let (user, bot) = match (nested_user, tags_a_user_token) {
            (Some(user), _) => (Some(user), top_level),
            (None, true) => (top_level, None),
            (None, false) => (None, top_level),
        };

        if user.is_none() && bot.is_none() {
            return Err(OAuthError::MissingField("access_token"));
        }

        let team = self.team.unwrap_or_default();
        let team = team.id.map(|id| TeamIdentity {
            name: team.name.unwrap_or_else(|| id.clone()),
            id,
        });

        Ok(TokenResponse {
            team,
            user_id: authed_user.id,
            user,
            bot,
        })
    }
}

fn split_scopes(raw: Option<String>) -> Vec<String> {
    raw.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::credential::Readiness;

    fn parse(json: &str) -> Result<TokenResponse, OAuthError> {
        serde_json::from_str::<RawResponse>(json)
            .unwrap()
            .into_token_response()
    }

    #[test]
    fn authorization_code_exchange_reads_the_user_token_from_authed_user() {
        let response = parse(
            r#"{
                "ok": true,
                "team": {"id": "T1", "name": "Acme"},
                "authed_user": {
                    "id": "U1",
                    "access_token": "xoxp-user",
                    "scope": "users:read,chat:write"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(response.team().unwrap().name, "Acme");
        assert_eq!(response.user_id.as_deref(), Some("U1"));
        let user = response.user.unwrap();
        assert_eq!(user.token.expose_secret(), "xoxp-user");
        assert_eq!(user.scopes, ["users:read", "chat:write"]);
        assert!(response.bot.is_none());
    }

    #[test]
    fn authorization_code_exchange_reads_both_tokens_with_their_own_scopes() {
        let response = parse(
            r#"{
                "ok": true,
                "access_token": "xoxb-bot",
                "scope": "chat:write,users:read",
                "token_type": "bot",
                "team": {"id": "T1", "name": "Acme"},
                "authed_user": {
                    "id": "U1",
                    "access_token": "xoxp-user",
                    "scope": "search:read.public"
                }
            }"#,
        )
        .unwrap();

        let user = response.user.unwrap();
        let bot = response.bot.unwrap();
        assert_eq!(user.token.expose_secret(), "xoxp-user");
        assert_eq!(user.scopes, ["search:read.public"]);
        assert_eq!(bot.token.expose_secret(), "xoxb-bot");
        assert_eq!(bot.scopes, ["chat:write", "users:read"]);
    }

    #[test]
    fn rotating_exchange_carries_expiry_and_refresh_tokens() {
        let response = parse(
            r#"{
                "ok": true,
                "access_token": "xoxe.xoxb-bot",
                "expires_in": 43200,
                "refresh_token": "xoxe-bot-refresh",
                "token_type": "bot",
                "team": {"id": "T1", "name": "Acme"},
                "authed_user": {
                    "id": "U1",
                    "access_token": "xoxe.xoxp-user",
                    "expires_in": 43200,
                    "refresh_token": "xoxe-user-refresh"
                }
            }"#,
        )
        .unwrap();

        let user = response.user.unwrap();
        assert_eq!(user.expires_in, Some(43200));
        assert_eq!(
            user.refresh_token.unwrap().expose_secret(),
            "xoxe-user-refresh"
        );
        let bot = response.bot.unwrap();
        assert_eq!(bot.expires_in, Some(43200));
        assert_eq!(
            bot.refresh_token.unwrap().expose_secret(),
            "xoxe-bot-refresh"
        );
    }

    #[test]
    fn user_refresh_is_read_from_the_top_level_by_its_token_type() {
        let response = parse(
            r#"{
                "ok": true,
                "id": "U1234",
                "scope": "chat:write",
                "access_token": "xoxe.xoxp-refreshed",
                "expires_in": 43200,
                "refresh_token": "xoxe-next",
                "token_type": "user"
            }"#,
        )
        .unwrap();

        assert!(response.bot.is_none());
        let user = response.user.unwrap();
        assert_eq!(user.token.expose_secret(), "xoxe.xoxp-refreshed");
        assert_eq!(user.refresh_token.unwrap().expose_secret(), "xoxe-next");
        assert_eq!(user.expires_in, Some(43200));
        assert_eq!(user.scopes, ["chat:write"]);
    }

    #[test]
    fn bot_refresh_is_read_from_the_top_level_by_its_token_type() {
        let response = parse(
            r#"{
                "ok": true,
                "access_token": "xoxe.xoxb-refreshed",
                "expires_in": 43200,
                "refresh_token": "xoxe-next",
                "token_type": "bot",
                "scope": "chat:write"
            }"#,
        )
        .unwrap();

        assert!(response.user.is_none());
        let bot = response.bot.unwrap();
        assert_eq!(bot.token.expose_secret(), "xoxe.xoxb-refreshed");
        assert_eq!(bot.expires_in, Some(43200));
    }

    #[test]
    fn issued_tokens_without_an_expiry_become_permanent_credentials() {
        let response = parse(r#"{"ok": true, "access_token": "xoxb-static"}"#).unwrap();
        let credential = response.bot.unwrap().into_credential(Utc::now());
        assert!(!credential.expires());
        assert!(credential.refresh_token.is_none());
    }

    #[test]
    fn expiring_tokens_become_credentials_dated_from_the_exchange() {
        let response = parse(
            r#"{"ok": true, "access_token": "xoxe.xoxb", "expires_in": 43200, "token_type": "bot"}"#,
        )
        .unwrap();
        let now = Utc::now();
        let credential = response.bot.unwrap().into_credential(now);
        assert_eq!(credential.expires_at, Some(now + Duration::seconds(43200)));
        assert_eq!(credential.readiness(now), Readiness::Ready);
    }

    #[test]
    fn surfaces_the_api_error_code() {
        let err = parse(r#"{"ok": false, "error": "invalid_code"}"#).unwrap_err();
        assert!(matches!(err, OAuthError::ExchangeFailed(code) if code == "invalid_code"));
    }

    #[test]
    fn rejects_a_response_carrying_no_token() {
        let err = parse(r#"{"ok": true, "team": {"id": "T1", "name": "T"}}"#).unwrap_err();
        assert!(matches!(err, OAuthError::MissingField("access_token")));
    }

    #[test]
    fn a_refresh_response_without_team_has_no_team_identity() {
        let response = parse(
            r#"{"ok": true, "access_token": "xoxe.xoxb", "expires_in": 43200, "token_type": "bot"}"#,
        )
        .unwrap();
        assert!(response.team.is_none());
        assert!(matches!(
            response.team().unwrap_err(),
            OAuthError::MissingField("team.id")
        ));
    }

    #[test]
    fn team_name_falls_back_to_the_team_id_when_absent() {
        let response =
            parse(r#"{"ok": true, "access_token": "xoxb", "team": {"id": "T1"}}"#).unwrap();
        let team = response.team().unwrap();
        assert_eq!(team.id, "T1");
        assert_eq!(team.name, "T1");
    }
}
