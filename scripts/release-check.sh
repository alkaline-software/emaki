#!/bin/zsh
# Everything a tag will be held to, checked here before it is pushed: the
# version in Cargo.toml and Cargo.lock, the changelog section, a clean
# tree, the tests, the build, and a Windows type check. Each of these has
# failed a release after its tag was out (v0.1.5: a line that did not
# compile on Windows), and a tag that fails is a tag redone.
#
#   scripts/release-check.sh 0.1.6
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
v="${1:?version, e.g. 0.1.6}"
fail() { print -u2 "release-check: $1"; exit 1; }

grep -q "^version = \"$v\"$" Cargo.toml || fail "Cargo.toml does not say version = \"$v\""
scripts/release-notes.sh "$v" > /dev/null || fail "CHANGELOG.md has no section for v$v"
lines=$(scripts/release-notes.sh "$v" | grep -c '^- ' || true)
(( lines <= 12 )) || fail "the v$v changelog section has $lines lines; a dozen at most"
# --locked is how CI builds: a lock that was not refreshed fails here.
cargo check --locked -p emaki-app -p emaki-core 2>&1 | tail -1
[[ -z "$(git status --porcelain)" ]] || fail "uncommitted changes; the tag would not hold them"
git rev-parse -q --verify "refs/tags/v$v" > /dev/null && fail "the tag v$v exists already (WORKFLOW.md, redoing a release)"

cargo test --locked -p emaki-core 2>&1 | grep -E "^test result|FAILED|panicked" || true
cargo test --locked -p emaki-core > /dev/null 2>&1 || fail "the core tests fail"
cargo build --locked -p emaki-app 2>&1 | tail -1

# The Windows job is the one that cannot be run here; the MinGW type check
# catches what breaks it most, a line gated for the wrong platform.
if rustup target list --installed | grep -q x86_64-pc-windows-gnu && command -v x86_64-w64-mingw32-gcc > /dev/null; then
  CARGO_TARGET_DIR=target/xwingnu CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
    cargo check --locked -p emaki-app -p emaki-core --tests --target x86_64-pc-windows-gnu 2>&1 | tail -1 \
    || fail "the Windows type check fails"
else
  fail "no Windows type check: brew install mingw-w64 && rustup target add x86_64-pc-windows-gnu"
fi
print "release-check: v$v is ready to tag at $(git rev-parse --short HEAD)"
