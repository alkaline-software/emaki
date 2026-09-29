# AGENTS.md

## What this is

**Emaki** — a CLI that mirrors every Claude Code session into markdown and
serves a live web view of it. Python 3 standard library only; the viewer is
vanilla JS with no build step. Nothing here enters a session's context window,
and nothing ever writes to a transcript.

## The one idea

**Copy first, render second.** Claude Code deletes transcripts after
`cleanupPeriodDays` (default 30) with no warning. Every other tool in this space
reads `~/.claude/projects` and stops there, so they all inherit that expiry.
`archive.py` runs before anything else in `poll_session` for exactly this
reason: if rendering throws, the bytes are already safe.

Compaction is *not* a threat — it appends a boundary and keeps writing to the
same file, leaving earlier rows intact. Verified against a real session that
dropped 460k tokens from context and kept all 440 pre-boundary rows.

**The JSONL transcript is the source of truth.** Claude Code writes a complete
record of every session to `~/.claude/projects/<mangled-cwd>/<session>.jsonl`,
and every hook payload carries `transcript_path`. Emaki parses that; hooks
only say *when* to look.

**A session is four files, not one.** Missing any of them makes "fully
reproducible" untrue::

    <project>/<session>.jsonl
    <project>/<session>/subagents/agent-*.jsonl
    <project>/<session>/subagents/agent-*.meta.json
    <project>/<session>/tool-results/<id>.txt

**The archive is a peer source, not a backup.** `index_sessions` unions live and
archived transcripts, so a session Claude Code has deleted stays listed,
searchable, renderable and exportable. It is flagged `archived` and shown as
`kept`.

Everything else follows:

- The model is a **pure function** of the transcript, so the markdown is
  **regenerable**. There is no append-only bookkeeping, no reserved call
  numbers, no supersede-by-appending-a-duplicate, no `flock` between writers.
  Writes are `tmp` + `os.replace`.
- Capture **does not depend on hooks**. If the daemon is down, nothing is lost.
- `emaki build --all` **backfills every session ever run**.

The earlier version of this project (then called scribe) reconstructed
conversations from hook stdin instead, and its README conceded the result was
"not a full audit trail". Do not reintroduce that. If you need something the model lacks, get it from the
transcript.

## The native app (Rust + GPUI)

`crates/` is Emaki as a desktop app, the successor to the Python daemon and
web viewer below. Same ideas, same on-disk layout, no browser:

```
Cargo.toml                 workspace; the zed revision is pinned in Cargo.lock
crates/emaki-core/        everything without a window
  src/model.rs               Session / Round / Item / ToolCall, plus AgentId
  src/adapters/              one per agent: claude.rs, codex.rs; index_all()
  src/transcript.rs          JSONL tail-by-offset, peek(), the cheap index
  src/build.rs               Claude rows -> model, turn_state()
  src/archive.rs             copy-first mirror; runs before anything renders
  src/render_md.rs store.rs  model -> CommonMark on disk
  src/search.rs              FTS5 over every item, ~/.emaki/search.db
  src/driver.rs              a headless `claude -p` child on stream-json
  src/watcher.rs             notify over every agent's data roots
  src/bin/emaki-core.rs     list | render | build | archive | sync | search | bench | peers | inbox | drive
  tests/core.rs
crates/emaki-app/         the window
  src/hub.rs                 threads: scan -> archive -> index, drivers, watcher
  src/workbench.rs           sidebar, session list, board, search, composer
  src/transcript.rs          drawing rounds, tool cards, thoughts, subagents
  src/main.rs                menus, key bindings, the window
  src/sys.rs                 open, reveal, the person's name: per OS
  src/ui_state.rs            ~/.emaki/state/ui.json, what the window remembers
  assets/icon/               the icon in every size, cut from scripts/icon/logo.png
scripts/make-app.sh        Emaki.app bundle for a quick local run, ad-hoc signed
scripts/make-icon.sh       remakes assets/icon from scripts/icon/logo.png
scripts/release-mac.sh     the signed, notarized, Finder-laid-out disk image
scripts/dmg/               the disk image's background and the script that draws it
scripts/release-notes.sh   one version's section of CHANGELOG.md, the release notes
.github/workflows/rust.yml     tests and a build on macOS, Windows, Linux, every push
.github/workflows/release.yml  installers on a v* tag, via cargo-packager
```

```
cargo build -p emaki-app && ./target/debug/Emaki
cargo test -p emaki-core
EMAKI_OPEN=<session-id prefix> ./target/debug/Emaki    # open a session on launch
EMAKI_PAGE=new|sessions|board ./target/debug/Emaki     # land on a page
```

**The board is the web viewer's board.** Same four columns (needs you,
planning, working, your turn) with the same empty-state lines, the same card
(project, branch, title, a status chip with a clock, "since", the one or two
actions that make sense), and done as a strip with a count until opened.
Every card names its agent, because the board mixes them.

**The rest of the window is laid out like the Claude desktop app.** One
collapsible sidebar (⌘⇧S): the wordmark, an accent "New session" entry,
Board, Sessions and Search, then Agents (one row per agent with its count
and a live dot, plus Kept only), Projects and Recents, and an account-style
footer that carries the status line. The content pane has a 48px top strip
with the title centred and actions on the right; when the sidebar is hidden
the strip makes room for the traffic lights. Everything readable sits in one
column of `CONTENT_W` (768px), centred in whatever is left of the window:
the conversation, the sessions page (a serif title, filter pills, rows),
and the home page, which is a serif greeting over the composer with recent
folders as pills beneath. A round in the list is wrapped in an explicit
centring parent, because the list lays each item out on its own and an
auto margin has nothing to push against there. Narrower than `NARROW_W`
(880px) the sidebar leaves the row and comes back only as an overlay over
a scrim (the top-strip button, ⌘⇧S), which any click or Escape puts away;
`sidebar_open` keeps the preference for when the window is wide again. The
window goes down to 560 by 480, where the column runs edge to edge with
its 24px gutters, as the Claude app does. The composer is a floating
card with a round accent send button, mode/model pills (driver channels
only), a "+" that opens the file picker, and a row of attachment chips; it
also takes drops and image pastes. The textarea grows with its content from
three to twelve rows, which also keeps the caret laid out, so the caret
rectangle macOS asks for (dictation, the input-source badge) is real. The hint line under it says which channel
a message would take. Prompts are rounded
surface boxes on the right, attachments above the words (pictures as
tiles, then file chips), replies are plain prose under the agent's mark,
stopping `REPLY_INSET` (40px) short of the prompts' right edge so the two
voices sit at different widths, as in the Claude app.
The palette is `crates/emaki-app/themes/emaki.json`, one config per
appearance, loaded by `install_theme` in `main.rs`: cream and charcoal with a
terracotta accent (`primary`), blue for working, green for your turn. Our
mark (`assets/icons/mark.svg`) is served by `assets.rs`, which wraps the
gpui-component icon set.

**What carried over unchanged.** `~/.emaki` is read and written in the same
layout: `archive/<project>/<session>.jsonl` plus sidecars, `logs/`, the
project registry in `state/projects.json`, `config.json`. An archive made by
the Python Emaki is picked up as-is. The markdown a session renders to is
byte-identical to the Python renderer's except JSON key order inside tool
arguments and the `You (web)` label, now `You (emaki)`.

**Adapters.** This is the shape borrowed from Wake (`iAmCorey/Wake`): an
`Adapter` knows an agent's data roots, lists sessions cheaply, peeks one, and
parses one into the shared model. Claude Code and Codex exist; a new agent is
a new file under `adapters/` and a variant of `AgentId`. Everything above the
adapters (archive, search, the window) never branches on the agent except to
label it.

**Other agents archive under a leading underscore.** Claude Code keeps the flat
`archive/<project>/` layout for compatibility; Codex lives in
`archive/_codex/<project>/`. A project slug is `[a-z0-9-]`, so an underscore
can never collide with one, and `iter_archived(ClaudeCode)` skips those
directories.

**No hooks.** The app has no daemon and installs nothing into Claude Code's
settings. Presence is the registry, a driver of our own when we started the
process, and a ten-minute mtime grace for anything else. A fresh file with
neither is a session that just ended (an interactive Claude Code always has
an inbox), so the composer offers to resume it.

**Codex has no stop reason.** Its phase is derived from the built model's tail
(`adapters::turn_state_from_session`), not from the rows.

**One copy of gpui.** gpui-component depends on the unpinned zed git source. A
`rev` on our own gpui dependency produces a second copy that gpui-component
does not build against. The pin lives in `Cargo.lock` instead:
`cargo update -p gpui --precise <rev>` moves it.

**The search database is separate.** `search.db`, not the Python daemon's
`index.db`; the two schemas differ and each would rebuild on the other's
version number.

**Two channels, checked in this order.** `Workbench::reply_via_for` answers
`driver` when a headless child of ours is behind the session, `inbox` when
Claude Code's registry (`~/.claude/sessions/<pid>.json`, read by
`emaki_core::peer`) shows a terminal session with an inbox, `spawn` when
neither exists and the folder is still there. The driver is checked first
because its child registers an inbox of its own. `hub.is_live` trusts the
driver and the registry before the mtime grace. A message on the inbox
channel goes down the same wire the Python `peer.py` used, one connection
per message, wrapped in Claude Code's `<cross-session-message>` envelope with
no permission mode asserted; `emaki-core peers` lists what the registry
sees and `emaki-core inbox <id> <text>` delivers by hand.

**Never splice a list item the reader may be inside.** gpui's `ListState`
moves the scroll anchor to the start of any spliced range that contains it.
A live session's last round is one tall item, so re-splicing it on every
reload sent the reader from wherever they had scrolled to the top of that
round, the middle of the conversation. Items render from the current
session on every frame and are re-measured as they render, so `set_detail`
only tells the list about a change in count (append new rounds, reset on a
rewrite), and toggling a tool, thought, run or thumbnail just notifies.

**A Read opens on what came back.** Its subject names the file, and for a
long time that was all a successful Read showed: the contents live on
disk. That left a Read of a picture with a dimmed chevron and nothing to
unfold. Now `tool_has_body` says yes whenever a result came back: text
shows clipped like any output, in the file's language; a picture is read
back out of the transcript the way a pasted one is (`ToolCall::result_uuid`
and `result_index` name the result row and block, and
`transcript::image_block_bytes` looks inside a `tool_result` block for its
first image) and drawn as a tile that opens the lightbox.

**Runs of tool calls fold.** Three or more consecutive tool calls (thoughts
between them included) draw as one row in `render_run`: the count, a tally
by kind, the last subject, the total time, and the agent's turning mark with
"running…" while one is still going. Opening the row shows every call, each
folding on its own. Fewer than three stay inline.

**The agent's mark moves while it works.** `agent_glyph` turns Claude's mark
and breathes Codex's whenever `is_working` (board column working or
planning) holds, in the round header, the status row under the transcript,
the top bar, recents, the sessions page and board cards. Each place passes
its own animation id.

**The focused text field must be visible to assistive apps.** Two gaps
stood between the composer and a dictation app, both found with the AX
probe in the session scratchpad (`AXFocusedUIElement` of the app answered
`AXWindow`). First, gpui-component's input frame tracks a focus handle of
its own and only asks whether it *contains* the focus, so gpui never marks
the editor as the focused accessibility node; the composer wrapper in
`render_composer` tracks the editor's real handle with a
`MultilineTextInput` role and the typed value. Second, AppKit resolves the
application's focused element through the key window, and gpui's window
class never forwards `accessibilityFocusedUIElement` to its content view
(the same gap gpui-component patches for `accessibilityHitTest:`);
`a11y.rs` adds that forwarder to the window class at open. `EMAKI_A11Y=1`
prints gpui's own view of the tree on every draw. With both, the focused
element is an `AXTextArea` carrying the text.

**Attachments show as what they are, whole.** In the composer a picture is
a thumbnail from its file; anything else gets a typed icon
(`assets/icons/file-*.svg`, chosen by `assets::file_icon_path`) with name
and size. A thumbnail is never a crop: `image_dims` reads the pixel size
out of the PNG, JPEG, GIF or WebP header (`file_image_dims` for a file,
kept per session in `Detail::sizes`; for a pasted block, alongside the
bytes in `Thumb`; for a composer chip, in `Attachment::size` at attach
time), and `fit_thumb` gives the tile the picture's own shape inside a
maximum box, drawn with `ObjectFit::Contain`. Only a picture whose header
says nothing gets a fixed box, letterboxed. In a transcript, `render_attachment` draws a pasted image from the
kept file when the prompt names one, or reads the block back out of the
JSONL (`transcript::image_block_bytes`, cached per session in
`Detail::thumbs`, loaded off the main thread) when it does not.

**The conversation is set in Anthropic Serif, or Anthropic Sans.** Those
are the Claude desktop app's own fonts and not ours to ship, so `fonts.rs`
loads them at start from a Claude app installed on this machine (its
`Resources/fonts` on macOS, the Squirrel install under `LOCALAPPDATA` on
Windows) through `text_system().add_fonts`, and resolves the family names
the text system reports. Without a Claude app the serif falls back to
Georgia and the sans to the window's face, and the settings panel says so.
The face is applied to the transcript container only (`render_detail`),
which the markdown view inherits; code stays in the mono face, the chrome
and the composer stay in the UI face. The choice is `app.chat_font` in
`config.json` (`serif`, the default, or `sans`), changed from the settings
panel (⌘, on macOS, Ctrl+, and Win+, elsewhere, or the app menu) through
`Config::edit`, which rewrites the file without the environment overrides
`Config::load` applies. `driver.claude_path` in the same file names the
`claude` binary when `PATH` does not.

**The composer is plain text, on purpose.** It is a growing `TextareaState`
(three to twelve rows) with no markdown rendering of its own; markdown in a
message renders once sent, like any prompt. Live rendering was tried three
ways (a preview block beside the text, a highlighted editor mode, a
block-by-block editor, then MiniNotes' CodeMirror editor in a webview) and
dropped: the toolkit has no editable rich-text view, and a native webview
draws above every gpui overlay. Do not bring it back without a real
in-toolkit rich editor. What the composer does keep: attachments by paste
(`paste_attachments` captures the input's own `Paste`), by drop and by the
"+" picker, thumbnails and typed icons on the chips, and the accessibility
wrapper below.

**Own inputs get a focus wrapper.** The toolkit's input frame tracks a
focus handle of its own, so focusing an `InputState` from code (⌘K) lands
on a handle with no dispatch node: no key bindings above it, nothing for
assistive apps. Every input the window owns (the composer, the search field) sits in a
`div` that `track_focus`es the state's handle and carries the text role. Probing note: when driving the
window from a script, send real key codes (System Events `key code` or a
`CGEvent` with the right virtual key); a unicode string on virtual key 0
reaches the composer but not a single-line input.

**A swallowed click must end the text drag.** gpui-component's window
selection layer begins a drag on every left mouse-down, anywhere, and ends
it on the bubble-phase mouse-up. A click handler that calls
`stop_propagation` eats that mouse-up, the layer stays in its drag, and
every later mouse move extends a selection nobody is making (that was
"moving the cursor highlights text after opening a preview"). Every click
the window swallows goes through `swallow_click`, which calls
`gpui_base::TextSelection::end` before stopping propagation; `gpui-base` is
a direct dependency for that one call. The lightbox backdrop is also
`occlude`d so nothing beneath it hears the mouse.

**Attachments open on click.** A picture opens in the lightbox over the
window (Escape or a click outside closes it, with Open and Reveal in Finder
for a file); any other file opens with the app the system keeps for it. Both
the transcript tiles and the composer chips go through
`preview_attachment`.

**Attachments are files first.** A pasted image is written under
`~/.emaki/uploads/<session>/` before anything else happens; dropped or
picked files stay where they are. `fold_attachments` turns them into content
blocks for a driver (images only) and into `Attached file: <path>` lines for
everything else, so the inbox channel, which the TUI reads as prose, still
gets the path. The paste hook is a `capture_action` for the input's own
`Paste` on the composer wrapper: it takes images and file lists off the
clipboard and lets text through to the textarea.

**Windows and Linux are build targets, not afterthoughts.** Everything
Unix-only is gated, with a stated fallback: `peer` (the inbox is a Unix
socket, so `registry` is empty and `send` refuses elsewhere, and the
composer falls through to the driver); `transcript::file_id` (the inode on
Unix, the NTFS file index through `winapi-util` on Windows, because the
archive must tell a rewritten transcript from an appended one); the
`claude` lookup (`std::env::split_paths`, `claude.exe` and the npm
`claude.cmd` shim on Windows, the installers' directories after `PATH`);
and `sys.rs` for opening a file, revealing it and the person's name, so no
macOS command is shelled out from the window. `paths::decode_project_dir`
knows the `C--Users-...` shape. Without a Windows machine, the closest
check is a MinGW type check, which catches every `cfg` mistake but not a
linker one:

```
brew install mingw-w64 && rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_DIR=target/xwingnu CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
  cargo check -p emaki-app -p emaki-core --target x86_64-pc-windows-gnu
```

The MSVC target cannot be checked from a Mac (bundled SQLite wants the
Windows headers) and neither can Linux (fontconfig wants a pkg-config
sysroot); `rust.yml` is the real test for both.

**Tests on every push, installers on tags.** `rust.yml` runs the core
tests, builds the app and runs the CLI on all three OSes, with
`Swatinem/rust-cache` because a cold GPUI build is long. `release.yml`
runs on a `v*` tag: it refuses a tag that disagrees with the workspace
version or has no section in `CHANGELOG.md`, builds per target (both Mac
architectures from the Apple Silicon runner), packages with
`cargo-packager` (config under `[package.metadata.packager]` in
`crates/emaki-app/Cargo.toml`), and then one `release` job makes the
GitHub Release with the tag's changelog section as its notes
(`scripts/release-notes.sh`) and everything attached under stable names
(`Emaki-mac-arm64.dmg`, `Emaki-mac-x64.dmg`,
`Emaki-windows-x64-setup.exe`, `Emaki-linux-x64.AppImage`,
`Emaki-linux-x64.deb`) so links to `releases/latest/download/<name>` never
go stale. The changelog is written for the person installing; the first
release taught that four package jobs each creating the release leaves
four auto-generated changelogs on the page and nothing a user can read.

**The Mac build is signed the way Pingfan's other apps are**, by
`scripts/release-mac.sh`: a Developer ID signature with the hardened
runtime and a secure timestamp, and with `--notarize` notarization,
stapling and validation (the keychain profile `notarytool-profile` on a
laptop; `APPLE_ID`, `APPLE_APP_PASSWORD` and `APPLE_TEAM_ID` in CI). The
disk image is laid out by Finder over `scripts/dmg/background.png` ("Drag
Emaki to Applications", the app's cream and terracotta, drawn by
`scripts/dmg/background.py` with Pillow and LXGW WenKai Medium, as
MiniNotes' is): a read-write image is mounted, an AppleScript sets the
660x420 window, the 128pt icons at 165 and 495, and the picture, and
`hdiutil convert` makes the compressed read-only image from it. Only
Finder writes a `.DS_Store` Finder honours, which is why the layout is not
generated offline. In CI the identity comes from the
`MACOS_CERTIFICATE_P12` and `MACOS_CERTIFICATE_PASSWORD` secrets; without
them the bundle is ad-hoc signed and the docs' "Open Anyway" steps apply,
so a fork still builds. Until those secrets are in the repository the Mac
images ship from the laptop: after the `release` job has made the release,

```
scripts/release-mac.sh --notarize
scripts/release-mac.sh --notarize --target x86_64-apple-darwin
gh release upload v0.1.0 dist/Emaki-mac-arm64.dmg dist/Emaki-mac-x64.dmg --clobber
```

replaces the ad-hoc images with signed ones under the same names. Upload
after the job, not before: `action-gh-release` replaces same-named assets.
Windows is unsigned until SignPath. The script deletes `dist/Emaki.app`
once it is inside the image: left there, Launchpad and Spotlight list a
second Emaki beside the installed one (`make-app.sh` keeps its bundle,
being for a local run). The packager does not run the build
(its `beforePackagingCommand` cannot see `--target`), so build first:
`cargo build --release -p emaki-app`, then `scripts/release-mac.sh
--no-build [--notarize]`, or on other platforms `cargo packager --release
-p emaki-app --formats <fmt>`. To redo a release under the same tag:
delete the release and the tag on both sides, tag again, push the tag.

**Everything is Emaki now, and the old name is only history.** The rename
on 2026-09-28 reached the crates (`emaki-core`, `emaki-app`), the Python
package and CLI (`emaki/`, `bin/emaki`, `bin/emaki-hook`), the environment
variables (`EMAKI_HOME`, `EMAKI_OPEN`, `EMAKI_PAGE`, `EMAKI_A11Y`,
`EMAKI_TIMING`, `EMAKI_CLAUDE`, `EMAKI_DISABLE`, `EMAKI_DRIVER_MODE`,
`EMAKI_DRIVER_MODEL`), the data directory and the envelope's sender name.
Three things still know the old name, on purpose: `paths::root` moves
`~/.scribe` to `~/.emaki` whole the first time it runs (both the Rust and
the Python side; a failed move starts fresh rather than half of each);
`paths::relocate_legacy` points an `Attached file:` path a transcript
recorded under `~/.scribe` at the moved file; and `PAGE_SENDER_LEGACY`
keeps rows the app sent under the old envelope name rendering as yours.
The word "scribe" inside "describe", "subscribe" and "transcribe" was left
alone, so a future sweep must use word boundaries too. Outside the
repository: Claude Code's hooks in `~/.claude/settings.json` name the hook
script by path and had to be repointed at `bin/emaki-hook` (`emaki
install` rewrites them), an old `scribe serve` daemon kept recreating
`~/.scribe` until it was stopped, and the `uv tool` named `scribe` and
the checkout folder itself are the person's to rename.
`sys::install_dock_icon` gives a bare `target/debug/Emaki` the Dock icon at
startup; a bundle has it from `Emaki.icns`; on Windows `build.rs` compiles
`icon.ico` into the executable.

**The icon is JP's logo, cut out.** `scripts/icon/logo.png` is the picture
JP generated: a terracotta plate on a white ground, a cream scroll curling
toward the viewer at the top-left and bottom-left and still rolled on the
right, a dark terminal panel on the open sheet with a `>_` prompt and grey,
blue and green lines. A drawn copy of it was tried twice
(`@napi-rs/canvas`) and never matched the original's curls, so the picture
itself is the source now. `scripts/icon/cut.js` finds the plate as
everything that is not white, makes the rest transparent, un-blends the
anti-aliased edge from the white it was drawn on, fits the plate to 824 of
a 1024 canvas (the rounded square on Apple's icon grid, so the Dock shows
it at every other icon's size) and shrinks that by area averaging to every
smaller size, writing the `.ico` too; `scripts/make-icon.sh` runs it and
makes the `.icns` with `iconutil`. The `.icns` alone is the plate filling
the whole canvas and opaque to its corners: macOS 26 masks every app icon
to its own rounded square over a grey backing, so anything transparent (a
margin, the plate's own rounder corners, its soft edge) shows as a grey
border in Finder, the switcher and Spotlight. That was the grey border
around the icon in the disk image, and a thin rim of it stayed until the
corners were filled by extending the plate's edge colour outward; the
system's mask cuts them to its shape. The PNGs keep the grid, because the
Dock draws the PNG the app sets at start (`sys::install_dock_icon`) as it
is, and Windows and Linux want the margin. Replace the logo, run the script,
rebuild (the Dock icon is `include_bytes!`), commit the assets.

**The window remembers itself.** `ui_state.rs` keeps
`~/.emaki/state/ui.json`: bounds, sidebar, page, the open tabs, the active
one and where each was scrolled (`ListState::logical_scroll_top`, put back
with `scroll_to` when the tab is opened again). It is written when any of
that changes, bounds changes at most every two seconds, and again at quit;
the bounds are reused only when their centre is still on a screen.
`EMAKI_PAGE` and `EMAKI_OPEN` still win over it. Tabs are keys in
`Workbench::tabs`; there is one `Detail` at a time and switching tabs
reloads from disk, which the numbers below say costs nothing a person can
see. ⌘W is one global `CloseTab` binding that closes the showing tab, then
the last tab, then the window: a context-bound binding would lose to a
global one whenever the focus sits in the composer. The app stays running
with no window, and `on_reopen` in `main.rs` (a Dock click, or a second
launch) opens it again; without that handler the Dock icon did nothing.

**Opening is measured, not guessed.** `emaki-core bench [<id>...]` times
read, build and render for a transcript (the five largest by default) and
`EMAKI_TIMING=1` makes the app print load and hand-over time per open.
The largest transcript on the machine this was built on (137 MB) reads in
about 80 ms and builds in about 80 ms more; the app opens a 66 MB session
in under 100 ms end to end. A tail-first reader was planned and dropped on
those numbers; bring it back only if bench shows a session over about half
a second.

**The driver is the conversation.** Messages typed in the window go to a
`claude -p --input-format stream-json` child kept alive between turns and
closed after `driver.idle_min`. `--resume <id>` appends to the same transcript,
so the page shows the reply the same way it shows a terminal turn: by reading
the file. Permission prompts arrive as `can_use_tool` and are answered from a
card; silence for ten minutes is a deny. Everything in the Python driver's
docstring about the wire still holds.

## Layout

```
bin/emaki            CLI entry
bin/emaki-hook       hook entry — tiny on purpose, runs on every tool call
emaki/
  sockpath.py           where the control socket lives (leaf module, no imports)
  paths.py  config.py   ~/.emaki layout, settings
  transcript.py         JSONL tail-by-offset + the cheap session index
  model.py              Session / Round / Text / Thinking / ToolCall / Notice
  build.py              rows -> model. THE builder.
  render_md.py          model -> CommonMark
  render_json.py        model -> viewer payload
  redact.py             secret scrubbing, applied to both renderers
  store.py              where a log lives and what goes in it
  daemon.py             HTTP + SSE + watcher + control socket
  control.py            approval holds and the reply queue (no transport)
  peer.py               messages into a session: the inbox socket
  driver.py             a headless Claude Code child the page drives (stream-json)
  catalog.py            what `/` completes to: skills and commands, disk + Claude
  search.py             FTS5 index over every session, incremental
  explain.py            Haiku explainer, content-addressed cache
  install.py            writing hooks into settings.json
  export.py  replay.py
  viewer/              index.html styles.css app.js rail.js theme-boot.js
                       compose.js (DOM-free composer rules) marked.min.js (vendored, MIT)
tests/   test_*.py  test_rail.mjs  test_compose.mjs  helpers.py  fake_claude.py
```

## Invariants

- **Standard library only**, in every module. The one vendored file is
  `marked.min.js` (MIT).
- **The builder is total.** Unknown row types are skipped, never raised on. New
  ones appear in Claude Code releases; a logger that crashes on one is worse
  than useless.
- **The hook never blocks and never fails.** It reads stdin, talks to the daemon
  over a Unix socket, prints the reply, exits 0. All logic lives in
  `daemon.dispatch`, so hook behaviour changes without reinstalling. The single
  exception is `PermissionRequest`, which is *designed* to be held.
- **One builder, two renderers.** If markdown and the viewer disagree, the bug
  is that something bypassed `build.py`.
- **Explanations are the only thing that spends tokens.** Cheap calls are
  answered from their own arguments; answers are cached by content hash; the
  model call happens on a thread, never in the hook's path.

## Things that bite

**Never feed the archive its own file as a source.** Once a session outlives its
original it re-enters the index pointing at the archived copy. Without the
guards in `mirror_file` / `archive_ref`, the rotate-then-copy path renames the
destination aside and then fails to read the source it just moved — losing the
canonical file and rotating again on every sweep. Two independent checks exist
because the failure is silent and permanent.

**Subagent rows are all `isSidechain: true`.** They are a sidechain *of the
parent*, but the whole conversation at their own level, so a nested `build()`
must be called with `nested=True` or it filters out every row and produces zero
rounds. Both the file-based and inline paths hit this.

**Subagents link back explicitly.** `agent-*.meta.json` carries `toolUseId`,
naming the exact `Task` call that spawned it. Use that, not contiguous-run
guessing — the latter cannot tell two parallel subagents apart.

**`AF_UNIX` paths are capped at ~104 bytes** regardless of filesystem limits.
`sockpath.py` exists because of this and because the daemon and the hook must
agree on the answer — a disagreement silently disables every hook.

**The explainer child is a real Claude Code session** and gets a transcript of
its own. It runs with `cwd` inside `~/.emaki/run/explain` precisely so
`transcript.index_sessions` can recognise and drop those. It also sets
`EMAKI_DISABLE=1` and `--setting-sources ""` so it cannot fire our hooks.
Do **not** switch it to `--bare`: that reads auth only from `ANTHROPIC_API_KEY`,
never the keychain, which breaks subscription users.

**`ai-title` is not always a title.** When a session runs under a named agent,
Claude Code overwrites that field with the *agent's name*. `transcript.pick_title`
takes the newest title that is neither a known `agentName` nor slug-shaped.

**A rewritten transcript must replace, not append.** `--resume` and compaction
rewrite in place. `TranscriptTail.restarted` signals it and `LiveSession.poll`
drops its accumulated rows; without that the session doubles.

**Cache-read tokens are not a total.** Every assistant message re-reads the whole
cached prefix. `Usage.total` deliberately excludes `cache_read`.

**`system` rows with `subtype: local_command`** carry the same XML-ish wrappers
as user rows (`<command-name>`, `<local-command-stdout>`). Route them through
`strip_wrappers` or raw markup lands in the log.

**Raw HTML in a transcript is content, not markup.** Someone writing
"maybe the `<aside>` option" means those characters; marked treats it as an HTML
block and swallows the paragraph. The viewer overrides marked's `html` renderer
to escape.

**`.chip.note` and the rail's note element must not share a class.** They did
once, and the chips inherited `position: absolute`. The rail element is
`.rail-note`.

## Messages from the page

`peer.py` puts a message typed in the viewer in front of a session. Claude
Code 2.1 registers every session in `~/.claude/sessions/<pid>.json` with a
`messagingSocketPath`, and publishes the token a peer must present in
`<pid>.<hash>.key` beside it. The wire is two JSON lines on that socket: an
`auth` frame, then a `user` frame whose content is wrapped in Claude Code's own
`<cross-session-message from-name="emaki">` envelope. The envelope is what
makes the transcript row carry `origin.name` and a clean `origin.body`;
`build.peer_message` keys on those, so a page message renders as `source: web`
and one from another Claude session as `peer`, with no prose parsing unless
`origin.body` is missing. Those rows are `isMeta: true`; the builder and
`turn_state` must treat a peer row as a prompt or the log shows a reply to
nothing.

`Hub.reply_via` decides the channel per session and the viewer only shows it:
`driver` when a headless child of ours is behind it, `inbox` when the registry
has the session, `queue` (Stop hook, opt-in) for a live process without one,
`spawn` when there is no process (the first message starts a driver). The
driver is checked before the inbox because its child registers an inbox of its
own. `is_live` trusts the driver and the registry before any hook, so a session
with either is never "done". `head.caps` says what the composer may offer on
that channel (attachments as blocks or paths, whether mode and model can be
set, whether there is something to interrupt); the page never guesses.

**Do not assert `from-mode`.** Claude Code holds a message that asserts no
permission mode when the recipient runs with permissions bypassed, and asks in
the terminal. That check is what stops a less trusted process steering a more
trusted session. Emaki is such a process as far as Claude Code can tell; the
user's remedy is `crossSessionInbound: accept` in their own settings.

**`claude --bg --resume` forks.** It starts a copy under a new id. Only
`claude -p --resume <id>` appends to the same transcript, so that is what a
finished session gets. `peer.child_env` strips every `CLAUDE*` variable except
`CLAUDE_CONFIG_DIR`: the daemon is usually a grandchild of a session and would
otherwise hand the child its parent's id, inbox and token. When the child exits
the session is added to `ended`, or its fresh mtime would count as a process
for ten minutes and the composer would hide.

## The driver

`driver.py` keeps one `claude -p --input-format stream-json --output-format
stream-json --permission-prompt-tool stdio` child per driven session, alive
between turns, closed after `driver.idle_min`. Its docstring records what
2.1.272 actually does on that wire; the short version:

- **`--permission-prompt-tool stdio` or no prompts.** With the default flags a
  permission prompt is answered by a local deny and never reaches the host.
  With it, `can_use_tool` arrives as a `control_request`, and the daemon
  answers it from the same `control.PendingCall` hold the hook path uses. The
  `PermissionRequest` hook returns `{}` for a driven session so there is one
  card per call, and silence is a deny because there is no terminal to fall
  back to.
- **`system/init` comes with every turn**, not at startup; `initialize` is
  what returns the command catalogue with descriptions.
- **A mode switch writes no `permission-mode` row**; the next user row carries
  `permissionMode`. `turn_state` reads both.
- **Never `--bare`** (keychain auth). Hooks stay on: the child's own
  `SessionStart`/`SessionEnd` feed presence like any session.
- **Two writers.** If the registry shows a terminal process for a driven
  session, `reap_drivers` stops the child once idle and the channel flips to
  `inbox`.

**A session started from the page has no file for a second or two.**
`Hub.drafts` holds it meanwhile: `index_payload` prepends a draft card and
`snapshot` answers with a draft head, both flagged `draft`. The page shows
"starting" and reloads when the `sessions` event carries the id without the
flag; the draft is dropped the moment the index sees the file. Uploads made
on the new-session view live under `uploads/new/` and are moved under the id
by `rehome_uploads` before the first message goes out.

**The slash catalogue has two halves.** Disk (`catalog.scan`: user, project
and plugin skills/commands, front matter read without YAML) knows scope; only
a driver's `initialize` answer knows the bundled skills and built-ins, with
descriptions, so `catalog.remember` keeps the last one under
`~/.emaki/cache/commands.json` for sessions with no driver. `available`
decides per channel: a built-in on an inbox session is refused on the page
with the reason, because the TUI reads a cross-session message as prose.

**Uploads are files first.** `Hub.save_upload` writes to
`~/.emaki/uploads/<session>/<id>-<name>` and the id is resolved by scanning
that directory, so a daemon restart forgets nothing. The browser's
`Content-Type` is recorded but the `image` flag comes from the bytes
(`sniff_image`): only a real PNG/JPEG/GIF/WebP becomes a content block. The
page shows thumbnails through `blob:` URLs, which is why `img-src` allows
`blob:`.

**A fresh mtime is not a reason to hide the composer.** `presence_kind` ranks
what says a process exists (driver, inbox, hook, mtime). The board keeps
treating a recent file as live, but `reply_via` offers `spawn` when the only
evidence is the mtime: an interactive Claude Code always has an inbox, so a
recent file with none is a session that just ended.

`tests/fake_claude.py` speaks the same wire and writes real transcript rows,
so `test_driver.py` and `TestDriverDelivery` run without Claude. Point
`EMAKI_CLAUDE` at any binary to drive something else.

## The rail

`emaki/viewer/rail.js` minimises total squared displacement from each note's anchor,
subject to no overlap. Substituting `x[i] = top[i] - cumulativeOffset[i]` turns
the ordering constraint into "x is non-decreasing", making it isotonic
regression, solved exactly by pool-adjacent-violators in O(n).

A naive forward pass (`top = max(anchor, prevBottom + gap)`) is a ratchet: notes
only move down, so one tall note pushes every note below it permanently off its
anchor. That was the previous viewer's bug.

Focus is a large weight on one note, then a pass that pins it exactly and pushes
the two sides apart — which is the "snap together" interaction, out of the same
solver. The module is DOM-free so `tests/test_rail.mjs` can drive it directly,
including a brute-force optimality check.

## The board

`#/board` shows live sessions as cards in columns: *needs you*, *planning*,
*working*, *your turn*, and a collapsed *done* strip for everything with no
process behind it. The column is `build.turn_state(rows)`, a function of the
transcript's tail like everything else: `stop_reason: end_turn` is your turn,
a trailing `tool_use` is working, a trailing `AskUserQuestion` or
`ExitPlanMode` needs you, the latest `permission-mode` row says whether
working is planning. `peek` computes it for the index from the same tail slice
it reads the title from; `LiveSession.rebuild` computes it from the whole file
so the elapsed clock can find the prompt behind a megabyte of tool output.

The daemon adds only what a file cannot know, in `Hub.card_for`: whether a
process is alive (`presence`, fed by every hook and cleared by `SessionEnd`,
with a ten-minute mtime grace for machines without hooks), whether an approval
is being held here, and the last `Notification` type (`permission_prompt`
means a dialog is up in the terminal; `idle_prompt` means Claude has been
waiting). Those refine the phase; they never replace it. Cards travel on the
`sessions` and `card` SSE events, which every stream receives, so the board
subscribes with no session id.

The board is redrawn whole on every change. Unlike the conversation column a
card holds no state worth preserving, so the reconcile-in-place rule below
does not apply to it.

`peek` results are cached on (size, mtime). The index is rescanned every four
seconds, and without the cache every rescan re-read the tail of every
transcript on the machine.

## Viewer state

**A restarted daemon reloads the page.** The SSE `hello` frame carries
`asset_stamp()`, a hash of the viewer files computed once per process. The
page keeps the first stamp it sees and reloads when a reconnect brings a
different one, so a restart on new code never leaves yesterday's script
reading today's payloads.


Every round and item carries a server-assigned `key` (`_key_round` in
`daemon.py`; tool calls key on their id, everything else on position — safe
because transcripts are append-only). An update replaces only the nodes whose
payload changed, so open `<details>`, scroll position, and selection survive
because they are never touched. **Never re-render `#column` wholesale.**

## Security

The daemon binds loopback, rejects foreign `Host` headers (DNS rebinding), and
serves `default-src 'none'; script-src 'self'` — which is why the theme
bootstrap is a file rather than an inline script. Transcript content is
sanitised in the DOM as well. All of this matters because the page shares an
origin with an API that can approve tool calls.

## Testing

```
python3 -m unittest discover -s tests -t tests
node tests/test_rail.mjs
node tests/test_compose.mjs
```

`tests/helpers.py::Isolated` redirects `EMAKI_HOME` and `CLAUDE_CONFIG_DIR`
to temp dirs; inherit from it for anything touching disk. The hook tests run the
real `bin/emaki-hook` executable against a real socket, because the contract
worth testing is what Claude Code actually sees on stdout and how long it waits.

Both suites must pass before committing.

## The home page

`#/` is the board. There is no landing page of tiles and charts; the
question the page answers on arrival is "what needs me", and the board's
columns answer it. `#/new` is the one other page: a folder picker over the
composer, from which a driver starts a fresh session.

`search.py` still fills `session_stats` in the same incremental pass that
builds the FTS index, and `overview()` sums it for a range; nothing in the
viewer reads it now, but it is one blob per session and costs the index
nothing to keep.

## Search

`search.py` keeps an FTS5 index at `~/.emaki/index.db`, one document per
*item* — a prompt, a reply paragraph, a thought, a tool call. Coarser and a hit
in a long session tells you nothing about where to look; finer and snippets lose
context. Sync is incremental on (size, mtime) and runs on a worker thread, never
on the request path.

`fts_query()` quotes every term. Users type `rm -rf`, `a:b`, a lone `"` — all
FTS5 syntax that would otherwise raise mid-keystroke. A trailing `*` survives as
a prefix match.

Snippets are delimited with `\x02`/`\x03` rather than markup, so a hit can
never carry HTML out of a transcript and into the page.

A "hit" is a matching document, not a term occurrence. Two mentions in one
prompt are one hit.
