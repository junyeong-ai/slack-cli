//! `config path` and `config edit` are how a user repairs a config the CLI
//! refuses, so neither may depend on that config loading. The routing that
//! guarantees it lives in `run`, above `Config::load`, where only running the
//! binary can reach it.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_slack-cli");

/// A config every command is refused for: the excluded scope is not one any
/// method declares.
fn rejected(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, "[auth]\nexclude_scopes = [\"users:reed\"]\n").unwrap();
    path
}

/// An editor that records the file it was asked to open. `true` is not a
/// program on a stock Windows install, and a stub that only exits proves
/// nothing about whether the editor ran at all.
fn stub_editor(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let opened = dir.join("opened.txt");
    let (name, body) = if cfg!(windows) {
        (
            "editor.cmd",
            format!("@echo %1> \"{}\"\r\n", opened.display()),
        )
    } else {
        (
            "editor.sh",
            format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", opened.display()),
        )
    };
    let editor = dir.join(name);
    std::fs::write(&editor, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (editor, opened)
}

fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    let (editor, _) = stub_editor(dir);
    Command::new(BIN)
        .args(args)
        .env("EDITOR", editor)
        .output()
        .expect("binary runs")
}

/// What the editor was handed, or `None` when it was never launched.
fn opened_file(dir: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(dir.join("opened.txt"))
        .ok()
        .map(|opened| opened.trim().to_string())
}

#[test]
fn config_path_answers_while_the_config_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = rejected(dir.path());

    let output = run(
        dir.path(),
        &["--config", path.to_str().unwrap(), "config", "path"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout.trim(), path.display().to_string());
}

#[test]
fn config_edit_opens_while_the_config_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = rejected(dir.path());

    let output = run(
        dir.path(),
        &["--config", path.to_str().unwrap(), "config", "edit"],
    );
    assert!(output.status.success(), "{output:?}");
}

/// The arm that routes `edit` around the load is otherwise only guarded by
/// convention: sending it down the loaded path reaches an `unreachable!`.
#[test]
fn config_edit_opens_a_valid_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[cache]\nttl_users_hours = 24\n").unwrap();

    let output = run(
        dir.path(),
        &["--config", path.to_str().unwrap(), "config", "edit"],
    );
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unreachable"), "{stderr}");
    assert_eq!(
        opened_file(dir.path()).as_deref(),
        Some(path.to_str().unwrap()),
        "the editor must be handed the config"
    );
}

/// `show` reads the config, so it stays refused — the repair commands are the
/// exception, not the rule.
#[test]
fn config_show_still_refuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = rejected(dir.path());

    let output = run(
        dir.path(),
        &["--config", path.to_str().unwrap(), "config", "show"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("users:reed"), "{stderr}");
    assert!(stderr.contains(&path.display().to_string()), "{stderr}");
}

/// Reaching the editor before the config loads means the load no longer
/// rejects a `--config` that is not a file at all.
#[test]
fn config_edit_refuses_a_path_that_is_not_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("a-directory");
    std::fs::create_dir(&target).unwrap();

    let output = run(
        dir.path(),
        &["--config", target.to_str().unwrap(), "config", "edit"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("is not a file"), "{stderr}");
}

/// Slack matches a redirect URL exactly, so a port the app cannot have
/// registered — an ephemeral one above all — is refused where an invalid
/// argument belongs: at the parser, before a browser is ever opened.
#[test]
fn an_unusable_callback_port_is_a_usage_error() {
    let mut child = Command::new(BIN)
        // A test for a refusal must not depend on the refusal to stay
        // harmless: without `--no-browser`, the regression it exists to catch
        // opens a browser on the machine running it.
        .args([
            "auth",
            "login",
            "--method",
            "pkce",
            "--no-browser",
            "--port",
            "0",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("binary runs");

    // Accepting the port instead binds one and waits minutes for a redirect,
    // so finishing at all is part of what this asserts.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().expect("child is waitable") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            panic!("the port was accepted; the login waited for a callback");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let output = child.wait_with_output().expect("output is readable");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("--port"), "{stderr}");
}
