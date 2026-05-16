use std::time::Duration;

use url::Url;
use uuid::Uuid;

use crate::auth::errors::OAuthError;

use super::browser::BrowserOpener;
use super::callback::LoopbackReceiver;
use super::exchange::{ExchangeRequest, TokenExchange, TokenResponse};
use super::pkce::PkceVerifier;
use super::scopes::REQUIRED_USER_SCOPES;

const AUTHORIZE_URL: &str = "https://slack.com/oauth/v2/authorize";

pub struct PkceRunOptions {
    pub no_browser: bool,
    pub callback_timeout: Duration,
}

pub async fn run_pkce(
    client_id: &str,
    receiver: LoopbackReceiver,
    exchange: TokenExchange,
    options: PkceRunOptions,
) -> Result<TokenResponse, OAuthError> {
    let verifier = PkceVerifier::new();
    let state = Uuid::new_v4().to_string();
    run_pkce_authorized(client_id, receiver, exchange, options, verifier, state).await
}

pub async fn run_pkce_authorized(
    client_id: &str,
    receiver: LoopbackReceiver,
    exchange: TokenExchange,
    options: PkceRunOptions,
    verifier: PkceVerifier,
    expected_state: String,
) -> Result<TokenResponse, OAuthError> {
    let redirect_uri = receiver.redirect_uri();
    let challenge = verifier.challenge();
    let url = authorize_url(
        client_id,
        &redirect_uri,
        challenge.as_str(),
        &expected_state,
    );

    let browser = if options.no_browser {
        BrowserOpener::disabled()
    } else {
        BrowserOpener::auto()
    };
    if browser.open(url.as_str()).is_ok() {
        eprintln!("Opened browser for Slack authorization.");
    } else {
        eprintln!("Open this URL in a browser to authorize:\n  {url}");
    }

    let callback = receiver.accept_once(options.callback_timeout).await?;
    if callback.state != expected_state {
        return Err(OAuthError::StateMismatch);
    }

    exchange
        .exchange_authorization_code(ExchangeRequest {
            code: &callback.code,
            client_id,
            redirect_uri: &redirect_uri,
            code_verifier: verifier.as_str(),
        })
        .await
}

fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> Url {
    let mut url = Url::parse(AUTHORIZE_URL).expect("static URL");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("user_scope", &REQUIRED_USER_SCOPES.join(","))
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    url
}
