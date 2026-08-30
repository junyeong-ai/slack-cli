use std::time::Duration;

use url::Url;
use uuid::Uuid;

use crate::auth::errors::OAuthError;

use super::browser;
use super::callback::LoopbackReceiver;
use super::client::OAuthClient;
use super::exchange::{Grant, TokenExchange, TokenResponse};
use super::pkce::PkceVerifier;

const AUTHORIZE_URL: &str = "https://slack.com/oauth/v2/authorize";

/// One trip through Slack's authorization-code flow, from opening the consent
/// screen to holding the issued tokens.
pub struct Authorization<'a> {
    pub client: &'a OAuthClient,
    pub user_scopes: &'a [String],
    pub open_browser: bool,
    pub callback_timeout: Duration,
}

impl Authorization<'_> {
    pub async fn run(
        self,
        receiver: LoopbackReceiver,
        exchange: TokenExchange,
    ) -> Result<TokenResponse, OAuthError> {
        self.run_with(
            receiver,
            exchange,
            PkceVerifier::new(),
            Uuid::new_v4().to_string(),
        )
        .await
    }

    /// The flow with its two unpredictable inputs supplied by the caller, so
    /// tests can drive it against fixed values.
    pub async fn run_with(
        self,
        receiver: LoopbackReceiver,
        exchange: TokenExchange,
        verifier: PkceVerifier,
        expected_state: String,
    ) -> Result<TokenResponse, OAuthError> {
        let redirect_uri = receiver.redirect_uri();
        let url = self.authorize_url(&redirect_uri, &verifier, &expected_state);

        if self.open_browser && browser::open(url.as_str()) {
            eprintln!("Opened browser for Slack authorization.");
        } else {
            eprintln!("Open this URL in a browser to authorize:\n  {url}");
        }

        let callback = receiver.accept_once(self.callback_timeout).await?;
        if callback.state != expected_state {
            return Err(OAuthError::StateMismatch);
        }

        exchange
            .execute(
                self.client,
                Grant::AuthorizationCode {
                    code: &callback.code,
                    redirect_uri: &redirect_uri,
                    code_verifier: verifier.as_str(),
                },
            )
            .await
    }

    /// Slack treats a loopback redirect as a non-web URI: it requires PKCE
    /// there and refuses bot scopes outright, so only user scopes are asked
    /// for.
    fn authorize_url(&self, redirect_uri: &str, verifier: &PkceVerifier, state: &str) -> Url {
        let mut url = Url::parse(AUTHORIZE_URL).expect("static URL");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", &self.client.id);
            query.append_pair("user_scope", &self.user_scopes.join(","));
            query.append_pair("redirect_uri", redirect_uri);
            query.append_pair("code_challenge", verifier.challenge().as_str());
            query.append_pair("code_challenge_method", "S256");
            query.append_pair("state", state);
        }
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn query_of(verifier: &PkceVerifier) -> HashMap<String, String> {
        let user = ["users:read", "chat:write"].map(String::from).to_vec();
        let client = OAuthClient::new("123.456");
        let authorization = Authorization {
            client: &client,
            user_scopes: &user,
            open_browser: false,
            callback_timeout: Duration::from_secs(1),
        };
        authorization
            .authorize_url("http://127.0.0.1:53682/callback", verifier, "state-1")
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[test]
    fn the_authorize_url_carries_pkce_and_only_user_scopes() {
        let verifier = PkceVerifier::from_raw("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        let query = query_of(&verifier);

        assert_eq!(query["client_id"], "123.456");
        assert_eq!(query["user_scope"], "users:read,chat:write");
        assert!(!query.contains_key("scope"));
        assert_eq!(
            query["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["state"], "state-1");
        assert_eq!(query["redirect_uri"], "http://127.0.0.1:53682/callback");
    }
}
