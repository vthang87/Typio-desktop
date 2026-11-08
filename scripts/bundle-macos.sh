#!/usr/bin/env bash
# Builds target/release/Typio.app and installs a copy in ~/Applications. macOS does not keep
# Accessibility access for apps run from external drives, so always run the installed copy.
# macOS also ties Accessibility access to the app's signature:
# set TYPIO_SIGN_IDENTITY to a code signing identity to keep access across rebuilds.
# Without one the app is signed ad hoc and access must be granted again after each build.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
app="$root/target/release/Typio.app"

cargo build --release -p typio-app --manifest-path "$root/Cargo.toml"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/release/typio-app" "$app/Contents/MacOS/Typio"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>io.github.vthang87.typio</string>
  <key>CFBundleName</key><string>Typio</string>
  <key>CFBundleDisplayName</key><string>Typio</string>
  <key>CFBundleExecutable</key><string>Typio</string>
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

installed="$HOME/Applications/Typio.app"
pkill -f "$installed/Contents/MacOS/Typio" 2>/dev/null || true
mkdir -p "$HOME/Applications"
rm -rf "$installed"
ditto "$app" "$installed"
echo "Built $app and installed $installed"
