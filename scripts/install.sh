#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<EOF
Usage: install.sh [-h]

Settings come from the environment, so they survive \`curl … | bash\`:

  SLACK_CLI_VERSION   Release tag to install (default: the latest)
  INSTALL_DIR         Where the binary goes (default: \$HOME/.local/bin)

Options:
  -h, --help          Show this help
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

BINARY_NAME="slack-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
REPO="junyeong-ai/slack-cli"
SKILL_NAME="slack-workspace"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"
SCRIPT_PATH="${BASH_SOURCE[0]:-$0}"
ORIGINAL_DIR="$(pwd)"
if SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" 2>/dev/null && pwd -P)"; then
    :
else
    SCRIPT_DIR="$ORIGINAL_DIR"
fi
PROJECT_ROOT="$ORIGINAL_DIR"
SKILL_SOURCE_DIR=""
SKILL_TMP_DIR=""
BINARY_TMP_DIR=""

if [ -f "$SCRIPT_DIR/../Cargo.toml" ]; then
    PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
fi

PROJECT_SKILL_DIR="$PROJECT_ROOT/.claude/skills/$SKILL_NAME"

cleanup() {
    if [ -n "$SKILL_TMP_DIR" ]; then
        rm -rf "$SKILL_TMP_DIR"
    fi
    if [ -n "$BINARY_TMP_DIR" ]; then
        rm -rf "$BINARY_TMP_DIR"
    fi
    return 0
}

trap cleanup EXIT

prompt_choice() {
    local prompt="$1"
    local default="$2"
    local choice=""

    if [ -t 0 ]; then
        read -r -p "$prompt" choice || choice=""
    else
        choice="$default"
    fi

    echo "${choice:-$default}"
}

display_path() {
    local path="$1"

    if [ "$path" = "$HOME" ]; then
        echo "\$HOME"
    elif [[ "$path" == "$HOME/"* ]]; then
        echo "\$HOME/${path#"$HOME"/}"
    else
        echo "$path"
    fi
}

is_valid_release_version() {
    local version="$1"

    [[ "$version" =~ ^[0-9][0-9A-Za-z._+-]*$ ]]
}

detect_platform() {
    local os
    local arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        linux)
            if is_musl_system; then
                os="unknown-linux-musl"
            else
                os="unknown-linux-gnu"
            fi
            ;;
        darwin) os="apple-darwin" ;;
        *) echo "Unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "Unsupported architecture: $arch"; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

is_musl_system() {
    [ -f /etc/alpine-release ] && return 0
    ldd --version 2>&1 | grep -qi musl
}

compute_sha256() {
    local file="$1"

    if command -v sha256sum >/dev/null; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        return 1
    fi
}

get_latest_version() {
    local latest_url
    latest_url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") || return 1
    case "$latest_url" in
        "https://github.com/$REPO/releases/tag/v"*)
            latest_url="${latest_url#"https://github.com/$REPO/releases/tag/v"}"
            latest_url="${latest_url%%[/?#]*}"
            is_valid_release_version "$latest_url" || return 1
            echo "$latest_url"
            ;;
        *) return 1 ;;
    esac
}

download_binary() {
    local version="$1"
    local target="$2"
    local archive
    local url
    local checksum_url
    local binary_path

    archive="slack-cli-v${version}-${target}.tar.gz"
    url="https://github.com/$REPO/releases/download/v${version}/${archive}"
    checksum_url="${url}.sha256"

    BINARY_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slack-cli-install.XXXXXX")

    echo "📥 Downloading $archive..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "$url"); then
        echo "❌ Download failed" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 1
    fi

    echo "🔐 Verifying checksum..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "$checksum_url"); then
        echo "❌ Checksum download failed for $archive" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 2
    fi

    # Compare digests directly instead of `sha256sum -c`: the "latest" alias
    # assets share the digest of their versioned originals, so the filename
    # inside the .sha256 file must not participate in verification.
    local expected
    local actual
    expected=$(awk '{print $1; exit}' "$BINARY_TMP_DIR/${archive}.sha256")
    actual=$(compute_sha256 "$BINARY_TMP_DIR/$archive") || {
        echo "❌ No checksum tool found (need sha256sum or shasum)" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 2
    }
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "❌ Checksum mismatch for $archive" >&2
        echo "   expected: ${expected:-<empty>}" >&2
        echo "   actual:   $actual" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 2
    fi
    echo "   ✅ SHA-256 verified" >&2

    verify_signature "$url" "$archive" "$version" || {
        rm -rf "$BINARY_TMP_DIR"
        return 2
    }

    echo "📦 Extracting..." >&2
    if ! (cd "$BINARY_TMP_DIR" && tar -xzf "$archive") >&2; then
        rm -rf "$BINARY_TMP_DIR"
        return 2
    fi
    binary_path="$BINARY_TMP_DIR/$BINARY_NAME"

    if [ ! -x "$binary_path" ]; then
        echo "❌ Archive did not contain executable $BINARY_NAME" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 2
    fi

    echo "$binary_path"
}

# Sigstore verification of the release signature. Skipped with a note when
# cosign is not installed; with cosign present, a missing bundle or a failed
# verification is fatal — a stripped signature must not downgrade the install
# to checksum-only trust. Releases are signed from v0.4.0 onward.
verify_signature() {
    local url="$1"
    local archive="$2"
    local version="$3"

    if ! command -v cosign >/dev/null; then
        echo "ℹ️  cosign not found; skipping signature verification (SHA-256 already checked)" >&2
        return 0
    fi

    echo "🔏 Verifying sigstore signature..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "${url}.bundle"); then
        echo "❌ Signature bundle missing for $archive" >&2
        echo "   Releases v0.4.0 and later ship a sigstore bundle; a missing bundle" >&2
        echo "   means the assets should not be trusted." >&2
        return 1
    fi

    if ! cosign verify-blob \
        --bundle "$BINARY_TMP_DIR/${archive}.bundle" \
        --certificate-identity "https://github.com/$REPO/.github/workflows/release.yml@refs/tags/v${version}" \
        --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
        "$BINARY_TMP_DIR/$archive" >&2; then
        echo "❌ Signature verification failed for $archive" >&2
        return 1
    fi
    echo "   ✅ Signature verified" >&2
}

build_from_source() {
    if [ ! -f "$PROJECT_ROOT/Cargo.toml" ]; then
        echo "❌ Source build requires running inside a slack-cli checkout" >&2
        exit 1
    fi

    echo "🔨 Building from source (rustup uses the toolchain pinned in rust-toolchain.toml)..." >&2
    if ! (cd "$PROJECT_ROOT" && cargo build --release --locked) >&2; then
        echo "❌ Build failed" >&2
        exit 1
    fi
    echo "$PROJECT_ROOT/target/release/$BINARY_NAME"
}

install_binary() {
    local binary_path="$1"

    local staged
    mkdir -p "$INSTALL_DIR"
    staged=$(mktemp "$INSTALL_DIR/.slack-cli.XXXXXX")
    if ! cp "$binary_path" "$staged" || ! chmod +x "$staged"; then
        rm -f "$staged"
        return 1
    fi

    if [[ "$OSTYPE" == "darwin"* ]]; then
        codesign --force --deep --sign - "$staged" 2>/dev/null || true
    fi
    if ! mv -f "$staged" "$INSTALL_DIR/$BINARY_NAME"; then
        rm -f "$staged"
        return 1
    fi

    echo "✅ Installed to $INSTALL_DIR/$BINARY_NAME" >&2
}

get_skill_version() {
    local skill_md="$1"
    [ -f "$skill_md" ] || { echo unknown; return; }
    awk '
        NR == 1 { if ($0 != "---") exit 1; next }
        /^---$/ { exit }
        /^metadata:$/ { metadata = 1; next }
        /^[^[:space:]]/ { metadata = 0 }
        /^version:[[:space:]]/ || (metadata && /^  version:[[:space:]]/) {
            sub(/^[[:space:]]*version:[[:space:]]*/, "")
            gsub(/^"|"$/, "")
            print
            found = 1
            exit
        }
        END { if (!found) exit 1 }
    ' "$skill_md" || echo unknown
}

check_skill_exists() {
    [ -e "$USER_SKILL_DIR" ] || [ -L "$USER_SKILL_DIR" ]
}

compare_versions() {
    local ver1="$1"
    local ver2="$2"
    local key1
    local key2

    if [ "$ver1" = "$ver2" ]; then
        echo "equal"
    elif [ "$ver1" = "unknown" ] || [ "$ver2" = "unknown" ]; then
        echo "unknown"
    elif ! key1=$(version_key "$ver1") || ! key2=$(version_key "$ver2"); then
        echo "unknown"
    elif [[ "$key1" < "$key2" ]]; then
        echo "older"
    elif [[ "$key1" > "$key2" ]]; then
        echo "newer"
    else
        echo "equal"
    fi
}

version_key() {
    local version="${1#v}"
    local major
    local minor
    local patch

    version="${version%%[-+]*}"
    IFS=. read -r major minor patch _ <<<"$version"
    major="${major:-0}"
    minor="${minor:-0}"
    patch="${patch:-0}"

    [[ "$major" =~ ^[0-9]+$ ]] || return 1
    [[ "$minor" =~ ^[0-9]+$ ]] || return 1
    [[ "$patch" =~ ^[0-9]+$ ]] || return 1

    printf '%09d.%09d.%09d\n' "$major" "$minor" "$patch"
}

# Where a locally edited skill is set aside.
#
# Never beside $USER_SKILL_DIR: everything under ~/.claude/skills is discovered
# as a skill, so a copy kept there comes back as a second entry competing with
# the real one rather than an inert archive. The CLI's own configuration
# directory is where slack-cli already keeps what it owns, and `config path` is
# the only thing that knows where that is on this platform.
preserved_dir() {
    local config_path=""

    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        config_path=$("$INSTALL_DIR/$BINARY_NAME" config path 2>/dev/null) || config_path=""
    fi
    [ -n "$config_path" ] || return 1

    echo "$(dirname "$config_path")/skill-backups"
}

preserve_skill() {
    local installed_version
    installed_version=$(get_skill_version "$USER_SKILL_DIR/SKILL.md")
    is_valid_release_version "$installed_version" || installed_version=unknown
    local keep
    local kept

    keep=$(preserved_dir) || return 1
    mkdir -p "$keep" || return 1
    kept=$(mktemp -d "$keep/$SKILL_NAME.v$installed_version.XXXXXX") || return 1
    if [ -d "$USER_SKILL_DIR" ]; then
        if ! cp -R "$USER_SKILL_DIR/." "$kept/"; then
            rm -rf "$kept"
            echo "❌ Could not preserve the installed skill; leaving it unchanged" >&2
            return 1
        fi
    elif ! cp -P "$USER_SKILL_DIR" "$kept/original"; then
        rm -rf "$kept"
        echo "❌ Could not preserve the installed skill path; leaving it unchanged" >&2
        return 1
    fi
    echo "📦 Previous skill preserved at: $(display_path "$kept")" >&2
}

install_skill() {
    local parent
    local staging
    parent=$(dirname "$USER_SKILL_DIR")
    mkdir -p "$parent" || return 1
    staging=$(mktemp -d "$parent/.$SKILL_NAME.XXXXXX") || return 1
    if ! cp -R "$SKILL_SOURCE_DIR" "$staging/replacement"; then
        rm -rf "$staging"
        return 1
    fi
    if check_skill_exists; then
        if ! preserve_skill; then
            rm -rf "$staging"
            return 1
        fi
        if ! mv "$USER_SKILL_DIR" "$staging/previous"; then
            rm -rf "$staging"
            return 1
        fi
    fi
    if ! mv "$staging/replacement" "$USER_SKILL_DIR"; then
        if [ -e "$staging/previous" ] || [ -L "$staging/previous" ]; then
            mv "$staging/previous" "$USER_SKILL_DIR" || {
                echo "❌ Could not restore the previous skill; preserved at $staging/previous" >&2
                return 1
            }
        fi
        rm -rf "$staging"
        return 1
    fi
    rm -rf "$staging"
    echo "   ✅ Skill installed" >&2
}

prepare_skill_source() {
    local ref="$1"
    local from_checkout="${2:-false}"
    local expected="${ref#v}"

    if [ "$from_checkout" = true ] && [ -f "$PROJECT_SKILL_DIR/SKILL.md" ]; then
        if [ "$(get_skill_version "$PROJECT_SKILL_DIR/SKILL.md")" != "$expected" ]; then
            echo "❌ Checkout skill does not match binary v$expected" >&2
            return 1
        fi
        SKILL_SOURCE_DIR="$PROJECT_SKILL_DIR"
        return 0
    fi

    SKILL_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slack-cli-skill.XXXXXX")
    SKILL_SOURCE_DIR="$SKILL_TMP_DIR/$SKILL_NAME"
    mkdir -p "$SKILL_SOURCE_DIR"
    if ! curl -fsSL \
        "https://raw.githubusercontent.com/$REPO/$ref/.claude/skills/$SKILL_NAME/SKILL.md" \
        -o "$SKILL_SOURCE_DIR/SKILL.md"; then
        return 1
    fi
    if [ "$(get_skill_version "$SKILL_SOURCE_DIR/SKILL.md")" != "$expected" ]; then
        echo "❌ Downloaded skill does not match binary v$expected" >&2
        return 1
    fi
}

prompt_skill_installation() {
    local ref="$1"
    local choice

    if ! prepare_skill_source "$ref" "${2:-false}"; then
        echo "" >&2
        echo "⚠️  Could not fetch $SKILL_NAME skill from $ref; skipping skill installation" >&2
        return 0
    fi

    local project_version
    project_version=$(get_skill_version "$SKILL_SOURCE_DIR/SKILL.md")

    echo "" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "🤖 Claude Code Skill Installation" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "" >&2
    echo "Skill: $SKILL_NAME (v$project_version)" >&2
    echo "" >&2

    if check_skill_exists; then
        local existing_version
        local comparison
        existing_version=$(get_skill_version "$USER_SKILL_DIR/SKILL.md")
        comparison=$(compare_versions "$existing_version" "$project_version")

        echo "Status: Already installed (v$existing_version)" >&2
        echo "" >&2

        case "$comparison" in
            equal)
                echo "✅ Latest version installed" >&2
                echo "" >&2
                choice=$(prompt_choice "Reinstall? [y/N]: " "")
                if [[ "$choice" =~ ^[yY]$ ]]; then
                    install_skill || true
                else
                    echo "   ⏭️  Skipped" >&2
                fi
                ;;
            older)
                echo "🔄 New version available: v$project_version" >&2
                echo "" >&2
                choice=$(prompt_choice "Update? [Y/n]: " "")
                if [[ "$choice" =~ ^[nN]$ ]]; then
                    echo "   ⏭️  Keeping current version" >&2
                elif install_skill; then
                    echo "   ✅ Updated to v$project_version" >&2
                    echo "   To go back: SLACK_CLI_VERSION=$existing_version install.sh" >&2
                fi
                ;;
            newer)
                echo "⚠️  Installed version (v$existing_version) > project version (v$project_version)" >&2
                echo "" >&2
                choice=$(prompt_choice "Downgrade? [y/N]: " "")
                if [[ "$choice" =~ ^[yY]$ ]]; then
                    install_skill || true
                else
                    echo "   ⏭️  Keeping current version" >&2
                fi
                ;;
            *)
                echo "⚠️  Version comparison failed" >&2
                echo "" >&2
                choice=$(prompt_choice "Reinstall? [y/N]: " "")
                if [[ "$choice" =~ ^[yY]$ ]]; then
                    install_skill || true
                else
                    echo "   ⏭️  Skipped" >&2
                fi
                ;;
        esac
    else
        echo "Installation options:" >&2
        echo "" >&2
        echo "  [1] User-level install (RECOMMENDED)" >&2
        echo "      → ~/.claude/skills/ (available in all projects)" >&2
        echo "" >&2
        echo "  [2] Skip" >&2
        echo "" >&2

        choice=$(prompt_choice "Choose [1-2] (default: 1): " "1")
        case "$choice" in
            2)
                echo "" >&2
                echo "⏭️  Skipped" >&2
                ;;
            1|"")
                echo "" >&2
                install_skill || return 1
                echo "" >&2
                echo "🎉 Skill installed successfully!" >&2
                echo "" >&2
                echo "Claude Code can now:" >&2
                echo "  • Execute Slack queries automatically" >&2
                echo "  • Search users and channels" >&2
                echo "  • Retrieve message history" >&2
                ;;
            *)
                echo "" >&2
                echo "❌ Invalid choice. Skipped." >&2
                ;;
        esac
    fi

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
}

acquire_install_lock() {
    command -v perl >/dev/null || {
        echo "❌ Installation requires Perl with core Fcntl for filesystem locking" >&2
        return 1
    }
    mkdir -p "$HOME/.claude" || return 1
    exec 9>>"$HOME/.claude/.slack-cli-install.lock" || return 1
    perl -MFcntl=:flock -e 'flock(STDIN, LOCK_EX) or die "Could not acquire installer lock: $!\n"' <&9 || return 1
}

main() {
    acquire_install_lock || return 1
    echo "🚀 Installing Slack CLI..." >&2
    echo "" >&2

    local binary_path=""
    local target
    local version
    local skill_ref
    local from_checkout=false
    local actual_version
    local binary_dir
    local display_install_dir
    local command_name
    local method
    local pinned
    target=$(detect_platform)
    version="${SLACK_CLI_VERSION:-}"
    version="${version#v}"
    if [ "$version" = "latest" ]; then
        version=""
    fi
    pinned="$version"

    if [ -n "$version" ] && ! is_valid_release_version "$version"; then
        echo "❌ Invalid release version: $version" >&2
        exit 1
    fi

    if command -v curl >/dev/null; then
        echo "Installation method:" >&2
        echo "  [1] Download prebuilt binary (RECOMMENDED - fast)" >&2
        echo "  [2] Build from source (requires rustup; uses the repo's pinned toolchain)" >&2
        echo "" >&2
        method=$(prompt_choice "Choose [1-2] (default: 1): " "1")

        case "$method" in
            2)
                binary_path=$(build_from_source)
                from_checkout=true
                [ -n "$pinned" ] || version=""
                ;;
            1|"")
                if [ -z "$version" ]; then
                    version=$(get_latest_version) || {
                        echo "❌ Could not resolve the latest release; set SLACK_CLI_VERSION to install a specific release" >&2
                        exit 1
                    }
                fi
                binary_path=$(download_binary "$version" "$target") || exit 1
                binary_dir="$(dirname "$binary_path")"
                case "$(basename "$binary_dir")" in
                    slack-cli-install.*) BINARY_TMP_DIR="$binary_dir" ;;
                esac
                ;;
            *)
                echo "❌ Invalid choice" >&2
                exit 1
                ;;
        esac
    else
        [ -z "$version" ] && echo "⚠️  Cannot fetch latest version, building from source" >&2
        binary_path=$(build_from_source)
        from_checkout=true
    fi

    actual_version=$("$binary_path" --version)
    actual_version="${actual_version#slack-cli }"
    if ! is_valid_release_version "$actual_version"; then
        echo "❌ Could not determine binary version" >&2
        exit 1
    fi
    if [ -n "$version" ] && [ "$actual_version" != "$version" ]; then
        echo "❌ Expected binary v$version, got v$actual_version" >&2
        exit 1
    fi
    skill_ref="v$actual_version"

    install_binary "$binary_path"

    echo "" >&2
    display_install_dir=$(display_path "$INSTALL_DIR")
    command_name="$BINARY_NAME"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
        echo "✅ $INSTALL_DIR is in PATH" >&2
        ;;
        *)
        command_name="$display_install_dir/$BINARY_NAME"
        echo "⚠️  $INSTALL_DIR not in PATH" >&2
        echo "" >&2
        echo "Add to shell profile (~/.bashrc, ~/.zshrc):" >&2
        echo "  export PATH=\"$display_install_dir:\$PATH\"" >&2
        ;;
    esac
    echo "" >&2

    if [ -x "$INSTALL_DIR/$BINARY_NAME" ]; then
        echo "Installed version:" >&2
        "$INSTALL_DIR/$BINARY_NAME" --version >&2
        echo "" >&2
    fi

    prompt_skill_installation "$skill_ref" "$from_checkout"

    echo "" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "🎉 Installation Complete!" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "" >&2
    echo "Next steps:" >&2
    echo "" >&2
    echo "1. Authenticate:        $command_name auth login" >&2
    echo "2. Refresh cache:       $command_name cache refresh" >&2
    echo "3. Search users:        $command_name users <query>" >&2
    exec 9>&-
    echo "" >&2
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main
fi
