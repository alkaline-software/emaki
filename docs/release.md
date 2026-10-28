# Releasing Emaki

What to run, in order, to cut a release, and what to do when a step fails. `docs/platform.md` (CI and releases) says how the pieces fit.

## What a release is

A `v*` tag pushed to `alkaline-software/emaki` runs `release.yml`. Six package jobs build the app on macOS (both architectures, from the Apple Silicon runner), Windows and Linux (x64 and ARM each, on runners of their own), then one `release` job creates the GitHub Release, as a draft, with the tag's section of `CHANGELOG.md` as its notes and attaches the installers under stable names:

| asset | built by | signed |
|---|---|---|
| `Emaki-mac-arm64.dmg` | CI, then replaced from this machine | Developer ID, notarized, stapled |
| `Emaki-mac-x64.dmg` | CI, then replaced from this machine | Developer ID, notarized, stapled |
| `Emaki-windows-x64-setup.exe` | CI (Velopack) | Azure Artifact Signing, once its secrets are set; unsigned before |
| `Emaki-windows-arm64-setup.exe` | CI (Velopack) | the same |
| `AlkalineSoftware.Emaki-<version>-win-<arch>-full.nupkg`, `releases.win-<arch>.json` | CI (Velopack) | the package and feed a Windows copy updates itself from; the name starts with the pack id |
| `Emaki-linux-x64.AppImage` | CI | n/a |
| `Emaki-linux-x64.deb` | CI | n/a |
| `Emaki-linux-arm64.AppImage` | CI | n/a |
| `Emaki-linux-arm64.deb` | CI | n/a |

The names never change, so `releases/latest/download/<name>` links stay valid. The Windows feed and package carry the version and are found through the feed, whose own name is stable. The Microsoft Store's `.msix` packages and the snaps are not on the release: CI keeps them with the run (`store-win-x64`, `store-win-arm64`, `snap-amd64`, `snap-arm64`) for their stores (see "The Microsoft Store" and "The Snap Store"). CI's own Mac images are ad-hoc signed because the signing secrets are not in the repository (see "Signing in CI"); the notarized ones are built here by `scripts/release-mac.sh` and uploaded over them.

## One-time setup on a Mac

- Rust via rustup, plus the Intel target: `rustup target add x86_64-apple-darwin`.
- `cargo install cargo-packager --version 0.11.8 --locked`, the version `release.yml` pins.
- Xcode or its command line tools (`codesign`, `hdiutil`, `notarytool`, `stapler`).
- The certificate `Developer ID Application: Pingfan Hu (XC2WL5WN7J)` in the login keychain (`security find-identity -v -p codesigning` lists it).
- The notarytool keychain profile `notarytool-profile` (`xcrun notarytool store-credentials notarytool-profile` once, with the Apple ID, an app-specific password and team `XC2WL5WN7J`).
- `gh auth login` with an account that has write access to the repository.
- For the Windows type check in `scripts/release-check.sh`: `brew install mingw-w64` and `rustup target add x86_64-pc-windows-gnu`.
- For the disk-image background only: Python with Pillow and the LXGW WenKai Medium font in `~/Library/Fonts`. The icon needs nothing beyond a Mac (`python3` and `swift` ship with it).

## Each release

### 1. Bump the version

The version lives in one place, `version = "X.Y.Z"` in the workspace's `Cargo.toml`; both crates inherit it. `Cargo.lock` records it too, and CI builds with `--locked`, so refresh the lock and commit it with the rest:

```
cargo check -p emaki-app
```

### 2. Write the changelog section

Add `## vX.Y.Z` to `CHANGELOG.md`, newest first, written for the person installing the app: what changed for them, not which files moved. It is a numbered list, a topic in bold and then one sentence, small fixes gathered under **Fixes**, ten items at most. `scripts/release-notes.sh X.Y.Z` prints exactly what the release page will show.

### 3. Check it before the tag

Commit the bump and the changelog, then:

```
caffeinate -d -t 14400 &             # the display stays on, so the screen stays unlocked
scripts/release-check.sh X.Y.Z
```

The notary credential is out of reach while the screen is locked (see "When something fails"). Emaki's Keep the display on holds while a background command waits on CI too, but only where that setting is on and the session is Emaki's to see. An agent starts `caffeinate` as a background command of its own all the same, for the four hours a release with a redo can take.

The check holds the commit to everything the tag will be held to: the version in `Cargo.toml`, a changelog section of ten items at most, a notary service that answers to the saved credential (step 5 needs it, an hour later), a lock that builds with `--locked`, a clean tree, the core tests (once more with Windows' one-megabyte stack), the app build, and a Windows type check. Do not tag until it says "ready to tag". A tag that fails in CI has to be redone, and every redo means building and notarizing the Mac images again.

### 4. Push, wait for the tests, then tag

`rust.yml` runs on a push to `main`, and it is the only place the tests run on Windows and Linux (step 3 type-checks Windows and runs nothing there). So push, let that run finish, and tag only once it is green on all three:

```
git push origin HEAD                 # the branch
git push origin HEAD:main            # fast-forward main; starts rust.yml
gh run list --repo alkaline-software/emaki --workflow rust --limit 1
gh run watch <run-id> --repo alkaline-software/emaki --exit-status
git tag -a vX.Y.Z -m "Emaki vX.Y.Z"
git push origin vX.Y.Z
```

The Mac images (step 5) can be built while it runs: the commit is the one that will be tagged, unless the tests send you back.

The tag push starts `release.yml`. It takes about twenty minutes (the cold GPUI build), and ends with a **draft** release holding CI's installers: nothing is public, and `releases/latest` (what the app's own updater follows) still names the previous version. Watch it:

```
gh run list --repo alkaline-software/emaki --workflow release --limit 3
gh run watch <run-id> --repo alkaline-software/emaki --exit-status
```

### 5. Build, sign and notarize the Mac images here

From the tagged commit: an image built before the last fix is an image of other code, and `release-publish.sh` refuses one older than the tag's commit. Both architectures build on an Apple Silicon Mac. The script packages the bundle with cargo-packager, signs it with the Developer ID and the hardened runtime, lays the disk image out through Finder over `scripts/dmg/background.png`, compresses it, signs it, submits it to the notary service, waits, staples and validates:

```
cargo build --release --locked -p emaki-app
cargo build --release --locked -p emaki-app --target x86_64-apple-darwin
scripts/release-mac.sh --no-build --notarize
scripts/release-mac.sh --no-build --notarize --target x86_64-apple-darwin
```

Each run ends with `built dist/Emaki-mac-<arch>.dmg (X.Y.Z, <arch>, signed, notarized)` and leaves only the two images in `dist/`. The four commands together take about ten minutes. Check each run's own exit code and last line, not output piped through `tail`.

### 6. Install and run it on Windows and Linux

Before anything is public, the draft's own installers are installed and opened on the other systems, in VMPal's VMs on this Mac (`Windows 11` and `Ubuntu 26`, both ARM): CI builds and tests the code, and nothing else installs the packages a person downloads. Download them from the draft:

```
gh release download vX.Y.Z --repo alkaline-software/emaki --dir dist/try --pattern 'Emaki-windows-*-setup.exe' --pattern 'Emaki-linux-arm64.*'
```

- **Windows**: copy `Emaki-windows-arm64-setup.exe` and `Emaki-windows-x64-setup.exe` (which Windows on ARM runs emulated) into the VM, run each, and see the splash and then the window open; the app is under `%LOCALAPPDATA%\AlkalineSoftware.Emaki`, and an older one there is updated in place. A file copied in by VMPal has no mark of a download, so first give it the one a browser gives (`Set-Content <file> -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3"`) and look at what Windows shows first: without Artifact Signing (see "Signing on Windows") SmartScreen stops it with "Windows protected your PC" and an unknown publisher.
- **Linux**: install the `.deb` with `sudo apt install ./Emaki-linux-arm64.deb` and open Emaki from the app grid; make the AppImage executable and run it. Both must open the window with no error in a terminal started beside them; the AppImage's `xkbcommon: ERROR ... unrecognized keysym` lines are its own older keyboard library reading the system's Compose file, and are not ours. The x64 Linux packages have no machine here and are tried only by CI's build.
- In each, the window opens on the new-session page, the sidebar lists the agents' sessions, a session opens, a file opens from the files panel, and the terminal panel runs a shell. A VM with no sessions gets one: a short transcript in `~/.claude/projects/<the folder's path, each character but a letter or digit made a dash>/<id>.jsonl` shows in the sidebar while the app runs. Then close the window and open it again twice: it comes back where it was. On Windows, drag it somewhere else and close it at once: it opens where it was dropped. On Linux the close button is the one the window draws (GNOME draws none): resize the window, close it at once, and `~/.emaki/state/ui.json` is written at that moment with the new size.

A package that does not install or open is a reason to stop: fix it, then redo the release from step 3 ("Redoing a release under the same tag").

### 7. Put them on the release and publish it

Once the workflow is green and both images are built:

```
scripts/release-publish.sh X.Y.Z
```

It refuses unless the release workflow for the tag has finished and succeeded (`action-gh-release` replaces same-named assets, so an upload made while the job runs is overwritten by it) and both images are notarized, stapled and newer than the tag's commit. Then it uploads them over CI's ad-hoc ones, downloads them back and compares checksums, checks the downloaded copies the way Gatekeeper will, and publishes the draft as the latest release. The release is not done until this prints "published with notarized Mac images": a release published with CI's ad-hoc images is one the app updates itself from and macOS then refuses.

### 8. Install the release build here

Drag from the disk image, or:

```
hdiutil attach -nobrowse dist/Emaki-mac-arm64.dmg
ditto "/Volumes/Emaki vX.Y.Z/Emaki.app" /Applications/Emaki.app.new
mv /Applications/Emaki.app "$TMPDIR/Emaki.app.old" && mv /Applications/Emaki.app.new /Applications/Emaki.app
rm -rf "$TMPDIR/Emaki.app.old"
hdiutil detach "/Volumes/Emaki vX.Y.Z"
```

The new bundle is copied beside the old and put in its place by a rename, never copied over it: the installed Emaki may be running (the person keeps it open, and the agent may be its child), and a copy over a running app's executable changes the pages macOS has mapped and checks against their signature, which can end the app with the session in it. Renamed away, the old files stay whole for as long as the copy running them lives.

Keep no other `Emaki.app` on the disk (`dist/`, a scratch folder, `scripts/make-app.sh`'s output): Launchpad and Spotlight list each one. Step 9 removes the ones a release leaves.

### 9. Delete the compiled files

Every release ends with this step, and a release is not finished until it has run. Nothing compiled is kept between releases: disk space is worth more here than the minutes the next build takes, and all of it is rebuilt from the source. Do it without being asked, once step 7 printed "published with notarized Mac images" and the installed copy opens:

```
scripts/release-clean.sh
```

- It runs `cargo clean`, which deletes `target/`: every build made while developing and for the release, and with it the dev build's bundle. Months of incremental builds grow it past 80 GB.
- It deletes `dist/`, the two disk images, which are on the release by now.
- It asks Spotlight for every `Emaki.app` and fails if there is one other than `/Applications/Emaki.app`.
- It prints `du -sh .` on the repository, which should be well under a gigabyte.

It refuses in two cases: the installed copy is not a notarized release, or `dist/` holds images of a version that is not published (a redo of the release wants them).

The agent cutting the release is often a child of the dev build (`scripts/dev-app.sh`), whose bundle is in `target/`. The script cleans under it all the same: a running program keeps the files it has open, so the app and the session go on. The app has no bundle on disk from then on, so the script ends by handing over: `scripts/relaunch.sh --installed` quits the dev build once the turn is over and opens `/Applications/Emaki.app`. Make it the last command of the release, and tell the person the window they are looking at will close and the installed one will open, with the session there to be continued.

When the agent is a child of the installed Emaki instead, `ps -o ppid= -p $PPID` names it. Step 8 then put the new bundle in place under a running copy of the last version, which goes on running it: the person quits the installed Emaki and opens it again to run the new one, and the session is there to be continued.

## Redoing a release under the same tag

Delete the release and the tag on both sides, cancel a run still going (`gh run cancel <run-id>`), then repeat from step 3 with the new commit:

```
gh release delete vX.Y.Z --yes --repo alkaline-software/emaki
git tag -d vX.Y.Z
git push origin :refs/tags/vX.Y.Z
git tag -a vX.Y.Z -m "Emaki vX.Y.Z"
git push origin vX.Y.Z
```

The Mac images must be rebuilt from the new commit (step 5), the other systems tried again (step 6), and published again (step 7).

Claude Code's auto mode refuses an agent the first three commands, as a destructive git action, unless the person has allowed `gh release delete` and `git push origin :refs/tags/*` in `.claude/settings.local.json`. Without that rule the agent asks the person to run them in a terminal, and goes on from the tag once they are done. A command typed with `!` runs only in the session's own window, the Emaki whose child the session is, not in another Emaki showing the same session.

## Signing in CI

`release.yml` signs and notarizes on the runner when five repository secrets exist; step 5 then disappears and step 7 is only `gh release edit vX.Y.Z --draft=false --latest`. Write access is enough to set them:

```
security export -k login.keychain-db -t identities -f pkcs12 -P "<passphrase>" -o /tmp/cert.p12
gh secret set MACOS_CERTIFICATE_P12 --repo alkaline-software/emaki < <(base64 -i /tmp/cert.p12)
gh secret set MACOS_CERTIFICATE_PASSWORD --repo alkaline-software/emaki --body "<passphrase>"
gh secret set APPLE_ID --repo alkaline-software/emaki --body "<Apple ID email>"
gh secret set APPLE_APP_PASSWORD --repo alkaline-software/emaki --body "<app-specific password>"
gh secret set APPLE_TEAM_ID --repo alkaline-software/emaki --body "XC2WL5WN7J"
rm /tmp/cert.p12
```

## Signing on Windows

SmartScreen stops an unsigned installer ("Windows protected your PC"). `release.yml` has `vpk` sign Emaki.exe, Velopack's programs and Setup.exe with Azure Artifact Signing (formerly Trusted Signing) when these exist in the repository, and leaves them unsigned otherwise. Once, in the Azure portal, with an Azure subscription:

1. Create an Artifact Signing account (Basic, about $9.99 a month) in a region; its endpoint is the region's, `https://<region>.codesigning.azure.net/`.
2. Identity validation, Individual (the United States or Canada), with a government ID. It takes from hours to a few business days.
3. A certificate profile of type Public Trust on the validated identity.
4. An app registration (Microsoft Entra ID): its client ID, tenant ID and a client secret. On the signing account, Access control, give it the role Artifact Signing Certificate Profile Signer.
5. Then:

```
gh secret set AZURE_TENANT_ID --repo alkaline-software/emaki --body "<tenant id>"
gh secret set AZURE_CLIENT_ID --repo alkaline-software/emaki --body "<client id>"
gh secret set AZURE_CLIENT_SECRET --repo alkaline-software/emaki --body "<client secret>"
gh variable set ARTIFACT_SIGNING_ENDPOINT --repo alkaline-software/emaki --body "https://<region>.codesigning.azure.net/"
gh variable set ARTIFACT_SIGNING_ACCOUNT --repo alkaline-software/emaki --body "<account name>"
gh variable set ARTIFACT_SIGNING_PROFILE --repo alkaline-software/emaki --body "<certificate profile name>"
```

The client secret expires (two years at most): set a new one before it does. A new certificate still gets SmartScreen's warning for the first releases, until downloads build its reputation; the same identity across releases keeps what was built. `Get-AuthenticodeSignature .\Emaki-windows-x64-setup.exe` on Windows says whether a release's installer is signed.

## The Microsoft Store

The Store signs what it sells, so its copy never meets SmartScreen. Once:

1. Register as an individual developer at storedeveloper.microsoft.com (free) with a Microsoft account.
2. In Partner Center, create a new app and reserve the name Emaki.
3. Product management, Product identity gives three values. Then:

```
gh variable set STORE_IDENTITY_NAME --repo alkaline-software/emaki --body "<Package/Identity/Name>"
gh variable set STORE_PUBLISHER --repo alkaline-software/emaki --body "<Package/Identity/Publisher>"
gh variable set STORE_PUBLISHER_NAME --repo alkaline-software/emaki --body "<Package/Properties/PublisherDisplayName>"
```

Each release then leaves `Emaki-windows-x64.msix` and `Emaki-windows-arm64.msix` with the release run. After step 7:

```
gh run download <run-id> --repo alkaline-software/emaki --pattern 'store-*' --dir dist/store
```

In Partner Center, start a submission, upload both packages under Packages, fill the listing the first time (the description and screenshots), and submit. The package asks for two restricted capabilities, `runFullTrust` and `unvirtualizedResources`; for the second the submission asks why: a terminal app whose shells and agents must install into the real AppData, where the rest of the system sees what they install. Certification takes from hours to a few days. The Store updates its copies itself; the app shows no Updates in Settings there.

## The Snap Store

App Center warns "Potentially unsafe" about every `.deb` opened from a file, whoever made it; a snap from the Snap Store installs with no warning, and snapd keeps it current. Once:

1. Sign in at snapcraft.io with an Ubuntu One account and register the name `emaki` (snapcraft.io/register-snap).
2. Ask for classic confinement: a post in the "store-requests" category of forum.snapcraft.io, naming the snap and why. Emaki is a terminal app for coding agents: it runs the person's shell and agents, which must install and run as they do outside it, and it reads their sessions in `~/.claude` and `~/.codex`, which a strict snap cannot see. The reviewers answer on the forum, in days to a couple of weeks.
3. The upload credentials, from any machine with snapcraft (`sudo snap install snapcraft --classic` on Ubuntu, `brew install snapcraft` on a Mac):

```
snapcraft export-login --snaps=emaki --acls package_access,package_push,package_update,package_release - | gh secret set SNAPCRAFT_STORE_CREDENTIALS --repo alkaline-software/emaki
```

4. Once classic is granted:

```
gh variable set SNAP_CHANNEL --repo alkaline-software/emaki --body stable
```

Each release run makes `emaki_amd64.snap` and `emaki_arm64.snap` and keeps them with the run (`snap-amd64`, `snap-arm64`); with the secret and the variable set it uploads both and releases them to that channel. Before then, the first upload is by hand, and waits for the classic review:

```
gh run download <run-id> --repo alkaline-software/emaki --pattern 'snap-*' --dir dist/snap
snapcraft upload --release=stable dist/snap/snap-arm64/emaki_arm64.snap
snapcraft upload --release=stable dist/snap/snap-amd64/emaki_amd64.snap
```

The store page (snapcraft.io/emaki) takes its icon, screenshots and links from the listing in the snap's dashboard, filled once.

## The pieces a release depends on

**The icon.** Change the drawing in `scripts/icon/icon.html`, run `scripts/make-icon.sh` (WebKit, so on a Mac) to remake `crates/emaki-app/assets/icon`, rebuild, and commit the assets. Open the page in a browser to see every size on one sheet. How it is drawn is in `docs/platform.md`, The app icon.

**The disk-image background.** `scripts/dmg/background.png`, drawn by `scripts/dmg/background.py` (Pillow, LXGW WenKai Medium) in the app's cream and its periwinkle accent: "Drag Emaki to Applications" and an arrow, 1320x840 at 2x for the 660x420 Finder window. Run the script only when the picture changes; the PNG is committed.

**The layout.** `release-mac.sh` mounts a read-write image and has Finder set the window, the icons and the background, then converts to a compressed read-only image. Only Finder writes a `.DS_Store` Finder honors, so the layout is not generated offline. This works on the GitHub macOS runners too.

## When something fails

Every failure met while cutting a release gets a line here once it is solved, with what would have caught it, and the steps above are changed so it is caught before the tag next time.

- **A test passes here and fails on Windows or Linux** (the `rust` workflow goes red after the push): the tests only run on the other systems in CI, which is why step 4 waits for that run before tagging. Windows has failed on its own rules: a file name's trailing dot is ignored, so `main.rs.` answered as `main.rs` (v0.1.6, after the tag).
- **A test that makes a git repository fails on Windows only, on a line end**: Git for Windows has `core.autocrlf` on, so a file it checks out or restores comes back with `\r\n`. A test's own repository sets `core.autocrlf false` with `git config`, not `-c` on the test's own commands: the code under test runs git itself and reads the repository's settings.
- **A git test fails on Windows only, on a remote that is a folder**: a test's remote is a temp folder, `C:/Users/...` there, and a parser of remotes that takes `x:y` for `host:path` reads it as a host "C". One letter before the colon is a drive (`git::host_of`), as git has it.
- **A test that runs a Unix shell fails on Windows only**, with nothing on its error output: the `bash` a Windows runner finds does not read a file a Mac's or Linux's does (v0.4.1, before the tag). A test of something only Unix uses is `#[cfg(unix)]`, as the code it tests is.
- **A test overflows its stack on Windows only** ("has overflowed its stack"): Windows gives a thread a megabyte, a Mac and Linux eight, and a reader that recurses as deep as a document is nested (Typst's parser and layout under a Typst or LaTeX file, or a formula) passed here. Such reading runs on a thread of its own with a deep stack (`on_deep_stack`), and `release-check.sh` runs the core tests again with a megabyte (`RUST_MIN_STACK`).
- **On Windows the window opens a little lower at each launch**: its place was saved from the wrong bounds (`docs/window.md`, What the window remembers). Step 6 closes and opens the window twice to catch it.
- **One Mac package job in `release.yml` fails right after "satisfies its Designated Requirement", with no message, while the other passes**: `hdiutil create` on the runner ("Resource busy"). `release-mac.sh` tries it five times and says why. If it still fails, re-run only that job, which keeps the tag and the images built here: `gh run rerun <run-id> --failed --repo alkaline-software/emaki`. The `release` job waits for all six.
- **`release-mac.sh` stops with "Finder ... AppleEvent timed out (-1712)"**: macOS asked whether the terminal may control Finder and nobody answered. Allow it (System Settings, Privacy & Security, Automation) and run the script again.
- **`release-mac.sh --notarize` or `release-check.sh` stops with "No Keychain password item found for profile: notarytool-profile"** while the profile is in place: the Mac's screen is locked. `store-credentials` keeps the profile in the keychain that opens only while the screen is unlocked, and this Mac locks it the moment the display sleeps, so a release run overnight finds no profile from then on, in an agent's session or anyone's, while builds and CI go on. It is not the sandbox or which Emaki the session runs in. Notarize while the person is at the Mac; a run that reaches step 5 with the screen locked waits for it.
- **CI refuses the tag with "Cargo.toml says X but the tag is vY", "CHANGELOG.md has no section '## vX.Y.Z'", a `--locked` failure, or a Windows job's "cannot find type" or unresolved import**: `scripts/release-check.sh` was skipped, which catches each of these. Fix it (a line used on every platform that leans on something gated for Unix or macOS, for the last), commit, redo the tag.
- **Notarization rejected**: `xcrun notarytool log <submission-id> --keychain-profile notarytool-profile`. The usual causes are a binary signed without `--options runtime` or an unsigned helper inside the bundle; the script signs with `--deep` and the hardened runtime, so look for a new binary the packager added.
- **The Finder layout step fails**: the volume `Emaki vX.Y.Z` was already mounted, or Finder had no session. The script detaches a stale volume first; `hdiutil info` shows what is mounted.
- **Two Emakis in Launchpad or Spotlight**: a second bundle somewhere on disk. `mdfind "kMDItemFSName == 'Emaki.app'"` lists them; keep only `/Applications/Emaki.app`. The dev build's bundle is made in `target/debug/dev.noindex/`, which Spotlight passes by, and step 9 removes it with the rest of `target/`.
- **The Mac images on the release are ad-hoc signed**, or macOS says the app "cannot be opened" after an update: step 7 was skipped. Run `scripts/release-publish.sh X.Y.Z`. To check an installed copy: `spctl -a -vv /Applications/Emaki.app` must say "Notarized Developer ID"; "rejected" with `Signature=adhoc` from `codesign -dv` is CI's image. Reinstall from `dist/` (step 8).
- **"Node.js NN is deprecated" on a run**: an action is a major version behind. `gh api repos/<owner>/<action>/releases/latest -q .tag_name` gives the current one; move `rust.yml` and `release.yml` together.
