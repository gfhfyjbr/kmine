#!/usr/bin/env bash
# Cargo runner for macOS. Stamps kmine.app so Dock, Finder, and Cmd-Tab
# pick up AppIcon from the bundle instead of a runtime setApplicationIconImage.
#
# Tahoe reads the compiled asset catalog (Assets.car) produced from
# AppIcon.icon. The .icns is a square-raster fallback for older macOS —
# never a pre-rounded PNG, or the squircle rim aliases when scaled.
set -euo pipefail

bin="${1:?}"
shift

if [[ "$(basename "$bin")" != "kmine" ]]; then
  exec "$bin" "$@"
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/.." && pwd)"
profile_dir="$(cd "$(dirname "$bin")" && pwd)"
app="$profile_dir/kmine.app"
macos="$app/Contents/MacOS"
resources="$app/Contents/Resources"
plist="$root/assets/icon/Info.plist"
icns="$root/assets/icon/AppIcon.icns"
icon="$root/assets/icon/AppIcon.icon"

if [[ ! -f "$plist" || ! -f "$icns" ]]; then
  echo "macos-run: missing $plist or $icns" >&2
  exit 1
fi

mkdir -p "$macos" "$resources"
cp "$plist" "$app/Contents/Info.plist"
cp "$icns" "$resources/AppIcon.icns"
if [[ -d "$icon" ]]; then
  rm -rf "$resources/AppIcon.icon"
  cp -R "$icon" "$resources/AppIcon.icon"
fi

actool="$(xcrun --find actool 2>/dev/null || true)"
if [[ -n "$actool" && -d "$icon" ]]; then
  car_tmp="$(mktemp -d)"
  if "$actool" "$icon" \
      --app-icon AppIcon \
      --compile "$car_tmp" \
      --output-partial-info-plist "$car_tmp/partial.plist" \
      --minimum-deployment-target 13.0 \
      --platform macosx \
      --target-device mac \
      --include-all-app-icons \
      >/dev/null 2>&1; then
    if [[ -f "$car_tmp/Assets.car" ]]; then
      cp "$car_tmp/Assets.car" "$resources/Assets.car"
    fi
  fi
  rm -rf "$car_tmp"
fi

printf 'APPL????' > "$app/Contents/PkgInfo"
cp "$bin" "$macos/kmine"
chmod +x "$macos/kmine"
touch "$app"
lsregister="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [[ -x "$lsregister" ]]; then
  "$lsregister" -f "$app" >/dev/null 2>&1 || true
fi

exec "$macos/kmine" "$@"
