#!/bin/zsh
# The last step of a release: put the notarized Mac images built here over
# the ad-hoc ones CI attached, check them from the outside, and publish
# the draft. Refuses until the release workflow for the tag has succeeded
# (an upload made while it runs is overwritten by it) and unless both
# images in dist/ are notarized and were built from the tag's commit.
#
#   scripts/release-publish.sh 0.1.6
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/bin:$PATH"
v="${1:?version, e.g. 0.1.6}"
repo="alkaline-software/emaki"
fail() { print -u2 "release-publish: $1"; exit 1; }

state=$(gh run list --repo $repo --workflow release --branch "v$v" --limit 1 --json status,conclusion,headSha -q '.[0] | "\(.status) \(.conclusion) \(.headSha)"')
[[ "$state" == completed\ success\ * ]] || fail "the release workflow for v$v is not done and green: ${state:-no run}"
sha=${state##* }
[[ "$sha" == "$(git rev-parse "v$v^{commit}")" ]] || fail "the local tag v$v is not the commit CI built ($sha)"

for a in arm64 x64; do
  img="dist/Emaki-mac-$a.dmg"
  [[ -f $img ]] || fail "$img is missing (WORKFLOW.md step 4)"
  spctl -a -t open --context context:primary-signature -v $img 2>&1 | grep -q "Notarized Developer ID" || fail "$img is not notarized"
  xcrun stapler validate $img > /dev/null || fail "$img has no stapled ticket"
  # Built before the tag's commit was made: an image of other code.
  (( $(stat -f %m $img) >= $(git log -1 --format=%ct "v$v") )) || fail "$img is older than the commit v$v points at; rebuild it"
done

gh release upload "v$v" dist/Emaki-mac-arm64.dmg dist/Emaki-mac-x64.dmg --clobber --repo $repo
check=$(mktemp -d)
gh release download "v$v" --pattern 'Emaki-mac-*.dmg' --dir $check --repo $repo
for a in arm64 x64; do
  [[ "$(shasum -a 256 < $check/Emaki-mac-$a.dmg)" == "$(shasum -a 256 < dist/Emaki-mac-$a.dmg)" ]] || fail "the published Emaki-mac-$a.dmg is not the one in dist/"
  spctl -a -t open --context context:primary-signature -v $check/Emaki-mac-$a.dmg 2>&1 | grep -q "Notarized Developer ID" || fail "the published Emaki-mac-$a.dmg is not notarized"
done
rm -rf $check
gh release edit "v$v" --draft=false --latest --repo $repo > /dev/null
print "release-publish: v$v is published with notarized Mac images"
gh release view "v$v" --repo $repo --json url,isDraft -q '"\(.url) draft=\(.isDraft)"'
