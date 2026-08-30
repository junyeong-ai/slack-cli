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

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .env("EDITOR", "true")
        .output()
        .expect("binary runs")
}

#[test]
fn config_path_answers_while_the_config_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = rejected(dir.path());

    let output = run(&["--config", path.to_str().unwrap(), "config", "path"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout.trim(), path.display().to_string());
}

#[test]
fn config_edit_opens_while_the_config_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = rejected(dir.path());

    let output = run(&["--config", path.to_str().unwrap(), "config", "edit"]);
    assert!(output.status.success(), "{output:?}");
}

/// The arm that routes `edit` around the load is otherwise only guarded by
/// convention: sending it down the loaded path reaches an `unreachable!`.
#[test]
fn config_edit_opens_a_valid_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[cache]\nttl_users_hours = 24\n").unwrap();

    let output = run(&["--config", path.to_str().unwrap(), "config", "edit"]);
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unreachable"), "{stderr}");
}

/// `show` reads the config, so it stays refused — the repair commands are the
/// exception, not the rule.
#[test]
fn config_show_still_refuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = rejected(dir.path());

    let output = run(&["--config", path.to_str().unwrap(), "config", "show"]);
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

    let output = run(&["--config", target.to_str().unwrap(), "config", "edit"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("is not a file"), "{stderr}");
}
