#!/usr/bin/env bash
set -e

BINARY_NAME="slack"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "🚀 Installing Slack CLI..."
echo

# Build release binary
echo "📦 Building release binary..."
cargo build --release

# Create install directory if it doesn't exist
mkdir -p "$INSTALL_DIR"

# Copy binary
echo "📋 Installing to $INSTALL_DIR/$BINARY_NAME"
cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo
echo "✅ Installation complete!"
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

# Setup instructions
echo "📝 Next steps:"
echo
echo "1. Initialize configuration:"
echo "   $BINARY_NAME config init"
echo
echo "2. Refresh cache:"
echo "   $BINARY_NAME cache refresh"
echo
echo "3. Search users:"
echo "   $BINARY_NAME users <query>"
echo
