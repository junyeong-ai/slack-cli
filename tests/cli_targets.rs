use serde_json::json;
use wiremock::matchers::{header, path, query_param};
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
async fn invalid_date_filters_fail_before_any_slack_request() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["messages", "C00000001", "--oldest", "NaN"],
        vec!["messages", "C00000001", "--latest", "1e100"],
        vec!["search", "query", "--before", "inf"],
        vec!["search", "query", "--after", "9223372036854775808"],
        vec!["messages", "#uncached", "--oldest", "NaN"],
        vec![
            "search",
            "query",
            "--channel",
            "#uncached",
            "--before",
            "NaN",
        ],
    ] {
        let output = command(dir.path(), &server)
            .env("SLACK_USER_TOKEN", "xoxp-test")
            .args(&args)
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Invalid Unix timestamp"),
            "{error}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_token_without_private_channel_access_still_targets_public_channels() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    Mock::given(path("/conversations.list"))
        .and(query_param("types", "private_channel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": false, "error": "missing_scope", "needed": "groups:read"
        })))
        .mount(&server)
        .await;
    Mock::given(path("/conversations.list"))
        .and(query_param("types", "public_channel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channels": [{"id": "C00000001", "name": "general", "is_channel": true}],
            "response_metadata": {"next_cursor": ""}
        })))
        .mount(&server)
        .await;
    Mock::given(path("/chat.postMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "channel": "C00000001", "ts": "1792200000.000001"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let fresh = tempfile::tempdir().unwrap();
    let missing = command(fresh.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .args(["send", "#private-team", "--text", "hello"])
        .output()
        .await
        .unwrap();
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    let error: serde_json::Value = serde_json::from_slice(&missing.stderr).unwrap();
    let message = error["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("fetched just now") && message.contains("groups:read"),
        "{message}"
    );

    let sent = command(dir.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .args(["send", "#general", "--text", "hello"])
        .output()
        .await
        .unwrap();
    assert!(sent.status.success(), "{sent:?}");

    let refreshed = command(dir.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .args(["cache", "refresh", "channels"])
        .output()
        .await
        .unwrap();
    assert!(refreshed.status.success(), "{refreshed:?}");
    let report: serde_json::Value = serde_json::from_slice(&refreshed.stdout).unwrap();
    assert_eq!(
        report["unreadable"],
        json!([{"type": "private_channel", "scope": "groups:read"}])
    );
}

#[tokio::test]
async fn a_token_that_may_list_no_channel_type_reports_the_refusal() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    Mock::given(path("/conversations.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": false, "error": "missing_scope"
        })))
        .mount(&server)
        .await;

    let output = command(dir.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .args(["send", "#general", "--text", "hello"])
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "missing_scope");
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
    // One channel request per default conversation type (public, private).
    for (endpoint, key, rows, requests) in [
        (
            "/conversations.list",
            "channels",
            json!([{"id":"C00000001","name":"removed","is_channel":true}]),
            2,
        ),
        (
            "/users.list",
            "members",
            json!([{"id":"U00000001","name":"removed","is_bot":false}]),
            1,
        ),
    ] {
        Mock::given(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,key:rows,"response_metadata":{"next_cursor":""}
            })))
            .expect(requests)
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
    for (endpoint, key, requests) in [
        ("/conversations.list", "channels", 2),
        ("/users.list", "members", 1),
    ] {
        Mock::given(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,key:[],"response_metadata":{"next_cursor":""}
            })))
            .expect(requests)
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

#[tokio::test]
async fn user_targets_use_the_operation_actor_instead_of_cached_human_dms() {
    use wiremock::matchers::{body_partial_json, method, query_param};
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    for (token, channels, cursor) in [
        (
            "xoxp-user",
            json!([{"id":"DHUMAN123","is_im":true,"user":"UTARGET12"}]),
            "",
        ),
        ("xoxb-bot", json!([]), "next-im-page"),
    ] {
        Mock::given(path("/conversations.list"))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,"channels":channels,"response_metadata":{"next_cursor":cursor}
            })))
            .with_priority(2)
            .mount(&server)
            .await;
    }
    Mock::given(path("/conversations.list"))
        .and(header("authorization", "Bearer xoxb-bot"))
        .and(query_param("cursor", "next-im-page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"channels":[{"id":"DBOT12345","is_im":true,"user":"UTARGET12"}],
            "response_metadata":{"next_cursor":""}
        })))
        .with_priority(1)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/chat.postMessage"))
        .and(header("authorization", "Bearer xoxb-bot"))
        .and(body_partial_json(json!({"channel":"DBOT12345"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"channel":"DBOT12345","ts":"1.000001"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .and(header("authorization", "Bearer xoxp-user"))
        .and(query_param("channel", "DHUMAN123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"messages":[],"has_more":false})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/bookmarks.list"))
        .and(header("authorization", "Bearer xoxb-bot"))
        .and(query_param("channel_id", "DBOT12345"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"bookmarks":[]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/assistant.search.context"))
        .and(header("authorization", "Bearer xoxp-user"))
        .and(body_partial_json(json!({"context_channel_id":"DHUMAN123"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"results":{}})))
        .expect(1)
        .mount(&server)
        .await;
    for args in [
        vec!["cache", "refresh", "channels"],
        vec!["send", "UTARGET12", "-t", "hello"],
        vec!["messages", "UTARGET12"],
        vec!["bookmarks", "UTARGET12"],
        vec!["search", "hello", "--channel", "UTARGET12"],
    ] {
        let output = command(dir.path(), &server)
            .env("SLACK_USER_TOKEN", "xoxp-user")
            .env("SLACK_BOT_TOKEN", "xoxb-bot")
            .args(&args)
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.url.path() != "/conversations.open")
    );
}

#[tokio::test]
async fn missing_bot_dm_never_falls_back_to_a_human_sender() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    Mock::given(path("/conversations.list"))
        .and(header("authorization", "Bearer xoxb-bot"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"ok":true,"channels":[],"response_metadata":{"next_cursor":""}}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = command(dir.path(), &server)
        .env("SLACK_USER_TOKEN", "xoxp-user")
        .env("SLACK_BOT_TOKEN", "xoxb-bot")
        .args(["send", "UTARGET12", "-t", "hello"])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("selected bot token"));
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.url.path() == "/conversations.list")
    );
}
