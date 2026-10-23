#!/bin/sh
# Renders assets/brand/icon.svg into the platform icons:
#   assets/macos/AppIcon.icns   the app bundle's icon
#   assets/linux/brook.png      512px, for the desktop entry
# Needs resvg (cargo install resvg) and, for the .icns, macOS's iconutil.
set -eu
cd "$(dirname "$0")/.."

ICONSET=$(mktemp -d)/AppIcon.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  resvg -w "$size" -h "$size" assets/brand/icon.svg "$ICONSET/icon_${size}x${size}.png"
  double=$((size * 2))
  resvg -w "$double" -h "$double" assets/brand/icon.svg "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o assets/macos/AppIcon.icns
resvg -w 512 -h 512 assets/brand/icon.svg assets/linux/brook.png
echo "rendered assets/macos/AppIcon.icns and assets/linux/brook.png"
