#!/bin/zsh
# Build Emaki.app: release binary + scribe-core CLI beside it, Info.plist,
# ad-hoc signature. Usage: scripts/make-app.sh [--run]
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

APP=dist/Emaki.app
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

cargo build --release -p scribe-app -p scribe-core --bin Emaki --bin scribe-core

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/Emaki "$APP/Contents/MacOS/Emaki"
cp target/release/scribe-core "$APP/Contents/MacOS/scribe-core"
cp crates/scribe-app/assets/icon/icon.icns "$APP/Contents/Resources/Emaki.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Emaki</string>
  <key>CFBundleDisplayName</key><string>Emaki</string>
  <key>CFBundleIdentifier</key><string>com.pingfanhu.scribe</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>Emaki</string>
  <key>CFBundleIconFile</key><string>Emaki</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT</string>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$APP"
echo "built $APP ($VERSION)"
if [[ "${1:-}" == "--run" ]]; then
  open "$APP"
fi
