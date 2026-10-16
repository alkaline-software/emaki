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
# own, target/debug/dev.noindex/Emaki.app, and is opened through the
# system with an environment that holds only what a Dock launch has.
#
# The folder's name ends in ".noindex", which Spotlight passes by: in
# target/debug itself the bundle was indexed, and Launchpad and Spotlight
# listed a second Emaki beside the installed one.
#
# The bundle is signed with a signing identity of the developer's when the
# keychain has one (EMAKI_SIGN_IDENTITY, else the first "Developer ID
# Application"). macOS keeps what an app was allowed (the Documents
# folder, the Desktop) by who signed it; a build signed by nobody is a new
# app each time it is built, and the system asks again at every launch
# after a build. With no identity it is signed by nobody and does ask.
# The executable is a copy, since signing writes into the file.
#
# Elsewhere the build is simply run.

cd "$(dirname "$0")/.." || exit 1
bin=target/debug/Emaki
[ -x "$bin" ] || { echo "no $bin: cargo build -p emaki-app" >&2; exit 1; }
[ "$(uname)" = Darwin ] || exec "./$bin"

app=target/debug/dev.noindex/Emaki.app
# The bundle as it was before it had a folder of its own.
rm -rf target/debug/Emaki.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
rm -f "$app/Contents/MacOS/Emaki"
cp -c "$bin" "$app/Contents/MacOS/Emaki" 2>/dev/null || cp "$bin" "$app/Contents/MacOS/Emaki"
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

identity="${EMAKI_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
codesign --force --sign "${identity:--}" "$app" >/dev/null 2>&1 || codesign --force --sign - "$app" >/dev/null 2>&1

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
