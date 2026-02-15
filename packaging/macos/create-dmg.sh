#!/usr/bin/env bash
# Create a .dmg installer for bisc.
#
# Prerequisites:
#   1. Run bundle.sh first to create bisc.app
#   2. Sign the .app bundle with a Developer ID certificate
#
# Usage:
#   ./packaging/macos/create-dmg.sh
#
# Output: target/release/bisc-0.1.0.dmg

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP_DIR="$PROJECT_ROOT/target/release/bisc.app"
DMG_PATH="$PROJECT_ROOT/target/release/bisc-0.1.0.dmg"
DMG_STAGING="$PROJECT_ROOT/target/release/dmg-staging"

if [[ ! -d "$APP_DIR" ]]; then
    echo "Error: bisc.app not found. Run bundle.sh first."
    exit 1
fi

echo "==> Creating .dmg..."

rm -rf "$DMG_STAGING" "$DMG_PATH"
mkdir -p "$DMG_STAGING"

cp -R "$APP_DIR" "$DMG_STAGING/"
ln -s /Applications "$DMG_STAGING/Applications"

hdiutil create -volname "bisc" \
    -srcfolder "$DMG_STAGING" \
    -ov -format UDZO \
    "$DMG_PATH"

rm -rf "$DMG_STAGING"

echo "==> .dmg created at: $DMG_PATH"
echo "    Size: $(du -h "$DMG_PATH" | cut -f1)"
echo ""
echo "==> To notarize:"
echo "    xcrun notarytool submit '$DMG_PATH' --apple-id YOUR_APPLE_ID --team-id YOUR_TEAM_ID --password YOUR_APP_PASSWORD --wait"
echo "    xcrun stapler staple '$DMG_PATH'"
