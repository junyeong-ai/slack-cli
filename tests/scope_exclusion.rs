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
        .env("APPDATA", dir);
}

/// Every user scope the CLI would request, excluded, so the authorization
/// would grant nothing.
fn excludes_everything(dir: &std::path::Path) -> std::path::PathBuf {
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
    let scopes: serde_json::Value =
        serde_json::from_slice(&listed.stdout).expect("auth scopes emits JSON");
    let entries: Vec<&str> = scopes["user"]
        .as_array()
        .expect("user scopes")
        .iter()
        .map(|scope| scope.as_str().expect("scope is a string"))
        .collect();
    assert!(!entries.is_empty(), "the CLI must request something");

    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        format!(
            "[auth]\nclient_id = \"1.2\"\nexclude_scopes = {}\n",
            serde_json::to_string(&entries).unwrap()
        ),
    )
    .unwrap();
    path
}

#[test]
fn a_login_excluding_every_scope_is_refused_before_the_browser_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = excludes_everything(dir.path());

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
