#![cfg(unix)]

use std::process::Command;

fn run(body: &str) {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!("source \"$INSTALL_SCRIPT\"\n{body}"))
        .env(
            "INSTALL_SCRIPT",
            concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/install.sh"),
        )
        .env("INSTALL_DIR", dir.path().join("bin"))
        .env("TEST_ROOT", dir.path())
        .env("PACKAGE_VERSION", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn release_installs_fetch_the_exact_tag_instead_of_using_a_checkout() {
    run(r#"
PROJECT_SKILL_DIR="$TEST_ROOT/project"
mkdir -p "$PROJECT_SKILL_DIR"
printf '%s\n' '---' 'version: 9.9.9' '---' > "$PROJECT_SKILL_DIR/SKILL.md"
curl() {
    [ "$2" = 'https://raw.githubusercontent.com/junyeong-ai/slack-cli/v1.2.3/.claude/skills/slack-workspace/SKILL.md' ]
    printf '%s\n' '---' 'metadata:' '  version: "1.2.3"' '---' > "$4"
}
prepare_skill_source v1.2.3
[ "$(get_skill_version "$SKILL_SOURCE_DIR/SKILL.md")" = 1.2.3 ]
"#);
}

#[test]
fn a_missing_release_skill_is_not_replaced_with_main() {
    run(r#"
curl() { printf '%s\n' "$2" >> "$TEST_ROOT/requests"; return 22; }
if prepare_skill_source v1.2.3; then exit 1; fi
[ "$(wc -l < "$TEST_ROOT/requests" | tr -d ' ')" = 1 ]
[ "$(cat "$TEST_ROOT/requests")" = 'https://raw.githubusercontent.com/junyeong-ai/slack-cli/v1.2.3/.claude/skills/slack-workspace/SKILL.md' ]
"#);
}

#[test]
fn mismatched_downloads_and_checkouts_are_rejected() {
    run(r#"
PROJECT_SKILL_DIR="$TEST_ROOT/project"
mkdir -p "$PROJECT_SKILL_DIR"
printf '%s\n' '---' 'version: 9.9.9' '---' > "$PROJECT_SKILL_DIR/SKILL.md"
if prepare_skill_source v1.2.3 true; then exit 1; fi
curl() { printf '%s\n' '---' 'version: 9.9.9' '---' > "$4"; }
if prepare_skill_source v1.2.3; then exit 1; fi
"#);
}

#[test]
fn binary_replacement_preserves_the_old_inode_and_failed_installs_preserve_the_binary() {
    run(r#"
mkdir -p "$INSTALL_DIR"
printf old > "$INSTALL_DIR/slack-cli"
ln "$INSTALL_DIR/slack-cli" "$TEST_ROOT/running"
printf new > "$TEST_ROOT/download"
install_binary "$TEST_ROOT/download"
[ "$(cat "$INSTALL_DIR/slack-cli")" = new ]
[ "$(cat "$TEST_ROOT/running")" = old ]
if install_binary "$TEST_ROOT/missing"; then exit 1; fi
[ "$(cat "$INSTALL_DIR/slack-cli")" = new ]
[ "$(find "$INSTALL_DIR" -name '.slack-cli.*' | wc -l | tr -d ' ')" = 0 ]
"#);
}

#[test]
fn the_shipped_skill_version_matches_the_binary() {
    run(r#"[ "$(get_skill_version "$PROJECT_SKILL_DIR/SKILL.md")" = "$PACKAGE_VERSION" ]"#);
}

#[test]
fn a_binary_with_the_wrong_release_version_is_rejected_before_replacement() {
    run(r#"
mkdir -p "$INSTALL_DIR"
printf old > "$INSTALL_DIR/slack-cli"
printf '%s\n' '#!/bin/sh' 'echo slack-cli 9.9.9' > "$TEST_ROOT/download"
chmod +x "$TEST_ROOT/download"
curl() { :; }
get_latest_version() { echo 1.2.3; }
detect_platform() { echo test; }
prompt_choice() { echo 1; }
download_binary() { echo "$TEST_ROOT/download"; }
if (main); then exit 1; fi
[ "$(cat "$INSTALL_DIR/slack-cli")" = old ]
"#);
}

#[test]
fn a_failed_download_does_not_start_a_source_build() {
    run(r#"
curl() { :; }
get_latest_version() { echo 1.2.3; }
detect_platform() { echo test; }
prompt_choice() { echo 1; }
download_binary() { return 1; }
build_from_source() { touch "$TEST_ROOT/built"; return 1; }
if (main); then exit 1; fi
[ ! -e "$TEST_ROOT/built" ]
"#);
}

#[test]
fn an_explicit_checkout_build_installs_its_own_matching_skill_version() {
    run(r#"
printf '%s\n' '#!/bin/sh' 'echo slack-cli 9.9.9' > "$TEST_ROOT/download"
chmod +x "$TEST_ROOT/download"
curl() { :; }
get_latest_version() { return 1; }
detect_platform() { echo test; }
prompt_choice() { echo 2; }
build_from_source() { echo "$TEST_ROOT/download"; }
prompt_skill_installation() { [ "$1" = v9.9.9 ] && [ "$2" = true ]; }
main
"#);
}

#[test]
fn an_unresolved_latest_release_stops_before_downloading_an_alias() {
    run(r#"
curl() { :; }
get_latest_version() { return 1; }
detect_platform() { echo test; }
prompt_choice() { echo 1; }
download_binary() { touch "$TEST_ROOT/downloaded"; return 1; }
if (main); then exit 1; fi
[ ! -e "$TEST_ROOT/downloaded" ]
"#);
}
