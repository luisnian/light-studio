#!/usr/bin/env bash
# Builds "Light Studio.app" and a .dmg from it. Run on a Mac, from the repository root:
#   ./scripts/make-dmg.sh            # for this Mac's chip
#   ./scripts/make-dmg.sh universal  # Apple Silicon + Intel in one app
set -euo pipefail

cd "$(dirname "$0")/.."
NAME="Light Studio"
BIN="light-studio"
ID="com.luisnieto.lightstudio"
VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
OUT="dist"
APP="$OUT/$NAME.app"
DMG="$OUT/Light-Studio-$VERSION.dmg"

rm -rf "$OUT" && mkdir -p "$OUT"

if [[ "${1:-}" == "universal" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release -p light-studio --target aarch64-apple-darwin
  cargo build --release -p light-studio --target x86_64-apple-darwin
  mkdir -p "$OUT/bin"
  lipo -create -output "$OUT/bin/$BIN" \
    "target/aarch64-apple-darwin/release/$BIN" "target/x86_64-apple-darwin/release/$BIN"
  EXE="$OUT/bin/$BIN"
else
  cargo build --release -p light-studio
  EXE="target/release/$BIN"
fi

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$EXE" "$APP/Contents/MacOS/$BIN"

# icon: .icns from the PNGs in assets/
ICONSET="$OUT/AppIcon.iconset"
mkdir -p "$ICONSET"
cp assets/icon-32.png   "$ICONSET/icon_16x16@2x.png"
cp assets/icon-32.png   "$ICONSET/icon_32x32.png"
cp assets/icon-64.png   "$ICONSET/icon_32x32@2x.png"
cp assets/icon-128.png  "$ICONSET/icon_128x128.png"
cp assets/icon-256.png  "$ICONSET/icon_128x128@2x.png"
cp assets/icon-256.png  "$ICONSET/icon_256x256.png"
cp assets/icon-512.png  "$ICONSET/icon_256x256@2x.png"
cp assets/icon-512.png  "$ICONSET/icon_512x512.png"
cp assets/icon-1024.png "$ICONSET/icon_512x512@2x.png"
sips -z 16 16 assets/icon-32.png --out "$ICONSET/icon_16x16.png" >/dev/null
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleDevelopmentRegion</key><string>es</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>Light Studio añade tus planes al Calendario.</string>
</dict>
</plist>
PLIST

# ad-hoc signature (no Apple developer account): lets the app run once opened with right-click > Open
codesign --force --deep --sign - "$APP"

# .dmg with the app and a shortcut to /Applications
STAGE="$OUT/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "$NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE" "$OUT/bin"

echo
echo "Listo: $DMG"
