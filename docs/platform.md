# Platform

About `vendor/`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/`,
`scripts/`, `crates/emaki-core/src/update.rs` and
`crates/emaki-app/src/sys.rs`.

## Rules

- Never put a `rev` on our own gpui dependency. gpui-component depends on
  the unpinned zed git source, and a `rev` makes a second copy of gpui it
  does not build against. The pin is in `Cargo.lock`:
  `cargo update -p gpui --precise <rev>` moves it.
- Every change under `vendor/gpui-component/` is marked `(Emaki addition.)`
  in the source and listed in `vendor/gpui-component/UPSTREAM.md`, or it is
  lost at the next move to a newer upstream.
- Everything Unix-only is gated with a stated fallback. No macOS command is
  shelled out from the window; per-OS work goes in `sys.rs`.
- The `.icns` is the full square, opaque to its corners. macOS 26 masks
  every app icon to its own rounded square over a grey backing, and
  anything transparent (a margin, rounder corners) shows as a grey border
  in Finder, the switcher and Spotlight.
- Upload the notarized Mac images after CI's release job, which replaces
  same-named assets.
- Leave no second `Emaki.app` on disk, or Launchpad lists two.
- Only Finder writes a `.DS_Store` Finder honours, so the disk image's
  layout runs through AppleScript on a mounted image.
- A sweep for the old name must use word boundaries: "describe",
  "subscribe" and "transcribe" contain "scribe".

## The vendored toolkit

`vendor/gpui-component/` holds the four gpui-component crates Emaki uses
(`ui`, `base`, `macros`, `assets`), copied from the revision the crate
manifests still name, with upstream's licence and a trimmed workspace
manifest of their own so their `workspace = true` references resolve. The
root `Cargo.toml` excludes `vendor` from the workspace and `[patch]`es the
git source with those paths, so the `git` + `rev` dependencies keep saying
where the code came from while the build uses the copies.

Why: the markdown view hard-coded strong text to weight 700 and gave inline
code the paragraph's face, and gpui's highlight styles carry no family, so
neither could be changed from outside. A fork on GitHub was the
alternative; one repository was preferred.

`vendor/air_r_parser/` is the one crate of Air (Posit's R formatter)
that could not be taken as it is. Air is not on crates.io and is used by
git revision (`emaki-core/Cargo.toml`); its parser pins tree-sitter 0.24
and the window's highlighting is on 0.26, and cargo links one
tree-sitter. The copy is that crate with those two lines changed and
nothing else; its `UPSTREAM.md` says how to move it.

`UPSTREAM.md` lists the eleven changes, each with what upstream did, and
says how to move to a newer revision: copy the crates over, re-apply the
list, build. They cover the markdown view (strong weight, the inline-code
family and its plate), the scrollbar's fade, ranges a textarea can colour
(`set_marks`), and the input's Up, Down, ⇧↑, ⇧↓ and caret following.

## Windows and Linux

What is gated, and its fallback:

- `peer`: the inbox is a Unix socket, so elsewhere `registry` is empty,
  `send` refuses, and the composer falls through to the driver.
- `transcript::file_id`: the inode on Unix, the NTFS file index through
  `winapi-util` on Windows. The archive must tell a rewritten transcript
  from an appended one.
- The `claude` lookup: `std::env::split_paths`, `claude.exe` and the npm
  `claude.cmd` shim on Windows, the installers' directories after `PATH`.
- The `git` lookup (`git::binary`): `git.exe` under Git for Windows'
  folders on Windows; Homebrew's folders and `/usr/bin` elsewhere, the
  Mac's `/usr/bin/git` only with Xcode's tools there.
- `paths::decode_project_dir` knows the `C--Users-...` shape.

Without a Windows machine the closest check is a MinGW type check, which
catches every `cfg` mistake but not a linker one:

```
brew install mingw-w64 && rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_DIR=target/xwingnu CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
  cargo check -p emaki-app -p emaki-core --target x86_64-pc-windows-gnu
```

The MSVC target cannot be checked from a Mac (bundled SQLite wants the
Windows headers) and neither can Linux (fontconfig wants a pkg-config
sysroot). `rust.yml` is the real test for both.

## CI and releases

WORKFLOW.md is the procedure, scripts included. The shape behind it:

- `rust.yml`, every push: the core tests, an app build and the CLI on all
  three OSes, with `Swatinem/rust-cache` because a cold GPUI build is long.
- `release.yml`, on a `v*` tag: four package jobs build with
  `cargo-packager`, then one `release` job makes the GitHub Release with
  the tag's section of `CHANGELOG.md` as its notes and the installers under
  stable names.
- Until the signing secrets are in the repository, CI's Mac images are
  ad-hoc. The notarized ones are built on this machine
  (`scripts/release-mac.sh`) and uploaded over them
  (`scripts/release-publish.sh`), which is why CI leaves the release a
  draft.

## The old name

The app was called scribe, and before that there was a Python CLI, daemon
and web viewer; both are only in git history. Three things still know the
old name, on purpose, to read what was written under it:

- `paths::root` moves `~/.scribe` to `~/.emaki` whole the first time it
  runs. A failed move starts fresh rather than half of each.
- `paths::relocate_legacy` points an `Attached file:` path a transcript
  recorded under `~/.scribe` at the moved file.
- `PAGE_SENDER_LEGACY` keeps rows the app sent under the old envelope name
  rendering as yours.

## The app icon

`scripts/icon/icon.html` is the source, drawn on a canvas: a handscroll
(絵巻) seen from the front with a conversation on its sheet, in the
Alkaline Software organisation icon's colours plus terracotta. It was
designed outside this repository; this copy is the app's source.

- `drawIcon(canvas, icon, px, bleed)` draws any pixel size, as the full
  square (`bleed`) or the rounded app icon with its inset and shadow. The
  page itself is the proof sheet.
- `scripts/icon/render.swift` loads the page in a `WKWebView` with no
  window and writes what `exportIcon(size, bleed)` returns, so every size
  is drawn on its own pixels and none is a scaled copy.
  `scripts/icon/ico.py` packs the `.ico`; `scripts/make-icon.sh` runs it
  all on a Mac alone. WORKFLOW.md says how to regenerate.
- The PNGs are the rounded icon with its margin, which the Dock, Windows
  and Linux want. Only the `.icns` is the full square.
- `sys::install_dock_icon` gives a bare `target/debug/Emaki` the Dock icon
  at startup; a bundle has it from `Emaki.icns`; on Windows `build.rs`
  compiles `icon.ico` into the executable.
- On a Mac the build in `target/debug` is started as a bundle,
  `target/debug/Emaki.app`, opened through the system with a Dock launch's
  environment (`scripts/dev-app.sh`, which `scripts/relaunch.sh` ends in).
  A bare executable opened from the Dock or the Finder is run inside the
  default terminal app, in a window of it, and every Emaki started from a
  session's shell after that inherits that terminal's marks
  (`__CFBundleIdentifier`, its ssh agent's socket) and hands them to every
  Claude Code it starts. `pty::child_env` drops the identifier as well.
- The conversation reads down to about 64px. At 16px the icon is two bars
  with a pale block between.

Tried and dropped: an icon cut out of a generated picture, which read as a
render and could not be changed without generating again; and `draw.py`,
an SVG rasterised by Cocoa from one master.

## Self-update

`update.rs` in the core.

- The newest version is the tag `releases/latest` redirects to: no API, no
  rate limit. The installer is `releases/download/v<version>/<asset>` under
  the stable names WORKFLOW.md fixes, fetched with `ureq` on rustls.
- macOS: the disk image is mounted, `Emaki.app` copied beside the running
  bundle with `ditto` (signature and attributes kept), swapped in with two
  renames and opened with `open -n`; the old process quits. A bare
  `target/debug/Emaki` refuses, having no bundle to replace.
- Windows starts the installer and quits. Linux replaces the running
  AppImage (`APPIMAGE`) and restarts it.
- `Hub::check_updates` and `install_update` run on threads and report as
  `HubEvent::Update`. `Workbench::check_updates_daily` runs from the clock
  tick when `app.check_updates` is on and speaks only when it finds a newer
  version. `state/update.json` keeps when the last check ran.
- The settings panel shows as little as it can: the version, one button
  that reads *Check for updates* and then *Update to x* in the same place,
  a *Release notes* link only then, a tick for the daily check, and a
  detail line only when there is something to say. Tried and dropped:
  three pills and two sentences of explanation.

## Probing

- `emaki-core update` is the check from a terminal.
- `EMAKI_KEYS=rename,caret,up,caret,down,caret` prints the caret's place
  in the rename field after each key. `EMAKI_KEYS=shift-up` and a
  forty-line `EMAKI_TYPE` show the vendored composer changes.
