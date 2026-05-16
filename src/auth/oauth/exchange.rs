use serde::Deserialize;

use crate::auth::errors::OAuthError;
use crate::auth::secret::{self, Secret};

pub struct TokenExchange {
    pub api_base_url: String,
    pub http: reqwest::Client,
}

#[derive(Debug)]
pub struct TokenResponse {
    pub user_token: Option<Secret>,
    pub bot_token: Option<Secret>,
    pub team_id: String,
    pub team_name: String,
    pub user_id: Option<String>,
    pub scopes: Vec<String>,
}

pub struct ExchangeRequest<'a> {
    pub code: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub code_verifier: &'a str,
}

impl TokenExchange {
    pub async fn exchange_authorization_code(
        &self,
        request: ExchangeRequest<'_>,
    ) -> Result<TokenResponse, OAuthError> {
        let form: [(&str, &str); 4] = [
            ("code", request.code),
            ("client_id", request.client_id),
            ("redirect_uri", request.redirect_uri),
            ("code_verifier", request.code_verifier),
        ];
        let body = serde_urlencoded::to_string(form)
            .map_err(|e| OAuthError::ExchangeFailed(format!("failed to encode form: {e}")))?;

        let endpoint = format!(
            "{}/oauth.v2.access",
            self.api_base_url.trim_end_matches('/')
        );
        let raw: RawResponse = self
            .http
            .post(&endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?
            .json()
            .await?;
        raw.into_token_response()
    }
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
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
    scope: Option<String>,
}

impl RawResponse {
    fn into_token_response(self) -> Result<TokenResponse, OAuthError> {
        if !self.ok {
            return Err(OAuthError::ExchangeFailed(
                self.error.unwrap_or_else(|| "unknown_error".into()),
            ));
        }

        let team = self.team.unwrap_or_default();
        let team_id = team.id.ok_or(OAuthError::MissingField("team.id"))?;
        let team_name = team.name.unwrap_or_else(|| team_id.clone());

        let authed = self.authed_user.unwrap_or_default();
        let user_token = authed.access_token.map(secret::new);
        let bot_token = self.access_token.map(secret::new);

        if user_token.is_none() && bot_token.is_none() {
            return Err(OAuthError::MissingField("access_token"));
        }

        let scopes = self
            .scope
            .or(authed.scope)
            .map(split_scopes)
            .unwrap_or_default();

        Ok(TokenResponse {
            user_token,
            bot_token,
            team_id,
            team_name,
            user_id: authed.id,
            scopes,
        })
    }
}

fn split_scopes(raw: String) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn raw(json: &str) -> RawResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parses_pkce_user_token_response() {
        let r = raw(r#"{
                "ok": true,
                "team": {"id": "T1", "name": "Acme"},
                "authed_user": {
                    "id": "U1",
                    "access_token": "xoxp-test",
                    "scope": "users:read,chat:write"
                }
            }"#);
        let resp = r.into_token_response().unwrap();
        assert_eq!(resp.team_id, "T1");
        assert_eq!(resp.team_name, "Acme");
        assert_eq!(resp.user_id.as_deref(), Some("U1"));
        assert_eq!(resp.user_token.unwrap().expose_secret(), "xoxp-test");
        assert!(resp.bot_token.is_none());
        assert_eq!(resp.scopes, vec!["users:read", "chat:write"]);
    }

    #[test]
    fn surfaces_error_field() {
        let r = raw(r#"{"ok": false, "error": "invalid_code"}"#);
        let err = r.into_token_response().unwrap_err();
        assert!(matches!(err, OAuthError::ExchangeFailed(s) if s == "invalid_code"));
    }

    #[test]
    fn rejects_response_without_access_token() {
        let r = raw(r#"{"ok": true, "team": {"id": "T1", "name": "T"}}"#);
        let err = r.into_token_response().unwrap_err();
        assert!(matches!(err, OAuthError::MissingField("access_token")));
    }
}
