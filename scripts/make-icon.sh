#!/usr/bin/env bash
# Renders assets/icon.svg into assets/AppIcon.icns with Swift and tools that ship with macOS.
# Run it again after changing the logo, then commit both files.
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
echo "Wrote $root/assets/AppIcon.icns"
