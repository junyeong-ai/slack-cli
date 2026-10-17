use std::sync::Arc;

use serde_json::json;
use slack_cli::auth::{AuthLoadOptions, Authenticator, EnvOverrides};
use slack_cli::config::Config;
use slack_cli::events::owner::EventOwner;
use slack_cli::slack::SlackClient;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn client(
    dir: &std::path::Path,
    endpoint: &str,
    user: Option<&str>,
    bot: Option<&str>,
) -> (Arc<Authenticator>, SlackClient) {
    let auth = Arc::new(
        Authenticator::load(AuthLoadOptions {
            store_path: dir.join("auth.json"),
            api_base_url: endpoint.to_owned(),
            explicit_profile: None,
            overrides: EnvOverrides {
                user_token: user.map(Into::into),
                bot_token: bot.map(Into::into),
                app_token: None,
            },
        })
        .await
        .unwrap(),
    );
    let mut config = Config::default();
    config.connection.api_base_url = endpoint.to_owned();
    (auth.clone(), SlackClient::new(config, auth).unwrap())
}

#[tokio::test]
async fn environment_stores_follow_verified_owners_across_token_changes_and_work_offline() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    for (token, team, user) in [
        ("xoxp-a", "TA", "UA"),
        ("xoxp-a-new", "TA", "UA"),
        ("xoxp-b", "TB", "UB"),
    ] {
        Mock::given(path("/auth.test"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"ok":true,"team":"team","team_id":team,"user":"user","user_id":user}),
            ))
            .expect(1)
            .mount(&server)
            .await;
    }
    let mut namespaces = Vec::new();
    for token in ["xoxp-a", "xoxp-a-new", "xoxp-b"] {
        let (auth, slack) = client(dir.path(), &server.uri(), Some(token), None).await;
        let owner = EventOwner::resolve(&auth, &slack, dir.path(), &server.uri(), None)
            .await
            .unwrap();
        namespaces.push(owner.namespace().unwrap());
        assert_eq!(
            EventOwner::resolve(&auth, &slack, dir.path(), &server.uri(), None)
                .await
                .unwrap()
                .namespace()
                .unwrap(),
            owner.namespace().unwrap()
        );
    }
    assert_eq!(namespaces[0], namespaces[1]);
    assert_ne!(namespaces[0], namespaces[2]);
    server.verify().await;
    server.reset().await;
    let (auth, slack) = client(dir.path(), &server.uri(), Some("xoxp-a"), None).await;
    assert_eq!(
        EventOwner::resolve(&auth, &slack, dir.path(), &server.uri(), None)
            .await
            .unwrap()
            .namespace()
            .unwrap(),
        namespaces[0]
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn mixed_workspace_credentials_never_create_a_binding() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    for (token, team) in [("xoxp-a", "TA"), ("xoxb-b", "TB")] {
        Mock::given(path("/auth.test"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"ok":true,"team":"team","team_id":team,"user":"user","user_id":"U1"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
    }
    let (auth, slack) = client(dir.path(), &server.uri(), Some("xoxp-a"), Some("xoxb-b")).await;
    assert!(
        EventOwner::resolve(&auth, &slack, dir.path(), &server.uri(), None)
            .await
            .is_err()
    );
    assert!(!dir.path().join("identities").exists());
}

#[tokio::test]
async fn named_stores_survive_relogin_but_never_change_principal_or_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let profile = |user: &str, time: &str| {
        json!({"version":2,"active_profile":"work","profiles":{"work":{
            "method":"static","workspace":{"team_id":"TA","team_name":"Team","user_id":user},
            "tokens":{"user":{"token":"xoxp-test"}},"authorized_at":time
        }}})
    };
    let mut namespaces = Vec::new();
    for (user, time, endpoint) in [
        ("UA", "2026-10-17T00:00:00Z", "http://127.0.0.1:1"),
        ("UA", "2026-10-17T01:00:00Z", "http://127.0.0.1:1/"),
        ("UB", "2026-10-17T02:00:00Z", "http://127.0.0.1:1"),
        ("UA", "2026-10-17T00:00:00Z", "http://127.0.0.1:2"),
    ] {
        std::fs::write(
            dir.path().join("auth.json"),
            profile(user, time).to_string(),
        )
        .unwrap();
        let (auth, slack) = client(dir.path(), endpoint, None, None).await;
        namespaces.push(
            EventOwner::resolve(&auth, &slack, dir.path(), endpoint, None)
                .await
                .unwrap()
                .namespace()
                .unwrap(),
        );
    }
    assert_eq!(namespaces[0], namespaces[1]);
    assert_ne!(namespaces[0], namespaces[2]);
    assert_ne!(namespaces[0], namespaces[3]);
}
