//! A reader that stops early, as `| head` does, closes the pipe while the CLI
//! is still writing. Rust ignores `SIGPIPE`, so the write fails instead of the
//! signal ending the process; only the binary shows what that failure becomes.

use std::process::Command;

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
