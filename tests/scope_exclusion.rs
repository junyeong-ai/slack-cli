//! `exclude_scopes` only means anything if `auth login` reads it. The wiring
//! from the config to the authorization request runs inside `run`, so only the
//! binary reaches it — a unit test on the helper passes whether or not the
//! caller hands it the configured list.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_slack-cli");

/// The config directory is resolved per platform — XDG on Unix, the Known
/// Folders on Windows — so a test that redirects only `HOME` still reads and
/// writes the developer's real store on Windows.
fn isolate(command: &mut Command, dir: &std::path::Path) {
    command
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env("USERPROFILE", dir)
        .env("APPDATA", dir)
        .env("LOCALAPPDATA", dir);
}

fn free_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// A config excluding every user scope but the last `keep` of them, and the
/// scopes it leaves behind.
fn excluding_all_but(dir: &std::path::Path, keep: usize) -> (std::path::PathBuf, Vec<String>) {
    let empty = dir.join("defaults.toml");
    std::fs::write(&empty, "").unwrap();
    let mut listed = Command::new(BIN);
    listed.args([
        "--config",
        empty.to_str().unwrap(),
        "--json",
        "auth",
        "scopes",
    ]);
    isolate(&mut listed, dir);
    let listed = listed.output().expect("binary runs");
    assert!(
        listed.status.success(),
        "auth scopes failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let scopes: serde_json::Value =
        serde_json::from_slice(&listed.stdout).expect("auth scopes emits JSON");
    let entries: Vec<&str> = scopes["user"]
        .as_array()
        .expect("user scopes")
        .iter()
        .map(|scope| scope.as_str().expect("scope is a string"))
        .collect();
    assert!(entries.len() > keep, "the CLI must request something");

    let split = entries.len() - keep;
    let excluded = &entries[..split];
    let kept: Vec<String> = entries[split..].iter().map(|s| (*s).to_string()).collect();

    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        format!(
            "[auth]\nclient_id = \"1.2\"\nexclude_scopes = {}\n",
            serde_json::to_string(excluded).unwrap()
        ),
    )
    .unwrap();
    (path, kept)
}

#[test]
fn a_login_excluding_every_scope_is_refused_before_the_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let (path, kept) = excluding_all_but(dir.path(), 0);
    assert!(kept.is_empty());

    let mut command = Command::new(BIN);
    command
        .args([
            "--config",
            path.to_str().unwrap(),
            "auth",
            "login",
            "--method",
            "pkce",
            "--no-browser",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    isolate(&mut command, dir.path());
    let mut child = command.spawn().expect("binary runs");

    // A login that is not refused binds the callback port and waits minutes for
    // a redirect, so finishing at all is part of what this asserts.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().expect("child is waitable") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            // It is holding the callback port and would keep it for minutes.
            child.kill().ok();
            child.wait().ok();
            panic!("the login was not refused; it waited for a callback");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let output = child.wait_with_output().expect("output is readable");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!status.success(), "stderr: {stderr}");
    assert!(stderr.contains("exclude_scopes"), "{stderr}");
    assert!(
        !stdout.contains("slack.com/oauth"),
        "no authorize URL may be offered: {stdout}"
    );
}

/// The refusal must be about there being nothing left, not about there being
/// exclusions at all: a partial exclusion still authorizes, asking for exactly
/// what survived.
#[test]
fn a_partial_exclusion_still_authorizes_for_what_remains() {
    let dir = tempfile::tempdir().unwrap();
    let (path, kept) = excluding_all_but(dir.path(), 1);
    let port = free_loopback_port().to_string();

    let mut command = Command::new(BIN);
    command
        .args([
            "--config",
            path.to_str().unwrap(),
            "auth",
            "login",
            "--method",
            "pkce",
            "--no-browser",
            "--port",
            &port,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    isolate(&mut command, dir.path());
    let mut child = command.spawn().expect("binary runs");

    // It offers the URL and then waits for a redirect that never comes, so the
    // URL is read from the pipe rather than by waiting for the process.
    let mut reader = std::io::BufReader::new(child.stderr.take().expect("stderr piped"));
    let url = read_authorize_url(&mut reader);
    child.kill().ok();
    child.wait().ok();

    let url = url.expect("an authorize URL is offered");
    // Compared as a set: one scope is a prefix of another, so a substring
    // check would find an excluded scope inside a requested one.
    assert_eq!(requested_scopes(&url), kept, "url: {url}");
}

/// The `user_scope` parameter, decoded. Only the two escapes the CLI can emit
/// for a scope name are handled, so an unexpected encoding fails loudly here
/// rather than quietly comparing equal.
fn requested_scopes(url: &str) -> Vec<String> {
    let query = url.split_once("user_scope=").expect("user_scope present").1;
    let raw = query.split('&').next().unwrap_or_default();
    assert!(
        !raw.contains('%')
            || raw
                .replace("%3A", "")
                .replace("%2C", "")
                .find('%')
                .is_none(),
        "unhandled escape in {raw}"
    );
    raw.replace("%3A", ":")
        .replace("%2C", ",")
        .split(',')
        .filter(|scope| !scope.is_empty())
        .map(str::to_string)
        .collect()
}

/// The CLI prints the authorization URL on stderr when it cannot open a
/// browser. Reads until it appears or the stream ends.
fn read_authorize_url(reader: &mut impl std::io::BufRead) -> Option<String> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Some(start) = line.find("https://slack.com/oauth") {
            return Some(line[start..].trim().to_string());
        }
    }
}
