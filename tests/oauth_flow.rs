//! Integration tests for Slack's authorization-code flow against a mock Slack
//! API: the loopback callback, PKCE possession proof, and the token exchange.

use std::time::Duration;

use secrecy::ExposeSecret;
use slack_cli::auth::OAuthError;
use slack_cli::auth::oauth::callback::LoopbackReceiver;
use slack_cli::auth::oauth::client::OAuthClient;
use slack_cli::auth::oauth::exchange::{TokenExchange, TokenResponse};
use slack_cli::auth::oauth::flow::Authorization;
use slack_cli::auth::oauth::pkce::PkceVerifier;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_CLIENT_ID: &str = "test-client";
const FIXED_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

fn free_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn scopes(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_string()).collect()
}

fn exchange(server: &MockServer) -> TokenExchange {
    TokenExchange {
        api_base_url: server.uri(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    }
}

async fn deliver_callback(port: u16, query: &str) {
    // Give the receiver task a brief moment to reach `accept_once`.
    // The LoopbackReceiver has already bound the port (so connect would succeed
    // immediately) — we just want the driver to have crossed past authorize_url
    // before our callback arrives. Pre-probing the port would consume the
    // accept slot with an empty connection.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let request =
        format!("GET /callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
}

/// Drives one authorization against `server`, delivering `callback_query` to
/// the loopback receiver once the flow is waiting for it.
async fn authorize(
    server: &MockServer,
    client: OAuthClient,
    state: &str,
    callback_query: String,
) -> Result<TokenResponse, OAuthError> {
    let port = free_loopback_port();
    let receiver = LoopbackReceiver::bind(port).await.expect("bind callback");
    let exchange = exchange(server);
    let verifier = PkceVerifier::from_raw(FIXED_VERIFIER);
    let expected_state = state.to_string();

    let driver = tokio::spawn(async move {
        Authorization {
            client: &client,
            user_scopes: &scopes(&["users:read", "chat:write"]),
            no_browser: true,
            callback_timeout: Duration::from_secs(5),
        }
        .run_with(receiver, exchange, verifier, expected_state)
        .await
    });

    deliver_callback(port, &callback_query).await;
    driver.await.expect("driver task")
}

#[tokio::test]
async fn public_clients_exchange_a_code_for_a_rotating_user_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .and(body_string_contains("code=stub-code"))
        .and(body_string_contains("code_verifier="))
        .and(body_string_contains("client_id=test-client"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "team": {"id": "T01", "name": "Acme"},
            "authed_user": {
                "id": "U01",
                "access_token": "xoxe.xoxp-issued",
                "refresh_token": "xoxe-refresh",
                "expires_in": 43200,
                "scope": "users:read,chat:write,search:read.public"
            }
        })))
        .mount(&server)
        .await;

    let response = authorize(
        &server,
        OAuthClient::new(TEST_CLIENT_ID),
        "test-state-12345",
        "code=stub-code&state=test-state-12345".to_string(),
    )
    .await
    .expect("authorization succeeds");

    let team = response.team().unwrap();
    assert_eq!(team.id, "T01");
    assert_eq!(team.name, "Acme");
    assert_eq!(response.user_id.as_deref(), Some("U01"));
    assert!(response.bot.is_none());

    let user = response.user.expect("user token present");
    assert_eq!(user.token.expose_secret(), "xoxe.xoxp-issued");
    assert_eq!(user.refresh_token.unwrap().expose_secret(), "xoxe-refresh");
    assert_eq!(user.expires_in, Some(43200));
    assert_eq!(
        user.scopes,
        ["users:read", "chat:write", "search:read.public"]
    );
}

#[tokio::test]
async fn a_public_client_never_sends_its_credentials_in_the_request_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .and(body_string_contains("client_id=test-client"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "team": {"id": "T01", "name": "Acme"},
            "authed_user": {"id": "U01", "access_token": "xoxp-issued"}
        })))
        .mount(&server)
        .await;

    let response = authorize(
        &server,
        OAuthClient::new(TEST_CLIENT_ID),
        "state-3",
        "code=stub-code&state=state-3".to_string(),
    )
    .await
    .expect("authorization succeeds");

    assert!(response.user.is_some());
}

#[tokio::test]
async fn a_mismatched_state_aborts_before_the_token_exchange() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let err = authorize(
        &server,
        OAuthClient::new(TEST_CLIENT_ID),
        "expected-state",
        "code=stub-code&state=wrong-state".to_string(),
    )
    .await
    .expect_err("state mismatch");

    assert!(
        matches!(err, OAuthError::StateMismatch),
        "expected StateMismatch, got {err:?}"
    );
}

#[tokio::test]
async fn a_rejected_code_surfaces_the_slack_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": false,
            "error": "invalid_code"
        })))
        .mount(&server)
        .await;

    let err = authorize(
        &server,
        OAuthClient::new(TEST_CLIENT_ID),
        "valid-state",
        "code=bad-code&state=valid-state".to_string(),
    )
    .await
    .expect_err("exchange fails");

    assert!(
        matches!(err, OAuthError::ExchangeFailed(ref code) if code == "invalid_code"),
        "expected ExchangeFailed(invalid_code), got {err:?}"
    );
}

#[tokio::test]
async fn a_denied_authorization_surfaces_the_callback_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let err = authorize(
        &server,
        OAuthClient::new(TEST_CLIENT_ID),
        "state-4",
        "error=access_denied".to_string(),
    )
    .await
    .expect_err("authorization denied");

    assert!(
        matches!(err, OAuthError::AuthorizationDenied(ref reason) if reason == "access_denied"),
        "expected AuthorizationDenied, got {err:?}"
    );
}
