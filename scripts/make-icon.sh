#!/usr/bin/env bash
# Renders assets/icon.svg into the app icons with Swift and tools that ship with macOS:
# AppIcon.icns (macOS), AppIcon.ico (Windows) and tray.png / tray-off.png (Windows tray).
# Run it again after changing the logo, then commit the files.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# AppKit draws the SVG onto a transparent canvas (Quick Look would add a white background).
master="$work/icon.png"
swift "$root/scripts/render-svg.swift" "$root/assets/icon.svg" "$master" 1024
[ -f "$master" ] || { echo "Could not render $root/assets/icon.svg" >&2; exit 1; }

set_dir="$work/AppIcon.iconset"
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$master" --out "$set_dir/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$master" --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$root/assets/AppIcon.icns"

ico_sizes=()
for size in 16 24 32 48 64 256; do
  sips -z "$size" "$size" "$master" --out "$work/ico_$size.png" >/dev/null
  ico_sizes+=("$work/ico_$size.png")
done
python3 "$root/scripts/make-ico.py" "$root/assets/AppIcon.ico" "${ico_sizes[@]}"

swift "$root/scripts/render-svg.swift" "$root/assets/icon.svg" "$root/assets/tray.png" 32
swift "$root/scripts/render-svg.swift" "$root/assets/icon.svg" "$root/assets/tray-off.png" 32 gray
echo "Wrote AppIcon.icns, AppIcon.ico, tray.png and tray-off.png in $root/assets"
