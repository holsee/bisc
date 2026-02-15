#!/usr/bin/env bash
# Build a macOS .app bundle for bisc.
#
# Usage:
#   ./packaging/macos/bundle.sh [--universal]
#
# Options:
#   --universal   Build a universal binary (arm64 + x86_64)
#
# Output: target/release/bisc.app/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APP_DIR="$PROJECT_ROOT/target/release/bisc.app"

echo "==> Building bisc release binary..."

if [[ "${1:-}" == "--universal" ]]; then
    echo "    Building universal binary (arm64 + x86_64)..."
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    BINARY_PATH="$PROJECT_ROOT/target/release/bisc-universal"
    lipo -create \
        "$PROJECT_ROOT/target/aarch64-apple-darwin/release/bisc" \
        "$PROJECT_ROOT/target/x86_64-apple-darwin/release/bisc" \
        -output "$BINARY_PATH"
else
    cargo build --release
    BINARY_PATH="$PROJECT_ROOT/target/release/bisc"
fi

echo "==> Creating .app bundle..."

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$BINARY_PATH" "$APP_DIR/Contents/MacOS/bisc"
cp "$SCRIPT_DIR/Info.plist" "$APP_DIR/Contents/Info.plist"

# Copy icon if it exists
if [[ -f "$SCRIPT_DIR/bisc.icns" ]]; then
    cp "$SCRIPT_DIR/bisc.icns" "$APP_DIR/Contents/Resources/bisc.icns"
else
    echo "    Warning: no bisc.icns found, .app will use default icon"
fi

echo "==> .app bundle created at: $APP_DIR"
echo "    Binary size: $(du -h "$APP_DIR/Contents/MacOS/bisc" | cut -f1)"
echo ""
echo "To sign:  codesign --sign 'Developer ID Application: ...' --deep '$APP_DIR'"
echo "To create .dmg: see packaging/macos/create-dmg.sh"
