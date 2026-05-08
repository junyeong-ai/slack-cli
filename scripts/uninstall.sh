#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="slack-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
SKILL_NAME="slack-workspace"
USER_SKILL_DIR="$HOME/.claude/skills/$SKILL_NAME"
CONFIG_DIR="$HOME/.config/slack-cli"

ASSUME_YES=false
REMOVE_SKILL=""
BACKUP_SKILL=""
REMOVE_CONFIG=""

usage() {
    cat <<EOF
Usage: uninstall.sh [options]

Options:
  -y, --yes             Remove binary, skill, and configuration without prompts
      --remove-skill    Remove the installed user-level skill
      --keep-skill      Keep the installed user-level skill
      --backup-skill    Back up the user-level skill before removal
      --no-backup-skill Remove the user-level skill without backup
      --remove-config   Remove configuration and cache
      --keep-config     Keep configuration and cache
  -h, --help            Show this help
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        -y|--yes)
            ASSUME_YES=true
            ;;
        --remove-skill)
            REMOVE_SKILL=yes
            ;;
        --keep-skill)
            REMOVE_SKILL=no
            ;;
        --backup-skill)
            BACKUP_SKILL=yes
            ;;
        --no-backup-skill)
            BACKUP_SKILL=no
            ;;
        --remove-config)
            REMOVE_CONFIG=yes
            ;;
        --keep-config)
            REMOVE_CONFIG=no
            ;;
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
    shift
done

if [ "$ASSUME_YES" = true ]; then
    REMOVE_SKILL="${REMOVE_SKILL:-yes}"
    BACKUP_SKILL="${BACKUP_SKILL:-yes}"
    REMOVE_CONFIG="${REMOVE_CONFIG:-yes}"
fi

prompt_yes_no() {
    local prompt="$1"
    local default="$2"
    local configured="$3"
    local reply=""

    case "$configured" in
        yes|no)
            [ "$configured" = yes ]
            return
            ;;
    esac

    if [ -t 0 ]; then
        read -r -n 1 -p "$prompt" reply || reply=""
        echo
    else
        reply="$default"
    fi

    reply="${reply:-$default}"
    [[ "$reply" =~ ^[Yy]$ ]]
}

remove_empty_dir() {
    local dir="$1"

    if [ -d "$dir" ] && [ -z "$(find "$dir" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
        rmdir "$dir"
    fi
}

echo "🗑️  Uninstalling Slack CLI..."
echo

if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    rm "$INSTALL_DIR/$BINARY_NAME"
    echo "✅ Removed binary: $INSTALL_DIR/$BINARY_NAME"
else
    echo "⚠️  Binary not found at $INSTALL_DIR/$BINARY_NAME"
fi
echo

if [ -d "$USER_SKILL_DIR" ]; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🤖 Claude Code Skill Found"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo
    echo "User-level skill detected at: $USER_SKILL_DIR"
    echo

    if prompt_yes_no "Remove Claude Code skill? [y/N]: " "n" "$REMOVE_SKILL"; then
        if prompt_yes_no "Create backup before removing? [Y/n]: " "y" "$BACKUP_SKILL"; then
            timestamp=$(date +%Y%m%d_%H%M%S)
            backup_dir="$USER_SKILL_DIR.backup_$timestamp"
            cp -R "$USER_SKILL_DIR" "$backup_dir"
            echo "📦 Backup created: $backup_dir"
        fi

        rm -rf "$USER_SKILL_DIR"
        echo "✅ Removed user-level skill"

        remove_empty_dir "$HOME/.claude/skills"
        remove_empty_dir "$HOME/.claude"
    else
        echo "⏭️  Kept user-level skill"
    fi
    echo
else
    echo "ℹ️  No user-level skill found at $USER_SKILL_DIR"
    echo
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "⚙️  Configuration & Cache"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

if [ -d "$CONFIG_DIR" ]; then
    echo "Found configuration directory: $CONFIG_DIR"
    echo

    if prompt_yes_no "Remove configuration and cache? [y/N]: " "n" "$REMOVE_CONFIG"; then
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
echo
echo "• Project-level skill (if any) remains at .claude/skills/$SKILL_NAME"
echo "  This is distributed via git and shared with your team"
echo
echo "• Environment variables are not removed automatically:"
echo "  - SLACK_BOT_TOKEN"
echo "  - SLACK_USER_TOKEN"
echo
echo "• To reinstall: ./install.sh"
echo
