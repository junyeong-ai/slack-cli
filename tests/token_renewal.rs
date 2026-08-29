//! Integration tests for the credential lifecycle the `Authenticator` owns:
//! when a rotating token is renewed, when it is left alone, what reaches disk
//! afterwards, and how a store written by an older schema is carried forward.

use chrono::{Duration, Utc};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use slack_cli::auth::{AuthLoadOptions, Authenticator, EnvOverrides, TokenKind, TokenPolicy};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Store {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl Store {
    fn with(contents: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&contents).unwrap()).unwrap();
        Self { _dir: dir, path }
    }

    fn read(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.path).unwrap()).unwrap()
    }

    async fn authenticator(&self, api_base_url: String) -> Authenticator {
        Authenticator::load(AuthLoadOptions {
            store_path: self.path.clone(),
            api_base_url,
            overrides: EnvOverrides::default(),
            explicit_profile: None,
        })
        .await
        .unwrap()
    }
}

fn profile(user: Value, client: Value) -> Value {
    json!({
        "version": 2,
        "active_profile": "acme",
        "profiles": {
            "acme": {
                "method": "pkce",
                "workspace": {"team_id": "T01", "team_name": "Acme", "user_id": "U01"},
                "tokens": {"user": user},
                "client": client,
                "authorized_at": "2026-01-01T00:00:00Z"
            }
        }
    })
}

fn expiring_in(minutes: i64) -> Value {
    json!({
        "token": "xoxe.xoxp-current",
        "refresh_token": "xoxe-refresh-1",
        "expires_at": Utc::now() + Duration::minutes(minutes),
        "scopes": ["users:read"]
    })
}

async fn refresh_endpoint(server: &MockServer, response: Value) {
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=xoxe-refresh-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(server)
        .await;
}

fn renewed_user_token() -> Value {
    json!({
        "ok": true,
        "token_type": "user",
        "access_token": "xoxe.xoxp-renewed",
        "refresh_token": "xoxe-refresh-2",
        "expires_in": 43200,
        "scope": "users:read"
    })
}

#[tokio::test]
async fn an_expiring_token_is_renewed_before_it_is_handed_out() {
    let server = MockServer::start().await;
    refresh_endpoint(&server, renewed_user_token()).await;

    let store = Store::with(profile(expiring_in(30), json!({"id": "123.456"})));
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-renewed");
}

#[tokio::test]
async fn a_renewal_persists_the_successor_pair_to_disk() {
    let server = MockServer::start().await;
    refresh_endpoint(&server, renewed_user_token()).await;

    let store = Store::with(profile(expiring_in(30), json!({"id": "123.456"})));
    let auth = store.authenticator(server.uri()).await;
    auth.token_for(TokenPolicy::UserRequired).await.unwrap();

    let saved = store.read();
    let user = &saved["profiles"]["acme"]["tokens"]["user"];
    assert_eq!(user["token"], "xoxe.xoxp-renewed");
    assert_eq!(user["refresh_token"], "xoxe-refresh-2");
    assert_eq!(user["scopes"], json!(["users:read"]));
    assert_ne!(user["expires_at"], Value::Null);

    // A separate process reading the store afterwards must see the successor,
    // not spend the refresh token Slack has already revoked.
    let reopened = store.authenticator(server.uri()).await;
    let token = reopened.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-renewed");
}

#[tokio::test]
async fn a_token_outside_the_renewal_window_is_used_as_is() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let store = Store::with(profile(expiring_in(600), json!({"id": "123.456"})));
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-current");
}

#[tokio::test]
async fn a_token_without_an_expiry_is_never_renewed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let store = Store::with(profile(
        json!({"token": "xoxp-permanent", "scopes": []}),
        json!({"id": "123.456"}),
    ));
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxp-permanent");
}

#[tokio::test]
async fn a_token_that_cannot_be_renewed_is_used_until_it_actually_expires() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let store = Store::with(profile(
        json!({
            "token": "xoxe.xoxp-no-refresh",
            "expires_at": Utc::now() + Duration::minutes(30),
            "scopes": []
        }),
        json!({"id": "123.456"}),
    ));
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-no-refresh");
}

#[tokio::test]
async fn an_expired_token_without_a_refresh_token_asks_for_a_fresh_login() {
    let server = MockServer::start().await;
    let store = Store::with(profile(
        json!({
            "token": "xoxe.xoxp-dead",
            "expires_at": Utc::now() - Duration::hours(1),
            "scopes": []
        }),
        json!({"id": "123.456"}),
    ));
    let auth = store.authenticator(server.uri()).await;

    let err = auth
        .token_for(TokenPolicy::UserRequired)
        .await
        .expect_err("cannot renew");
    let message = err.to_string();
    assert!(message.contains("no refresh token"), "{message}");
    assert!(message.contains("slack-cli auth login"), "{message}");
}

#[tokio::test]
async fn a_spent_token_without_a_recorded_client_asks_for_a_fresh_login() {
    let server = MockServer::start().await;
    let mut contents = profile(expiring_in(-1), Value::Null);
    contents["profiles"]["acme"]
        .as_object_mut()
        .unwrap()
        .remove("client");
    let store = Store::with(contents);
    let auth = store.authenticator(server.uri()).await;

    let err = auth
        .token_for(TokenPolicy::UserRequired)
        .await
        .expect_err("cannot renew");
    let message = err.to_string();
    assert!(message.contains("not recorded"), "{message}");
}

#[tokio::test]
async fn a_confidential_client_renews_with_basic_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .and(header("authorization", "Basic MTIzLjQ1Njpzc2g="))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(renewed_user_token()))
        .mount(&server)
        .await;

    let store = Store::with(profile(
        expiring_in(30),
        json!({"id": "123.456", "secret": "ssh"}),
    ));
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-renewed");
}

#[tokio::test]
async fn a_refusal_from_slack_is_reported_against_the_profile() {
    let server = MockServer::start().await;
    refresh_endpoint(
        &server,
        json!({"ok": false, "error": "invalid_refresh_token"}),
    )
    .await;

    let store = Store::with(profile(expiring_in(-1), json!({"id": "123.456"})));
    let auth = store.authenticator(server.uri()).await;

    let err = auth
        .token_for(TokenPolicy::UserRequired)
        .await
        .expect_err("renewal fails");
    let message = err.to_string();
    assert!(message.contains("acme"), "{message}");
    assert!(message.contains("invalid_refresh_token"), "{message}");
}

#[tokio::test]
async fn a_schema_1_store_is_carried_forward_on_first_open() {
    let server = MockServer::start().await;
    let store = Store::with(json!({
        "version": 1,
        "active_profile": "acme",
        "profiles": {
            "acme": {
                "method": "pkce",
                "workspace": {"team_id": "T01", "team_name": "Acme", "user_id": "U01"},
                "tokens": {"user": "xoxp-legacy", "bot": "xoxb-legacy"},
                "scopes": ["users:read", "chat:write"],
                "client_id": "123.456",
                "authorized_at": "2026-01-01T00:00:00Z"
            }
        }
    }));

    let auth = store.authenticator(server.uri()).await;
    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxp-legacy");

    let saved = store.read();
    assert_eq!(saved["version"], 2);
    let profile = &saved["profiles"]["acme"];
    assert_eq!(profile["tokens"]["user"]["token"], "xoxp-legacy");
    assert_eq!(
        profile["tokens"]["user"]["scopes"],
        json!(["users:read", "chat:write"])
    );
    assert_eq!(profile["tokens"]["bot"]["token"], "xoxb-legacy");
    assert_eq!(profile["client"]["id"], "123.456");
    assert!(profile.get("client_id").is_none());
    assert!(profile.get("scopes").is_none());
}

/// Slack revokes a refresh token the moment it is used, so two invocations
/// racing to renew the same credential must not both spend it. The store lock
/// serialises them and the loser re-reads the successor the winner wrote.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_invocations_renew_a_credential_exactly_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth.v2.access"))
        .and(body_string_contains("refresh_token=xoxe-refresh-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(150))
                .set_body_json(renewed_user_token()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let store = Store::with(profile(expiring_in(30), json!({"id": "123.456"})));
    let first = store.authenticator(server.uri()).await;
    let second = store.authenticator(server.uri()).await;

    let (a, b) = tokio::join!(
        first.token_for(TokenPolicy::UserRequired),
        second.token_for(TokenPolicy::UserRequired),
    );

    assert_eq!(a.unwrap().expose_secret(), "xoxe.xoxp-renewed");
    assert_eq!(b.unwrap().expose_secret(), "xoxe.xoxp-renewed");

    // `.expect(1)` is asserted when the server drops; make that explicit here
    // so the reason for the assertion is visible at the point it matters.
    drop(server);
}

/// A credential only enters the renewal window while it is still valid, so a
/// refusal from Slack must not turn a working token into a failed command.
#[tokio::test]
async fn a_refused_renewal_falls_back_to_the_token_still_in_date() {
    let server = MockServer::start().await;
    refresh_endpoint(&server, json!({"ok": false, "error": "internal_error"})).await;

    let store = Store::with(profile(expiring_in(30), json!({"id": "123.456"})));
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-current");
}

#[tokio::test]
async fn a_missing_client_falls_back_to_the_token_still_in_date() {
    let server = MockServer::start().await;
    let mut contents = profile(expiring_in(30), Value::Null);
    contents["profiles"]["acme"]
        .as_object_mut()
        .unwrap()
        .remove("client");
    let store = Store::with(contents);
    let auth = store.authenticator(server.uri()).await;

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-current");
}

/// The path `auth status --verify` and `auth logout` take: a named profile
/// resolved and renewed the same way an API call would.
#[tokio::test]
async fn a_named_profile_renews_before_its_token_is_handed_out() {
    let server = MockServer::start().await;
    refresh_endpoint(&server, renewed_user_token()).await;

    let store = Store::with(profile(expiring_in(30), json!({"id": "123.456"})));
    let auth = store.authenticator(server.uri()).await;

    let token = auth
        .token_for_profile("acme", TokenKind::User)
        .await
        .unwrap();
    assert_eq!(token.expose_secret(), "xoxe.xoxp-renewed");
}

/// Opening an already-migrated store must not rewrite it: the upgrade write
/// belongs to the process that actually performs the upgrade, and only while
/// it holds the lock.
#[tokio::test]
async fn a_second_open_leaves_an_already_migrated_store_untouched() {
    let server = MockServer::start().await;
    let store = Store::with(json!({
        "version": 1,
        "active_profile": "acme",
        "profiles": {
            "acme": {
                "method": "static",
                "workspace": {"team_id": "T01", "team_name": "Acme"},
                "tokens": {"user": "xoxp-legacy"},
                "scopes": [],
                "authorized_at": "2026-01-01T00:00:00Z"
            }
        }
    }));

    let _first = store.authenticator(server.uri()).await;
    let migrated = std::fs::read(&store.path).unwrap();
    assert_eq!(store.read()["version"], 2);

    let _second = store.authenticator(server.uri()).await;
    assert_eq!(
        std::fs::read(&store.path).unwrap(),
        migrated,
        "a second open rewrote a store that was already at the current schema"
    );
}

#[tokio::test]
async fn environment_tokens_bypass_the_store_entirely() {
    let server = MockServer::start().await;
    let store = Store::with(profile(expiring_in(-60), json!({"id": "123.456"})));

    let auth = Authenticator::load(AuthLoadOptions {
        store_path: store.path.clone(),
        api_base_url: server.uri(),
        overrides: EnvOverrides {
            user_token: Some(secrecy::SecretString::new(
                "xoxp-from-env".to_string().into_boxed_str(),
            )),
            bot_token: None,
        },
        explicit_profile: None,
    })
    .await
    .unwrap();

    let token = auth.token_for(TokenPolicy::UserRequired).await.unwrap();
    assert_eq!(token.expose_secret(), "xoxp-from-env");
}
