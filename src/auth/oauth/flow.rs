use std::time::Duration;

use url::Url;
use uuid::Uuid;

use crate::auth::errors::OAuthError;

use super::browser::BrowserOpener;
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
    pub bot_scopes: &'a [String],
    pub no_browser: bool,
    pub callback_timeout: Duration,
}

impl Authorization<'_> {
    pub async fn run(
        self,
        receiver: LoopbackReceiver,
        exchange: TokenExchange,
    ) -> Result<TokenResponse, OAuthError> {
        let verifier = self.client.is_public().then(PkceVerifier::new);
        self.run_with(receiver, exchange, verifier, Uuid::new_v4().to_string())
            .await
    }

    /// The flow with its two unpredictable inputs supplied by the caller, so
    /// tests can drive it against fixed values.
    pub async fn run_with(
        self,
        receiver: LoopbackReceiver,
        exchange: TokenExchange,
        verifier: Option<PkceVerifier>,
        expected_state: String,
    ) -> Result<TokenResponse, OAuthError> {
        let redirect_uri = receiver.redirect_uri();
        let url = self.authorize_url(&redirect_uri, verifier.as_ref(), &expected_state);

        let browser = if self.no_browser {
            BrowserOpener::disabled()
        } else {
            BrowserOpener::auto()
        };
        if browser.open(url.as_str()).is_ok() {
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
                    code_verifier: verifier.as_ref().map(PkceVerifier::as_str),
                },
            )
            .await
    }

    /// Slack routes a loopback redirect from a public client as a desktop
    /// redirect: it demands PKCE and refuses bot scopes there. A confidential
    /// client redirects as a server would, and may ask for both scope kinds.
    fn authorize_url(
        &self,
        redirect_uri: &str,
        verifier: Option<&PkceVerifier>,
        state: &str,
    ) -> Url {
        let mut url = Url::parse(AUTHORIZE_URL).expect("static URL");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", &self.client.id);
            query.append_pair("user_scope", &self.user_scopes.join(","));
            if !self.client.is_public() {
                query.append_pair("scope", &self.bot_scopes.join(","));
            }
            query.append_pair("redirect_uri", redirect_uri);
            if let Some(verifier) = verifier {
                query.append_pair("code_challenge", verifier.challenge().as_str());
                query.append_pair("code_challenge_method", "S256");
            }
            query.append_pair("state", state);
        }
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::secret;
    use std::collections::HashMap;

    fn scopes(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_string()).collect()
    }

    fn query_of(client: &OAuthClient, verifier: Option<&PkceVerifier>) -> HashMap<String, String> {
        let user = scopes(&["users:read", "chat:write"]);
        let bot = scopes(&["chat:write"]);
        let authorization = Authorization {
            client,
            user_scopes: &user,
            bot_scopes: &bot,
            no_browser: true,
            callback_timeout: Duration::from_secs(1),
        };
        authorization
            .authorize_url("http://127.0.0.1:53682/callback", verifier, "state-1")
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[test]
    fn public_clients_send_pkce_and_only_user_scopes() {
        let verifier = PkceVerifier::from_raw("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        let query = query_of(&OAuthClient::public("123.456"), Some(&verifier));

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

    #[test]
    fn confidential_clients_request_bot_scopes_without_pkce() {
        let client = OAuthClient::confidential("123.456", secret::new("shh"));
        let query = query_of(&client, None);

        assert_eq!(query["user_scope"], "users:read,chat:write");
        assert_eq!(query["scope"], "chat:write");
        assert!(!query.contains_key("code_challenge"));
        assert!(!query.contains_key("code_challenge_method"));
    }
}
