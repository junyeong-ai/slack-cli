use std::time::Duration;

use secrecy::ExposeSecret;
use slack_cli::auth::OAuthError;
use slack_cli::auth::oauth::callback::LoopbackReceiver;
use slack_cli::auth::oauth::exchange::TokenExchange;
use slack_cli::auth::oauth::flow::{PkceRunOptions, run_pkce_authorized};
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

#[tokio::test]
async fn pkce_flow_completes_end_to_end() {
    let mock_server = MockServer::start().await;

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
                "access_token": "xoxp-issued-token",
                "scope": "users:read,chat:write,search:read"
            }
        })))
        .mount(&mock_server)
        .await;

    let port = free_loopback_port();
    let receiver = LoopbackReceiver::bind(port).await.expect("bind callback");

    let exchange = TokenExchange {
        api_base_url: mock_server.uri(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
    };
    let verifier = PkceVerifier::from_raw(FIXED_VERIFIER);
    let state = "test-state-12345".to_string();

    let state_for_callback = state.clone();
    let driver = tokio::spawn(async move {
        run_pkce_authorized(
            TEST_CLIENT_ID,
            receiver,
            exchange,
            PkceRunOptions {
                no_browser: true,
                callback_timeout: Duration::from_secs(5),
            },
            verifier,
            state,
        )
        .await
    });

    deliver_callback(port, &format!("code=stub-code&state={state_for_callback}")).await;

    let response = driver.await.expect("driver task").expect("PKCE response");
    let user_token = response.user_token.expect("user token present");
    assert_eq!(user_token.expose_secret(), "xoxp-issued-token");
    assert_eq!(response.team_id, "T01");
    assert_eq!(response.team_name, "Acme");
    assert_eq!(response.user_id.as_deref(), Some("U01"));
    assert_eq!(
        response.scopes,
        vec!["users:read", "chat:write", "search:read"]
    );
}

#[tokio::test]
async fn pkce_flow_rejects_mismatched_state() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&mock_server)
        .await;

    let port = free_loopback_port();
    let receiver = LoopbackReceiver::bind(port).await.unwrap();
    let exchange = TokenExchange {
        api_base_url: mock_server.uri(),
        http: reqwest::Client::new(),
    };
    let verifier = PkceVerifier::from_raw(FIXED_VERIFIER);

    let driver = tokio::spawn(async move {
        run_pkce_authorized(
            TEST_CLIENT_ID,
            receiver,
            exchange,
            PkceRunOptions {
                no_browser: true,
                callback_timeout: Duration::from_secs(5),
            },
            verifier,
            "expected-state".to_string(),
        )
        .await
    });

    deliver_callback(port, "code=stub-code&state=wrong-state").await;

    let err = driver.await.unwrap().unwrap_err();
    assert!(
        matches!(err, OAuthError::StateMismatch),
        "expected StateMismatch, got {err:?}"
    );
}

#[tokio::test]
async fn pkce_flow_surfaces_token_exchange_error() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": false,
            "error": "invalid_code"
        })))
        .mount(&mock_server)
        .await;

    let port = free_loopback_port();
    let receiver = LoopbackReceiver::bind(port).await.unwrap();
    let exchange = TokenExchange {
        api_base_url: mock_server.uri(),
        http: reqwest::Client::new(),
    };
    let verifier = PkceVerifier::from_raw(FIXED_VERIFIER);
    let state = "valid-state".to_string();

    let state_cb = state.clone();
    let driver = tokio::spawn(async move {
        run_pkce_authorized(
            TEST_CLIENT_ID,
            receiver,
            exchange,
            PkceRunOptions {
                no_browser: true,
                callback_timeout: Duration::from_secs(5),
            },
            verifier,
            state,
        )
        .await
    });

    deliver_callback(port, &format!("code=bad-code&state={state_cb}")).await;

    let err = driver.await.unwrap().unwrap_err();
    assert!(
        matches!(err, OAuthError::ExchangeFailed(ref s) if s == "invalid_code"),
        "expected ExchangeFailed(invalid_code), got {err:?}"
    );
}
