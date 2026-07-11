#!/usr/bin/env bash
set -euo pipefail

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

    latest_url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null || true)
    case "$latest_url" in
        */releases/tag/v*)
            latest_url="${latest_url##*/releases/tag/v}"
            echo "${latest_url%%[/?#]*}"
            return 0
            ;;
    esac

    curl -sf "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' \
        | sed -E 's/.*"v([^"]+)".*/\1/' \
        || echo ""
}

download_binary() {
    local version="$1"
    local target="$2"
    local archive
    local url
    local checksum_url
    local binary_path

    if [ -n "$version" ]; then
        archive="slack-cli-v${version}-${target}.tar.gz"
        url="https://github.com/$REPO/releases/download/v${version}/${archive}"
    else
        archive="slack-cli-${target}.tar.gz"
        url="https://github.com/$REPO/releases/latest/download/${archive}"
    fi
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
        echo "❌ Checksum download failed" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 1
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
        return 1
    }
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "❌ Checksum mismatch for $archive" >&2
        echo "   expected: ${expected:-<empty>}" >&2
        echo "   actual:   $actual" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 1
    fi
    echo "   ✅ SHA-256 verified" >&2

    verify_signature "$url" "$archive" || {
        rm -rf "$BINARY_TMP_DIR"
        return 1
    }

    echo "📦 Extracting..." >&2
    if ! (cd "$BINARY_TMP_DIR" && tar -xzf "$archive") >&2; then
        rm -rf "$BINARY_TMP_DIR"
        return 1
    fi
    binary_path="$BINARY_TMP_DIR/$BINARY_NAME"

    if [ ! -x "$binary_path" ]; then
        echo "❌ Archive did not contain executable $BINARY_NAME" >&2
        rm -rf "$BINARY_TMP_DIR"
        return 1
    fi

    echo "$binary_path"
}

# Sigstore verification of the release signature. Opportunistic by design:
# skipped with a note when cosign is not installed, but a present cosign that
# fails to verify is a hard failure — never fall back past a bad signature.
verify_signature() {
    local url="$1"
    local archive="$2"

    if ! command -v cosign >/dev/null; then
        echo "ℹ️  cosign not found; skipping signature verification (SHA-256 already checked)" >&2
        return 0
    fi

    echo "🔏 Verifying sigstore signature..." >&2
    if ! (cd "$BINARY_TMP_DIR" && curl -fsSLO "${url}.bundle"); then
        echo "⚠️  Signature bundle not published for this release; skipping" >&2
        return 0
    fi

    if ! cosign verify-blob \
        --bundle "$BINARY_TMP_DIR/${archive}.bundle" \
        --certificate-identity-regexp "^https://github.com/$REPO/" \
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
    if ! (cd "$PROJECT_ROOT" && cargo build --release) >&2; then
        echo "❌ Build failed" >&2
        exit 1
    fi
    echo "$PROJECT_ROOT/target/release/$BINARY_NAME"
}

install_binary() {
    local binary_path="$1"

    mkdir -p "$INSTALL_DIR"
    cp "$binary_path" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    if [[ "$OSTYPE" == "darwin"* ]]; then
        codesign --force --deep --sign - "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true
    fi

    echo "✅ Installed to $INSTALL_DIR/$BINARY_NAME" >&2
}

get_skill_version() {
    local skill_md="$1"
    [ -f "$skill_md" ] && grep "^version:" "$skill_md" 2>/dev/null | sed 's/version: *//' || echo "unknown"
}

check_skill_exists() {
    [ -d "$USER_SKILL_DIR" ] && [ -f "$USER_SKILL_DIR/SKILL.md" ]
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

backup_skill() {
    local timestamp
    local backup_dir
    timestamp=$(date +%Y%m%d_%H%M%S)
    backup_dir="$USER_SKILL_DIR.backup_$timestamp"

    echo "📦 Creating backup: $backup_dir" >&2
    cp -r "$USER_SKILL_DIR" "$backup_dir"
    echo "   ✅ Backup created" >&2
}

install_skill() {
    echo "📋 Installing skill to $USER_SKILL_DIR" >&2
    mkdir -p "$(dirname "$USER_SKILL_DIR")"
    rm -rf "$USER_SKILL_DIR"
    cp -r "$SKILL_SOURCE_DIR" "$USER_SKILL_DIR"
    echo "   ✅ Skill installed" >&2
}

prepare_skill_source() {
    local ref="$1"
    local skill_url
    local fallback_url

    if [ -f "$PROJECT_SKILL_DIR/SKILL.md" ]; then
        SKILL_SOURCE_DIR="$PROJECT_SKILL_DIR"
        return 0
    fi

    SKILL_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/slack-cli-skill.XXXXXX")
    SKILL_SOURCE_DIR="$SKILL_TMP_DIR/$SKILL_NAME"
    mkdir -p "$SKILL_SOURCE_DIR"

    skill_url="https://raw.githubusercontent.com/$REPO/$ref/.claude/skills/$SKILL_NAME/SKILL.md"
    if ! curl -fsSL "$skill_url" -o "$SKILL_SOURCE_DIR/SKILL.md"; then
        if [ "$ref" != "main" ]; then
            fallback_url="https://raw.githubusercontent.com/$REPO/main/.claude/skills/$SKILL_NAME/SKILL.md"
            echo "⚠️  Skill not found at $ref; trying main" >&2
            if curl -fsSL "$fallback_url" -o "$SKILL_SOURCE_DIR/SKILL.md"; then
                return 0
            fi
        fi

        rm -rf "$SKILL_TMP_DIR"
        SKILL_TMP_DIR=""
        SKILL_SOURCE_DIR=""
        return 1
    fi

    return 0
}

prompt_skill_installation() {
    local ref="$1"
    local choice

    if ! prepare_skill_source "$ref"; then
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
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Skipped" >&2
                ;;
            older)
                echo "🔄 New version available: v$project_version" >&2
                echo "" >&2
                choice=$(prompt_choice "Update? [Y/n]: " "")
                [[ ! "$choice" =~ ^[nN]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; echo "   ✅ Updated to v$project_version" >&2; } || echo "   ⏭️  Keeping current version" >&2
                ;;
            newer)
                echo "⚠️  Installed version (v$existing_version) > project version (v$project_version)" >&2
                echo "" >&2
                choice=$(prompt_choice "Downgrade? [y/N]: " "")
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Keeping current version" >&2
                ;;
            *)
                echo "⚠️  Version comparison failed" >&2
                echo "" >&2
                choice=$(prompt_choice "Reinstall? [y/N]: " "")
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Skipped" >&2
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
                install_skill
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

main() {
    echo "🚀 Installing Slack CLI..." >&2
    echo "" >&2

    local binary_path=""
    local target
    local version
    local skill_ref="main"
    local binary_dir
    local display_install_dir
    local command_name
    local method
    target=$(detect_platform)
    version="${SLACK_CLI_VERSION:-}"
    version="${version#v}"
    if [ "$version" = "latest" ]; then
        version=""
    fi

    if command -v curl >/dev/null; then
        if [ -z "$version" ]; then
            version=$(get_latest_version)
            version="${version#v}"
        fi

        if [ -n "$version" ] && ! is_valid_release_version "$version"; then
            echo "❌ Invalid release version: $version" >&2
            exit 1
        fi

        if [ -n "$version" ]; then
            skill_ref="v$version"
            echo "Version: v$version" >&2
        else
            echo "Version: latest release" >&2
        fi
        echo "" >&2
        echo "Installation method:" >&2
        echo "  [1] Download prebuilt binary (RECOMMENDED - fast)" >&2
        echo "  [2] Build from source (requires rustup; uses the repo's pinned toolchain)" >&2
        echo "" >&2
        method=$(prompt_choice "Choose [1-2] (default: 1): " "1")

        case "$method" in
            2)
                binary_path=$(build_from_source)
                ;;
            1|"")
                binary_path=$(download_binary "$version" "$target") || {
                    echo "⚠️  Download failed, falling back to source build" >&2
                    binary_path=$(build_from_source)
                }
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
    fi

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

    prompt_skill_installation "$skill_ref"

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
    echo "" >&2
}

main
