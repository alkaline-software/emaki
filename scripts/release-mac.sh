#!/bin/zsh
# The macOS release build, the way Pingfan's other apps ship: a Developer ID
# signature with the hardened runtime, a disk image laid out by Finder (the
# app, an arrow, the Applications link, over scripts/dmg/background.png),
# and (with --notarize) notarization and stapling. One script for a laptop,
# where the notarytool keychain profile does the talking, and for CI, where
# the identity comes from secrets (see .github/workflows/release.yml).
#
#   scripts/release-mac.sh [--target <triple>] [--no-build] [--identity <name>|-]
#                          [--notarize] [--out <path.dmg>]
#
#   --identity -   ad-hoc signature (a fork or a machine without the certificate)
#   --notarize     xcrun notarytool with APPLE_ID/APPLE_APP_PASSWORD/APPLE_TEAM_ID
#                  when set, else the keychain profile $NOTARY_PROFILE
#                  (default notarytool-profile); then staple and validate
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

IDENTITY="${EMAKI_SIGN_IDENTITY:-Developer ID Application: Pingfan Hu (XC2WL5WN7J)}"
NOTARY_PROFILE="${NOTARY_PROFILE:-notarytool-profile}"
TARGET=""; BUILD=1; NOTARIZE=0; OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --no-build) BUILD=0; shift ;;
    --identity) IDENTITY="$2"; shift 2 ;;
    --notarize) NOTARIZE=1; shift ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done

case "${TARGET:-$(uname -m)}" in
  aarch64*|arm64) ARCH=arm64 ;;
  x86_64*) ARCH=x64 ;;
  *) ARCH="$(uname -m)" ;;
esac
OUT="${OUT:-dist/Emaki-mac-$ARCH.dmg}"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TARGET_FLAG=(); [[ -n "$TARGET" ]] && TARGET_FLAG=(--target "$TARGET")

if (( BUILD )); then
  cargo build --release --locked -p emaki-app "${TARGET_FLAG[@]}"
fi
rm -rf dist/Emaki.app
cargo packager --release -p emaki-app "${TARGET_FLAG[@]}" --formats app
APP=dist/Emaki.app
[[ -d "$APP" ]] || { echo "packager left no $APP" >&2; exit 1; }

# Sign the bundle. The hardened runtime (--options runtime) and a secure
# timestamp are what notarization requires; --deep reaches the CLI beside
# the app binary too.
if [[ "$IDENTITY" == "-" ]]; then
  codesign --force --deep --sign - "$APP"
else
  codesign --force --deep --sign "$IDENTITY" --options runtime --timestamp "$APP"
fi
codesign --verify --deep --strict --verbose=2 "$APP"

# The disk image. Finder lays the window out on a read-write image (the
# layout lives in the volume's .DS_Store, and only Finder writes one Finder
# will honour), then hdiutil compresses it read-only. The window is 660x420
# points over scripts/dmg/background.png, which is that size at 2x.
VOLNAME="Emaki v$VERSION"
STAGE=dist/dmg-staging
RW=dist/Emaki-rw.dmg
detach_stale() {
  local dev
  for dev in $(hdiutil info | grep -F "/Volumes/$VOLNAME" | awk '{print $1}'); do
    hdiutil detach "$dev" -force >/dev/null 2>&1 || true
  done
}
detach_stale
rm -rf "$STAGE" "$RW" "$OUT"
mkdir -p "$STAGE/.background" "$(dirname "$OUT")"
cp -R "$APP" "$STAGE/Emaki.app"
ln -s /Applications "$STAGE/Applications"
cp scripts/dmg/background.png "$STAGE/.background/background.png"
mkdir -p "$STAGE/.fseventsd" && touch "$STAGE/.fseventsd/no_log"
# hdiutil fails now and then with "Resource busy" on a machine that has
# just built (a GitHub runner most of all), so it is tried a few times, and
# without -quiet, which hid why v0.1.9's Intel job stopped here.
made=""
for attempt in 1 2 3 4 5; do
  if hdiutil create -volname "$VOLNAME" -srcfolder "$STAGE" -ov -format UDRW "$RW"; then
    made=1
    break
  fi
  echo "release-mac: hdiutil create failed (attempt $attempt), trying again" >&2
  sleep $((attempt * 3))
done
[ -n "$made" ] || { echo "release-mac: could not create the disk image" >&2; exit 1; }
ATTACHED=$(hdiutil attach -readwrite -noverify -noautoopen "$RW")
DEVICE=$(echo "$ATTACHED" | awk '/^\/dev\// {print $1; exit}')
osascript >/dev/null <<APPLESCRIPT
tell application "Finder"
  tell disk "$VOLNAME"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {100, 100, 760, 520}
    set opts to the icon view options of container window
    set arrangement of opts to not arranged
    set icon size of opts to 128
    set text size of opts to 14
    set background picture of opts to file ".background:background.png"
    set position of item "Emaki.app" of container window to {165, 260}
    set position of item "Applications" of container window to {495, 260}
    -- Housekeeping items sit far outside the window for anyone showing hidden files.
    repeat with n in {".background", ".fseventsd", ".Trashes", ".DS_Store"}
      try
        set position of item (n as text) of container window to {1400, 700}
      end try
    end repeat
    close
    open
    update without registering applications
    delay 2
    close
  end tell
end tell
APPLESCRIPT
sync
hdiutil detach "$DEVICE" -force >/dev/null
hdiutil convert -quiet "$RW" -format UDZO -imagekey zlib-level=9 -ov -o "$OUT"
# The bundle lives on in the image only. Left in dist/ it is a second
# Emaki for Launchpad and Spotlight beside the installed one.
rm -rf "$STAGE" "$RW" "$APP" dist/.cargo-packager
if [[ "$IDENTITY" != "-" ]]; then
  codesign --force --sign "$IDENTITY" "$OUT"
fi

if (( NOTARIZE )); then
  if [[ "$IDENTITY" == "-" ]]; then
    echo "an ad-hoc signature cannot be notarized" >&2; exit 1
  fi
  if [[ -n "${APPLE_ID:-}" ]]; then
    xcrun notarytool submit "$OUT" --apple-id "$APPLE_ID" --password "$APPLE_APP_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
  else
    xcrun notarytool submit "$OUT" --keychain-profile "$NOTARY_PROFILE" --wait
  fi
  xcrun stapler staple "$OUT"
  xcrun stapler validate "$OUT"
fi

HOW=$([[ "$IDENTITY" == "-" ]] && echo "ad-hoc" || echo "signed")
(( NOTARIZE )) && HOW="$HOW, notarized"
echo "built $OUT ($VERSION, $ARCH, $HOW)"
