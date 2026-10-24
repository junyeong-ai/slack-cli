//! What reaches stdout and stderr, and what happens when either cannot be
//! written. A reader that stops early, as `| head` does, closes the pipe while
//! the CLI is still writing; Rust ignores `SIGPIPE`, so the write fails instead
//! of the signal ending the process. Only the binary shows what that failure,
//! or the log subscriber `main` installs, turns into.

use std::process::Command;

use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const BIN: &str = env!("CARGO_BIN_EXE_slack-cli");

#[test]
fn a_closed_stdout_ends_the_command_quietly() {
    let (reader, writer) = std::io::pipe().expect("pipe");
    drop(reader);
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");

    let output = Command::new(BIN)
        .args(["--config", config.to_str().unwrap(), "config", "path"])
        .stdout(writer)
        .output()
        .expect("binary runs");

    assert!(
        output.status.success(),
        "status {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_closed_stderr_keeps_the_exit_status() {
    let (reader, writer) = std::io::pipe().expect("pipe");
    drop(reader);
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[auth]\nexclude_scopes = [\"users:reed\"]\n").unwrap();

    for json in [true, false] {
        let mut command = Command::new(BIN);
        command.args(["--config", config.to_str().unwrap()]);
        if json {
            command.arg("--json");
        }
        let status = command
            .args(["users", "anyone"])
            .stderr(writer.try_clone().unwrap())
            .status()
            .expect("binary runs");
        assert_eq!(status.code(), Some(1), "json={json}: {status:?}");
    }
}

/// A command whose only request is refused with 429 once, so it logs one
/// rate-limit warning and then fails with `rate_limited`. The internal-app
/// distribution keeps `conversations.history` off the one-a-minute limit, so
/// the retry waits only for `Retry-After`.
async fn rate_limited(json: bool) -> std::process::Output {
    let server = MockServer::start().await;
    Mock::given(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[connection]\napi_base_url = {:?}\napp_distribution = \"marketplace_or_internal\"\n\n[retry]\nmax_attempts = 2\n",
            server.uri()
        ),
    )
    .unwrap();

    let mut command = tokio::process::Command::new(BIN);
    command
        .env("HOME", dir.path())
        .env("APPDATA", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("SLACK_USER_TOKEN", "xoxp-test")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_PROFILE")
        .env_remove("NO_COLOR")
        .env_remove("RUST_LOG")
        .arg("--config")
        .arg(&config)
        .arg("--data-dir")
        .arg(dir.path().join("cache"));
    if json {
        command.arg("--json");
    }
    let output = command
        .args(["messages", "C00000001", "--limit", "1"])
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    output
}

#[tokio::test]
async fn json_mode_writes_every_stderr_line_as_json() {
    let output = rate_limited(true).await;
    let stderr = String::from_utf8(output.stderr).unwrap();
    let lines: Vec<serde_json::Value> = stderr
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|err| panic!("{err}: {line}")))
        .collect();

    let [warning, envelope] = lines.as_slice() else {
        panic!("expected a warning and the error envelope: {stderr}");
    };
    assert_eq!(warning["level"], "WARN", "{warning}");
    assert_eq!(warning["fields"]["method"], "conversations.history");
    assert_eq!(envelope["error"]["code"], "rate_limited");
}

#[tokio::test]
async fn text_logs_carry_no_color_codes_off_a_terminal() {
    let output = rate_limited(false).await;
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Rate limited"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr:?}");
}
