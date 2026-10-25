#!/usr/bin/env bash
# The Snap Store's package, from a Linux build: the same binary as the .deb, with snapd's own metadata (crates/emaki-app/linux/snap/), packed by `snap pack`, which needs no build container. Classic confinement, as an editor's or a terminal's: Emaki runs the person's shell and agents, which a sandbox would confine along with it.
#
#   scripts/linux-snap.sh <binary> <version> <amd64|arm64> <out.snap>
set -euo pipefail
bin=$1 version=$2 arch=$3 out=$4
here=$(cd "$(dirname "$0")/.." && pwd)
meta="$here/crates/emaki-app/linux/snap"
# snapd refuses a snap whose files are not world-readable, mktemp's folder among them.
umask 022
tree=$(mktemp -d)
chmod 755 "$tree"
trap 'rm -rf "$tree"' EXIT
mkdir -p "$tree/bin" "$tree/meta/gui"
install -m 755 "$bin" "$tree/bin/Emaki"
sed -e "s/{{VERSION}}/$version/" -e "s/{{ARCH}}/$arch/" "$meta/snap.yaml" > "$tree/meta/snap.yaml"
cp "$meta/emaki.desktop" "$tree/meta/gui/emaki.desktop"
cp "$here/crates/emaki-app/assets/icon/icon-512.png" "$tree/meta/gui/icon.png"
mkdir -p "$(dirname "$out")"
snap pack "$tree" "$(dirname "$out")" --filename="$(basename "$out")"
