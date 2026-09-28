# Scribe native app: plan of action

Plan for the `gpui-app` branch (Rust + GPUI desktop app). Written by JP after a review of the branch as of 2026-09-27; checkmarks and the notes in italics record what landed on the branch on 2026-09-28.

**Status:** Phases 1, 2, 3 and 5 are implemented and verified on macOS; Windows and Linux are type-checked locally (MinGW target) and wait for the first CI run. Phase 4 was measured and needs no work. Phase 6 has not started. The app is called Emaki now (binary, menu, wordmark, bundle, installer names); the crates, the `scribe` CLI, `~/.scribe` and the repository keep the old name until the organisation decision.

## Goals

- Easy to download and install from GitHub Releases on **macOS and Windows**, and on **Linux** where practical.
- Fully open source; no monetization.
- **Fast**, especially when opening and scrolling very long conversations.
- **Several sessions** open and running at once.
- **Robust restore**: close the app, reopen it, and the sessions you were looking at are back and ready to continue.
- A **permanent record** of every conversation (the copy-first archive).

## Decisions so far

- **Keep Rust + GPUI.** Speed on long conversations is the top priority, and GPUI's GPU rendering (the approach Zed and Sublime Text take) serves that best. GPUI supports macOS (Metal), Windows (DirectX) and Linux (wgpu). We accept that GPUI is pre-1.0 and will handle upstream breaking changes as they come.
- **Keep the `scribe-core` / `scribe-app` split.** The core (archive, index, search, watcher, adapters, driver) has no UI dependency and stays the foundation.
- **Mac signed and notarized; Windows unsigned for now.** *Changed 2026-09-28:* Pingfan already holds a Developer ID (`Developer ID Application: Pingfan Hu (XC2WL5WN7J)`) and a `notarytool` keychain profile from his other apps, so `scripts/release-mac.sh` signs with the hardened runtime, makes the disk image and notarizes, on a laptop or in CI. For CI the repository needs five secrets: `MACOS_CERTIFICATE_P12` (the certificate exported from Keychain Access as a .p12, then `base64 -i cert.p12 | pbcopy`), `MACOS_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_APP_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID` (`XC2WL5WN7J`). Without them the workflow falls back to an ad-hoc signature, so a fork still builds. Windows waits for SignPath (free for open source); the docs explain SmartScreen until then.
- **Installers are built only on tagged releases**, not on every commit.
- **Load the tail of long conversations first.** *Measured and dropped, see Phase 4.*
- **Documentation site in Quarto**, in a `docs/` folder of the main repo, published to GitHub Pages.

## Phase 1: Build and behave correctly on Windows

- [x] **Inbox channel (`peer.rs`).** Uses `UnixStream` and an ungated `libc::kill`. Put the module behind `#[cfg(unix)]` and hide the inbox channel on Windows until we confirm how Claude Code exposes session inboxes there. The driver channel (`claude -p`) still works on Windows.
- [x] **Archive safety (`transcript.rs` / `archive.rs`).** `inode_of` returns `0` on non-Unix, so the archive can't detect a replaced transcript on Windows. Use the Windows file ID (e.g. via `winapi-util`).
- [x] **Locating `claude` on Windows.** Claude Code may be installed as a `.cmd` shim that `Command::new("claude")` won't find. Resolve the path with the `which` crate, with a config override. *Done without the crate: `PATH` split the portable way, `claude.exe` and `claude.cmd` tried, the installers' folders as fallbacks, and `driver.claude_path` in `config.json` as the override.*
- [x] **Mac-only commands in `workbench.rs`.** Replace `open` / `open -R` with the `opener` crate (including reveal-in-folder), and `id -F` with the `whoami` crate. *Reveal is three lines per OS in `sys.rs` instead of `opener`'s reveal feature, which pulls in D-Bus on Linux.*
- [x] **Windows paths.** Verify project-folder mangling and path handling for `C:\Users\...`-style paths; add tests.
- [x] **Protocol guard for the driver** (the risk at the end of this file). *`TESTED_CLAUDE_VERSION` (2.1.283) is compared with `claude --version` at driver start, a newer release is noted on stderr, and a recorded-wire test keeps the parser honest.*

## Phase 2: Continuous integration

- [x] Add `.github/workflows/rust.yml`: `cargo test` plus a build check on `macos-latest`, `windows-latest` and `ubuntu-latest`.
- [x] Trigger on pushes to `main` and `gpui-app`, and on pull requests.
- [x] Cache builds (e.g. `Swatinem/rust-cache`) to keep GPUI compile times manageable.
- [x] Install GPUI's Linux system libraries (Wayland/X11/xkbcommon, etc.) on the Ubuntu runner. *The list is zed's own and may need a name fixed on the first run.*
- [x] Keep the existing Python `tests.yml` as is.

Phases 1 and 2 go together in one pull request: Windows CI is how we confirm the Phase 1 fixes work. *Written, not yet run: nothing has been pushed.*

## Phase 3: Release installers on tags

- [x] Configure `cargo-packager`: `.dmg` (macOS), NSIS `.exe` installer (Windows), AppImage and `.deb` (Linux). *Config under `[package.metadata.packager]` in `crates/scribe-app/Cargo.toml`; verified locally on macOS. The packager does not run the build, so `cargo build --release` comes first.*
- [x] Use **stable installer file names** so download links never change. *`Emaki-mac-arm64.dmg`, `Emaki-mac-x64.dmg`, `Emaki-windows-x64-setup.exe`, `Emaki-linux-x64.AppImage`, `Emaki-linux-x64.deb`.*
- [x] Add `.github/workflows/release.yml`, triggered by tags like `v0.2.0`: build on each OS and attach installers to a GitHub Release. *It also refuses a tag that disagrees with the workspace version. Not yet exercised: it needs a tag.*
- [x] Build macOS for both Apple Silicon and Intel. *Both from the Apple Silicon runner with `--target`; signed and notarized when the secrets are set, ad-hoc otherwise (see Decisions).*
- [ ] Later: auto-updates via `cargo-packager`'s updater.
- [ ] A first tagged release, so Phase 6 has something to link to.

## Phase 4: Fast loading of long conversations

The conversation view already uses GPUI's virtualized `list`, so only visible messages are drawn. The remaining cost is parsing on open.

- [x] **Measure first.** Add a timing benchmark that loads the largest transcripts in the archive. *`scribe-core bench [<id>...]` times read, build and render per transcript, and `SCRIBE_TIMING=1` makes the app print load and hand-over time per open. On the five largest transcripts on Pingfan's machine (debug CLI; release is lower):*

  | bytes | read | build | render | rounds |
  |---|---|---|---|---|
  | 137 MB | 78 ms | 82 ms | 3 ms | 50 |
  | 66 MB | 47 ms | 61 ms | 9 ms | 34 |
  | 47 MB | 27 ms | 28 ms | 1 ms | 32 |

  *In the app the 66 MB session opens in 65 to 85 ms end to end, and the list hands over in under a millisecond.*
- [ ] **Tail-first loading.** Read backward from the end of the transcript, show the most recent rounds immediately, and parse older rounds in the background as the user scrolls up. Build on the offset-based reader in `transcript.rs`. *Not needed on the numbers above: the whole open path is under a fifth of a second for the largest session. Revisit only if bench shows a transcript over about half a second.*

## Phase 5: Multiple sessions and restore on reopen

- [x] **Save window state** to `~/.scribe/state/ui.json`: open sessions, the active one, scroll positions, sidebar state, window size and position. Restore it on launch. *`ui_state.rs`; written on change and at quit; the bounds are reused only when their centre is still on a screen. Verified: move, quit, relaunch brought back position, tabs, the active tab and its scroll position.*
- [x] **Tabs for open sessions.** The backend already runs one `claude -p` child per session; this is UI work. *One tab per open session in the top strip, with the agent's mark turning while it works; ⌘W closes the showing tab, then the last tab, then the window.*
- [x] **Clean shutdown and lazy resume.** Close driver processes cleanly on quit; on reopen, restart each with `--resume` only when the user sends it a message.

## Phase 6: Quarto documentation site

Build after Phase 3, once there are real installers and screenshots.

- [ ] Quarto website in `docs/`, published to GitHub Pages by the official Quarto publish Action when `docs/` changes on `main`.
- [ ] **Landing page:** short description, screenshot, download buttons per OS using `releases/latest/download/<stable name>` links.
- [ ] **Install page:** step-by-step instructions with screenshots for opening the unsigned Mac app ("Open Anyway") and getting past the Windows SmartScreen warning; Linux AppImage/`.deb` steps.
- [ ] **Features page.**
- [ ] **"How Scribe keeps your conversations"** page explaining the copy-first archive and why Claude Code's 30-day cleanup matters.

## Known risk to watch

The driver relies on Claude Code's stream-json control protocol (`control_request`, `can_use_tool`), which isn't a stable public API and was verified against one Claude Code version (2.1.272). Add a version check and tests that flag protocol changes when Claude Code updates. *Done, see Phase 1; the tested release is 2.1.283 now.*
