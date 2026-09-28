#!/bin/zsh
# Regenerate the app icon from scripts/icon/draw.js into
# crates/scribe-app/assets/icon: every PNG size, icon.ico for Windows and,
# on macOS, icon.icns through iconutil. Needs node.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/bin:$PATH"
OUT=crates/scribe-app/assets/icon
(cd scripts/icon && test -d node_modules/@napi-rs/canvas || npm install --silent --no-audit --no-fund)
node scripts/icon/draw.js "$OUT"
if command -v iconutil >/dev/null; then
  iconutil -c icns "$OUT/icon.iconset" -o "$OUT/icon.icns"
fi
rm -rf "$OUT/icon.iconset"
ls -la "$OUT"
