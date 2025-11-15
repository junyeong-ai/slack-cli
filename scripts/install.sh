#!/usr/bin/env bash
set -e

BINARY_NAME="slack"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
SKILL_NAME="slack-workspace"
PROJECT_SKILL_DIR=".claude/skills/$SKILL_NAME"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"

echo "🚀 Installing Slack CLI..."
echo

# ============================================================================
# Skill Installation Functions
# ============================================================================

get_skill_version() {
    local skill_md="$1"
    if [ -f "$skill_md" ]; then
        grep "^version:" "$skill_md" 2>/dev/null | sed 's/version: *//' || echo "unknown"
    else
        echo "unknown"
    fi
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
        # Simple version comparison (assumes semantic versioning)
        if [ "$(printf '%s\n' "$ver1" "$ver2" | sort -V | head -n1)" = "$ver1" ]; then
            if [ "$ver1" != "$ver2" ]; then
                echo "older"
            else
                echo "equal"
            fi
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
    echo "   ✅ Backup created successfully"
}

install_skill() {
    echo "📋 Installing skill to $USER_SKILL_DIR"

    # Create parent directory if needed
    mkdir -p "$(dirname "$USER_SKILL_DIR")"

    # Copy skill files
    cp -r "$PROJECT_SKILL_DIR" "$USER_SKILL_DIR"

    echo "   ✅ Skill installed successfully"
}

prompt_skill_installation() {
    if [ ! -d "$PROJECT_SKILL_DIR" ]; then
        echo "ℹ️  No Claude Code skill found in project"
        return 0
    fi

    local project_version=$(get_skill_version "$PROJECT_SKILL_DIR/SKILL.md")

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🤖 Claude Code Skill Installation"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "This project includes a Claude Code skill for Slack workspace queries."
    echo "The skill enables Claude to execute Slack queries automatically."
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
                echo "✅ You have the latest version installed"
                echo ""
                read -p "Reinstall anyway? [y/N]: " choice
                case "$choice" in
                    y|Y)
                        backup_skill
                        rm -rf "$USER_SKILL_DIR"
                        install_skill
                        ;;
                    *)
                        echo "   ⏭️  Skipped"
                        ;;
                esac
                ;;
            older)
                echo "🔄 New version available: v$project_version"
                echo ""
                read -p "Update to v$project_version? [Y/n]: " choice
                case "$choice" in
                    n|N)
                        echo "   ⏭️  Keeping current version"
                        ;;
                    *)
                        backup_skill
                        rm -rf "$USER_SKILL_DIR"
                        install_skill
                        echo "   ✅ Updated to v$project_version"
                        ;;
                esac
                ;;
            newer)
                echo "⚠️  Your installed version (v$existing_version) is newer than project version (v$project_version)"
                echo ""
                read -p "Downgrade to v$project_version? [y/N]: " choice
                case "$choice" in
                    y|Y)
                        backup_skill
                        rm -rf "$USER_SKILL_DIR"
                        install_skill
                        ;;
                    *)
                        echo "   ⏭️  Keeping current version"
                        ;;
                esac
                ;;
            *)
                echo "⚠️  Version comparison failed"
                echo ""
                read -p "Reinstall anyway? [y/N]: " choice
                case "$choice" in
                    y|Y)
                        backup_skill
                        rm -rf "$USER_SKILL_DIR"
                        install_skill
                        ;;
                    *)
                        echo "   ⏭️  Skipped"
                        ;;
                esac
                ;;
        esac
    else
        echo "Installation options:"
        echo ""
        echo "  [1] User-level install (RECOMMENDED)"
        echo "      → Install to ~/.claude/skills/"
        echo "      → Available in ALL projects with Claude Code"
        echo "      → Survives project deletion"
        echo ""
        echo "  [2] Project-level only"
        echo "      → Use skill only in this project"
        echo "      → Requires project directory to work"
        echo ""
        echo "  [3] Skip installation"
        echo "      → CLI will work, but Claude won't auto-query Slack"
        echo ""

        read -p "Choose [1-3] (default: 1): " choice
        case "$choice" in
            2)
                echo ""
                echo "✅ Using project-level skill only"
                echo "   Location: $(pwd)/$PROJECT_SKILL_DIR"
                echo ""
                echo "ℹ️  Skill will only work when Claude Code is opened in this project directory"
                ;;
            3)
                echo ""
                echo "⏭️  Skill installation skipped"
                echo ""
                echo "ℹ️  You can install the skill later by running this script again"
                ;;
            1|"")
                echo ""
                install_skill
                echo ""
                echo "🎉 Skill installed successfully!"
                echo ""
                echo "Claude Code can now:"
                echo "  • Execute Slack queries automatically"
                echo "  • Find users and channels"
                echo "  • Retrieve message history"
                echo "  • Search conversations"
                echo ""
                ;;
            *)
                echo ""
                echo "❌ Invalid choice. Skipping skill installation."
                ;;
        esac
    fi

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

# ============================================================================
# Binary Installation
# ============================================================================

# Build release binary
echo "📦 Building release binary..."
cargo build --release

# Create install directory if it doesn't exist
mkdir -p "$INSTALL_DIR"

# Copy binary
echo "📋 Installing to $INSTALL_DIR/$BINARY_NAME"
cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

# macOS: Ad-hoc sign the binary to prevent "Killed: 9" errors
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "🔏 Signing binary (macOS)..."
    codesign --force --deep --sign - "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true
fi

echo
echo "✅ Binary installation complete!"
echo
echo "Binary installed to: $INSTALL_DIR/$BINARY_NAME"
echo

# Check if in PATH
if echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo "✅ $INSTALL_DIR is in your PATH"
    echo
    echo "You can now run: $BINARY_NAME --help"
else
    echo "⚠️  $INSTALL_DIR is not in your PATH"
    echo
    echo "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo
    echo "Then reload your shell:"
    echo "  source ~/.zshrc  # or ~/.bashrc"
fi
echo

# Check version
if command -v "$BINARY_NAME" &> /dev/null; then
    echo "Installed version:"
    "$BINARY_NAME" --version
    echo
fi

# ============================================================================
# Skill Installation Prompt
# ============================================================================

prompt_skill_installation

# ============================================================================
# Final Message
# ============================================================================

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🎉 Installation Complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📝 Next steps:"
echo ""
echo "1. Initialize configuration:"
echo "   $BINARY_NAME config init"
echo ""
echo "2. Refresh cache:"
echo "   $BINARY_NAME cache refresh"
echo ""
echo "3. Search users:"
echo "   $BINARY_NAME users <query>"
echo ""
