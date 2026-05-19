//! Integration tests for the Slack messages client against a mock Slack API.
//!
//! Covers the user-visible contract: chat.postMessage / chat.update / chat.delete
//! / chat.getPermalink request shapes and conversations.history's
//! `include_all_metadata=true` invariant, plus end-to-end metadata
//! round-tripping through `SlackMessage`.

use std::sync::Arc;

use secrecy::SecretString;
use serde_json::{Value, json};
use slack_cli::auth::{AuthLoadOptions, Authenticator, EnvOverrides};
use slack_cli::config::Config;
use slack_cli::slack::{MessageMetadata, MessagePayload, SlackClient};
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn secret(value: &str) -> SecretString {
    SecretString::new(value.to_string().into_boxed_str())
}

/// Slack expects POST bodies as JSON; capture the last request via a hook.
struct CaptureBody {
    sink: tokio::sync::mpsc::UnboundedSender<Value>,
    response: serde_json::Value,
}

impl Respond for CaptureBody {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        if let Ok(body) = serde_json::from_slice::<Value>(&req.body) {
            let _ = self.sink.send(body);
        } else {
            let _ = self
                .sink
                .send(json!({"_raw": String::from_utf8_lossy(&req.body)}));
        }
        ResponseTemplate::new(200).set_body_json(self.response.clone())
    }
}

fn capture(
    response: serde_json::Value,
) -> (CaptureBody, tokio::sync::mpsc::UnboundedReceiver<Value>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (CaptureBody { sink: tx, response }, rx)
}

/// Returns the `SlackClient` plus the tempdir backing the (unused) auth store.
/// Bind the tempdir for the test's lifetime — dropping it cleans the fs.
async fn test_client(server: &MockServer) -> (SlackClient, tempfile::TempDir) {
    let mut config = Config::default();
    config.connection.api_base_url = server.uri();
    config.connection.rate_limit_per_minute = 600;

    let store_dir = tempfile::tempdir().unwrap();
    let store_path = store_dir.path().join("auth.json");

    let overrides = EnvOverrides {
        user_token: Some(secret("xoxp-test-user")),
        bot_token: Some(secret("xoxb-test-bot")),
    };
    let authenticator = Authenticator::load(AuthLoadOptions {
        store_path,
        overrides,
        explicit_profile: None,
    })
    .unwrap();

    let client = SlackClient::new(config, Arc::new(authenticator)).unwrap();
    (client, store_dir)
}

#[tokio::test]
async fn send_posts_blocks_metadata_and_thread() {
    let server = MockServer::start().await;
    let (responder, mut rx) = capture(json!({
        "ok": true,
        "channel": "C123",
        "ts": "1700000000.000200",
    }));
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .respond_with(responder)
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;

    let payload = MessagePayload {
        text: Some("fallback".into()),
        blocks: Some(vec![
            json!({"type": "section", "text": {"type": "mrkdwn", "text": "*hello*"}}),
        ]),
        attachments: None,
        metadata: Some(MessageMetadata {
            event_type: "deploy_done".into(),
            event_payload: json!({"version": "1.2.3"}),
        }),
    };

    let result = client
        .messages
        .send("C123", payload, Some("1700000000.000100"))
        .await
        .expect("send succeeds");

    assert_eq!(result.channel, "C123");
    assert_eq!(result.ts, "1700000000.000200");

    let body = rx.recv().await.unwrap();
    assert_eq!(body["channel"], json!("C123"));
    assert_eq!(body["text"], json!("fallback"));
    assert_eq!(body["blocks"][0]["type"], json!("section"));
    assert_eq!(body["thread_ts"], json!("1700000000.000100"));
    assert_eq!(body["metadata"]["event_type"], json!("deploy_done"));
    assert_eq!(body["metadata"]["event_payload"]["version"], json!("1.2.3"));
}

#[tokio::test]
async fn send_rejects_empty_payload_without_calling_api() {
    let server = MockServer::start().await;
    // No mock registered: any HTTP call would 404 and surface as an error
    // distinct from the validation error below.
    let (client, _store) = test_client(&server).await;

    let err = client
        .messages
        .send("C123", MessagePayload::default(), None)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("--text") && msg.contains("--blocks") && msg.contains("--attachments"));
}

#[tokio::test]
async fn update_clears_blocks_via_explicit_empty_array() {
    // chat.update with `blocks: []` is Slack's documented "remove blocks"
    // intent; the library must forward it untouched.
    let server = MockServer::start().await;
    let (responder, mut rx) = capture(json!({
        "ok": true,
        "channel": "C123",
        "ts": "1700000000.000200",
    }));
    Mock::given(method("POST"))
        .and(path("/chat.update"))
        .respond_with(responder)
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;
    let payload = MessagePayload {
        blocks: Some(vec![]),
        ..Default::default()
    };
    client
        .messages
        .update("C123", "1700000000.000200", payload)
        .await
        .expect("clear-blocks update succeeds");

    let body = rx.recv().await.unwrap();
    assert_eq!(body["blocks"], json!([]));
    assert!(body.get("text").is_none());
    assert!(body.get("attachments").is_none());
}

#[tokio::test]
async fn update_omits_thread_ts_field() {
    let server = MockServer::start().await;
    let (responder, mut rx) = capture(json!({
        "ok": true,
        "channel": "C123",
        "ts": "1700000000.000200",
    }));
    Mock::given(method("POST"))
        .and(path("/chat.update"))
        .respond_with(responder)
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;

    let payload = MessagePayload {
        text: Some("edited".into()),
        ..Default::default()
    };
    client
        .messages
        .update("C123", "1700000000.000200", payload)
        .await
        .unwrap();

    let body = rx.recv().await.unwrap();
    assert_eq!(body["channel"], json!("C123"));
    assert_eq!(body["ts"], json!("1700000000.000200"));
    assert_eq!(body["text"], json!("edited"));
    assert!(body.get("thread_ts").is_none());
}

#[tokio::test]
async fn permalink_returns_url_from_chat_get_permalink() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/chat.getPermalink"))
        .and(query_param("channel", "C123"))
        .and(query_param("message_ts", "1700000000.000200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": "C123",
            "permalink": "https://acme.slack.com/archives/C123/p1700000000000200",
        })))
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;
    let link = client
        .messages
        .permalink("C123", "1700000000.000200")
        .await
        .unwrap();
    assert_eq!(
        link,
        "https://acme.slack.com/archives/C123/p1700000000000200"
    );
}

#[tokio::test]
async fn history_always_requests_include_all_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .and(query_param("include_all_metadata", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [
                {
                    "ts": "1700000000.000300",
                    "user": "U123",
                    "text": "post-deploy ping",
                    "metadata": {
                        "event_type": "deploy_done",
                        "event_payload": {"version": "1.2.3"}
                    }
                }
            ],
        })))
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;
    let (messages, _) = client
        .messages
        .history("C123", 10, None, None, None)
        .await
        .unwrap();

    assert_eq!(messages.len(), 1);
    let metadata = messages[0].metadata.as_ref().expect("metadata present");
    assert_eq!(metadata.event_type, "deploy_done");
    assert_eq!(metadata.event_payload["version"], json!("1.2.3"));
}

#[tokio::test]
async fn replies_always_requests_include_all_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.replies"))
        .and(query_param("include_all_metadata", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [
                {"ts": "1700000000.000100", "user": "U123", "text": "parent"},
                {"ts": "1700000000.000200", "user": "U456", "text": "reply"}
            ],
        })))
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;
    let messages = client
        .messages
        .replies("C123", "1700000000.000100", 50)
        .await
        .unwrap();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn delete_body_contains_channel_and_ts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.delete"))
        .and(body_partial_json(
            json!({"channel": "C123", "ts": "1700000000.000200"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": "C123",
            "ts": "1700000000.000200",
        })))
        .mount(&server)
        .await;

    let (client, _store) = test_client(&server).await;
    let result = client
        .messages
        .delete("C123", "1700000000.000200")
        .await
        .unwrap();
    assert_eq!(result.ts, "1700000000.000200");
}
