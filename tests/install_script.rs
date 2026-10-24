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
        .env("HOME", dir.path())
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
fn release_downloads_verify_the_selected_tag_and_reject_another_tags_signature() {
    run(r#"
TMPDIR="$TEST_ROOT"
mkdir -p "$TEST_ROOT/source"
printf '%s\n' '#!/bin/sh' 'echo slack-cli 1.2.3+build.4' > "$TEST_ROOT/source/slack-cli"
chmod +x "$TEST_ROOT/source/slack-cli"
tar -C "$TEST_ROOT/source" -czf "$TEST_ROOT/release.tar.gz" slack-cli
curl() {
    local name="${2##*/}"
    case "$name" in
        *.sha256) compute_sha256 "$TEST_ROOT/release.tar.gz" > "$name" ;;
        *.bundle) printf signed > "$name" ;;
        *) cp "$TEST_ROOT/release.tar.gz" "$name" ;;
    esac
}
cosign() {
    local identity=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --certificate-identity) identity="$2"; shift ;;
        esac
        shift
    done
    [ "$identity" = "https://github.com/junyeong-ai/slack-cli/.github/workflows/release.yml@refs/tags/v$SIGNED_RELEASE" ]
}
SIGNED_RELEASE=1.2.3+build.4
downloaded=$(download_binary 1.2.3+build.4 test)
[ -x "$downloaded" ]
SIGNED_RELEASE=9.9.9
if download_binary 1.2.3+build.4 test; then exit 1; fi
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

#[test]
fn skill_replacement_preserves_all_user_files_and_stops_on_preservation_failure() {
    run(r#"
USER_SKILL_DIR="$TEST_ROOT/installed"
SKILL_SOURCE_DIR="$TEST_ROOT/source"
mkdir -p "$USER_SKILL_DIR" "$SKILL_SOURCE_DIR"
printf shipped > "$USER_SKILL_DIR/SKILL.md"
printf custom > "$USER_SKILL_DIR/reference.md"
printf next > "$SKILL_SOURCE_DIR/SKILL.md"
preserved_dir() { echo "$TEST_ROOT/backups"; }
install_skill
[ "$(cat "$USER_SKILL_DIR/SKILL.md")" = next ]
[ "$(cat "$TEST_ROOT"/backups/*/reference.md)" = custom ]
printf edited > "$USER_SKILL_DIR/SKILL.md"
cp() {
    [ "$2" != "$USER_SKILL_DIR/." ] || return 1
    command cp "$@"
}
if install_skill; then exit 1; fi
[ "$(cat "$USER_SKILL_DIR/SKILL.md")" = edited ]
"#);
}

#[test]
fn failed_skill_staging_and_swap_keep_the_previous_installation() {
    run(r#"
USER_SKILL_DIR="$TEST_ROOT/installed"
SKILL_SOURCE_DIR="$TEST_ROOT/missing"
mkdir -p "$USER_SKILL_DIR"
printf original > "$USER_SKILL_DIR/SKILL.md"
if install_skill; then exit 1; fi
[ "$(cat "$USER_SKILL_DIR/SKILL.md")" = original ]
SKILL_SOURCE_DIR="$TEST_ROOT/source"
mkdir -p "$SKILL_SOURCE_DIR"
printf next > "$SKILL_SOURCE_DIR/SKILL.md"
preserved_dir() { echo "$TEST_ROOT/backups"; }
mv() {
    case "$1" in */replacement) return 1 ;; esac
    command mv "$@"
}
if install_skill; then exit 1; fi
[ "$(cat "$USER_SKILL_DIR/SKILL.md")" = original ]
command mv "$USER_SKILL_DIR" "$TEST_ROOT/actual"
ln -s actual "$USER_SKILL_DIR"
if install_skill; then exit 1; fi
[ -L "$USER_SKILL_DIR" ]
[ "$(cat "$USER_SKILL_DIR/SKILL.md")" = original ]
"#);
}

#[test]
fn the_installer_kernel_lock_survives_the_helper_and_releases_on_exit() {
    run(r#"
acquire_install_lock
export LOCK_PATH="$HOME/.claude/.slack-cli-install.lock"
if perl -MFcntl=:flock -e 'open(my $f, ">>", $ENV{LOCK_PATH}) or die $!; flock($f, LOCK_EX | LOCK_NB) or exit 7'; then exit 1; fi
exec 9>&-
perl -MFcntl=:flock -e 'open(my $f, ">>", $ENV{LOCK_PATH}) or die $!; flock($f, LOCK_EX | LOCK_NB) or exit 7'
"#);
}

#[test]
fn a_lock_failure_aborts_even_when_main_is_called_in_a_conditional() {
    run(r#"
acquire_install_lock() { return 1; }
detect_platform() { touch "$TEST_ROOT/unlocked-install"; }
if (main); then exit 1; fi
[ ! -e "$TEST_ROOT/unlocked-install" ]
"#);
}

#[test]
fn incomplete_skill_directories_and_non_directory_paths_are_preserved() {
    run(r#"
SKILL_SOURCE_DIR="$TEST_ROOT/source"
mkdir -p "$USER_SKILL_DIR" "$SKILL_SOURCE_DIR"
printf custom > "$USER_SKILL_DIR/user-reference.md"
printf next > "$SKILL_SOURCE_DIR/SKILL.md"
preserved_dir() { echo "$TEST_ROOT/backups"; }
prepare_skill_source() { return 0; }
prompt_choice() { echo y; }
prompt_skill_installation v1.0.0
[ "$(cat "$TEST_ROOT"/backups/*/user-reference.md)" = custom ]
[ "$(cat "$USER_SKILL_DIR/SKILL.md")" = next ]
command mv "$USER_SKILL_DIR" "$TEST_ROOT/first"
printf personal > "$USER_SKILL_DIR"
install_skill
[ "$(cat "$TEST_ROOT"/backups/*/original)" = personal ]
command mv "$USER_SKILL_DIR" "$TEST_ROOT/second"
ln -s missing-target "$USER_SKILL_DIR"
install_skill
[ -d "$USER_SKILL_DIR" ]
[ "$(find "$TEST_ROOT/backups" -name original -type l -exec readlink {} \;)" = missing-target ]
"#);
}

#[test]
fn uninstall_waits_for_the_installer_lock_before_removing_files() {
    run(r#"
acquire_install_lock
mkdir -p "$INSTALL_DIR" "$TEST_ROOT/shim" "$USER_SKILL_DIR"
printf binary > "$INSTALL_DIR/slack-cli"
printf skill > "$USER_SKILL_DIR/SKILL.md"
export REAL_PERL
REAL_PERL=$(command -v perl)
cat > "$TEST_ROOT/shim/perl" <<'SH'
#!/bin/bash
touch "$TEST_ROOT/uninstall-lock-attempt"
exec "$REAL_PERL" "$@"
SH
chmod +x "$TEST_ROOT/shim/perl"
PATH="$TEST_ROOT/shim:$PATH" bash "$(dirname "$INSTALL_SCRIPT")/uninstall.sh" -y > "$TEST_ROOT/uninstall.log" 2>&1 &
uninstaller=$!
trap 'kill "$uninstaller" 2>/dev/null || true' EXIT
for ((attempt=0; attempt<200; attempt++)); do
    [ ! -e "$TEST_ROOT/uninstall-lock-attempt" ] || break
    sleep 0.01
done
[ -e "$TEST_ROOT/uninstall-lock-attempt" ]
kill -0 "$uninstaller"
[ -f "$INSTALL_DIR/slack-cli" ]
[ -f "$USER_SKILL_DIR/SKILL.md" ]
exec 9>&-
wait "$uninstaller"
trap - EXIT
[ ! -e "$INSTALL_DIR/slack-cli" ]
[ ! -e "$USER_SKILL_DIR" ]
[ -f "$HOME/.claude/.slack-cli-install.lock" ]
acquire_install_lock
"#);
}

#[test]
fn piping_the_script_into_bash_runs_the_installer() {
    run(r#"
if SLACK_CLI_VERSION=not-a-version bash < "$INSTALL_SCRIPT" 2> "$TEST_ROOT/piped.log"; then exit 1; fi
grep -q 'Invalid release version: not-a-version' "$TEST_ROOT/piped.log" || { cat "$TEST_ROOT/piped.log" >&2; exit 1; }
"#);
}
