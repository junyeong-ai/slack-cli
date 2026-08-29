//! Integration tests for `slack-cli self update` against a mock GitHub, driving
//! the whole path a real update takes: resolve the release, download the bare
//! executable and its checksum, verify, and swap the file in place.

use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};
use slack_cli::update::{Action, UpdateRequest, run};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPO: &str = "junyeong-ai/slack-cli";
const TARGET_VERSION: &str = "99.0.0";
const PAYLOAD: &[u8] = b"#!/bin/sh\necho replaced\n";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn asset_name() -> String {
    let target = slack_cli::update::target::current().expect("a supported host");
    slack_cli::update::target::binary_asset(TARGET_VERSION, target)
}

/// Publishes a release whose bare executable, checksum and (optionally) a
/// deliberately wrong checksum are served by `server`.
async fn publish(server: &MockServer, checksum: &str) -> String {
    publish_with_bundle(server, checksum, false).await
}

async fn publish_with_bundle(server: &MockServer, checksum: &str, bundle: bool) -> String {
    let name = asset_name();
    let base = server.uri();

    let mut assets = vec![
        json!({"name": name, "browser_download_url": format!("{base}/dl/{name}")}),
        json!({"name": format!("{name}.sha256"),
               "browser_download_url": format!("{base}/dl/{name}.sha256")}),
    ];
    if bundle {
        assets.push(json!({"name": format!("{name}.bundle"),
                           "browser_download_url": format!("{base}/dl/{name}.bundle")}));
        publish_bundle(server).await;
    }

    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases/latest")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tag_name": format!("v{TARGET_VERSION}"),
            "assets": assets
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/dl/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PAYLOAD))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/dl/{name}.sha256")))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{checksum}  {name}\n")))
        .mount(server)
        .await;

    name
}

fn installed(dir: &Path) -> PathBuf {
    let path = dir.join("slack-cli");
    std::fs::write(&path, b"the binary that is running").unwrap();
    path
}

/// Guards the seam: an update must only ever touch the file the test handed
/// it. Without this a regression would silently overwrite the test runner.
fn assert_targeted(outcome_binary: &Path, dir: &Path) {
    let expected = dir.canonicalize().unwrap();
    assert!(
        outcome_binary.starts_with(&expected),
        "update targeted {} instead of {}",
        outcome_binary.display(),
        expected.display()
    );
}

fn request(server: &MockServer, binary: &Path) -> UpdateRequest {
    UpdateRequest {
        version: None,
        check: false,
        force: false,
        assume_yes: true,
        api_base: Some(server.uri()),
        binary: Some(binary.to_path_buf()),
        cosign: Some(false),
    }
}

/// Serves a signature bundle for the host asset. Its contents are not a real
/// signature, so any run that reaches cosign must abort.
async fn publish_bundle(server: &MockServer) {
    let name = format!("{}.bundle", asset_name());
    Mock::given(method("GET"))
        .and(path(format!("/dl/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn an_update_verifies_the_download_and_swaps_the_binary() {
    let server = MockServer::start().await;
    publish(&server, &hex(&Sha256::digest(PAYLOAD))).await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());

    let outcome = run(request(&server, &binary)).await.unwrap();

    assert_targeted(&outcome.binary, dir.path());
    assert_eq!(outcome.action, Action::Updated);
    assert_eq!(outcome.to, TARGET_VERSION);
    assert_eq!(outcome.from, env!("CARGO_PKG_VERSION"));
    assert_eq!(std::fs::read(&binary).unwrap(), PAYLOAD);
}

#[tokio::test]
async fn a_tampered_download_leaves_the_running_binary_alone() {
    let server = MockServer::start().await;
    publish(&server, &"0".repeat(64)).await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());
    let before = std::fs::read(&binary).unwrap();

    let err = run(request(&server, &binary))
        .await
        .expect_err("a checksum mismatch must abort the update");

    assert!(err.to_string().contains("checksum mismatch"), "{err}");
    assert_eq!(std::fs::read(&binary).unwrap(), before);
    let staged = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().starts_with('.'));
    assert!(!staged, "a staging file was left beside the binary");
}

/// Stripping the signature from a release must not talk the updater down to
/// checksum-only trust: whoever replaced the binary also controls its checksum.
#[tokio::test]
async fn a_release_without_a_signature_is_refused_when_cosign_is_installed() {
    let server = MockServer::start().await;
    publish(&server, &hex(&Sha256::digest(PAYLOAD))).await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());
    let before = std::fs::read(&binary).unwrap();

    let err = run(UpdateRequest {
        cosign: Some(true),
        ..request(&server, &binary)
    })
    .await
    .expect_err("a missing signature must abort the update");

    assert!(err.to_string().contains("no signature"), "{err}");
    assert_eq!(std::fs::read(&binary).unwrap(), before);
}

#[tokio::test]
async fn a_signature_that_does_not_verify_aborts_the_update() {
    let server = MockServer::start().await;
    publish_with_bundle(&server, &hex(&Sha256::digest(PAYLOAD)), true).await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());
    let before = std::fs::read(&binary).unwrap();

    let err = run(UpdateRequest {
        cosign: Some(true),
        ..request(&server, &binary)
    })
    .await
    .expect_err("an unverifiable signature must abort the update");

    assert_eq!(std::fs::read(&binary).unwrap(), before, "{err}");
}

#[tokio::test]
async fn a_check_reports_the_newer_release_without_downloading_it() {
    let server = MockServer::start().await;
    let name = publish(&server, &hex(&Sha256::digest(PAYLOAD))).await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());
    let before = std::fs::read(&binary).unwrap();

    let outcome = run(UpdateRequest {
        check: true,
        ..request(&server, &binary)
    })
    .await
    .unwrap();

    assert_eq!(outcome.action, Action::UpdateAvailable);
    assert_eq!(outcome.to, TARGET_VERSION);
    assert_eq!(std::fs::read(&binary).unwrap(), before);

    let downloads = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().contains(&name))
        .count();
    assert_eq!(downloads, 0, "--check downloaded release assets");
}

#[tokio::test]
async fn a_release_matching_the_running_version_is_left_alone() {
    let server = MockServer::start().await;
    let running = env!("CARGO_PKG_VERSION");
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases/latest")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tag_name": format!("v{running}"),
            "assets": []
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());

    let outcome = run(request(&server, &binary)).await.unwrap();
    assert_eq!(outcome.action, Action::AlreadyCurrent);
    assert_eq!(outcome.to, running);
}

#[tokio::test]
async fn a_release_without_the_host_asset_names_the_way_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/releases/latest")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tag_name": format!("v{TARGET_VERSION}"),
            "assets": [{"name": "unrelated", "browser_download_url": "https://example.test/x"}]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());

    let err = run(request(&server, &binary)).await.unwrap_err();
    let message = err.to_string();
    assert!(message.contains(&asset_name()), "{message}");
    assert!(message.contains("install.sh"), "{message}");
}

#[tokio::test]
async fn a_named_version_is_fetched_by_its_tag() {
    let server = MockServer::start().await;
    let name = asset_name();
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/{REPO}/releases/tags/v{TARGET_VERSION}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tag_name": format!("v{TARGET_VERSION}"),
            "assets": [
                {"name": name, "browser_download_url": format!("{base}/dl/{name}")},
                {"name": format!("{name}.sha256"),
                 "browser_download_url": format!("{base}/dl/{name}.sha256")},
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/dl/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PAYLOAD))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/dl/{name}.sha256")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!("{}  {name}\n", hex(&Sha256::digest(PAYLOAD)))),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let binary = installed(dir.path());

    let outcome = run(UpdateRequest {
        version: Some(TARGET_VERSION.to_string()),
        ..request(&server, &binary)
    })
    .await
    .unwrap();

    assert_targeted(&outcome.binary, dir.path());
    assert_eq!(outcome.action, Action::Updated);
    assert_eq!(std::fs::read(&binary).unwrap(), PAYLOAD);
}
