#!/usr/bin/env bash
# Build an AppImage for bisc.
#
# Prerequisites:
#   - cargo (Rust toolchain)
#   - linuxdeploy (https://github.com/linuxdeploy/linuxdeploy)
#     or appimagetool (https://github.com/AppImage/appimagetool)
#
# Usage:
#   ./packaging/linux/appimage.sh
#
# Output: target/release/bisc-x86_64.AppImage

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APPDIR="$PROJECT_ROOT/target/release/bisc.AppDir"

echo "==> Building bisc release binary..."
cargo build --release

echo "==> Binary details:"
echo "    Size: $(du -h "$PROJECT_ROOT/target/release/bisc" | cut -f1)"
echo "    Dynamic deps:"
ldd "$PROJECT_ROOT/target/release/bisc" | sed 's/^/        /'

echo "==> Creating AppDir structure..."

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$PROJECT_ROOT/target/release/bisc" "$APPDIR/usr/bin/bisc"
cp "$SCRIPT_DIR/bisc.desktop" "$APPDIR/usr/share/applications/bisc.desktop"
cp "$SCRIPT_DIR/bisc.desktop" "$APPDIR/bisc.desktop"

# Copy icon if it exists, otherwise create a placeholder
if [[ -f "$SCRIPT_DIR/bisc.png" ]]; then
    cp "$SCRIPT_DIR/bisc.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/bisc.png"
    cp "$SCRIPT_DIR/bisc.png" "$APPDIR/bisc.png"
else
    echo "    Warning: no bisc.png found, AppImage will use no icon"
    # Create a minimal 1x1 PNG as placeholder so appimagetool doesn't complain
    printf '\x89PNG\r\n\x1a\n' > "$APPDIR/bisc.png"
fi

# Create AppRun
cat > "$APPDIR/AppRun" << 'APPRUN'
#!/usr/bin/env bash
SELF="$(readlink -f "$0")"
APPDIR="$(dirname "$SELF")"
exec "$APPDIR/usr/bin/bisc" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

echo "==> AppDir created at: $APPDIR"

# Try to create AppImage if appimagetool is available
if command -v appimagetool &> /dev/null; then
    echo "==> Creating AppImage with appimagetool..."
    ARCH=x86_64 appimagetool "$APPDIR" "$PROJECT_ROOT/target/release/bisc-x86_64.AppImage"
    echo "==> AppImage created: target/release/bisc-x86_64.AppImage"
elif command -v linuxdeploy &> /dev/null; then
    echo "==> Creating AppImage with linuxdeploy..."
    linuxdeploy --appdir "$APPDIR" --output appimage
    echo "==> AppImage created"
else
    echo ""
    echo "==> To create the AppImage, install one of:"
    echo "    - appimagetool: https://github.com/AppImage/appimagetool/releases"
    echo "    - linuxdeploy:  https://github.com/linuxdeploy/linuxdeploy/releases"
    echo ""
    echo "    Then run: ARCH=x86_64 appimagetool '$APPDIR' target/release/bisc-x86_64.AppImage"
fi
