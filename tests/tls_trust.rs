//! A Linux system with no CA certificates, such as a minimal container, still
//! runs the CLI: it trusts the Mozilla roots built into the binary rather than
//! failing to build any HTTP client. `SSL_CERT_FILE` and `SSL_CERT_DIR` choose
//! where the roots are read from, so pointing them at nothing reproduces that
//! system, and because someone set them, the substitution is reported. macOS
//! and Windows verify through the OS and never read them.

#![cfg(target_os = "linux")]

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_slack-cli");

#[test]
fn a_system_without_ca_certificates_still_runs() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(BIN)
        .env("SSL_CERT_FILE", dir.path().join("none.pem"))
        .env("SSL_CERT_DIR", dir.path().join("none"))
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env_remove("SLACK_USER_TOKEN")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_PROFILE")
        .env_remove("RUST_LOG")
        .args(["--json", "auth", "scopes"])
        .output()
        .expect("binary runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "status {:?}, stderr: {stderr}",
        output.status
    );
    assert!(
        stderr.contains("SSL_CERT_FILE and SSL_CERT_DIR provide no CA certificates"),
        "{stderr}"
    );
}
