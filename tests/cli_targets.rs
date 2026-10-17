use serde_json::json;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn command(dir: &std::path::Path, server: &MockServer) -> tokio::process::Command {
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        format!("[connection]\napi_base_url = {:?}\n", server.uri()),
    )
    .unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_slack-cli"));
    command
        .current_dir(dir)
        .env("HOME", dir)
        .env("APPDATA", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env_remove("SLACK_USER_TOKEN")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_APP_TOKEN")
        .env_remove("SLACK_PROFILE")
        .arg("--config")
        .arg(config)
        .arg("--data-dir")
        .arg(dir.join("cache"))
        .arg("--json");
    command
}

#[tokio::test]
async fn channel_operations_require_a_unique_exact_name() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    Mock::given(path("/conversations.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"channels":[
                {"id":"C00000001","name":"general-archive","is_channel":true},
                {"id":"C00000002","name":"exact","is_channel":true},
                {"id":"C00000003","name":"duplicate","is_channel":true},
                {"id":"C00000004","name":"duplicate","is_channel":true}
            ],"response_metadata":{"next_cursor":""}
        })))
        .mount(&server)
        .await;
    Mock::given(path("/chat.postMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"channel":"C00000002","ts":"1792200000.000001"
        })))
        .expect(1)
        .mount(&server)
        .await;
    for (name, succeeds) in [
        ("#general", false),
        ("#exac", false),
        ("#duplicate", false),
        ("#EXACT", true),
    ] {
        let output = command(dir.path(), &server)
            .env("SLACK_USER_TOKEN", "xoxp-test")
            .args(["send", name, "--text", "requested message"])
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.success(), succeeds, "{name}: {output:?}");
    }
    let posts: Vec<_> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path() == "/chat.postMessage")
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&posts[0].body).unwrap()["channel"],
        "C00000002"
    );
}

#[tokio::test]
async fn static_login_validates_secondary_tokens_before_changing_the_store() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    for (token, response) in [
        (
            "xoxp-user",
            json!({"ok":true,"team":"Team","team_id":"TA","user":"Alice","user_id":"UA"}),
        ),
        (
            "xoxb-other",
            json!({"ok":true,"team":"Other","team_id":"TB","user":"Bot","user_id":"UB"}),
        ),
        ("xoxb-invalid", json!({"ok":false,"error":"invalid_auth"})),
        (
            "xoxb-valid",
            json!({"ok":true,"team":"Team","team_id":"TA","user":"Bot","user_id":"UB"}),
        ),
    ] {
        Mock::given(path("/auth.test"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;
    }
    let output = command(dir.path(), &server)
        .args([
            "--profile",
            "work",
            "auth",
            "login",
            "--method",
            "static",
            "--user-token",
            "xoxp-user",
        ])
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let status = command(dir.path(), &server)
        .args(["--profile", "work", "auth", "status"])
        .output()
        .await
        .unwrap();
    for bot in ["xoxb-other", "xoxb-invalid"] {
        let output = command(dir.path(), &server)
            .args([
                "--profile",
                "work",
                "auth",
                "login",
                "--method",
                "static",
                "--user-token",
                "xoxp-user",
                "--bot-token",
                bot,
            ])
            .output()
            .await
            .unwrap();
        assert!(!output.status.success(), "{bot}: {output:?}");
        let unchanged = command(dir.path(), &server)
            .args(["--profile", "work", "auth", "status"])
            .output()
            .await
            .unwrap();
        assert_eq!(unchanged.stdout, status.stdout);
    }
    let output = command(dir.path(), &server)
        .args([
            "--profile",
            "work",
            "auth",
            "login",
            "--method",
            "static",
            "--user-token",
            "xoxp-user",
            "--bot-token",
            "xoxb-valid",
        ])
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[tokio::test]
async fn empty_snapshots_replace_stale_rows_and_are_reused_without_refetching() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    for (endpoint, key, rows) in [
        (
            "/conversations.list",
            "channels",
            json!([{"id":"C00000001","name":"removed","is_channel":true}]),
        ),
        (
            "/users.list",
            "members",
            json!([{"id":"U00000001","name":"removed","is_bot":false}]),
        ),
    ] {
        Mock::given(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,key:rows,"response_metadata":{"next_cursor":""}
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let initial = command(dir.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .args(["cache", "refresh"])
        .output()
        .await
        .unwrap();
    assert!(initial.status.success(), "{initial:?}");
    server.verify().await;
    server.reset().await;
    for (endpoint, key) in [
        ("/conversations.list", "channels"),
        ("/users.list", "members"),
    ] {
        Mock::given(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,key:[],"response_metadata":{"next_cursor":""}
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let refreshed = command(dir.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .args(["cache", "refresh"])
        .output()
        .await
        .unwrap();
    assert!(refreshed.status.success(), "{refreshed:?}");
    for domain in ["users", "channels"] {
        for _ in 0..2 {
            let read = command(dir.path(), &server)
                .env("SLACK_USER_TOKEN", "xoxp-test")
                .args([domain, "removed"])
                .output()
                .await
                .unwrap();
            assert!(read.status.success(), "{read:?}");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&read.stdout).unwrap(),
                json!([])
            );
        }
    }
}
