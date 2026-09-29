#!/bin/sh
# Print one release's section of CHANGELOG.md, without its heading: the
# body of the GitHub Release. Fails when the version has no section, so a
# tag cannot ship with empty notes.
#
#   scripts/release-notes.sh 0.1.0
set -eu
cd "$(dirname "$0")/.."
v="${1:?version, e.g. 0.1.0}"
notes=$(awk -v h="## v$v" '
  $0 == h { on = 1; next }
  on && /^## / { exit }
  on { print }
' CHANGELOG.md)
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "CHANGELOG.md has no section '## v$v'" >&2
  exit 1
fi
printf '%s\n' "$notes" | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'
