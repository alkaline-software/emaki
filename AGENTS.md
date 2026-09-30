# AGENTS.md

## What this is

**Emaki** is a desktop app (Rust + GPUI, `crates/`) that keeps every
coding-agent session on this machine byte for byte, renders each to markdown,
searches all of them, and lets you continue a Claude Code session from the
window. Nothing here enters a session's context window, and nothing ever
writes to a transcript. It grew out of a Python CLI and web daemon, removed
on 2026-09-29; git before that date has them.

## The one idea

**Copy first, render second.** Claude Code deletes transcripts after
`cleanupPeriodDays` (default 30) with no warning. Every other tool in this space
reads `~/.claude/projects` and stops there, so they all inherit that expiry.
`archive::sweep` runs before anything else touches a session for exactly this
reason: if rendering fails, the bytes are already safe.

Compaction is *not* a threat — it appends a boundary and keeps writing to the
same file, leaving earlier rows intact. Verified against a real session that
dropped 460k tokens from context and kept all 440 pre-boundary rows.

**The JSONL transcript is the source of truth.** Claude Code writes a complete
record of every session to `~/.claude/projects/<mangled-cwd>/<session>.jsonl`.
Emaki parses that; a file watcher and a rescan only say *when* to look.

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
- Capture **does not depend on the app running**. Every launch backfills
  every session ever run.

The first version of this project (then called scribe) reconstructed
conversations from Claude Code hook stdin instead, and its README conceded
the result was "not a full audit trail". Do not reintroduce that. If you need
something the model lacks, get it from the transcript.

## The native app (Rust + GPUI)

`crates/` is all of Emaki. Same ideas and on-disk layout as the Python
daemon it replaced, no browser, no daemon:

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
  src/explain.rs             opaque tool calls in plain words, via `claude -p`
  src/update.rs              the newest release, its installer, and putting it in place
  src/statusline.rs          scripts/statusline.sh built in, installed to ~/.emaki/bin at launch, and the one setting
  src/terminal.rs            the agent's resume command as a script a terminal can be handed
  src/watcher.rs             notify over every agent's data roots
  src/bin/emaki-core.rs     list | render | build | archive | sync | search | bench | peers | inbox | explain | update | statusline | drive
  tests/core.rs
crates/emaki-app/         the window
  src/hub.rs                 threads: scan -> archive -> index, drivers, watcher
  src/workbench.rs           sidebar, session list, board, search, composer
  src/transcript.rs          drawing rounds, tool cards, thoughts, subagents
  src/main.rs                menus, key bindings, the window
  src/sys.rs                 open, reveal, open in a terminal, the person's name: per OS
  src/ui_state.rs            ~/.emaki/state/ui.json, what the window remembers
  assets/icon/               the icon in every size, cut from scripts/icon/logo.png
scripts/make-app.sh        Emaki.app bundle for a quick local run, ad-hoc signed
scripts/make-icon.sh       remakes assets/icon from scripts/icon/logo.png
scripts/release-mac.sh     the signed, notarized, Finder-laid-out disk image
scripts/dmg/               the disk image's background and the script that draws it
scripts/release-notes.sh   one version's section of CHANGELOG.md, the release notes
scripts/statusline.sh      Claude Code's status line, ours: prints the line, leaves the rate limits
WORKFLOW.md                how to cut a release, step by step
CHANGELOG.md               one section per release; the release job reads it
.github/workflows/rust.yml     tests and a build on macOS, Windows, Linux, every push
.github/workflows/release.yml  installers on a v* tag, via cargo-packager
```

```
cargo build -p emaki-app && ./target/debug/Emaki
cargo test -p emaki-core
EMAKI_OPEN=<session-id prefix> ./target/debug/Emaki    # open a session on launch
EMAKI_PAGE=new|sessions|board ./target/debug/Emaki     # land on a page
EMAKI_FIND=<text> ./target/debug/Emaki                 # open the find bar on that query
EMAKI_SETTINGS=1 ./target/debug/Emaki                  # open the settings panel
```

The last two exist for probing: a terminal without accessibility access
cannot press ⌘F or ⌘, in the window from a script, so a screenshot of
either state is one launch away. Point `EMAKI_HOME` at a scratch directory
to run a second copy beside the installed app without sharing its state.

**One Emaki at a time.** Before launching a build, quit the one running:
`pkill -x Emaki` stops both a bare `target/debug/Emaki` and the installed
`/Applications/Emaki.app`. Two copies share `~/.emaki` and both write
`state/ui.json`, the Dock shows two identical icons, and the one the
person looks at is usually the old build, so the change "is not there".
Relaunch, then check `pgrep -fl Emaki` lists one process.

**The board is the home of what needs you.** Four columns (needs you,
planning, working, your turn), a card per live session (project, branch,
title, a status chip with a clock, "since", the one or two actions that make
sense), and done as a strip with a count until opened. Every card names its
agent, because the board mixes them. The column is `build::turn_state`, a
function of the transcript's tail: `stop_reason: end_turn` is your turn, a
trailing `tool_use` is working, a trailing `AskUserQuestion` or
`ExitPlanMode` needs you, the latest permission mode says whether working is
planning. `peek` computes it for the index from the same tail slice it reads
the title from, cached on (size, mtime), so a rescan of every transcript on
the machine costs nothing.

**The rest of the window is laid out like the Claude desktop app.** One
collapsible sidebar (⌘⇧S): the wordmark, an accent "New session" entry,
Board, Sessions and Search, then Agents (one row per agent with its count
and a live dot, plus Kept only), Projects and Recents, and an account-style
footer with the person's name and nothing else: no version, no status. The content pane has a 48px top strip
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
rectangle macOS asks for (dictation, the input-source badge) is real. The row under it carries the limits on the left and, on the right,
which channel a message would take or what the app has to say (below). Prompts are rounded
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
layout the Python Emaki used: `archive/<project>/<session>.jsonl` plus
sidecars, `logs/`, the project registry in `state/projects.json`,
`config.json`, `cache/explanations.json`. An archive made by the Python
Emaki is picked up as-is. The markdown a session renders to is byte-identical
to the Python renderer's except JSON key order inside tool arguments and the
`You (web)` label, now `You (emaki)`. `~/.emaki/index.db` was the daemon's
search index; nothing reads it now.

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

**No hooks.** The app has no daemon and writes one thing into Claude Code's
settings, the status line, described with the limits row. Presence is the registry, a driver of our own when we started the
process, and a ten-minute mtime grace for anything else. A fresh file with
neither is a session that just ended (an interactive Claude Code always has
an inbox), so the composer offers to resume it. The Python daemon did write
seven hook entries into `~/.claude/settings.json`, each an absolute path
into the checkout; when the folder was renamed, Python exited 2 on the
missing file and Claude Code, which reads exit 2 as "block", refused every
prompt. Nothing may write a hook again. If a hook is ever needed, it is a
binary at a stable path outside the repository that never exits 2. The
status line is held to the same two rules, and a `statusLine` command
cannot block anything: if it fails, the terminal's line goes blank.

**Codex has no stop reason.** Its phase is derived from the built model's tail
(`adapters::turn_state_from_session`), not from the rows.

**One copy of gpui.** gpui-component depends on the unpinned zed git source. A
`rev` on our own gpui dependency produces a second copy that gpui-component
does not build against. The pin lives in `Cargo.lock` instead:
`cargo update -p gpui --precise <rev>` moves it.

**The toolkit is vendored, in this repository.** `vendor/gpui-component/`
holds the four gpui-component crates Emaki uses (`ui`, `base`, `macros`,
`assets`), copied from the revision the crate manifests still name, with
upstream's licence and a trimmed workspace manifest of their own so their
`workspace = true` references resolve. The root `Cargo.toml` excludes
`vendor` from the workspace and `[patch]`es the git source with those
paths, so the `git` + `rev` dependencies keep saying where the code came
from while the build uses the copies. This exists because the markdown
view hard-coded strong text to weight 700 and gave inline code the
paragraph's face, and gpui's highlight styles carry no family, so neither
could be changed from outside; a fork on GitHub was the alternative and
one repository was preferred. Every change is marked `(Emaki addition.)`
in the source and listed in `vendor/gpui-component/UPSTREAM.md`, which
also says how to move to a newer upstream revision: copy the crates over,
re-apply the list, build. Five changes so far: the strong weight and the
inline-code family as `TextViewStyle` settings (`md_view` sets 600 and the
theme's mono face, as the Claude app does), the input's Up on the first
line going to the start of the text, Down on the last to the end, and the
scrollbar fading a second after the last scroll instead of two.

**The search database is `search.db`.** FTS5, one document per item (a
prompt, a reply paragraph, a thought, a tool call): coarser and a hit in a
long session says nothing about where to look, finer and snippets lose
context. A hit is a matching document, not a term occurrence. Sync is
incremental on (size, mtime) on its own thread. `fts_query` quotes every
term, because people type `rm -rf`, `a:b` and a lone `"`, all FTS5 syntax
that would raise mid-keystroke; a trailing `*` survives as a prefix match.

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

**The agent's mark moves while it works.** Claude's glyph is Claude's own
starburst (`assets/icons/claude.svg`, the brand mark as simple-icons
carries it; Emaki's plain asterisk `mark.svg` stays on the wordmark and
the greeting). `agent_glyph` turns it once every 2.8 s while it breathes
twice a turn, to 82% of its size and 55% opacity, inside a fixed box so
nothing around it moves; that is the Python viewer's `spark` animation,
which is what the person remembered. Codex's glyph breathes. It moves
whenever `is_working` (board column working or planning) holds: in the
status row under the transcript, the top bar, recents, the sessions page
and board cards. The round header's mark is still, and there is no mark
at the foot of the conversation: one was tried, as the Claude app draws
it, and read as one too many beside the status row. Each place passes its
own animation id.

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
Windows) and resolves the family names the text system reports. The app
ships each face as one variable font, and gpui picks a weight by matching
against the faces it holds, so handed the bytes through
`text_system().add_fonts` it held one face per file (the default
instance, Regular) and drew every bold as regular: "**Pingfan**" in a
prompt came out plain. On macOS the files are registered with CoreText
for the process (`CTFontManagerRegisterFontsForURL`) instead: CoreText
lists a variable font's named instances (Light, Medium, Semibold, Bold,
their italics, in Text and Display) as faces of their own, and gpui's
family lookup falls through to the system source when it was not handed
the family, so bold finds a bold. Elsewhere the bytes still go in as they
are, and bold stays regular until that platform's text system learns
variable fonts. Without a Claude app the serif falls back to
Georgia and the sans to the window's face, and the settings panel says so.
The face is applied to the transcript container only (`render_detail`),
which the markdown view inherits; code stays in the mono face, the chrome
and the composer stay in the UI face. The choice is `app.chat_font` in
`config.json` (`serif`, the default, or `sans`), changed from the settings
panel (⌘, on macOS, Ctrl+, and Win+, elsewhere, or the app menu) through
`Config::edit`, which rewrites the file without the environment overrides
`Config::load` applies. `driver.claude_path` in the same file names the
`claude` binary when `PATH` does not.

**The look is four settings, all in `config.json` under `app`.**
`appearance` (`system`, `light`, `dark`), `accent` (a name from
`look::ACCENTS`), `chat_font` and `chat_size` (`small`, `medium`, `large`;
`AppConfig::chat_px` turns it into the reply's pixel size, the prompt half a
pixel under, thoughts two under; medium is 16px at line height 1.5, the
Claude desktop app's own body setting read from its stylesheet, which
also sets its bold to weight 600, matched through the vendored toolkit's
`strong_font_weight`). `look.rs` owns the first two: the palette
stays in `themes/emaki.json`, and an accent is a substitution over the
dozen keys that carry the terracotta there, painted into both configs
before they are handed to the toolkit, so `Theme::change` keeps the accent
on every appearance switch with nothing to patch afterwards. `look::apply`
draws the appearance config asks for and replaces every direct
`sync_system_appearance` call, including the one in the window's appearance
observer, which is what keeps a pinned appearance pinned when the system
flips. The settings panel (⌘,) also sets `driver.default_mode` and
`driver.default_model`, what a session started from the window begins in.

**Every mode and model is on a list, with a tick.** The pills under the
composer open a `Popover` (`picker` in `workbench.rs`) naming each choice
with a line on what it does; a pill that cycled on click hid the fourth
mode behind three clicks, and its label did not know `auto`, so auto mode
read as "Default permissions" and looked broken. `driver::mode_label`,
`mode_detail` and `model_label` are the words, in the core so they are
tested; `model_label` reads the id Claude Code reports in `system/init`
(`claude-opus-5-5` is "Opus 5.5") as well as the alias sent on the wire
(`opus` is "Opus" until the first turn confirms the version). A switch goes
through `Hub::set_driver_mode` / `set_driver_model`, which answer with the
mode Claude Code actually holds: a refused switch (bypass without the flag)
puts the pill back and says why on the status row. ⇧Tab in the composer
cycles the mode as Claude Code's terminal does; the wrapper captures the
textarea's own `OutdentInline` for it. Checked against 2.1.284:
`set_permission_mode` accepts `auto` and answers `{"mode": "auto"}` but
sends no `system/status` frame for it, unlike the other modes, so the
reply is what the driver trusts.

**Mode, model and effort show on every Claude conversation.** A driver
session's pills open pickers; a terminal session's are `chip_static`, read
from the transcript (`r.state.mode`, `Session::models.last()`,
`Session::effort`) and not clickable: the inbox reads everything as prose,
and a `control_request` frame sent to it is dropped without a reply
(tried against 2.1.284), so there is no channel to change them from here.
The row under the composer says so. Effort has no control request either
(`set_effort` is "Unsupported"), but `/effort <level>` as a user turn runs
as the local command it is, and Claude Code records it as a
`system/local_command` row with `commandRun: {command: "effort", args}`,
which `build` reads into `Session::effort`; `Driver::set_effort` sends that
turn and the pill updates when the file does.

**The limits row is the terminal's status line.** Under the composer, on
the left of the row the notice shares:
`Context 37% (386k of 1M) · 5h 3% (4h26m) · 7d 6% (6d7h)`, coloured at the
same thresholds as `~/.claude/statusline.sh`. The context is the
transcript's (`Session::context_tokens`, the last assistant row's input plus
cache read plus cache creation) over the model's window
(`limits::Limits::context_window`: what a driver's `result` frame reported
in `modelUsage`, else an assumption; `claude-fable-5-1` answered 1,000,000).
The five-hour and seven-day windows are per account and reach Emaki two
ways, the newer winning: a driver's `rate_limit_event` frames, and the
terminal's status line. Claude Code writes the windows to no file a
terminal session leaves behind; the status-line command in
`~/.claude/settings.json` is the only place it hands them out, on stdin,
once per refresh. So Emaki has a status line of its own,
`scripts/statusline.sh`, built into the binary (`statusline::SCRIPT`): it
prints the terminal's line and, when `~/.emaki/state` exists, leaves the
windows in `state/rate_limits.json`, written whole and renamed into place,
which `Limits::refresh_from_statusline` reads every second.
`statusline::ensure` runs in `Hub::start`, at every launch: it writes the
script to `~/.emaki/bin/statusline.sh`, a stable path outside any
checkout, and sets `statusLine` to `bash ~/.emaki/bin/statusline.sh`
unless it already says so, touching no other key. There is nothing to
switch on and no settings for it; the previous value is kept in
`state/statusline.json` and `emaki-core statusline restore` returns it
from a terminal. A copy run with `EMAKI_HOME` set leaves the settings
alone, or it would point Claude Code at its scratch tree. That is the
only write the app makes to Claude Code's settings, and the hooks rule
above says why it is allowed: the script never exits non-zero, never
writes to stderr, and the tests run it. The first version of the row
leaned on a hand-patched script outside the repository; the patch was
lost and the row froze, hence this. What was learned is kept in
`state/limits.json` with the time it was seen; the row says "limits as
of …" once that is older than five minutes and shows `--` until either
source has reported. The row has no tooltip: the line is the whole story.

**A permission card answers to the keyboard.** ↩ on an empty composer
allows the oldest card waiting on the session showing, ⇧↩ denies it, and
the oldest card says so on its buttons; with words typed, ↩ is a new line
as before. The textarea inserts the newline before it reports `PressEnter`,
so `answer_pending_by_key` clears the composer after answering. When
several cards are stacked the first also offers "Allow all".

**⌘F finds inside the conversation showing.** `find.rs` in the core lowers
every prompt, reply, thought and tool call (arguments, output, the
subagent's rounds counted against the Task call) once per session load,
and a query is a substring scan over that, so the answer is exactly what
the page shows and needs no index. The bar sits between the title strip
and the transcript: the field, "n of m", up, down, close; ↩ and ⇧↩ step
from the field, ⌘G and ⌘⇧G from anywhere, Escape closes. Stepping scrolls
the hit's round to the top of the view (`ListState::scroll_to`, item
offset zero; the list cannot address a point inside an item) and unfolds
whatever hides the item: the tool card, the thought, the folded run
(`transcript::run_start`). Every hit wears the same accent ring, whatever it
is: a faint one for a hit, a full one for the hit the bar is on, on a
prompt bubble, a reply paragraph, a tool card or a thought (`find_wrap`)
and a folded run with a hit inside. Tints and edge bars were tried and
read as clutter. A live reload recomputes the hits without moving the reader
(`compute_hits`); typing lands on the first hit at or after the round in
view (`run_find`). A hit in the search palette opens its session through
`open_with_find`, which puts the query in the bar and lands on the matched
round once the session has loaded (`find_pending`); the index numbers
rounds from one, the model from zero.

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

**A scroll gesture's momentum stays in the pane it began in.** macOS goes
on sending wheel events after the finger lifts, addressed to wherever the
pointer is by then, and gpui hands each to the scroll container under it
(its `touch_phase` tells `Started` and `Ended`, but a momentum event is
just `Moved`), so a flick in the conversation followed by a move to the
sidebar scrolled the sidebar. `Workbench::route_scroll` runs in the
capture phase from a raw listener a zero-size `canvas` registers at paint,
before any container: the pane under the pointer at `Started`, or at the
first event after `SCROLL_GAP` (a mouse wheel sends no phases), owns the
gesture, and an event that lands in the other pane is applied to the
owner's scroll position (`ListState::scroll_by` for the conversation, a
`ScrollHandle` on the sidebar, sessions and home containers) and stopped.
With the sidebar folded away there is one pane and nothing to do.
`EMAKI_SCROLL_DEBUG=1` prints every wheel event with its phase and owner.
Probing note: a synthetic `CGEvent` scroll carries `CGScrollPhase` values
(began 1, changed 2, ended 4) and `CGMomentumScrollPhase` (begin 1,
continue 2, end 3), not the `NSEventPhase` bits gpui reads; the checked
sequence is in the session scratchpad's `scroll.swift`.

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

**What the app has to say goes under the composer, on the right.** The
row under the composer card has the limits on its left and
`Workbench::notice` on its right, where the eye is after a send or a
click and where nothing else was: "opened in your terminal", "starting
claude…", "queued behind the running turn", every refusal ("Already open
in a terminal", "not sent: …", "claude exited: …") in the theme's danger
colour, and the daily update check's "Emaki x is available". A message
that went through gets no line: it shows up in the transcript, which says
it better than "sent" or "delivered" did, so those are gone.
Every notice fades after `NOTICE_SECS` on the clock tick, errors too,
and the row always takes `NOTICE_H` whether or not either side has words,
so the composer does not jump as one comes and goes. The right side
otherwise says why nothing can send, as before. Nothing but "Claude is
working" sits above the card; two things there was one too many. Nothing goes anywhere else: the sidebar
footer used to carry a status string, and the scanner's "indexed 1
session" and "archived 3 files" rewrote it after every turn of a live
session, so the footer blinked with bookkeeping nobody acts on. Those
counts are not shown now, and the two events that carried them are gone.

**The button at the top left continues the session in your terminal.**
`terminal.rs` in the core writes `~/.emaki/run/terminal/<session>.command`:
clear every `CLAUDE*` variable but `CLAUDE_CONFIG_DIR` (the same rule as
`driver::child_env`, because a terminal app started from inside a Claude
Code session carries its marker, and a resumed session that inherits it
stops writing its transcript), `cd` to the session's folder, `exec` the
agent's own resume command (`claude --resume <id>` with the binary the
driver resolves, `codex resume <id>`). `sys::open_in_terminal` hands it
over per OS: on macOS `open` gives the `.command` file to whatever app the
system keeps for shell scripts, Terminal unless another terminal claimed
the type, so the choice is the system's and there is no setting; Windows
opens a `cmd` window through `start` in that folder; Linux tries
`$TERMINAL`, then the usual emulators, with the script as the command.
One writer per transcript: `Workbench::terminal_check` refuses a session
whose registry entry shows a terminal already (the tooltip says so), one
kept only, one whose folder is gone, and one a driver of ours is
mid-reply on; an idle driver is stopped on the way out, and once the
terminal registers, the channel flips to `inbox` on its own. Checked by
hand against Kaku, which had claimed `.command` on this machine: the
session resumed in the folder, the row went live, and the second click
was refused.

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
runs on a `v*` tag: four package jobs build with `cargo-packager`, then
one `release` job makes the GitHub Release with the tag's section of
`CHANGELOG.md` as its notes and the installers under stable names.
`scripts/release-mac.sh` is the Mac build: Developer ID signature,
hardened runtime, a disk image laid out by Finder over
`scripts/dmg/background.png`, notarization and stapling. Until the signing
secrets are in the repository, CI's Mac images are ad-hoc and the
notarized ones are built here and uploaded over them after the release
job. **WORKFLOW.md is the procedure**: the version bump (three files and
`Cargo.lock`), the changelog, the tag, the Mac build and upload, redoing a
release, enabling CI signing, the icon and the background. Three facts an
agent needs even without opening it: only Finder writes a `.DS_Store`
Finder honours, so the layout runs through AppleScript on a mounted image;
the upload must follow the release job, which replaces same-named assets;
and no second `Emaki.app` may be left on disk, or Launchpad lists two.

**Everything is Emaki now, and the old name is only history.** The rename
on 2026-09-28 reached the crates (`emaki-core`, `emaki-app`), the
environment variables (`EMAKI_HOME`, `EMAKI_OPEN`, `EMAKI_PAGE`,
`EMAKI_A11Y`, `EMAKI_TIMING`, `EMAKI_CLAUDE`, `EMAKI_DISABLE`,
`EMAKI_DRIVER_MODE`, `EMAKI_DRIVER_MODEL`), the data directory and the
envelope's sender name. Three things still know the old name, on purpose:
`paths::root` moves `~/.scribe` to `~/.emaki` whole the first time it runs
(a failed move starts fresh rather than half of each);
`paths::relocate_legacy` points an `Attached file:` path a transcript
recorded under `~/.scribe` at the moved file; and `PAGE_SENDER_LEGACY`
keeps rows the app sent under the old envelope name rendering as yours.
The word "scribe" inside "describe", "subscribe" and "transcribe" was left
alone, so a future sweep must use word boundaries too. The Python CLI,
daemon, web viewer and their tests went on 2026-09-29, with the `uv tool`
that ran them and the hooks they had written; the app has never needed any
of it.
`sys::install_dock_icon` gives a bare `target/debug/Emaki` the Dock icon at
startup; a bundle has it from `Emaki.icns`; on Windows `build.rs` compiles
`icon.ico` into the executable.

**The icon is JP's logo, cut out.** `scripts/icon/logo.png` is the picture
JP generated; `scripts/icon/cut.js` finds the plate as everything that is
not white, makes the rest transparent and un-blends the anti-aliased edge,
and writes every size (a drawn copy was tried twice and never matched the
original's curls, so the picture itself is the source). The PNGs sit on
Apple's 824-of-1024 grid, which the Dock (`sys::install_dock_icon` sets the
PNG at start), Windows and Linux want; the `.icns` alone is the plate at
full bleed and opaque to its corners, because macOS 26 masks every app
icon to its own rounded square over a grey backing and shows anything
transparent, a margin or the plate's own rounder corners, as a grey border
in Finder, the switcher and Spotlight. WORKFLOW.md says how to regenerate
it.

**The window remembers itself.** `ui_state.rs` keeps
`~/.emaki/state/ui.json`: bounds, sidebar, page, the open tabs and the
active one. Where each tab was scrolled was remembered too, and put back
on the next open; that went, because every open should land at the end of
the conversation, where the newest turn is, which the list's bottom
alignment does on its own (an older `scroll` key in the file is ignored).
The list is in gpui's `FollowMode::Tail`, so it stays at the end while a
reply streams in: following pauses when the reader scrolls up, when a
find hit is scrolled to, or when `pin_scroll` gives the list a real top
so an opened card extends downward, and it resumes once the view is back
at the bottom. Without it, the first of those left the list anchored to
the prompt while the reply grew out of sight.
It is written when any of
that changes, bounds changes at most every two seconds, and again at quit;
the bounds are reused only when their centre is still on a screen.
The page is saved but not restored: the window opens on the new-session
page, as the Claude app opens on a new chat, and the tabs come back in the
sidebar; `EMAKI_PAGE` and `EMAKI_OPEN` pick something else. Tabs are keys
in `Workbench::tabs`; there is one `Detail` at a time and switching tabs
reloads from disk, which the numbers below say costs nothing a person can
see. ⌘W is one global `CloseTab` binding that closes the showing tab, then
the last tab (which lands on the new-session page with the caret in its
composer), then the window: a context-bound binding would lose to a
global one whenever the focus sits in the composer. The app stays running
with no window, and `on_reopen` in `main.rs` (a Dock click, or a second
launch) opens it again; without that handler the Dock icon did nothing.

**The focused element must be one the page draws.** gpui dispatches a
keystroke from the focused node, or from the window root when that node is
not in the frame, and the root sits above every handler in the workbench.
Clicking a session puts the caret in the composer; closing that last tab
used to land on the board, which draws no composer, and from then on ⌘W
and every other shortcut went nowhere. `Workbench::render` now moves the
focus to the workbench's own handle whenever the page is the board or the
sessions list and the composer still holds it, and `main.rs` answers a
`CloseTab` no view claimed by closing the window, as any app does.

**Opening is measured, not guessed.** `emaki-core bench [<id>...]` times
read, build and render for a transcript (the five largest by default) and
`EMAKI_TIMING=1` makes the app print load and hand-over time per open.
The largest transcript on the machine this was built on (137 MB) reads in
about 80 ms and builds in about 80 ms more; the app opens a 66 MB session
in under 100 ms end to end. A tail-first reader was planned and dropped on
those numbers; bring it back only if bench shows a session over about half
a second.

**The driver is the conversation.** Messages typed in the window go to a
`claude -p --input-format stream-json --output-format stream-json
--permission-prompt-tool stdio` child kept alive between turns and closed
after `driver.idle_min`. `--resume <id>` appends to the same transcript, so
the window shows the reply the same way it shows a terminal turn: by reading
the file. Permission prompts arrive as `can_use_tool` control requests and
are answered from a card; silence for ten minutes is a deny. Never `--bare`:
it reads auth only from `ANTHROPIC_API_KEY`, never the keychain, which
breaks subscription users. `driver::child_env` strips every `CLAUDE*`
variable except `CLAUDE_CONFIG_DIR`, because the app may itself be a
grandchild of a session and would otherwise hand the child its parent's id
and inbox. `claude --bg --resume` forks under a new id; only `claude -p
--resume` appends. If the registry shows a terminal process for a driven
session, `reap_drivers` stops the child once idle and the channel flips to
`inbox`, so two processes never write one transcript. The docstring at the
top of `driver.rs` records what each Claude Code release actually does on
the wire.

**The app updates itself from the GitHub release.** `update.rs` in the
core: the newest version is the tag `releases/latest` redirects to (no API,
no rate limit), the installer is `releases/download/v<version>/<asset>`
under the stable names WORKFLOW.md fixes, fetched with `ureq` on rustls.
Installing is per platform: on macOS the disk image is mounted, `Emaki.app`
copied beside the running bundle with `ditto` (signature and attributes
kept), swapped in with two renames and opened with `open -n`, and the old
process quits; a bare `target/debug/Emaki` refuses, since there is no
bundle to replace. Windows starts the installer and quits; Linux replaces
the running AppImage (`APPIMAGE`) and restarts it. `Hub::check_updates`
and `install_update` run on threads and report as `HubEvent::Update`;
`Workbench::check_updates_daily` runs the check once a day from the clock
tick when `app.check_updates` is on, saying something only when it finds a
newer version. `state/update.json` keeps when the last check ran. The
settings panel has the version, the last answer, *Check for updates*,
*Update to x*, *Release notes* and the daily tick. `emaki-core update` is
the check from a terminal.

**Every text is tried before the markdown view gets it.** The `markdown`
crate the toolkit's `TextView` parses with (1.0.0) panics on some inputs
("Cannot push to non-parent"); the smallest found is a paragraph followed by
two `---` lines, which YAML front matter quoted in a Codex tool result
produces, and a panic inside an element's layout aborts the app.
`markdown_is_safe` in `transcript.rs` runs the same parser under
`catch_unwind` first, once per distinct text, and `md_view` shows an unsafe
text as a code block instead. `markdown` is a direct dependency of the app
at that version for this one call.

**Opaque tool calls are explained in plain words.** `explain.rs` in the
core: a `Bash` heredoc, a piped chain, anything with an opaque shape
(`OPAQUE_MARKERS`) or longer than `explain.min_chars` is put to a small
model, `claude -p --model claude-haiku-4-5` with a fixed system prompt, no
settings, no MCP servers, no tools, and never `--bare`. Simple calls get a
canned line from their own arguments (`explain::canned`) and never reach a
model. Answers are keyed by a hash of the call's name and sorted arguments
and kept in `~/.emaki/cache/explanations.json`, bounded; `Explainer::lookup`
is cache-only and `attach` folds cached answers into a loaded session, so a
command explained once is explained everywhere it appears, however old.
The child is a real Claude Code session and leaves a transcript: it runs with
`cwd` under `~/.emaki/run/explain`, which `paths::is_explainer_cwd` makes the
index drop, and `prune_transcripts` sweeps those files hourly. The hub owns
one `Explainer` and asks it the moment a `Permission` event arrives, so the
card reads the answer while you decide; a tool card's *Explain* button asks
with `force`; scope `all` also asks for the tail of the session showing on
each live reload, never on a first open. Answers come back as
`HubEvent::Explained` into `Workbench::explanations`, an overlay by call id
the next load makes redundant. `emaki-core explain <command>` makes one real
call from a terminal; the tests never do.

## What the transcript teaches

Facts about Claude Code's files that the code depends on, learned the hard
way. The Rust side already honours each; keep it that way.

**Never feed the archive its own file as a source.** Once a session outlives
its original it re-enters the index pointing at the archived copy. Without
the guards in `archive.rs`, the rotate-then-copy path renames the destination
aside and then fails to read the source it just moved, losing the canonical
file and rotating again on every sweep. Two independent checks exist because
the failure is silent and permanent.

**Subagent rows are all `isSidechain: true`.** They are a sidechain *of the
parent*, but the whole conversation at their own level, so a nested build
must not filter on it or it produces zero rounds.

**Subagents link back explicitly.** `agent-*.meta.json` carries `toolUseId`,
naming the exact `Task` call that spawned it. Use that, not contiguous-run
guessing; the latter cannot tell two parallel subagents apart.

**`ai-title` is not always a title.** When a session runs under a named
agent, Claude Code overwrites that field with the *agent's name*.
`pick_title` takes the newest title that is neither a known `agentName` nor
slug-shaped.

**A rewritten transcript must replace, not append.** `--resume` and
compaction rewrite in place. `TranscriptTail::restarted` signals it (the
file identity, `transcript::file_id`, tells a rewrite from an append) and
the reader drops its accumulated rows; without that the session doubles.

**Cache-read tokens are not a total.** Every assistant message re-reads the
whole cached prefix. `Usage::total` deliberately excludes `cache_read`.

**`system` rows with `subtype: local_command`** carry the same XML-ish
wrappers as user rows (`<command-name>`, `<local-command-stdout>`). Route
them through `strip_wrappers` or raw markup lands in the log.

**Raw HTML in a transcript is content, not markup.** Someone writing "maybe
the `<aside>` option" means those characters. The markdown view must escape
it, never render it.

**Messages from the window.** Claude Code 2.1 registers every session in
`~/.claude/sessions/<pid>.json` with a `messagingSocketPath` and publishes
the token a peer must present in `<pid>.<hash>.key` beside it. The wire is
two JSON lines on that socket: an `auth` frame, then a `user` frame whose
content is wrapped in Claude Code's own `<cross-session-message
from-name="emaki">` envelope. The envelope is what makes the transcript row
carry `origin.name` and a clean `origin.body`; `build` keys on those, so a
message from the window renders as yours and one from another Claude session
as a peer. Those rows are `isMeta: true`; the builder and `turn_state` must
treat a peer row as a prompt or the log shows a reply to nothing. **Do not
assert `from-mode`**: Claude Code holds a message that asserts no permission
mode when the recipient runs with permissions bypassed, and asks in the
terminal. That check is what stops a less trusted process steering a more
trusted session, and Emaki is such a process as far as Claude Code can tell;
the person's remedy is `crossSessionInbound: accept` in their own settings.

## Testing

```
cargo test -p emaki-core
cargo build -p emaki-app
```

`tests/core.rs` points `EMAKI_HOME`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME` at
a temp tree (`isolated()`); inherit that for anything touching disk. The
suite must pass before committing, and `rust.yml` runs it on all three OSes.
