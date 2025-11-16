#!/usr/bin/env bash
set -e

BINARY_NAME="slack-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
SKILL_NAME="slack-workspace"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"
CONFIG_DIR="$HOME/.config/slack-cli"

echo "🗑️  Uninstalling Slack CLI..."
echo

# ============================================================================
# Binary Removal
# ============================================================================

if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    rm "$INSTALL_DIR/$BINARY_NAME"
    echo "✅ Removed binary: $INSTALL_DIR/$BINARY_NAME"
else
    echo "⚠️  Binary not found at $INSTALL_DIR/$BINARY_NAME"
fi
echo

# ============================================================================
# Skill Removal
# ============================================================================

if [ -d "$USER_SKILL_DIR" ]; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🤖 Claude Code Skill Found"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "User-level skill detected at: $USER_SKILL_DIR"
    echo ""
    read -p "Remove Claude Code skill? [y/N]: " -n 1 -r
    echo
    echo

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        read -p "Create backup before removing? [Y/n]: " -n 1 -r
        echo
        echo

        if [[ ! $REPLY =~ ^[Nn]$ ]]; then
            # Create backup
            local timestamp=$(date +%Y%m%d_%H%M%S)
            local backup_dir="$USER_SKILL_DIR.backup_$timestamp"
            cp -r "$USER_SKILL_DIR" "$backup_dir"
            echo "📦 Backup created: $backup_dir"
        fi

        rm -rf "$USER_SKILL_DIR"
        echo "✅ Removed user-level skill"

        # Cleanup empty parent directory if it exists
        if [ -d "$HOME/.claude/skills" ] && [ -z "$(ls -A "$HOME/.claude/skills")" ]; then
            rmdir "$HOME/.claude/skills"
            echo "   Cleaned up empty skills directory"
        fi
    else
        echo "⏭️  Kept user-level skill"
    fi
    echo
else
    echo "ℹ️  No user-level skill found at $USER_SKILL_DIR"
    echo
fi

# ============================================================================
# Configuration Removal
# ============================================================================

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "⚙️  Configuration & Cache"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

if [ -d "$CONFIG_DIR" ]; then
    echo "Found configuration directory: $CONFIG_DIR"
    echo ""
    read -p "Remove configuration and cache? [y/N]: " -n 1 -r
    echo
    echo

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "$CONFIG_DIR"
        echo "✅ Removed configuration and cache: $CONFIG_DIR"
    else
        echo "⏭️  Kept configuration and cache"
    fi
else
    echo "ℹ️  No configuration directory found"
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Uninstallation Complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

echo "ℹ️  Notes:"
echo ""
echo "• Project-level skill (if any) remains at .claude/skills/$SKILL_NAME"
echo "  This is distributed via git and shared with your team"
echo ""
echo "• Environment variables are NOT removed automatically"
echo "  If you have these in your shell profile, remove them manually:"
echo "  - SLACK_BOT_TOKEN"
echo "  - SLACK_USER_TOKEN"
echo ""
echo "• To reinstall: ./install.sh"
echo
