//! End-to-end tests for the Socket Mode daemon.
//!
//! A mock Slack Web API answers `auth.test`, `apps.connections.open` and
//! `conversations.history`, and a local WebSocket server stands in for Slack's
//! Socket Mode endpoint. That makes the whole pipeline — connect, receive,
//! acknowledge, deduplicate, evaluate, store, deliver — testable without a
//! workspace, which is the only way its ordering invariants stay honest.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use secrecy::SecretString;
use serde_json::{Value, json};
use slack_cli::auth::{AuthLoadOptions, Authenticator, EnvOverrides};
use slack_cli::config::{Config, EventRetention, EventsConfig, RuleConfig, SinkKind, default_sink};
use slack_cli::events::{self, DaemonLock, EventPaths, EventRuntime, StdoutFormat};
use slack_cli::slack::SlackClient;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const ME: &str = "U_TEST";

fn secret(value: &str) -> SecretString {
    SecretString::new(value.to_string().into_boxed_str())
}

/// Answers `conversations.replies` the way Slack does: the thread's parent is
/// always included, and everything else is filtered by `oldest`. A mock that
/// ignored `oldest` would make the pagination bound meaningless and hide what
/// the client-side cursor check is for.
struct ThreadReplies {
    parent: Value,
    replies: Vec<Value>,
}

impl Respond for ThreadReplies {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let oldest = req
            .url
            .query_pairs()
            .find(|(key, _)| key == "oldest")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        let mut messages = vec![self.parent.clone()];
        messages.extend(
            self.replies
                .iter()
                .filter(|reply| reply["ts"].as_str().unwrap_or_default() >= oldest.as_str())
                .cloned(),
        );

        ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": messages,
            "has_more": false,
        }))
    }
}

/// Answers `conversations.history` the way Slack does — only messages at or
/// after `oldest`. A mock that ignored it would let a wrong `oldest` pass.
struct HistorySince {
    messages: Vec<Value>,
}

impl Respond for HistorySince {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let oldest = req
            .url
            .query_pairs()
            .find(|(key, _)| key == "oldest")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        let messages: Vec<Value> = self
            .messages
            .iter()
            .filter(|m| m["ts"].as_str().unwrap_or_default() >= oldest.as_str())
            .cloned()
            .collect();

        ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "messages": messages, "has_more": false,
        }))
    }
}

/// Records the `Authorization` header of every request it answers, so a test
/// can assert which token a method was called with.
struct RecordAuth {
    sink: tokio::sync::mpsc::UnboundedSender<String>,
    response: Value,
}

impl Respond for RecordAuth {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let header = |name: &str| {
            req.headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let _ = self.sink.send(format!(
            "{} | {}",
            header("authorization"),
            header("content-type")
        ));
        ResponseTemplate::new(200).set_body_json(self.response.clone())
    }
}

/// A WebSocket server that greets, sends the given frames, then stays open so
/// the daemon does not reconnect mid-assertion. Acknowledgements are reported
/// as they arrive rather than at the end, because the connection outlives the
/// test's interest in it.
async fn socket_server(
    frames: Vec<Value>,
) -> (
    u16,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (acks, ack_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();

        ws.send(Message::text(
            json!({ "type": "hello", "num_connections": 1 }).to_string(),
        ))
        .await
        .unwrap();

        for frame in &frames {
            ws.send(Message::text(frame.to_string())).await.unwrap();
        }

        while let Some(Ok(message)) = ws.next().await {
            if let Ok(text) = message.to_text()
                && let Ok(value) = serde_json::from_str::<Value>(text)
                && let Some(id) = value.get("envelope_id").and_then(Value::as_str)
            {
                let _ = acks.send(id.to_string());
            }
        }
    });

    (port, ack_rx, handle)
}

async fn slack_for(server: &MockServer) -> (Arc<SlackClient>, Config, tempfile::TempDir) {
    let mut config = Config::default();
    config.connection.api_base_url = server.uri();
    config.connection.rate_limit_per_minute = 600;
    // These tests are about the pipeline, not about Slack's throttle. The
    // default distribution caps `conversations.history` at one request a
    // minute, which would make any test that pages sit out the clock.
    config.connection.app_distribution =
        slack_cli::config::SlackAppDistribution::MarketplaceOrInternal;

    let store_dir = tempfile::tempdir().unwrap();
    let authenticator = Authenticator::load(AuthLoadOptions {
        store_path: store_dir.path().join("auth.json"),
        api_base_url: server.uri(),
        overrides: EnvOverrides {
            user_token: Some(secret("xoxp-test-user")),
            bot_token: None,
            app_token: Some(secret("xapp-test-app")),
        },
        explicit_profile: None,
    })
    .await
    .unwrap();

    let slack = Arc::new(SlackClient::new(config.clone(), Arc::new(authenticator)).unwrap());
    (slack, config, store_dir)
}

fn mention_rule() -> RuleConfig {
    RuleConfig {
        name: "mention".into(),
        on: vec![slack_cli::config::EventKindConfig::Message],
        mentions_me: true,
        keywords: Vec::new(),
        from_users: Vec::new(),
        channels: Vec::new(),
        subscribe_emoji: None,
        include_own_messages: false,
        sinks: Vec::new(),
    }
}

/// A spooling configuration whose only sink is a command that always
/// succeeds, so delivery is exercised without writing to the test's stdout.
fn spooling(rules: Vec<RuleConfig>) -> EventsConfig {
    EventsConfig {
        mode: EventRetention::Spool,
        store_body: true,
        rules,
        sinks: vec![slack_cli::config::SinkConfig {
            kind: SinkKind::Exec,
            command: vec!["true".into()],
            ..default_sink("agent")
        }],
        ..EventsConfig::default()
    }
}

async fn mount_common(
    server: &MockServer,
    ws_port: u16,
) -> tokio::sync::mpsc::UnboundedReceiver<String> {
    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "team": "Acme",
            "team_id": "T01",
            "user": "tester",
            "user_id": ME,
        })))
        .mount(server)
        .await;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    Mock::given(method("POST"))
        .and(path("/apps.connections.open"))
        .respond_with(RecordAuth {
            sink: tx,
            response: json!({ "ok": true, "url": format!("ws://127.0.0.1:{ws_port}") }),
        })
        .mount(server)
        .await;

    rx
}

fn message_envelope(id: &str, ts: &str, text: &str) -> Value {
    json!({
        "type": "events_api",
        "envelope_id": id,
        "payload": {
            "team_id": "T01",
            "event_id": format!("Ev{id}"),
            "event": {
                "type": "message",
                "channel": "C0000001",
                "channel_type": "channel",
                "user": "U_OTHER",
                "text": text,
                "ts": ts,
            },
        },
    })
}

/// Polls until the store has `expected` events, or gives up. The daemon is
/// asynchronous by construction, so the test waits on its effect rather than
/// on a duration.
async fn wait_for_events(runtime: &EventRuntime, expected: usize) -> Vec<slack_cli::events::Event> {
    for _ in 0..100 {
        let events = runtime.store.pull("test", None, 50).unwrap();
        if events.len() >= expected {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    runtime.store.pull("test", None, 50).unwrap()
}

#[tokio::test]
async fn a_mention_travels_from_the_socket_into_the_event_log() {
    let (port, mut acks, _socket) = socket_server(vec![
        message_envelope("env-1", "1700000000.000100", "hey <@U_TEST> take a look"),
        message_envelope("env-2", "1700000001.000100", "unrelated chatter"),
    ])
    .await;

    let server = MockServer::start().await;
    let mut auth_headers = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;

    // Both envelopes are acknowledged, matched or not: acknowledgement is a
    // durability boundary, not a statement that the event was interesting.
    // Collected while the daemon is still up — the unmatched envelope produces
    // nothing to wait on in the store, so aborting first would race its ack.
    let mut acknowledged = Vec::new();
    while acknowledged.len() < 2 {
        match tokio::time::timeout(Duration::from_secs(5), acks.recv()).await {
            Ok(Some(id)) => acknowledged.push(id),
            _ => break,
        }
    }

    // The connection is opened with the app-level token and nothing else.
    let header = tokio::time::timeout(Duration::from_secs(5), auth_headers.recv())
        .await
        .expect("apps.connections.open should have been called")
        .unwrap();

    daemon.abort();

    assert_eq!(stored.len(), 1, "only the mention should have been stored");
    assert_eq!(stored[0].matched, vec!["mention".to_string()]);
    assert_eq!(stored[0].text.as_deref(), Some("hey <@U_TEST> take a look"));
    assert_eq!(stored[0].channel.as_deref(), Some("C0000001"));
    assert!(
        acknowledged.contains(&"env-1".to_string()) && acknowledged.contains(&"env-2".to_string()),
        "every envelope must be acknowledged, got {acknowledged:?}"
    );
    // The app-level token opens the connection, and Slack documents this one
    // method as form-encoded — the single call the daemon cannot start without.
    assert_eq!(
        header,
        "Bearer xapp-test-app | application/x-www-form-urlencoded"
    );
}

#[tokio::test]
async fn a_redelivered_envelope_is_stored_once() {
    let (port, _acks, _socket) = socket_server(vec![
        message_envelope("env-1", "1700000000.000100", "hi <@U_TEST>"),
        // Slack's redelivery: a new envelope id for the same message.
        message_envelope("env-2", "1700000000.000100", "hi <@U_TEST>"),
    ])
    .await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = runtime.store.pull("test", None, 50).unwrap();
    daemon.abort();

    assert_eq!(stored.len(), 1);
    assert_eq!(after.len(), 1, "the same message must not be stored twice");
}

#[tokio::test]
async fn streaming_mode_keeps_no_event_log_at_all() {
    // A timestamp from now, because the observable below is the recovery
    // cursor and that is bounded by `backfill_max_age_hours`.
    let recent = format!("{}.000100", chrono::Utc::now().timestamp() - 5);
    let (port, _acks, _socket) =
        socket_server(vec![message_envelope("env-1", &recent, "hi <@U_TEST>")]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = EventsConfig {
        mode: EventRetention::Stream,
        ..spooling(vec![mention_rule()])
    };

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    // Wait on something the daemon writes as it processes, not on the counters
    // — those only reach the record on the housekeeping tick, so polling them
    // would time out and leave every assertion below trivially true.
    let mut handled = false;
    for _ in 0..100 {
        handled = runtime
            .state
            .gaps(50)
            .unwrap()
            .iter()
            .any(|gap| gap.channel == "C0000001");
        if handled {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    daemon.abort();
    assert!(
        handled,
        "the daemon never processed the event, so the assertions below prove nothing"
    );

    assert!(!runtime.store.caps().durable);
    assert!(
        !runtime.paths.events_db().exists(),
        "streaming must not create an event log"
    );
    assert!(runtime.store.pull("test", None, 10).is_err());
    // The subscription and cursor database is still there: it holds positions,
    // never anything anyone said.
    assert!(runtime.paths.state_db().exists());
}

#[tokio::test]
async fn a_second_daemon_for_one_profile_is_refused() {
    let (port, _acks, _socket) = socket_server(vec![]).await;
    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let first = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        let slack = slack.clone();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    // Wait for the first to actually hold the lock rather than guessing at a
    // duration: on a slow runner a fixed sleep is a race the test loses by
    // starting the second daemon before the first has claimed anything.
    let held = EventPaths::new(dir.path(), "test").lock_file();
    for _ in 0..200 {
        if held.exists() && DaemonLock::acquire(&held).is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    let second = events::run(
        slack,
        config,
        runtime,
        events::DaemonOptions {
            stdout: StdoutFormat::Ndjson,
            stdout_only: false,
            announce: false,
        },
    )
    .await;
    first.abort();

    let err = second.expect_err("a second daemon must be refused");
    assert!(err.to_string().contains("already running"), "{err}");
}

/// Socket Mode replays nothing across a disconnect, so recovery is the only
/// thing standing between a restarted daemon and a hole in its coverage.
#[tokio::test]
async fn a_reconnect_recovers_what_the_gap_swallowed() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    let missed_ts = format!("{}.000100", chrono::Utc::now().timestamp() - 30);
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [{
                "ts": missed_ts,
                "user": "U_OTHER",
                "text": "while you were away <@U_TEST>",
            }],
            "has_more": false,
        })))
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    // The daemon was following this channel when it went down.
    runtime
        .state
        .advance_cursor(
            "C0000001",
            &format!("{}.000000", chrono::Utc::now().timestamp() - 120),
            true,
        )
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;
    daemon.abort();

    assert_eq!(
        stored.len(),
        1,
        "the missed mention should have been recovered"
    );
    assert_eq!(
        stored[0].source,
        slack_cli::events::EventSource::Backfill,
        "and it should say where it came from"
    );
    assert_eq!(
        stored[0].text.as_deref(),
        Some("while you were away <@U_TEST>")
    );
}

/// A channel no rule cares about is never read back, because recovery runs on
/// `conversations.history` — one request a minute for a non-Marketplace app.
#[tokio::test]
async fn recovery_never_reaches_a_channel_no_rule_cares_about() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .advance_cursor(
            "C0000009",
            &format!("{}.000000", chrono::Utc::now().timestamp() - 60),
            false,
        )
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    tokio::time::sleep(Duration::from_millis(600)).await;
    daemon.abort();

    // No `conversations.history` mock is mounted, so any attempt to read it
    // would have failed the request rather than silently doing nothing.
    let history_calls = server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request.url.path() == "/conversations.history")
        .count();
    assert_eq!(history_calls, 0);
}

/// Recovery reads `conversations.history`, which a non-Marketplace app may
/// call once a minute — twenty channels is twenty minutes. If that ran in
/// front of the read loop, the socket would sit open and unacknowledged for
/// all of it, Slack would redeliver, and past its threshold it would stop
/// delivering to the app. So a live event must overtake a slow recovery.
#[tokio::test]
async fn a_live_event_does_not_wait_for_a_slow_recovery() {
    let (port, _acks, _socket) = socket_server(vec![message_envelope(
        "env-live",
        "1700000000.000100",
        "live <@U_TEST>",
    )])
    .await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(20))
                .set_body_json(json!({ "ok": true, "messages": [], "has_more": false })),
        )
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .advance_cursor(
            "C0000001",
            &format!("{}.000000", chrono::Utc::now().timestamp() - 120),
            true,
        )
        .unwrap();

    let started = std::time::Instant::now();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;
    let elapsed = started.elapsed();
    daemon.abort();

    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].source, slack_cli::events::EventSource::Socket);
    assert!(
        elapsed < Duration::from_secs(10),
        "the live event waited {elapsed:?} for a recovery that takes 20s"
    );
}

/// A daemon outlives a network blip, but a missing token or a refused scope
/// will still be there next minute. Retrying it forever would bury the one
/// message that says what to do.
#[tokio::test]
async fn a_refused_authorization_stops_the_daemon_rather_than_looping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "team": "Acme",
            "team_id": "T01",
            "user": "tester",
            "user_id": ME,
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/apps.connections.open"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "ok": false, "error": "invalid_auth" })),
        )
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();

    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        events::run(
            slack,
            config,
            runtime,
            events::DaemonOptions {
                stdout: StdoutFormat::Ndjson,
                stdout_only: false,
                announce: false,
            },
        ),
    )
    .await
    .expect("the daemon must give up rather than retry a refused authorization forever");

    let err = outcome.expect_err("a refused authorization is not a transient failure");
    assert!(err.to_string().contains("invalid_auth"), "{err}");
}

/// `conversations.history` returns the newest messages in a range first, so
/// reading one page of a deep gap would move the cursor past a middle nobody
/// ever saw. Recovery pages until Slack runs out, within its bound.
#[tokio::test]
async fn recovery_pages_through_a_gap_deeper_than_one_response() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    let newest = format!("{}.000100", chrono::Utc::now().timestamp() - 20);
    let older = format!("{}.000100", chrono::Utc::now().timestamp() - 40);

    // First request (no cursor) hands back a page and a cursor.
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .and(wiremock::matchers::query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [{ "ts": newest, "user": "U_OTHER", "text": "newest <@U_TEST>" }],
            "has_more": true,
            "response_metadata": { "next_cursor": "page-2" },
        })))
        .mount(&server)
        .await;

    // Following it reaches the rest of the gap.
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .and(wiremock::matchers::query_param("cursor", "page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [{ "ts": older, "user": "U_OTHER", "text": "older <@U_TEST>" }],
            "has_more": false,
        })))
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .advance_cursor(
            "C0000001",
            &format!("{}.000000", chrono::Utc::now().timestamp() - 300),
            true,
        )
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 2).await;
    daemon.abort();

    if stored.len() != 2 {
        let seen: Vec<_> = server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| format!("{} {}", r.method, r.url))
            .collect();
        panic!(
            "expected both pages, got {} — requests: {seen:#?}",
            stored.len()
        );
    }
    let texts: Vec<_> = stored.iter().filter_map(|e| e.text.as_deref()).collect();
    assert!(texts.contains(&"newest <@U_TEST>"));
    assert!(texts.contains(&"older <@U_TEST>"));
}

/// An unmatched event still moves the channel's cursor — that is how recovery
/// knows where it got to — but it must not make the channel worth recovering,
/// or a workspace's every channel would join the rationed catch-up list.
#[tokio::test]
async fn an_unmatched_event_moves_the_cursor_without_claiming_recovery() {
    let quiet_ts = format!("{}.000100", chrono::Utc::now().timestamp() - 5);
    let (port, _acks, _socket) = socket_server(vec![
        json!({
            "type": "events_api",
            "envelope_id": "env-quiet",
            "payload": {
                "team_id": "T01",
                "event_id": "Ev-quiet",
                "event": {
                    "type": "message",
                    "channel": "C0000002",
                    "channel_type": "channel",
                    "user": "U_OTHER",
                    "text": "nothing to do with anyone",
                    "ts": quiet_ts,
                },
            },
        }),
        message_envelope("env-loud", "1700000000.000100", "ping <@U_TEST>"),
    ])
    .await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;
    daemon.abort();

    assert_eq!(stored.len(), 1, "only the mention is worth storing");
    assert_eq!(stored[0].channel.as_deref(), Some("C0000001"));

    // The quiet channel was followed but is not a recovery candidate.
    let recoverable = runtime.state.gaps(50).unwrap();
    assert!(
        !recoverable.iter().any(|gap| gap.channel == "C0000002"),
        "a channel that matched nothing must not join the catch-up list: {recoverable:?}"
    );
}

fn watched_thread_rule() -> RuleConfig {
    RuleConfig {
        name: "watched".into(),
        on: vec![
            slack_cli::config::EventKindConfig::Message,
            slack_cli::config::EventKindConfig::ReactionAdded,
            slack_cli::config::EventKindConfig::ReactionRemoved,
        ],
        mentions_me: false,
        keywords: Vec::new(),
        from_users: Vec::new(),
        channels: Vec::new(),
        subscribe_emoji: Some("eyes".into()),
        include_own_messages: false,
        sinks: Vec::new(),
    }
}

fn reaction_envelope(id: &str, kind: &str, item_ts: &str, event_ts: &str, user: &str) -> Value {
    json!({
        "type": "events_api",
        "envelope_id": id,
        "payload": {
            "team_id": "T01",
            "event_id": format!("Ev{id}"),
            "event": {
                "type": kind,
                "user": user,
                "reaction": "eyes",
                "item": { "type": "message", "channel": "C0000001", "ts": item_ts },
                "item_user": "U_OTHER",
                "event_ts": event_ts,
            },
        },
    })
}

fn thread_reply(id: &str, thread_ts: &str, ts: &str, text: &str) -> Value {
    json!({
        "type": "events_api",
        "envelope_id": id,
        "payload": {
            "team_id": "T01",
            "event_id": format!("Ev{id}"),
            "event": {
                "type": "message",
                "channel": "C0000001",
                "channel_type": "channel",
                "user": "U_OTHER",
                "text": text,
                "ts": ts,
                "thread_ts": thread_ts,
                "event_ts": ts,
            },
        },
    })
}

/// The whole point of the emoji rule, end to end: my reaction subscribes the
/// thread, the replies that follow reach the agent, taking the emoji off stops
/// them, and putting it back starts them again — the last of which only works
/// because a re-added reaction is a distinct delivery rather than a duplicate
/// of the first.
#[tokio::test]
async fn an_emoji_subscribes_a_thread_and_unsubscribes_it_again() {
    let root = "1700000000.000100";
    let (port, _acks, _socket) = socket_server(vec![
        // Before subscribing: not interesting.
        thread_reply("env-1", root, "1700000001.000100", "early reply"),
        reaction_envelope("env-2", "reaction_added", root, "1700000002.000000", ME),
        thread_reply("env-3", root, "1700000003.000100", "watched reply"),
        reaction_envelope("env-4", "reaction_removed", root, "1700000004.000000", ME),
        thread_reply("env-5", root, "1700000005.000100", "after unsubscribing"),
        // The re-add: a duplicate key here would leave the thread unwatched.
        reaction_envelope("env-6", "reaction_added", root, "1700000006.000000", ME),
        thread_reply("env-7", root, "1700000007.000100", "watched again"),
    ])
    .await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![watched_thread_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    wait_for_events(&runtime, 2).await;
    // Give anything that should not have matched a chance to show up.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let stored = runtime.store.pull("test", None, 50).unwrap();
    daemon.abort();

    let texts: Vec<_> = stored.iter().filter_map(|e| e.text.as_deref()).collect();
    assert_eq!(
        texts,
        vec!["watched reply", "watched again"],
        "only replies while the thread was subscribed should have matched"
    );
    assert!(
        stored
            .iter()
            .all(|e| e.matched == vec!["watched".to_string()])
    );
}

/// `conversations.history` returns a channel's top-level messages and never a
/// thread's replies, so without a thread pass the flow the emoji rule exists
/// for is exactly the one a disconnect loses.
#[tokio::test]
async fn recovery_reaches_replies_in_a_subscribed_thread() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    let root = "1700000000.000100";
    Mock::given(method("GET"))
        .and(path("/conversations.replies"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [
                { "ts": root, "user": "U_OTHER", "text": "the question" },
                {
                    "ts": "1700000050.000100",
                    "thread_ts": root,
                    "user": "U_OTHER",
                    "text": "the reply you missed",
                },
            ],
            "has_more": false,
        })))
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![watched_thread_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    // The subscription outlived the process that made it.
    runtime
        .state
        // Followed as far as the parent, which is where the reaction placed
        // the cursor when the subscription was made.
        .watch_thread("C0000001", root, "watched", "eyes", Some(ME), Some(root))
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;
    daemon.abort();

    let texts: Vec<_> = stored.iter().filter_map(|e| e.text.as_deref()).collect();
    assert_eq!(
        texts,
        vec!["the reply you missed"],
        "only what the thread cursor had not reached should be recovered"
    );
    assert!(
        stored
            .iter()
            .all(|e| e.source == slack_cli::events::EventSource::Backfill)
    );
}

/// The defect the thread cursor exists to prevent: recovery reading a
/// subscribed thread from the top on every reconnect. All the deduplication
/// layers expire — the seen keys after a day, the log rows as soon as a
/// consumer acknowledges them — so a thread followed over a weekend would be
/// re-delivered in full, repeatedly. The cursor is what makes the pass read
/// only what nobody has seen.
#[tokio::test]
async fn recovery_never_re_reads_a_thread_it_has_already_followed() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    let root = "1700000000.000100";
    let delivered = "1700000050.000100";
    // Slack answers `oldest` inclusively and always returns the parent, so
    // this is what a real reconnect sees once the thread has been followed.
    Mock::given(method("GET"))
        .and(path("/conversations.replies"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "messages": [
                { "ts": root, "user": "U_OTHER", "text": "the question" },
                {
                    "ts": delivered,
                    "thread_ts": root,
                    "user": "U_OTHER",
                    "text": "already delivered last week",
                },
            ],
            "has_more": false,
        })))
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![watched_thread_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .watch_thread("C0000001", root, "watched", "eyes", Some(ME), Some(root))
        .unwrap();
    // The daemon followed the thread this far before it went down — and the
    // spool has since been acknowledged and pruned, so nothing else remembers.
    runtime
        .state
        .advance_thread_cursor("C0000001", root, delivered)
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    // Wait for the recovery request itself, so "nothing was stored" cannot
    // pass merely because the pass had not run yet.
    for _ in 0..200 {
        let asked = server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .any(|r| r.url.path() == "/conversations.replies");
        if asked {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    daemon.abort();

    let stored = runtime.store.pull("test", None, 50).unwrap();
    assert!(
        stored.is_empty(),
        "a followed thread must not be delivered again: {:?}",
        stored
            .iter()
            .filter_map(|e| e.text.as_deref())
            .collect::<Vec<_>>()
    );
}

/// The other half: a reply that arrives live moves the thread's cursor, so the
/// next reconnect does not read it back.
#[tokio::test]
async fn a_live_reply_moves_the_thread_cursor() {
    let root = "1700000000.000100";
    let reply_ts = "1700000050.000100";
    let (port, _acks, _socket) =
        socket_server(vec![thread_reply("env-1", root, reply_ts, "a reply")]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![watched_thread_rule()]);
    // Keep recovery out of it; this is about the live path.
    config.events.backfill = false;

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .watch_thread("C0000001", root, "watched", "eyes", Some(ME), Some(root))
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    // Poll the cursor itself: it is committed after the event is stored, so
    // waiting on the store would read it a moment too early.
    let mut followed = String::new();
    for _ in 0..100 {
        followed = runtime.state.watched_threads(10).unwrap()[0]
            .cursor_ts
            .clone();
        if followed == reply_ts {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    daemon.abort();

    assert_eq!(
        followed, reply_ts,
        "the thread should be followed as far as the reply that was delivered"
    );
}

/// Every other test observes the log, which the pipeline writes *before* it
/// delivers — so they would all still pass if delivery were removed entirely.
/// This one watches the sink.
#[tokio::test]
async fn a_matched_event_actually_reaches_its_sink() {
    let (port, _acks, _socket) = socket_server(vec![message_envelope(
        "env-1",
        "1700000000.000100",
        "ping <@U_TEST>",
    )])
    .await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = EventsConfig {
        sinks: vec![slack_cli::config::SinkConfig {
            kind: SinkKind::Http,
            url: Some(format!("{}/hook", server.uri())),
            ..default_sink("agent")
        }],
        ..spooling(vec![mention_rule()])
    };

    // No local runtime handle: this test watches the sink, not the log.
    let dir = tempfile::tempdir().unwrap();
    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let mut posted: Option<Value> = None;
    for _ in 0..100 {
        posted = server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|request| request.url.path() == "/hook")
            .and_then(|request| serde_json::from_slice(&request.body).ok());
        if posted.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    daemon.abort();

    let posted = posted.expect("the matched event should have been delivered to the sink");
    assert_eq!(posted["schema"], "slack-cli.event/1");
    assert_eq!(posted["text"], "ping <@U_TEST>");
    assert_eq!(posted["matched"][0], "mention");
    assert_eq!(posted["channel"], "C0000001");
}

/// A long thread is where reading from the top goes wrong: the first page is
/// all messages the daemon has already delivered, and the replies it actually
/// missed are at the far end. The cursor is what makes recovery read the tail.
#[tokio::test]
async fn recovery_reads_the_tail_of_a_long_thread_not_its_head() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    let root = "1700000000.000100";
    let reply_ts = |index: usize| format!("17000001{index:02}.000100");
    let followed_to = reply_ts(55);

    let replies = (0..60)
        .map(|index| {
            json!({
                "ts": reply_ts(index),
                "thread_ts": root,
                "user": "U_OTHER",
                "text": format!("reply {index}"),
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/conversations.replies"))
        .respond_with(ThreadReplies {
            parent: json!({ "ts": root, "user": "U_OTHER", "text": "the question" }),
            replies,
        })
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![watched_thread_rule()]);

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .watch_thread("C0000001", root, "watched", "eyes", Some(ME), Some(root))
        .unwrap();
    runtime
        .state
        .advance_thread_cursor("C0000001", root, &followed_to)
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 4).await;
    daemon.abort();

    let mut texts: Vec<_> = stored.iter().filter_map(|e| e.text.as_deref()).collect();
    texts.sort_unstable();
    assert_eq!(
        texts,
        vec!["reply 56", "reply 57", "reply 58", "reply 59"],
        "only the replies past the cursor belong to a recovery"
    );
    assert!(
        !texts.contains(&"the question"),
        "the thread's own parent is not a missed reply"
    );
}

/// The weekend case. A daemon stops on Friday and starts on Monday; a mention
/// posted on Sunday is well inside `backfill_max_age_hours`, but the channel's
/// cursor is older than the horizon. Treating the horizon as an eligibility
/// cutoff would drop the whole channel — losing the Sunday mention too, in
/// silence, and then advancing the cursor past it on the next live message. It
/// is a clamp: the read starts at the horizon.
#[tokio::test]
async fn a_gap_older_than_the_horizon_is_read_from_the_horizon() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;

    let now = chrono::Utc::now().timestamp();
    let inside_the_window = format!("{}.000100", now - 6 * 3600);

    // Answers `oldest` the way Slack does, so the assertion is about what the
    // daemon asked for, not about what a permissive mock handed back.
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .respond_with(HistorySince {
            messages: vec![json!({
                "ts": inside_the_window,
                "user": "U_OTHER",
                "text": "sunday night <@U_TEST>",
            })],
        })
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);
    config.events.backfill_max_age_hours = 24;

    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    // Followed to Friday — three days before the horizon.
    runtime
        .state
        .advance_cursor("C0000001", &format!("{}.000000", now - 72 * 3600), true)
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    let stored = wait_for_events(&runtime, 1).await;
    daemon.abort();

    let texts: Vec<_> = stored.iter().filter_map(|e| e.text.as_deref()).collect();
    assert_eq!(
        texts,
        vec!["sunday night <@U_TEST>"],
        "a mention inside the window must survive a cursor older than it"
    );
}

/// A clamped read that finished has covered the horizon to now, so the channel
/// should be followed from the horizon afterwards. Without that it is
/// re-clamped, re-read at one rationed request a time, and re-warned about a
/// skipped stretch on every reconnect for as long as it stays quiet.
#[tokio::test]
async fn a_clamped_recovery_leaves_the_cursor_at_the_horizon() {
    let (port, _acks, _socket) = socket_server(vec![]).await;

    let server = MockServer::start().await;
    let _auth = mount_common(&server, port).await;
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .respond_with(HistorySince {
            messages: Vec::new(),
        })
        .mount(&server)
        .await;

    let (slack, mut config, _store_dir) = slack_for(&server).await;
    config.events = spooling(vec![mention_rule()]);
    config.events.backfill_max_age_hours = 24;

    let now = chrono::Utc::now().timestamp();
    let dir = tempfile::tempdir().unwrap();
    let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
    runtime
        .state
        .advance_cursor("C0000001", &format!("{}.000000", now - 72 * 3600), true)
        .unwrap();

    let daemon = {
        let config = config.clone();
        let runtime = EventRuntime::open(dir.path(), "test", &config).unwrap();
        tokio::spawn(async move {
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: StdoutFormat::Ndjson,
                    stdout_only: false,
                    announce: false,
                },
            )
            .await
        })
    };

    // The channel is quiet, so nothing is stored — wait on the cursor instead.
    //
    // Asserted as a window rather than an exact value: the daemon computes its
    // own horizon when recovery runs, which is necessarily a moment after the
    // one this test captured, so demanding the same second makes the test a
    // race the slower runner loses. What matters is that the cursor left
    // Friday and landed at roughly the horizon.
    let epoch = |cursor: &str| -> i64 {
        cursor
            .split('.')
            .next()
            .and_then(|whole| whole.parse().ok())
            .unwrap_or(0)
    };
    let arrived = |cursor: &str| {
        let at = epoch(cursor);
        at > now - 25 * 3600 && at < now - 23 * 3600
    };

    let mut followed = String::new();
    for _ in 0..100 {
        followed = runtime.state.gaps(10).unwrap()[0].last_ts.clone();
        if arrived(&followed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    daemon.abort();

    assert!(
        arrived(&followed),
        "a finished clamped read should leave the cursor at the horizon \
         (about {} , 24h back), got {followed} ({}h back)",
        now - 24 * 3600,
        (now - epoch(&followed)) / 3600
    );
}
