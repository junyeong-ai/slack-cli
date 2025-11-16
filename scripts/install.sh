#!/usr/bin/env bash
set -e

BINARY_NAME="slack-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
REPO="junyeong-ai/slack-cli"
SKILL_NAME="slack-workspace"
PROJECT_SKILL_DIR=".claude/skills/$SKILL_NAME"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"

detect_platform() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')
    local arch=$(uname -m)

    case "$os" in
        linux) os="unknown-linux-gnu" ;;
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

get_latest_version() {
    curl -sf "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' \
        | sed -E 's/.*"v([^"]+)".*/\1/' \
        || echo ""
}

download_binary() {
    local version="$1"
    local target="$2"
    local archive="slack-cli-v${version}-${target}.tar.gz"
    local url="https://github.com/$REPO/releases/download/v${version}/${archive}"
    local checksum_url="${url}.sha256"

    echo "📥 Downloading $archive..." >&2
    if ! curl -fLO "$url" 2>&1 | grep -v "%" >&2; then
        echo "❌ Download failed" >&2
        return 1
    fi

    echo "🔐 Verifying checksum..." >&2
    if curl -fLO "$checksum_url" 2>&1 | grep -v "%" >&2; then
        if command -v sha256sum >/dev/null; then
            sha256sum -c "${archive}.sha256" >&2 || return 1
        elif command -v shasum >/dev/null; then
            shasum -a 256 -c "${archive}.sha256" >&2 || return 1
        else
            echo "⚠️  No checksum tool found, skipping verification" >&2
        fi
    fi

    echo "📦 Extracting..." >&2
    tar -xzf "$archive" 2>&1 | grep -v "x " >&2
    rm -f "$archive" "${archive}.sha256"

    echo "$BINARY_NAME"
}

build_from_source() {
    echo "🔨 Building from source..." >&2
    if ! cargo build --release 2>&1 | grep -E "Compiling|Finished|error" >&2; then
        echo "❌ Build failed" >&2
        exit 1
    fi
    echo "target/release/$BINARY_NAME"
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

    if [ "$ver1" = "$ver2" ]; then
        echo "equal"
    elif [ "$ver1" = "unknown" ] || [ "$ver2" = "unknown" ]; then
        echo "unknown"
    else
        if [ "$(printf '%s\n' "$ver1" "$ver2" | sort -V | head -n1)" = "$ver1" ]; then
            [ "$ver1" != "$ver2" ] && echo "older" || echo "equal"
        else
            echo "newer"
        fi
    fi
}

backup_skill() {
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local backup_dir="$USER_SKILL_DIR.backup_$timestamp"

    echo "📦 Creating backup: $backup_dir" >&2
    cp -r "$USER_SKILL_DIR" "$backup_dir"
    echo "   ✅ Backup created" >&2
}

install_skill() {
    echo "📋 Installing skill to $USER_SKILL_DIR" >&2
    mkdir -p "$(dirname "$USER_SKILL_DIR")"
    cp -r "$PROJECT_SKILL_DIR" "$USER_SKILL_DIR"
    echo "   ✅ Skill installed" >&2
}

prompt_skill_installation() {
    [ ! -d "$PROJECT_SKILL_DIR" ] && return 0

    local project_version=$(get_skill_version "$PROJECT_SKILL_DIR/SKILL.md")

    echo "" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "🤖 Claude Code Skill Installation" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "" >&2
    echo "Skill: $SKILL_NAME (v$project_version)" >&2
    echo "" >&2

    if check_skill_exists; then
        local existing_version=$(get_skill_version "$USER_SKILL_DIR/SKILL.md")
        local comparison=$(compare_versions "$existing_version" "$project_version")

        echo "Status: Already installed (v$existing_version)" >&2
        echo "" >&2

        case "$comparison" in
            equal)
                echo "✅ Latest version installed" >&2
                echo "" >&2
                read -p "Reinstall? [y/N]: " choice
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Skipped" >&2
                ;;
            older)
                echo "🔄 New version available: v$project_version" >&2
                echo "" >&2
                read -p "Update? [Y/n]: " choice
                [[ ! "$choice" =~ ^[nN]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; echo "   ✅ Updated to v$project_version" >&2; } || echo "   ⏭️  Keeping current version" >&2
                ;;
            newer)
                echo "⚠️  Installed version (v$existing_version) > project version (v$project_version)" >&2
                echo "" >&2
                read -p "Downgrade? [y/N]: " choice
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Keeping current version" >&2
                ;;
            *)
                echo "⚠️  Version comparison failed" >&2
                echo "" >&2
                read -p "Reinstall? [y/N]: " choice
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Skipped" >&2
                ;;
        esac
    else
        echo "Installation options:" >&2
        echo "" >&2
        echo "  [1] User-level install (RECOMMENDED)" >&2
        echo "      → ~/.claude/skills/ (available in all projects)" >&2
        echo "" >&2
        echo "  [2] Project-level only" >&2
        echo "      → Works only in this project directory" >&2
        echo "" >&2
        echo "  [3] Skip" >&2
        echo "" >&2

        read -p "Choose [1-3] (default: 1): " choice
        case "$choice" in
            2)
                echo "" >&2
                echo "✅ Using project-level skill" >&2
                echo "   Location: $(pwd)/$PROJECT_SKILL_DIR" >&2
                ;;
            3)
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
    local target=$(detect_platform)
    local version=$(get_latest_version)

    if [ -n "$version" ] && command -v curl >/dev/null; then
        echo "Latest version: v$version" >&2
        echo "" >&2
        echo "Installation method:" >&2
        echo "  [1] Download prebuilt binary (RECOMMENDED - fast)" >&2
        echo "  [2] Build from source (requires Rust toolchain)" >&2
        echo "" >&2
        read -p "Choose [1-2] (default: 1): " method

        case "$method" in
            2)
                binary_path=$(build_from_source)
                ;;
            1|"")
                binary_path=$(download_binary "$version" "$target") || {
                    echo "⚠️  Download failed, falling back to source build" >&2
                    binary_path=$(build_from_source)
                }
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
    if echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo "✅ $INSTALL_DIR is in PATH" >&2
    else
        echo "⚠️  $INSTALL_DIR not in PATH" >&2
        echo "" >&2
        echo "Add to shell profile (~/.bashrc, ~/.zshrc):" >&2
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\"" >&2
    fi
    echo "" >&2

    if command -v "$BINARY_NAME" &>/dev/null; then
        echo "Installed version:" >&2
        "$BINARY_NAME" --version >&2
        echo "" >&2
    fi

    prompt_skill_installation

    echo "" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "🎉 Installation Complete!" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "" >&2
    echo "Next steps:" >&2
    echo "" >&2
    echo "1. Initialize config:   $BINARY_NAME config init" >&2
    echo "2. Refresh cache:       $BINARY_NAME cache refresh" >&2
    echo "3. Search users:        $BINARY_NAME users <query>" >&2
    echo "" >&2
}

main
