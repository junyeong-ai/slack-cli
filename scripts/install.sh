#!/usr/bin/env bash
set -e

BINARY_NAME="slack"
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
    local archive="slack-v${version}-${target}.tar.gz"
    local url="https://github.com/$REPO/releases/download/v${version}/${archive}"
    local checksum_url="${url}.sha256"

    echo "📥 Downloading $archive..."
    if ! curl -fLO "$url"; then
        echo "❌ Download failed"
        return 1
    fi

    echo "🔐 Verifying checksum..."
    if curl -fLO "$checksum_url"; then
        if command -v sha256sum >/dev/null; then
            sha256sum -c "${archive}.sha256" || return 1
        elif command -v shasum >/dev/null; then
            shasum -a 256 -c "${archive}.sha256" || return 1
        else
            echo "⚠️  No checksum tool found, skipping verification"
        fi
    fi

    echo "📦 Extracting..."
    tar -xzf "$archive"
    rm -f "$archive" "${archive}.sha256"

    echo "$BINARY_NAME"
}

build_from_source() {
    echo "🔨 Building from source..."
    cargo build --release
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

    echo "✅ Installed to $INSTALL_DIR/$BINARY_NAME"
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

    echo "📦 Creating backup: $backup_dir"
    cp -r "$USER_SKILL_DIR" "$backup_dir"
    echo "   ✅ Backup created"
}

install_skill() {
    echo "📋 Installing skill to $USER_SKILL_DIR"
    mkdir -p "$(dirname "$USER_SKILL_DIR")"
    cp -r "$PROJECT_SKILL_DIR" "$USER_SKILL_DIR"
    echo "   ✅ Skill installed"
}

prompt_skill_installation() {
    [ ! -d "$PROJECT_SKILL_DIR" ] && return 0

    local project_version=$(get_skill_version "$PROJECT_SKILL_DIR/SKILL.md")

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🤖 Claude Code Skill Installation"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Skill: $SKILL_NAME (v$project_version)"
    echo ""

    if check_skill_exists; then
        local existing_version=$(get_skill_version "$USER_SKILL_DIR/SKILL.md")
        local comparison=$(compare_versions "$existing_version" "$project_version")

        echo "Status: Already installed (v$existing_version)"
        echo ""

        case "$comparison" in
            equal)
                echo "✅ Latest version installed"
                echo ""
                read -p "Reinstall? [y/N]: " choice
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Skipped"
                ;;
            older)
                echo "🔄 New version available: v$project_version"
                echo ""
                read -p "Update? [Y/n]: " choice
                [[ ! "$choice" =~ ^[nN]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; echo "   ✅ Updated to v$project_version"; } || echo "   ⏭️  Keeping current version"
                ;;
            newer)
                echo "⚠️  Installed version (v$existing_version) > project version (v$project_version)"
                echo ""
                read -p "Downgrade? [y/N]: " choice
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Keeping current version"
                ;;
            *)
                echo "⚠️  Version comparison failed"
                echo ""
                read -p "Reinstall? [y/N]: " choice
                [[ "$choice" =~ ^[yY]$ ]] && { backup_skill; rm -rf "$USER_SKILL_DIR"; install_skill; } || echo "   ⏭️  Skipped"
                ;;
        esac
    else
        echo "Installation options:"
        echo ""
        echo "  [1] User-level install (RECOMMENDED)"
        echo "      → ~/.claude/skills/ (available in all projects)"
        echo ""
        echo "  [2] Project-level only"
        echo "      → Works only in this project directory"
        echo ""
        echo "  [3] Skip"
        echo ""

        read -p "Choose [1-3] (default: 1): " choice
        case "$choice" in
            2)
                echo ""
                echo "✅ Using project-level skill"
                echo "   Location: $(pwd)/$PROJECT_SKILL_DIR"
                ;;
            3)
                echo ""
                echo "⏭️  Skipped"
                ;;
            1|"")
                echo ""
                install_skill
                echo ""
                echo "🎉 Skill installed successfully!"
                echo ""
                echo "Claude Code can now:"
                echo "  • Execute Slack queries automatically"
                echo "  • Search users and channels"
                echo "  • Retrieve message history"
                ;;
            *)
                echo ""
                echo "❌ Invalid choice. Skipped."
                ;;
        esac
    fi

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

main() {
    echo "🚀 Installing Slack CLI..."
    echo ""

    local binary_path=""
    local target=$(detect_platform)
    local version=$(get_latest_version)

    if [ -n "$version" ] && command -v curl >/dev/null; then
        echo "Latest version: v$version"
        echo ""
        echo "Installation method:"
        echo "  [1] Download prebuilt binary (RECOMMENDED - fast)"
        echo "  [2] Build from source (requires Rust toolchain)"
        echo ""
        read -p "Choose [1-2] (default: 1): " method

        case "$method" in
            2)
                binary_path=$(build_from_source)
                ;;
            1|"")
                binary_path=$(download_binary "$version" "$target") || {
                    echo "⚠️  Download failed, falling back to source build"
                    binary_path=$(build_from_source)
                }
                ;;
            *)
                echo "❌ Invalid choice"
                exit 1
                ;;
        esac
    else
        [ -z "$version" ] && echo "⚠️  Cannot fetch latest version, building from source"
        binary_path=$(build_from_source)
    fi

    install_binary "$binary_path"

    echo ""
    if echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo "✅ $INSTALL_DIR is in PATH"
    else
        echo "⚠️  $INSTALL_DIR not in PATH"
        echo ""
        echo "Add to shell profile (~/.bashrc, ~/.zshrc):"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    echo ""

    if command -v "$BINARY_NAME" &>/dev/null; then
        echo "Installed version:"
        "$BINARY_NAME" --version
        echo ""
    fi

    prompt_skill_installation

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🎉 Installation Complete!"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Next steps:"
    echo ""
    echo "1. Initialize config:   $BINARY_NAME config init"
    echo "2. Refresh cache:       $BINARY_NAME cache refresh"
    echo "3. Search users:        $BINARY_NAME users <query>"
    echo ""
}

main
