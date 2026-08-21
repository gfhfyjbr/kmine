#!/usr/bin/env bash
# Rebuild AppIcon.icns and AppIcon-square.png from AppIcon.icon.
# The .icon is the source of truth. icns is a square raster fallback:
# no baked squircle, so Dock/Finder mask at the target size instead of
# scaling a pre-rounded 1024px bezel (that is what made the rim shimmer).
set -euo pipefail

root="$(cd "$(dirname "$BASH_SOURCE[0]")/.." && pwd)"
src="$root/assets/icon"
icon="$src/AppIcon.icon"
fg="$icon/Assets/foreground.png"

if [[ ! -f "$fg" ]]; then
  echo "macos-icon: missing $fg" >&2
  exit 1
fi
if ! command -v magick >/dev/null; then
  echo "macos-icon: need ImageMagick (magick)" >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
square="$tmp/square.png"
iconset="$tmp/AppIcon.iconset"
mkdir -p "$iconset"

magick -size 1024x1024 -define gradient:angle=25 \
  gradient:'#121110-#1b1a18' \
  "$fg" -gravity center -composite \
  "$square"

copy_size() {
  magick "$square" -filter Lanczos -resize "$1x$1" "$iconset/$2"
}

copy_size 16   icon_16x16.png
copy_size 32   icon_16x16@2x.png
copy_size 32   icon_32x32.png
copy_size 64   icon_32x32@2x.png
copy_size 128  icon_128x128.png
copy_size 256  icon_128x128@2x.png
copy_size 256  icon_256x256.png
copy_size 512  icon_256x256@2x.png
copy_size 512  icon_512x512.png
copy_size 1024 icon_512x512@2x.png

iconutil -c icns -o "$src/AppIcon.icns" "$iconset"
cp "$square" "$src/AppIcon-square.png"
echo "wrote $src/AppIcon.icns"
echo "wrote $src/AppIcon-square.png"
