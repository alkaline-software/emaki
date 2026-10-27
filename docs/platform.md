# Platform

About `vendor/`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/`, `scripts/`, `crates/emaki-core/src/update.rs` and `crates/emaki-app/src/sys.rs`.

## Rules

- `vendor/llama.cpp` is built against transcribe-cpp's ggml and must come from the llama.cpp commit that ggml was synced from (`vendor/llama.cpp/UPSTREAM.md`). Update the two together.
- Build and test from the repository's root, never inside `vendor/gpui-component`. It is a workspace of its own: cargo there resolves its own lock against another gpui, writes a second `target/` of gigabytes, and its library tests do not compile against our pin. `.gitignore` keeps both out of the changes.
- Never put a `rev` on our own gpui dependency. gpui-component depends on the unpinned zed git source, and a `rev` makes a second copy of gpui it does not build against. The pin is in `Cargo.lock`: `cargo update -p gpui --precise <rev>` moves it.
- Every change under `vendor/gpui-component/` is marked `(Emaki addition.)` in the source and listed in `vendor/gpui-component/UPSTREAM.md`, or it is lost at the next move to a newer upstream.
- Everything Unix-only is gated with a stated fallback. No macOS command is shelled out from the window; per-OS work goes in `sys.rs`.
- Windows links the C runtime in (`.cargo/config.toml`, `+crt-static`, as zed does). Linked as a DLL, it is `VCRUNTIME140.dll`, which a fresh Windows does not have, and Emaki.exe stopped at launch with a system error. A C dependency built another way than through the `cc` crate has to follow the same flag.
- Every program the app starts goes through `emaki_core::process::command`, never `Command::new`. A release build has no console on Windows (`windows_subsystem` in `main.rs`; until v0.2.9 a console window stood beside the app), and a console program started from it (git, claude, cmd) opens a console window of its own unless it is started with `CREATE_NO_WINDOW`, which `command` sets.
- Off a Mac the scrollbar's thumb keeps its length at the ends (the vendored `scrollbar.rs`, UPSTREAM.md 41): there is no rubber band to show, and measured from the offset as it ran past the end, the thumb shrank and grew at each step and shook. On Windows it is a thin line until the pointer is on it, as Windows 11's own is. gpui draws every scrollbar; the system's own cannot be had in its window.
- A window gpui says decorates itself (`Decorations::Client`, GNOME on Wayland, which draws no title bar for an app) draws its own buttons and edges (`csd.rs`): minimize, maximize and close in the system's order and on its side (`button_layout`, GNOME's `button-layout`), less what the window manager does not honor, at the middle of the strip, which leaves their room (`csd_room`); and edges a press at resizes it, none on a side tiled or with the window maximized. Close is the window's own (`close_window`), which asks about what is not saved. A window the system decorates (a Mac, Windows, X11, KDE) gets nothing from it. On GNOME a window's page came out narrower than its width says (a settings window of 820 laid its page out as if about 40 narrower), so a drawing of a set width runs out of its card there: size it from the room it is drawn in (`panels_room`), never from the window's width.
- A window's `app_id` is `Emaki`, the desktop file's name, or the Linux shell shows a blank icon for the running app.
- The Windows installer is Velopack's Setup.exe: no wizard and no button, the splash (`scripts/splash/`, the app's icon and name over the progress bar Velopack draws at its foot) while it puts Emaki in place, then Emaki opens. `main` answers Velopack's hooks first (`VelopackApp`); the crate and `vpk` in release.yml are pinned to the same version and move together. NSIS's wizard was the installer until v0.3.0, made one step and still a window of Windows 2000's look.
- Velopack's pack id is `AlkalineSoftware.Emaki` (`update::WINDOWS_PACK_ID`), its folder `%LOCALAPPDATA%\AlkalineSoftware.Emaki`. Under `Emaki`, NSIS's folder until v0.3.0, its installer found an app already installed there and asked whether to repair it. Once Velopack's copy runs it takes away what NSIS left (`update::drop_nsis_install`, at each start until it is gone): the program, the uninstaller, their folder, the uninstall entry and the Start menu folder. NSIS's uninstaller is not run: it ends every `Emaki.exe`, the new one too.
- SmartScreen stops an unsigned installer outright. The Windows build is signed in release.yml by Azure Artifact Signing, when the repository has its secrets (`docs/release.md`, Signing on Windows): `vpk` signs Emaki.exe, its own programs and Setup.exe. A new certificate still meets SmartScreen's warning until downloads build its reputation; the Microsoft Store's copy never does, since the Store signs it.
- The Store's copy is an `.msix` (`crates/emaki-app/windows/store/`, `scripts/windows-msix.ps1`), made in release.yml once Partner Center has reserved the app's identity. It runs at full trust with the package's write virtualization off (`unvirtualizedResources`): a shell or an agent started in Emaki installs into the real AppData, where everything else sees it. The Store updates it, so it shows no Updates in Settings and never checks (`update::from_store`, by its `WindowsApps` folder).
- Linux's first download is the `.deb`: a double click opens App Center, which installs it. A downloaded AppImage cannot be opened from the Files app until it is made executable, and an x64 one never runs on ARM; both were the "it does not open" of v0.3.0, whose site offered only the x64 AppImage. A browser on Linux seldom says which chip it runs on (Firefox says `Linux x86_64` on ARM too, and has no `userAgentData` to ask), so where it does not, the site's download opens its menu for the person to choose instead of guessing x64.
- App Center warns "Potentially unsafe" about every `.deb` opened from a file, with no check behind it (`local_deb_page.dart` in ubuntu/app-center), so nothing in the `.deb` can take the warning away. The Snap Store's copy has none: a snap of the same binary (`crates/emaki-app/linux/snap/`, `scripts/linux-snap.sh`, packed by `snap pack` with no build container), with classic confinement, because Emaki runs the person's shell and agents and reads `~/.claude` and `~/.codex`, none of which a strict snap can. It uses the system's libraries as the `.deb` does. snapd keeps it current, so it shows no Updates (`update::in_snap`), and snapd's `SNAP*` variables are kept from every child (`driver::child_env`): a shell or an agent started there is not in a snap.
- Windows and Linux ship for ARM as well as x64, and the updater asks for its own architecture's file. Every Windows or Linux VM on an Apple silicon Mac is ARM: there the x64 AppImage does not run at all, and the x64 Windows build only under emulation.
- The `.icns` is the full square, opaque to its corners. macOS 26 masks every app icon to its own rounded square over a gray backing, and anything transparent (a margin, rounder corners) shows as a gray border in Finder, the switcher and Spotlight.
- Upload the notarized Mac images after CI's release job, which replaces same-named assets.
- Leave no second `Emaki.app` where Spotlight indexes, or Launchpad lists two. The dev build's bundle is in a folder named `.noindex` for this, and `scripts/release-clean.sh` after a release takes the rest (`docs/release.md`).
- Only Finder writes a `.DS_Store` Finder honors, so the disk image's layout runs through AppleScript on a mounted image.
- A sweep for the old name must use word boundaries: "describe", "subscribe" and "transcribe" contain "scribe".

## The window's edges

- A light line under the window's foot, seen once the frame was dark, was the window behind: ours stood a point short of the screen's usable height, where whatever had filled the screen with it left it. Read off a screenshot, the line was two pixels outside our own last row. `main::filled` takes remembered bounds that are within two points of an edge of the usable area to that edge. The window opened at the full height covers it; nothing in the drawing is short.
- gpui leaves the Mac window's own background the system's, near white in the light appearance. `sys::set_window_ground` sets it to the frame's color whenever that changes (`Workbench::render`), so the window is never light under a dark frame while it is being drawn or resized.
- The system draws a rim one pixel wide inside the window's edge, a little lighter than what is under it, on every side. It is not ours to remove.
- `EMAKI_SHOT_FRAME=1` with `EMAKI_SHOT` keeps the system's shadow and rim in the picture, for reading the pixels at an edge.

## Office files' readers

WordCraft, DeckCraft and GridCraft (storytold, MIT OR Apache-2.0) draw a Word document's pages and a deck's slides and read a workbook's cells. None is on crates.io, so each is a `git` dependency pinned to a commit in the crate's manifest; a newer one is taken by changing that `rev` and looking at the files again. They are all Rust, with no system library. WordCraft compiles in faces of its own (Source Sans 3, Source Serif 4, Inter, JetBrains Mono, all under the SIL Open Font License) for a font the machine lacks, and both it and DeckCraft look through the system's font folders at the first document. Their `CRAFT_FONTS_DIR`, which builds more faces in, is left unset.

A Word document is also written in, through `wordcraft-engine` (`docs/panels.md`); it brings WordCraft's PDF and other-format writers with it, which nothing here calls. A Parquet file is read by Apache's `parquet` crate with its codecs; zstd is the one that builds C.

## HTML's engine

On a Mac and on Windows an HTML file is shown by the system's web view, through `wry` (`html_web.rs`); WebView2 has to be on the Windows machine, as it is on any current one. On Linux no web view can be set in a gpui window, and Blitz draws the file (`html_shot.rs`), built for that target alone. It is a beta, pinned exactly in the app's manifest, with `anyrender` and `anyrender_vello_cpu` at the versions that Blitz builds on. Its style engine is Stylo, Servo's, under the MPL-2.0: a file of it that is changed has to be published as changed. One is: `vendor/stylo_derive/`, Stylo's derive crate with one line changed (`UPSTREAM.md` there), `[patch]`ed over crates.io in the root manifest. Without it Stylo does not compile beside gpui, which turns on a `log` feature that adds a second conversion into `std::fmt::Error`. On Linux Blitz's fonts need fontconfig, which the build already had.

## The vendored toolkit

`vendor/gpui-component/` holds the four gpui-component crates Emaki uses (`ui`, `base`, `macros`, `assets`), copied from the revision the crate manifests still name, with upstream's license and a trimmed workspace manifest of their own so their `workspace = true` references resolve. The root `Cargo.toml` excludes `vendor` from the workspace and `[patch]`es the git source with those paths, so the `git` + `rev` dependencies keep saying where the code came from while the build uses the copies.

Why: the markdown view hard-coded strong text to weight 700 and gave inline code the paragraph's face, and gpui's highlight styles carry no family, so neither could be changed from outside. A fork on GitHub was the alternative; one repository was preferred.

`vendor/air_r_parser/` is the one crate of Air (Posit's R formatter) that could not be taken as it is. Air is not on crates.io and is used by git revision (`emaki-core/Cargo.toml`); its parser pins tree-sitter 0.24 and the window's highlighting is on 0.26, and cargo links one tree-sitter. The copy is that crate with those two lines changed and nothing else; its `UPSTREAM.md` says how to move it.

`vendor/stylo_derive/`, `vendor/block/` and `vendor/proc-macro-error2/` are crates.io crates copied at a release with a small change each, and `[patch.crates-io]` in the root `Cargo.toml` points at them: Stylo's derive so it builds beside gpui, and the last releases of `block` (the Objective-C blocks crate cocoa uses under gpui's Mac backend) and `proc-macro-error2` (Biome's derives, under Air) so that a future Rust still compiles them, since each tripped a lint cargo reported as a future incompatibility on every build. Each copy's `UPSTREAM.md` has the change and when the copy can go.

`UPSTREAM.md` lists the changes, each with what upstream did, and says how to move to a newer revision: copy the crates over, re-apply the list, build. They cover the markdown view (strong weight, the inline-code family and its plate), the scrollbar's fade, ranges a textarea can color (`set_marks`), and the input's Up, Down, ⇧↑, ⇧↓ and caret following.

## Windows and Linux

What is gated, and its fallback:

- `peer`: the inbox is a Unix socket, so elsewhere `registry` is empty, `send` refuses, and the composer falls through to the driver.
- `transcript::file_id`: the inode on Unix, the NTFS file index through `winapi-util` on Windows. The archive must tell a rewritten transcript from an appended one.
- The `claude` lookup: `std::env::split_paths`, `claude.exe` and the npm `claude.cmd` shim on Windows, the installers' directories after `PATH`.
- The `git` lookup (`git::binary`): `git.exe` under Git for Windows' folders on Windows; Homebrew's folders and `/usr/bin` elsewhere, the Mac's `/usr/bin/git` only with Xcode's tools there.
- `paths::decode_project_dir` knows the `C--Users-...` shape.

Without a Windows machine the closest check is a MinGW type check, which catches every `cfg` mistake but not a linker one:

```
brew install mingw-w64 && rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_DIR=target/xwingnu CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
  cargo check -p emaki-app -p emaki-core --target x86_64-pc-windows-gnu
```

The MSVC target cannot be checked from a Mac (bundled SQLite wants the Windows headers) and neither can Linux (fontconfig wants a pkg-config sysroot). `rust.yml` is the real test for both.

## CI and releases

`docs/release.md` is the procedure, scripts included. The shape behind it:

- `rust.yml`, every push: the core tests, an app build and the CLI on all three OSes, Windows and Linux on ARM too, with `Swatinem/rust-cache` because a cold GPUI build is long.
- `release.yml`, on a `v*` tag: six package jobs build with `cargo-packager` (Velopack on Windows), then one `release` job makes the GitHub Release with the tag's section of `CHANGELOG.md` as its notes and the installers under stable names.
- Until the signing secrets are in the repository, CI's Mac images are ad-hoc. The notarized ones are built on this machine (`scripts/release-mac.sh`) and uploaded over them (`scripts/release-publish.sh`), which is why CI leaves the release a draft.

## The old name

The app was called scribe, and before that there was a Python CLI, daemon and web viewer; both are only in git history. Three things still know the old name, on purpose, to read what was written under it:

- `paths::root` moves `~/.scribe` to `~/.emaki` whole the first time it runs. A failed move starts fresh rather than half of each.
- `paths::relocate_legacy` points an `Attached file:` path a transcript recorded under `~/.scribe` at the moved file.
- `PAGE_SENDER_LEGACY` keeps rows the app sent under the old envelope name rendering as yours.

## The app icon

`scripts/icon/icon.html` is the source, drawn on a canvas: a handscroll (絵巻) seen from the front with a conversation on its sheet, in the Alkaline Software organization icon's colors, the rolls in periwinkle (`docs/design.md`, The icon). It was designed outside this repository; this copy is the app's source.

- `drawIcon(canvas, icon, px, bleed)` draws any pixel size, as the full square (`bleed`) or the rounded app icon with its inset and shadow. The page itself is the proof sheet.
- `scripts/icon/render.swift` loads the page in a `WKWebView` with no window and writes what `exportIcon(size, bleed)` returns, so every size is drawn on its own pixels and none is a scaled copy. `scripts/icon/ico.py` packs the `.ico`; `scripts/make-icon.sh` runs it all on a Mac alone. `docs/release.md` says how to regenerate.
- The PNGs are the rounded icon with its margin, which the Dock, Windows and Linux want. Only the `.icns` is the full square.
- `sys::install_dock_icon` gives a bare `target/debug/Emaki` the Dock icon at startup; a bundle has it from `Emaki.icns`; on Windows `build.rs` compiles `icon.ico` into the executable.
- On a Mac the build in `target/debug` is started as a bundle, `target/debug/dev.noindex/Emaki.app`, opened through the system with a Dock launch's environment (`scripts/dev-app.sh`, which `scripts/relaunch.sh` ends in). A bare executable opened from the Dock or the Finder is run inside the default terminal app, in a window of it, and every Emaki started from a session's shell after that inherits that terminal's marks (`__CFBundleIdentifier`, its ssh agent's socket) and hands them to every Claude Code it starts. `pty::child_env` drops the identifier as well. The bundle is signed with the developer's signing identity when the keychain has one: macOS keeps a folder's permission by who signed the app, and a build signed by nobody is a new app after every build, asked about again at each launch.
- The conversation reads down to about 64px. At 16px the icon is two bars with a pale block between.

Tried and dropped: an icon cut out of a generated picture, which read as a render and could not be changed without generating again; and `draw.py`, an SVG rasterised by Cocoa from one master.

## Self-update

`update.rs` in the core.

- The newest version is the tag `releases/latest` redirects to: no API, no rate limit. The installer is `releases/download/v<version>/<asset>` under the stable names `docs/release.md` fixes, fetched with `ureq` on rustls.
- macOS: the disk image is mounted, `Emaki.app` copied beside the running bundle with `ditto` (signature and attributes kept) and swapped in with two renames. The old process quits, and a shell of its own waiting on that pid opens the new app once it is gone (`open_once_gone`). Opened beside the old one with `open -n`, the new app was a second Emaki to the Dock: an icon of its own at the Dock's end, and the one the person keeps there left without a running app. A bare `target/debug/Emaki` refuses, having no bundle to replace.
- Windows: a copy Velopack installed fetches the newest package Velopack's feed for its architecture names (`releases.win-<arch>.json`, read from `releases/latest/download`, the files and not GitHub's API), starts Velopack's updater, which waits for the app to quit, puts the package in place and opens it. A copy NSIS installed, or a build of one's own, fetches Setup.exe and starts it. Linux replaces the running AppImage (`APPIMAGE`) and restarts it; a `.deb` install has apt install the new `.deb` behind the system's password prompt (`pkexec`), then starts the new binary. Each fetches its own architecture's file.
- `Hub::check_updates` and `install_update` run on threads and report as `HubEvent::Update`. `Workbench::check_updates_daily` runs from the clock tick when `app.check_updates` is on and speaks only when it finds a newer version. `state/update.json` keeps when the last check ran.
- The settings window shows as little as it can: the version, one button that reads *Check for updates* and then *Update to x* in the same place, a *Release notes* link only then, a tick for the daily check, and a detail line only when there is something to say. Tried and dropped: three pills and two sentences of explanation.

## Probing

- `emaki-core update` is the check from a terminal.
- `EMAKI_GO=update:<version>` begins an update; on Windows `EMAKI_UPDATE_FEED` points Velopack at a folder of packages of one's own (`docs/probing.md`).
- `EMAKI_KEYS=rename,caret,up,caret,down,caret` prints the caret's place in the rename field after each key. `EMAKI_KEYS=shift-up` and a forty-line `EMAKI_TYPE` show the vendored composer changes.
