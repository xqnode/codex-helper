#!/usr/bin/env bash
# 在 macOS 上构建 Codex Helper.app 与 DMG（需在 Apple Silicon 或 Intel Mac 上运行）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
VERSION="$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
APP_NAME="Codex Helper"
APP_BUNDLE="$DIST/${APP_NAME}.app"
DMG_PATH="$DIST/CodexHelper-${VERSION}-macos.dmg"
BINARY="$ROOT/target/release/codex-helper"
INFO_PLIST="$ROOT/installer/macos/Info.plist"
README="$ROOT/installer/USAGE-zh-CN.txt"

echo "Codex Helper v${VERSION} (macOS)"

cd "$ROOT"
echo "Building release..."
cargo build --release

if [[ ! -f "$BINARY" ]]; then
    echo "Missing binary: $BINARY" >&2
    exit 1
fi

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

cp "$BINARY" "$APP_BUNDLE/Contents/MacOS/codex-helper"
chmod +x "$APP_BUNDLE/Contents/MacOS/codex-helper"

# 同步版本号到 Info.plist
cp "$INFO_PLIST" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${VERSION}" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${VERSION}" "$APP_BUNDLE/Contents/Info.plist"

if [[ -f "$README" ]]; then
    cp "$README" "$APP_BUNDLE/Contents/Resources/USAGE-zh-CN.txt"
fi

# 可选：从 PNG 生成 .icns（需 assets/codex-helper.png，可手动放置）
if [[ -f "$ROOT/assets/codex-helper.png" ]]; then
    ICONSET="$DIST/AppIcon.iconset"
    rm -rf "$ICONSET"
    mkdir -p "$ICONSET"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$ROOT/assets/codex-helper.png" \
            --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        sips -z "$double" "$double" "$ROOT/assets/codex-helper.png" \
            --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
    /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$APP_BUNDLE/Contents/Info.plist" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Set :CFBundleIconFile AppIcon" "$APP_BUNDLE/Contents/Info.plist"
    rm -rf "$ICONSET"
fi

mkdir -p "$DIST"
STAGING="$DIST/dmg-staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"
cp -R "$APP_BUNDLE" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
if [[ -f "$README" ]]; then
    cp "$README" "$STAGING/USAGE-zh-CN.txt"
fi

rm -f "$DMG_PATH"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG_PATH"
rm -rf "$STAGING"

SIZE_MB="$(du -m "$DMG_PATH" | awk '{print $1}')"
echo ""
echo "Done."
echo "  App: $APP_BUNDLE"
echo "  DMG: $DMG_PATH (${SIZE_MB} MB)"
echo ""
echo "Note: 公开发布前需 codesign + notarize，否则 Gatekeeper 可能拦截。"
