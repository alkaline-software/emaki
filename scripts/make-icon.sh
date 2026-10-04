#!/bin/zsh
# Regenerate the app icon from scripts/icon/icon.html (the drawing, on a
# canvas) into crates/emaki-app/assets/icon: the PNGs as the rounded app
# icon with its margin and shadow, icon.ico for Windows and, on macOS,
# icon.icns as the full square through iconutil. The page is run by WebKit
# (scripts/icon/render.swift), which draws every size on its own pixels, so
# this runs on a Mac; nothing else is needed.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=scripts/icon/icon.html
OUT=crates/emaki-app/assets/icon
TMP=$(mktemp -d)
SET="$TMP/icon.iconset"; mkdir -p "$SET"
jobs=()
for s in 1024 512 256 128 32; do jobs+=("$OUT/icon-$s.png:$s:margin"); done
for s in 256 128 64 48 32 16; do jobs+=("$TMP/ico-$s.png:$s:margin"); done
for s in 16 32 128 256 512; do
  jobs+=("$SET/icon_${s}x${s}.png:$s:bleed" "$SET/icon_${s}x${s}@2x.png:$((s * 2)):bleed")
done
swift scripts/icon/render.swift "$SRC" "${jobs[@]}"
python3 scripts/icon/ico.py "$OUT/icon.ico" "$TMP"/ico-{256,128,64,48,32,16}.png
if command -v iconutil >/dev/null; then
  iconutil -c icns "$SET" -o "$OUT/icon.icns"
fi
rm -rf "$TMP"
ls -la "$OUT"
