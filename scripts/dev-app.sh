#!/bin/sh
# Start the build in target/debug as the system starts an app, not as a
# shell starts a program.
#
#   scripts/dev-app.sh [arguments for `open`]
#
# On a Mac a bare executable is nobody's app. Opened from the Dock or the
# Finder it is run inside the default terminal app, in a window of that
# app, and it and everything it starts (every Claude Code session) carry
# that terminal's marks from then on: its bundle identifier, its ssh
# agent's socket. Started from a session's shell it inherits the same from
# whatever started the app before it. So the build goes in a bundle of its
# own, target/debug/Emaki.app, and is opened through the system with an
# environment that holds only what a Dock launch has. The bundle's
# executable is a hard link to the build, made again each time, since a
# build replaces the file.
#
# Elsewhere the build is simply run.

cd "$(dirname "$0")/.." || exit 1
bin=target/debug/Emaki
[ -x "$bin" ] || { echo "no $bin: cargo build -p emaki-app" >&2; exit 1; }
[ "$(uname)" = Darwin ] || exec "./$bin"

app=target/debug/Emaki.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
ln -f "$bin" "$app/Contents/MacOS/Emaki" 2>/dev/null || cp -f "$bin" "$app/Contents/MacOS/Emaki"
cp -f crates/emaki-app/assets/icon/icon.icns "$app/Contents/Resources/Emaki.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Emaki</string>
  <key>CFBundleDisplayName</key><string>Emaki</string>
  <key>CFBundleIdentifier</key><string>com.pingfanhu.emaki.dev</string>
  <key>CFBundleVersion</key><string>0</string>
  <key>CFBundleShortVersionString</key><string>dev</string>
  <key>CFBundleExecutable</key><string>Emaki</string>
  <key>CFBundleIconFile</key><string>Emaki</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# `open` hands the app the opener's environment, so the opener's is made
# the one the Dock would give: no shell's PATH, no terminal's marks, the
# system's own ssh agent. EMAKI_* go through, for a probe copy.
set -- "$@" "$app"
exec env -i \
    HOME="$HOME" USER="$USER" LOGNAME="${LOGNAME:-$USER}" SHELL="${SHELL:-/bin/zsh}" \
    PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    TMPDIR="$(getconf DARWIN_USER_TEMP_DIR)" \
    SSH_AUTH_SOCK="$(launchctl getenv SSH_AUTH_SOCK)" \
    $(env | grep -E '^(EMAKI_[A-Z_]+|CLAUDE_CONFIG_DIR|CODEX_HOME)=' | grep -v ' ' ) \
    /usr/bin/open -n "$@"
