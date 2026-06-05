#!/usr/bin/env bash
# 在 macOS 上构建 Codex Helper.app 与 DMG（需在 Apple Silicon 或 Intel Mac 上运行）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
VERSION="$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
APP_NAME="Codex Helper"
APP_BUNDLE="$DIST/${APP_NAME}.app"
DMG_PATH="$DIST/CodexHelper-${VERSION}-macos.dmg"
UNIVERSAL_BINARY="$ROOT/target/universal/codex-helper"
ICON_PNG="$ROOT/assets/codex-helper.png"
INFO_PLIST="$ROOT/installer/macos/Info.plist"
README="$ROOT/installer/USAGE-zh-CN.txt"

echo "Codex Helper v${VERSION} (macOS)"

cd "$ROOT"
echo "Building universal release (arm64 + x86_64)..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p "$ROOT/target/universal"
lipo -create \
    "$ROOT/target/aarch64-apple-darwin/release/codex-helper" \
    "$ROOT/target/x86_64-apple-darwin/release/codex-helper" \
    -output "$ROOT/target/universal/codex-helper"
BINARY="$UNIVERSAL_BINARY"
chmod +x "$BINARY"
echo "  OK  $(lipo -info "$BINARY")"

if [[ ! -f "$BINARY" ]]; then
    echo "Missing binary: $BINARY" >&2
    exit 1
fi

if [[ ! -f "$ICON_PNG" ]]; then
    echo "Missing icon PNG: $ICON_PNG" >&2
    echo "Expected build.rs to generate it from icon_render.rs (same as Windows .exe)." >&2
    exit 1
fi

install_app_icon() {
    local app_bundle="$1"
    local iconset="$DIST/AppIcon.iconset"
    local icns_path="$app_bundle/Contents/Resources/AppIcon.icns"

    rm -rf "$iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$ICON_PNG" \
            --out "$iconset/icon_${size}x${size}.png" >/dev/null
        local double=$((size * 2))
        sips -z "$double" "$double" "$ICON_PNG" \
            --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$icns_path"
    rm -rf "$iconset"
    echo "  OK  AppIcon.icns (from icon_render.rs / same as Windows exe)"
}

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

cp "$BINARY" "$APP_BUNDLE/Contents/MacOS/codex-helper"
chmod +x "$APP_BUNDLE/Contents/MacOS/codex-helper"

# 同步版本号到 Info.plist
cp "$INFO_PLIST" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${VERSION}" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${VERSION}" "$APP_BUNDLE/Contents/Info.plist"

install_app_icon "$APP_BUNDLE"

if [[ -f "$README" ]]; then
    cp "$README" "$APP_BUNDLE/Contents/Resources/USAGE-zh-CN.txt"
fi

# ad-hoc 签名：减轻「已损坏」误报（仍非公证，首次可能需右键打开）
if codesign --force --deep --sign - "$APP_BUNDLE" 2>/dev/null; then
    echo "  OK  ad-hoc codesign"
else
    echo "  WARN  codesign skipped (non-fatal)"
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

# DMG 卷标使用与 App 相同的图标
cp "$APP_BUNDLE/Contents/Resources/AppIcon.icns" "$STAGING/.VolumeIcon.icns"
if command -v SetFile >/dev/null 2>&1; then
    SetFile -a C "$STAGING"
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
