#!/bin/zsh
# Regenerate the app icon from scripts/icon/draw.py (the drawing, as SVG)
# into crates/emaki-app/assets/icon: the PNGs on Apple's 824-of-1024 grid,
# icon.ico for Windows and, on macOS, icon.icns at full bleed through
# iconutil. The SVG is rasterised by Cocoa (scripts/icon/render.swift), so
# this runs on a Mac; nothing else is needed.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=crates/emaki-app/assets/icon
TMP=$(mktemp -d)
python3 scripts/icon/draw.py "$TMP"
for s in 1024 512 256 128 32; do
  swift scripts/icon/render.swift "$TMP/icon-margin.svg" "$OUT/icon-$s.png" $s
done
for s in 256 128 64 48 32 16; do
  swift scripts/icon/render.swift "$TMP/icon-margin.svg" "$TMP/ico-$s.png" $s
done
python3 scripts/icon/ico.py "$OUT/icon.ico" "$TMP"/ico-{256,128,64,48,32,16}.png
if command -v iconutil >/dev/null; then
  SET="$TMP/icon.iconset"; mkdir -p "$SET"
  for s in 16 32 128 256 512; do
    swift scripts/icon/render.swift "$TMP/icon-bleed.svg" "$SET/icon_${s}x${s}.png" $s
    swift scripts/icon/render.swift "$TMP/icon-bleed.svg" "$SET/icon_${s}x${s}@2x.png" $((s * 2))
  done
  iconutil -c icns "$SET" -o "$OUT/icon.icns"
fi
rm -rf "$TMP"
ls -la "$OUT"
