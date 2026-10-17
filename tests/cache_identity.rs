#![cfg(unix)]

use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn profiles_and_environment_credentials_never_share_cached_results() {
    let server = MockServer::start().await;
    for (token, name) in [
        ("xoxp-a", "alice"),
        ("xoxp-b", "bob"),
        ("xoxp-env-a", "carol"),
        ("xoxp-env-b", "dave"),
    ] {
        Mock::given(method("GET"))
            .and(path("/users.list"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,"members":[{"id":format!("U{name}"),"name":name,"is_bot":false}],
                "response_metadata":{"next_cursor":""}
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let dir = tempfile::tempdir().unwrap();
    let auth_dir = dir.path().join("slack-cli");
    std::fs::create_dir(&auth_dir).unwrap();
    let mut profiles = serde_json::Map::new();
    for name in ["a", "b"] {
        profiles.insert(name.into(), json!({
            "method":"static","workspace":{"team_id":format!("T{name}"),"team_name":name},
            "tokens":{"user":{"token":format!("xoxp-{name}")}},"authorized_at":"2026-10-17T00:00:00Z"
        }));
    }
    std::fs::write(
        auth_dir.join("auth.json"),
        json!({"version":2,"active_profile":"a","profiles":profiles}).to_string(),
    )
    .unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[connection]\napi_base_url = {:?}\n", server.uri()),
    )
    .unwrap();
    for (profile, token, expected) in [
        ("a", None, "alice"),
        ("b", None, "bob"),
        ("a", None, "alice"),
        ("a", Some("xoxp-env-a"), "carol"),
        ("a", Some("xoxp-env-b"), "dave"),
    ] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_slack-cli"));
        command
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .env("XDG_CONFIG_HOME", dir.path())
            .env_remove("SLACK_USER_TOKEN")
            .env_remove("SLACK_BOT_TOKEN")
            .env_remove("SLACK_APP_TOKEN")
            .env_remove("SLACK_PROFILE")
            .arg("--config")
            .arg(&config)
            .arg("--data-dir")
            .arg(dir.path().join("shared-cache"))
            .args(["--profile", profile, "--json", "users", ""]);
        if let Some(token) = token {
            command.env("SLACK_USER_TOKEN", token);
        }
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let users: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(users[0]["name"], expected);
    }
    use std::os::unix::fs::PermissionsExt;
    for entry in std::fs::read_dir(dir.path().join("shared-cache")).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
