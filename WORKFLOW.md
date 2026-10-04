# Releasing Emaki

Everything about cutting a release: bumping the version, the changelog,
the tag, what CI builds, signing and notarizing the Mac images on this
machine, uploading them, redoing a release, and the pieces that feed it
(the icon, the disk-image background). AGENTS.md says how the pieces fit;
this file says what to run, in order.

## What a release is

A `v*` tag pushed to `alkaline-software/emaki` runs `release.yml`. Four
package jobs build the app on macOS (both architectures, from the Apple
Silicon runner), Windows and Linux, then one `release` job creates the
GitHub Release, as a draft, with the tag's section of `CHANGELOG.md` as
its notes and attaches the installers under stable names:

| asset | built by | signed |
|---|---|---|
| `Emaki-mac-arm64.dmg` | CI, then replaced from this machine | Developer ID, notarized, stapled |
| `Emaki-mac-x64.dmg` | CI, then replaced from this machine | Developer ID, notarized, stapled |
| `Emaki-windows-x64-setup.exe` | CI | unsigned (SmartScreen asks once) |
| `Emaki-linux-x64.AppImage` | CI | n/a |
| `Emaki-linux-x64.deb` | CI | n/a |

The names never change, so `releases/latest/download/<name>` links stay
valid. CI's own Mac images are ad-hoc signed because the signing secrets
are not in the repository (see "Signing in CI" below); the notarized ones
are built here by `scripts/release-mac.sh` and uploaded over them.

## One-time setup on a Mac

- Rust via rustup, plus the Intel target: `rustup target add x86_64-apple-darwin`.
- `cargo install cargo-packager --locked`.
- Xcode or its command line tools (`codesign`, `hdiutil`, `notarytool`, `stapler`).
- The certificate `Developer ID Application: Pingfan Hu (XC2WL5WN7J)` in the
  login keychain (`security find-identity -v -p codesigning` lists it).
- The notarytool keychain profile `notarytool-profile`
  (`xcrun notarytool store-credentials notarytool-profile` once, with the
  Apple ID, an app-specific password and team `XC2WL5WN7J`).
- `gh auth login` with an account that has write access to the repository.
- For the Windows type check in `scripts/release-check.sh`:
  `brew install mingw-w64` and `rustup target add x86_64-pc-windows-gnu`.
- For the disk-image background only: Python with Pillow and the LXGW
  WenKai Medium font in `~/Library/Fonts`. The icon needs nothing beyond
  a Mac (`python3` and `swift` ship with it).

## Each release

### 1. Bump the version

The version lives in one file:

```
Cargo.toml            version = "X.Y.Z"      (the workspace; both crates inherit it)
```

`Cargo.lock` records the crates' versions too, and CI builds with
`--locked`, so refresh it after the bump and commit it with the rest:

```
cargo check -p emaki-app
```

The release workflow refuses a tag whose number disagrees with `Cargo.toml`.

### 2. Write the changelog section

Add `## vX.Y.Z` to `CHANGELOG.md`, newest first, written for the person
installing the app: what changed for them, not which files moved. Keep it
short: one line per change, a dozen lines at most, small fixes grouped
into one. The
workflow refuses a tag with no section, and `scripts/release-notes.sh X.Y.Z`
prints exactly what the release page will show.

### 3. Check it before the tag

Commit the bump and the changelog, then:

```
scripts/release-check.sh X.Y.Z
```

It holds the commit to everything the tag will be held to: the version in
`Cargo.toml`, a lock that builds with `--locked`, a changelog section of a
dozen lines at most, a clean tree, the core tests, the app build, and a
Windows type check (MinGW; `brew install mingw-w64 && rustup target add
x86_64-pc-windows-gnu` once). Do not tag until it says "ready to tag". A
tag that fails in CI has to be redone, and every redo means building and
notarizing the Mac images again; v0.1.5 was tagged three times, the first
for a line that did not compile on Windows, which this check catches in a
minute.

### 4. Tag and push

Work happens on a branch; `main` is fast-forwarded to it at release.

```
git push origin HEAD                 # the branch
git push origin HEAD:main            # fast-forward main
git tag -a vX.Y.Z -m "Emaki vX.Y.Z"
git push origin vX.Y.Z
```

The tag push starts `release.yml`. It takes about twenty minutes (the
cold GPUI build), and ends with a **draft** release holding CI's
installers: nothing is public, and `releases/latest` (what the app's own
updater follows) still names the previous version. Watch it:

```
gh run list --repo alkaline-software/emaki --workflow release --limit 3
gh run watch <run-id> --repo alkaline-software/emaki --exit-status
```

### 5. Build, sign and notarize the Mac images here

While CI runs, and from the tagged commit: an image built before the last
fix is an image of other code, and `release-publish.sh` refuses one older
than the tag's commit. Both architectures build on an Apple Silicon Mac.
The script packages the bundle with cargo-packager, signs it with the
Developer ID and the hardened runtime, lays the disk image out through
Finder over `scripts/dmg/background.png`, compresses it, signs it, submits
it to the notary service, waits, staples and validates:

```
cargo build --release --locked -p emaki-app
cargo build --release --locked -p emaki-app --target x86_64-apple-darwin
scripts/release-mac.sh --no-build --notarize
scripts/release-mac.sh --no-build --notarize --target x86_64-apple-darwin
```

Each run ends with `built dist/Emaki-mac-<arch>.dmg (X.Y.Z, <arch>, signed,
notarized)` and leaves only the two images in `dist/`. Notarization takes a
few minutes per image; the four commands together about ten.

### 6. Put them on the release and publish it

Once the workflow is green and both images are built:

```
scripts/release-publish.sh X.Y.Z
```

It refuses unless the release workflow for the tag has finished and
succeeded (`action-gh-release` replaces same-named assets, so an upload
made while the job runs is overwritten by it) and both images are
notarized, stapled and newer than the tag's commit. Then it uploads them
over CI's ad-hoc ones, downloads them back and compares checksums, checks
the downloaded copies the way Gatekeeper will, and publishes the draft as
the latest release. The release is not done until this prints "published
with notarized Mac images". v0.1.5 was published by CI with the ad-hoc
images and the upload came half an hour later; the app updated itself
from them in between and macOS refused it.

### 7. Install the release build here

Drag from the disk image, or:

```
hdiutil attach dist/Emaki-mac-arm64.dmg
ditto "/Volumes/Emaki vX.Y.Z/Emaki.app" /Applications/Emaki.app
hdiutil detach "/Volumes/Emaki vX.Y.Z"
```

Keep no other `Emaki.app` on the disk (`dist/`, a scratch folder,
`scripts/make-app.sh`'s output): Launchpad and Spotlight list each one.

## Redoing a release under the same tag

Delete the release and the tag on both sides, cancel a run still going
(`gh run cancel <run-id>`), then repeat from step 3 with the new commit:

```
gh release delete vX.Y.Z --yes --repo alkaline-software/emaki
git tag -d vX.Y.Z
git push origin :refs/tags/vX.Y.Z
git tag -a vX.Y.Z -m "Emaki vX.Y.Z"
git push origin vX.Y.Z
```

The Mac images must be rebuilt from the new commit (step 5) and published
again (step 6).

## Signing in CI

`release.yml` signs and notarizes on the runner when five repository
secrets exist; step 5 then disappears and step 6 is only
`gh release edit vX.Y.Z --draft=false --latest`. Write access is enough to
set them:

```
security export -k login.keychain-db -t identities -f pkcs12 -P "<passphrase>" -o /tmp/cert.p12
gh secret set MACOS_CERTIFICATE_P12 --repo alkaline-software/emaki < <(base64 -i /tmp/cert.p12)
gh secret set MACOS_CERTIFICATE_PASSWORD --repo alkaline-software/emaki --body "<passphrase>"
gh secret set APPLE_ID --repo alkaline-software/emaki --body "<Apple ID email>"
gh secret set APPLE_APP_PASSWORD --repo alkaline-software/emaki --body "<app-specific password>"
gh secret set APPLE_TEAM_ID --repo alkaline-software/emaki --body "XC2WL5WN7J"
rm /tmp/cert.p12
```

Windows stays unsigned until SignPath (free for open source) is set up.

## The pieces a release depends on

**The icon.** `scripts/icon/icon.html` is the source: it draws the picture
on a canvas, at any size. `scripts/make-icon.sh` runs it (WebKit, through
`scripts/icon/render.swift`, so on a Mac) into `crates/emaki-app/assets/icon`:
the PNGs as the rounded icon with its margin and shadow (the Dock icon the
app sets at start, Windows, Linux), `icon.ico`, and `icon.icns` as the full
square, opaque to the corners, because macOS 26 masks every app icon to its
own rounded square over a grey backing and shows anything transparent as a
grey border. Open the page in a browser to see every size on one sheet.
Change the drawing, run the script, rebuild, commit the assets.

**The disk-image background.** `scripts/dmg/background.png`, drawn by
`scripts/dmg/background.py` (Pillow, LXGW WenKai Medium) in the app's cream
and terracotta: "Drag Emaki to Applications" and an arrow, 1320x840 at 2x
for the 660x420 Finder window. Run the script only when the picture changes;
the PNG is committed.

**The layout.** `release-mac.sh` mounts a read-write image and has Finder
set the window, the 128pt icons at 165 and 495, and the background, then
converts to a compressed read-only image. Only Finder writes a `.DS_Store`
Finder honours, so the layout is not generated offline. This works on the
GitHub macOS runners too.

## When something fails

- **"Cargo.toml says X but the tag is vY"**: the version and the tag
  disagree. Fix `Cargo.toml`, commit, redo the tag.
- **"CHANGELOG.md has no section '## vX.Y.Z'"**: add it, commit, redo the tag.
- **`--locked` fails in CI after a bump**: `Cargo.lock` was not refreshed.
  `cargo check -p emaki-app`, commit the lock.
- **Notarization rejected**: `xcrun notarytool log <submission-id>
  --keychain-profile notarytool-profile`. The usual causes are a binary
  signed without `--options runtime` or an unsigned helper inside the bundle;
  the script signs with `--deep` and the hardened runtime, so look for a new
  binary the packager added.
- **The Finder layout step fails**: the volume `Emaki vX.Y.Z` was already
  mounted, or Finder had no session. The script detaches a stale volume
  first; `hdiutil info` shows what is mounted.
- **Two Emakis in Launchpad or Spotlight**: a second bundle somewhere on
  disk. `mdfind "kMDItemCFBundleIdentifier == 'com.pingfanhu.emaki'"` lists
  them; keep only `/Applications/Emaki.app`.
- **The Mac images on the release are ad-hoc signed**, or macOS says the
  app "cannot be opened" after an update: step 6 was skipped. Run
  `scripts/release-publish.sh X.Y.Z`. To check an installed copy:
  `spctl -a -vv /Applications/Emaki.app` must say "Notarized Developer
  ID"; "rejected" with `Signature=adhoc` from `codesign -dv` is CI's
  image. Reinstall from `dist/` (step 7).
- **The Windows job fails with "cannot find type" or an unresolved
  import**: a line used on every platform leans on something gated for
  Unix or macOS. `scripts/release-check.sh` runs the type check that
  finds it; the macOS and Linux jobs pass without it.
- **"Node.js NN is deprecated" on a run**: an action is a major version
  behind. `gh api repos/<owner>/<action>/releases/latest -q .tag_name`
  gives the current one; move `rust.yml` and `release.yml` together.
- **A terminal says "Transcript saving is off, inherited
  CLAUDE_CODE_CHILD_SESSION marker"**: Emaki was started from inside a
  Claude Code session and a terminal app it launched carries that
  session's variables. Quit the terminal app and reopen it. When an agent
  starts Emaki, it must clear every `CLAUDE*` variable first.
