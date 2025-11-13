#!/usr/bin/env bash
set -e

BINARY_NAME="slack"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "🗑️  Uninstalling Slack CLI..."
echo

if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    rm "$INSTALL_DIR/$BINARY_NAME"
    echo "✅ Removed $INSTALL_DIR/$BINARY_NAME"
else
    echo "⚠️  Binary not found at $INSTALL_DIR/$BINARY_NAME"
fi

# Remove global config (optional)
echo
read -p "Remove global configuration? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    # Detect config directory based on OS
    if [[ "$OSTYPE" == "darwin"* ]]; then
        CONFIG_DIR="$HOME/Library/Application Support/slack-cli"
    else
        CONFIG_DIR="$HOME/.config/slack-cli"
    fi

    if [ -d "$CONFIG_DIR" ]; then
        rm -rf "$CONFIG_DIR"
        echo "✅ Removed $CONFIG_DIR"
    else
        echo "⚠️  Config directory not found at $CONFIG_DIR"
    fi
fi

# Remove cache (optional)
echo
read -p "Remove cache directory? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    # Detect cache directory based on OS
    if [[ "$OSTYPE" == "darwin"* ]]; then
        CACHE_DIR="$HOME/Library/Application Support/slack-cli/cache"
    else
        CACHE_DIR="$HOME/.local/share/slack-cli"
    fi

    if [ -d "$CACHE_DIR" ]; then
        rm -rf "$CACHE_DIR"
        echo "✅ Removed $CACHE_DIR"
    else
        echo "⚠️  Cache directory not found at $CACHE_DIR"
    fi
fi

echo
echo "✅ Uninstallation complete!"
echo
echo "Note: Environment variables are NOT removed automatically."
echo "If you have SLACK_* variables in your shell profile, remove them manually:"
echo "  - SLACK_BOT_TOKEN"
echo "  - SLACK_USER_TOKEN"
echo
