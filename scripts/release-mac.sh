#!/bin/zsh
# The macOS release build, the way Pingfan's other apps ship: a Developer ID
# signature with the hardened runtime, a disk image with an Applications
# link, and (with --notarize) notarization and stapling. One script for a
# laptop, where the notarytool keychain profile does the talking, and for
# CI, where the identity comes from secrets (see .github/workflows/release.yml).
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
  cargo build --release --locked -p scribe-app "${TARGET_FLAG[@]}"
fi
rm -rf dist/Emaki.app
cargo packager --release -p scribe-app "${TARGET_FLAG[@]}" --formats app
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

# The disk image: the app and an Applications link, compressed.
rm -rf dist/dmg-temp "$OUT"
mkdir -p dist/dmg-temp "$(dirname "$OUT")"
cp -R "$APP" dist/dmg-temp/
ln -s /Applications dist/dmg-temp/Applications
hdiutil create -volname "Emaki $VERSION" -srcfolder dist/dmg-temp -ov -format UDZO -imagekey zlib-level=9 "$OUT"
rm -rf dist/dmg-temp
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
