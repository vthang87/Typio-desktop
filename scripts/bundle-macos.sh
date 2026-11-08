#!/usr/bin/env bash
# Builds target/release/Typio.app and installs a copy in ~/Applications. macOS does not keep
# Accessibility access for apps run from external drives, so always run the installed copy.
# macOS also ties Accessibility access to the app's signature:
# set TYPIO_SIGN_IDENTITY to a code signing identity to keep access across rebuilds.
# Without one the app is signed ad hoc and access must be granted again after each build.
#
# TYPIO_UNIVERSAL=1 builds one binary for Apple silicon and Intel Macs.
# TYPIO_NO_INSTALL=1 only builds the app, as CI does.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
app="$root/target/release/Typio.app"

binary="$root/target/release/typio-app"
if [ "${TYPIO_UNIVERSAL:-}" = 1 ]; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p typio-app --target "$target" --manifest-path "$root/Cargo.toml"
  done
  binary="$root/target/release/typio-app-universal"
  lipo -create -output "$binary" \
    "$root/target/aarch64-apple-darwin/release/typio-app" \
    "$root/target/x86_64-apple-darwin/release/typio-app"
else
  cargo build --release -p typio-app --manifest-path "$root/Cargo.toml"
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Typio"
cp "$root/assets/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>io.github.vthang87.typio</string>
  <key>CFBundleName</key><string>Typio</string>
  <key>CFBundleDisplayName</key><string>Typio</string>
  <key>CFBundleExecutable</key><string>Typio</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --sign "${TYPIO_SIGN_IDENTITY:--}" "$app"

if [ "${TYPIO_NO_INSTALL:-}" = 1 ]; then
  echo "Built $app"
  exit 0
fi

installed="$HOME/Applications/Typio.app"
pkill -f "$installed/Contents/MacOS/Typio" 2>/dev/null || true
mkdir -p "$HOME/Applications"
rm -rf "$installed"
ditto "$app" "$installed"
echo "Built $app and installed $installed"
