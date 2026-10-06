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
  src/options.rs             the modes, models and effort levels an agent offers, as it lists them
  src/explain.rs             opaque tool calls in plain words, via `claude -p`
  src/update.rs              the newest release, its installer, and putting it in place
  src/statusline.rs          scripts/statusline.sh built in, installed to ~/.emaki/bin at launch, and the one setting
  src/terminal.rs            the agent's resume command as a script a terminal can be handed
  src/pty.rs                 a terminal of our own with no window: an interactive `claude` on a pty, its screen kept in memory
  src/watcher.rs             notify over every agent's data roots
  src/bin/emaki-core.rs     list | render | build | archive | sync | search | bench | peers | inbox | options | explain | update | statusline | drive | pty
  tests/core.rs
crates/emaki-app/         the window
  src/hub.rs                 threads: scan -> archive -> index, drivers, watcher
  src/workbench.rs           sidebar, session list, board, search, composer
  src/transcript.rs          drawing rounds, tool cards, thoughts, subagents
  src/main.rs                menus, key bindings, the window
  src/sys.rs                 open, reveal, open in a terminal, the person's name: per OS
  src/ui_state.rs            ~/.emaki/state/ui.json, what the window remembers
  assets/icon/               the icon in every size, drawn by scripts/icon/icon.html
scripts/make-app.sh        Emaki.app bundle for a quick local run, ad-hoc signed
scripts/make-icon.sh       remakes assets/icon from scripts/icon/icon.html
scripts/release-mac.sh     the signed, notarized, Finder-laid-out disk image
scripts/dmg/               the disk image's background and the script that draws it
scripts/release-notes.sh   one version's section of CHANGELOG.md, the release notes
scripts/release-check.sh   before a tag: version, lock, changelog, tests, build, Windows type check
scripts/release-publish.sh after CI: the notarized Mac images onto the draft release, then publish
scripts/statusline.sh      Claude Code's status line, ours: prints the line, leaves the rate limits
scripts/relaunch.sh        quit the running Emaki and start the new build, once the agent's turn is over
scripts/anthropic-mono.py  the Claude app's code font into ~/.emaki/fonts, plus its 0.9 copy for inline code
WORKFLOW.md                how to cut a release, step by step
CHANGELOG.md               one section per release; the release job reads it
STAGES.md                  the high-level plan: three stages, a page
PLAN.md                    the detailed plan: tasks, decisions, what has landed
.github/workflows/rust.yml     tests and a build on macOS, Windows, Linux, every push
.github/workflows/release.yml  installers on a v* tag, via cargo-packager
```

```
cargo build -p emaki-app && ./target/debug/Emaki
cargo test -p emaki-core
EMAKI_OPEN=<session-id prefix> ./target/debug/Emaki    # open a session on launch
EMAKI_PAGE=new|sessions|board ./target/debug/Emaki     # land on a page
EMAKI_FIND=<text> ./target/debug/Emaki                 # open the find bar on that query
EMAKI_SETTINGS=1 ./target/debug/Emaki                  # open the settings panel (=updates: on that section)
EMAKI_TYPE=/co ./target/debug/Emaki                    # put that text in the composer
EMAKI_QUESTION=1 EMAKI_OPEN=<id> ./target/debug/Emaki  # hold a sample question on that session
EMAKI_GO=terminal EMAKI_OPEN=<id> ./target/debug/Emaki  # press "go to the terminal" on it
EMAKI_GO=type:/status EMAKI_OPEN=<id> ./target/debug/Emaki  # type that into its terminal and send
EMAKI_GO=pill:effort EMAKI_OPEN=<id> ./target/debug/Emaki  # click a pill: pill:mode, pill:model, pill:effort; step:mode is ⇧Tab
EMAKI_GO="step:mode;button:terminal" EMAKI_OPEN=<id> ./target/debug/Emaki  # several steps, five seconds apart; button:terminal is the top-right button
EMAKI_GO=effort:high EMAKI_OPEN=<id> ./target/debug/Emaki  # send a value without the picker: effort:, model:
EMAKI_GO="open:<id2>;page:new" EMAKI_OPEN=<id> ./target/debug/Emaki  # go to another session, or to the new-session page
EMAKI_GO=folder:<name> EMAKI_OPEN=<id> ./target/debug/Emaki  # click that folder in the sidebar
EMAKI_GO="sidebar;float" EMAKI_OPEN=<id> ./target/debug/Emaki  # the sidebar's button, then the pointer on it; page:board is the board
EMAKI_GO="page:sessions;sessions:<folder>" EMAKI_OPEN=<id> ./target/debug/Emaki  # the sessions page's folders, then inside one
EMAKI_GO=menu EMAKI_OPEN=<id> ./target/debug/Emaki  # the session's right-click menu; renaming shows the rename field, name:<words> names it
EMAKI_GO=dialog:2 EMAKI_OPEN=<id> ./target/debug/Emaki  # press that in the terminal's dialog (a digit, or tab); answer:<words> types an answer, goto:<n> goes to that tab
EMAKI_KEYS=down,down,tab EMAKI_TYPE=/mod ./target/debug/Emaki  # press the slash list's keys (up, down, tab, esc)
EMAKI_GO=send:hello EMAKI_OPEN=<id> ./target/debug/Emaki  # send that message once the session is open (or with EMAKI_PAGE=new, start one)
EMAKI_TERM_KEYS=left,s EMAKI_GO=pill:effort EMAKI_OPEN=<id> ./target/debug/Emaki  # press keys on the terminal card (left, right, up, down, enter, esc, tab, or a letter)
EMAKI_SHOT=/tmp/shot.png EMAKI_OPEN=<id> ./target/debug/Emaki  # write a picture of the window there after EMAKI_SHOT_AFTER seconds (5) and quit
```

The last two exist for probing: a terminal without accessibility access
cannot press ⌘F or ⌘, in the window from a script, so a screenshot of
either state is one launch away. `EMAKI_SHOT` takes that screenshot from
inside (`sys::shoot_window`): a process may capture its own window without
Screen Recording access, which the terminal running the probe often lacks
(`screencapture` from Kaku answered "could not create image from window").
Point `EMAKI_HOME` at a scratch directory to run a second copy beside the
installed app without sharing its state; with `CLAUDE_CONFIG_DIR` at a
scratch tree holding one hand-written transcript, the copy archives and
indexes only that, and a state can be drawn without touching the real one.

**Each document has one job.** STAGES.md is where the app is going, in
three stages, short enough to hand to someone outside the project; a
change of direction goes there. PLAN.md is the detailed plan under it:
phases, tasks with checkmarks, decisions and their dates. This file is
how the code works and why. WORKFLOW.md is how to cut a release,
CHANGELOG.md what each release changed for the person installing it, and
README.md how to use the app and how it is built.

**One Emaki at a time.** Before launching a build, quit the one running:
`pkill -x Emaki` stops both a bare `target/debug/Emaki` and the installed
`/Applications/Emaki.app`. Two copies share `~/.emaki` and both write
`state/ui.json`, the Dock shows two identical icons, and the one the
person looks at is usually the old build, so the change "is not there".
Relaunch, then check `pgrep -fl Emaki` lists one process.

**After a change to the app, relaunch it with `scripts/relaunch.sh`.**
The person develops Emaki in Emaki, so at the end of any task that
changed the app, build it (`cargo build -p emaki-app`) and run
`scripts/relaunch.sh` as the last command before the final reply, without
being asked; a change to documents alone needs no relaunch. The script
returns at once, and fifteen seconds later quits the running Emaki and
starts `target/debug/Emaki`. The agent's session is a child of the app
(the hidden terminal), so quitting the app ends the session: the script
therefore also waits for the turn to be over, which is a state and not a
longer time. It finds the session's registry record
(`~/.claude/sessions/<pid>.json`, the nearest ancestor that has one) and
quits nothing while that says `busy`, then gives Claude Code two seconds
to write the turn's last rows. So the countdown is fifteen seconds or
the end of the reply, whichever is later. It was a fixed fifteen seconds
typed out by hand each time (`nohup sh -c 'sleep 15; pkill -x Emaki;
sleep 2; ./target/debug/Emaki'`), long enough for most replies and too
short for a long one. Five was tried with the guard and the person went
back to fifteen, keeping the guard: time to read the reply before the
window goes. A second
copy for a probe (`EMAKI_HOME`) is still the way to look at a change
before the relaunch, and never `pkill` by hand from inside the app.

**Two buttons sit beside the traffic lights, as in the Claude app.**
The sidebar and search (`Workbench::render_strip`), drawn
once over the window's top left corner and not inside the sidebar or the
top strip, so the sidebar button is in one place whether the sidebar is
there or not; the sidebar's own header is only room for them, and the
top strip leaves room when the sidebar is away (`strip_right`). The
traffic lights are set at (17, 18), the red one as far from the left
edge as from the top (17px each, measured on a window capture; 12 at
first, which the person saw sat left of the Claude app's), and the
strip is set a pixel down so the buttons' centres are on the lights'
(49.5px of a 2x capture for both, measured; the buttons sat 1px high),
starting 17px after the green light (11 at first, which read as
crowded). Search took the place of the sidebar's Search entry on
2026-10-06 and is there whether the sidebar is or not, so neither
button ever moves. There were back and forward arrows after it for an
hour, through a history of where the window had been (a list of stops
kept up to date at every draw); the person took them out, since the
tabs already are the way between sessions. With the sidebar folded away, the
pointer on its button floats it in over the content (`set_float`,
`render_sidebar_float`: no scrim, a shadow at its edge, sliding in and
out over `FLOAT_ANIM`), and it goes when the pointer leaves it
(`float_follow`, a mouse-move listener beside the scroll one, which asks
where the pointer is and not what is hovered); a click on the button
while it floats keeps it. The click that folds the sidebar away leaves
the pointer on the button, which is not a hover: `float_block` holds
until the pointer has left the button once. Checked with `EMAKI_SHOT`:
the float with a
`mouseMoved` `CGEvent` posted to the pid onto the button, and its going
with one posted off the sidebar. A real click on the button, and so the
hover it must not count, was not driven from a script.

**The sidebar is a name, three places, and two cards.** Under the
wordmark a hairline, then New session, Board and Projects (the entry
was "Sessions" until the page behind it listed folders first, then
"Folders" for an hour; ⌘L and `GoSessions` are unchanged; its icon is
Phosphor's `briefcase`, so it is not the folder the rows below wear);
then Agents
and Projects, each on a card of its own (`card` in `render_sidebar`: a
rounded outline in the sidebar's border colour on a ground a shade
toward the page's, its name at the top, which does not scroll). Each
card scrolls by itself with the toolkit's fading scrollbar at its edge
(`vertical_scrollbar`, as the settings panel and the conversation have):
the agents' shows `SIDE_AGENTS` rows (4) and scrolls for the rest
(`agents_scroll`), the folders' takes the height that is left
(`side_scroll`). The two are panes of their own to `route_scroll`
(`Pane::Agents`, by the scroller's bounds, and `Pane::Sidebar` for the
rest of the sidebar), so a flick in one that the pointer carries into
the other stays with the one it began in, as between the sidebar and
the conversation; at first the sidebar was one pane, and momentum from
the agents' card scrolled the folders. Not driven from a script.
Until 2026-10-06 the two were headed lists in one scroller under the
three entries, with nothing between the name and the entries.

**The sessions page has two levels, as the sidebar has.** A project is
a folder, and the window says "project" for the list and keeps the
folder icon on each row: the page is "Your projects" (or "Claude Code
projects", "Kept projects"), the card "Projects". The top is
the folders (`sessions_folder` none): a row each, headed by when it was
last worked in, with its count, its path and a chip for the most
pressing of its live sessions; a click goes inside
(`show_sessions_in`), where the sessions are headed by when under a
"Projects › folder" line that goes back up. The pills (All, one per
agent, Kept only) narrow either level and stay as the level changes;
inside a folder they count that folder's own. "N more" under a folder
in the sidebar goes inside that folder, "N more" at the foot of the
folders and the Projects entry go to the top, and an agent's row goes
to the top narrowed to that agent. A right click is Open in Finder on a
folder and the session's menu on a session. Before, the page was one
flat list of every session with the folder as a fourth pill. Checked
with `EMAKI_SHOT` on both levels and on the sidebar; a click on a row
and a real scroll inside either card were not driven from a script.

**Every icon is Phosphor's, regular weight.** The toolkit ships
Lucide's and asks for each by a file name (`IconName::Close` is
`icons/close.svg`), and `assets.rs` answers for a path before the
toolkit's own set does. So `crates/emaki-app/assets/icons/` holds a
Phosphor icon under each of those names (`close.svg` is Phosphor's `x`,
`inbox.svg` its `tray`, `search.svg` its `magnifying-glass`,
`settings.svg` its `gear`, `square-terminal.svg` its `terminal-window`,
`box.svg` its `cube`, `shield.svg` its `shield-check`), 97 of them, and
the icons the toolkit draws by itself (a close button, a caret) change
with the app's. No call site names Phosphor. The files are from
`phosphor-icons/core` (`assets/regular/<name>.svg`; `star-fill` from
`fill`), MIT, with `PHOSPHOR-LICENSE` beside them; a new icon is a file
there and a line in `OWN`. Not Phosphor: `claude.svg`, Claude's own
mark, and eleven toolkit icons Phosphor has no match for (the window
controls, the right and bottom panels, `inspector`, `resize-corner`,
`star-off`), none of which the window draws. Phosphor's regular line is
a little thinner than Lucide's at 16px. Changed on 2026-10-06 at the
person's asking; checked with `EMAKI_SHOT` on a session and on Settings.

**The board is the home of what needs you.** Four columns (needs you,
planning, working, your turn), a card per live session (project, branch,
title, a status chip with a clock, "since", the one or two actions that make
sense), and done as a row under them with a count, opening into a grid.
The columns are open regions under a hairline, not grey slabs, and an
empty one says so inside a dashed outline the height of a card. They sit
four across when the pane has room (`pane_w`, measured on every draw,
against `COL_MIN_W`), two by two when it does not, one under another in a
narrow window; the board never scrolls sideways. Every card names its
agent, because the board mixes them. The column is `build::turn_state`, a
function of the transcript's tail: `stop_reason: end_turn` is your turn, a
trailing `tool_use` is working, a trailing `AskUserQuestion` or
`ExitPlanMode` needs you, the latest permission mode says whether working is
planning. `peek` computes it for the index from the same tail slice it reads
the title from, cached on (size, mtime), so a rescan of every transcript on
the machine costs nothing.

**The rest of the window is laid out like the Claude desktop app.** One
collapsible sidebar (⌘⇧S): the app's icon and the wordmark (the Dock
icon from `assets/icon/icon-128.png`, served as `icon/app.png`, beside
"Emaki" at 22px, at a weight between regular and bold (Optima has
only those two: regular read as too thin once the name took the accent's
colour and bold as too thick, so the regular is drawn twice, half a
pixel apart), in an elegant sans,
`fonts::wordmark_family`: Optima, else Avenir Next, else Avenir, else the
window's own face; on macOS the pair has a row of its own under the
traffic lights, lined up with the entries below, and elsewhere it sits in
the top strip. Tried and dropped on the way: the plain asterisk with a
15px Georgia name, which did not stand out, a 20px bold italic Georgia,
the window's sans at 17px semibold, clean and still too quiet, Didot Bold
(the person wants a sans), and Avenir Next Bold, too thick),
set in the accent (`theme.primary`), so it follows the colour chosen
in Settings (a fixed gold was tried on 2026-10-05, and the person
preferred the accent),
an accent "New session" entry,
Board and Sessions, then Agents (one row per agent with its count
and a live dot, plus Kept only), then Folders: every project as a row,
newest first, with its session count, which a click opens onto its
sessions and closes again, the ten newest (`SIDE_FOLDERS`) and then "N more",
which goes to the sessions page (`folders_open`, kept in `ui.json`). A folder with a live session is
open without being asked (`sync_folders`): it opens when one of its
sessions goes live, and closes when the last stops if it was opened
that way and not clicked since; a click is the person's own choice and
stays until the folder next goes live or quiet. At first the newest
folder was the one open by default, which the person took for this rule
and asked for it. The sessions
under a folder are headed by when
(`format::bucket`: today, yesterday, this week, this month, earlier; the
sessions page uses the same heads), `FOLDER_ROWS` of them (5; 15 at first,
and the person asked for less) and then
"N more", which goes to the sessions page on that project. A session
has the agent's mark in the muted
ink unless it is live, so forty rows do not read as forty
accents, and a dot at its right in its board column's colour; a folder
wears what its sessions wear, its icon in the agent's colour while one
of them is live and its dot the most pressing of theirs (needs you,
then working, then your turn). Until 2026-10-06 there were two lists,
Projects (eight rows, each a filter on the sessions page) and Recents
(forty sessions of every folder together), A folder's sessions unfold and
fold away over `FOLDER_ANIM` (200ms): they sit in a box whose height is
the sum of its rows, every one of a fixed height, and `folder_anim`
(which folder, opened or closed, when, the click's number) runs that
height and the opacity up or down; a closing folder is still drawn
until the time is up. The rows are in a column of their own inside the
scroller and never shrink (`SIDE_ROW_H` 26, `SIDE_SESSION_H` 24): as
direct children of the scrolling flex column each gave up height when
the list was longer than the sidebar, down to its text, so 30px rows
drew at about 21 with a folder open and 24 with it closed, and the
whole list changed its spacing at a click. `EMAKI_GO=folder:<name>` is
that click; the end states were checked with it, the moving frames were
not captured. and an account-style
footer with the person's name and a settings gear, nothing else: no
version, no status. The content pane has a 48px top strip
with the title centred and actions on the right; when the sidebar is hidden
the strip makes room for the traffic lights. Everything readable sits in one
column of `CONTENT_W` (768px), centred in whatever is left of the window:
the conversation, the sessions page (a serif title, filter pills, rows
headed by when, a state chip at the right of a live row), and the home
page, which is a serif greeting behind the app's icon at 44px (the
same `icon/app.png` the sidebar has; until 2026-10-06 it was a plain
terracotta asterisk, `mark.svg`, which nothing draws now; the display face is
`fonts::display_family`, the Claude app's serif when it is here, so
titles and the conversation agree) with the date and the counts under it,
over the composer, with the six most recent folders as a grid of cards
beneath (`FOLDER_COLS` to a row, each a third of the column: the folder's
own name, its parents dimmed, the accent ring on the chosen one). The
folders were a cloud of full-path pills of every width, which read as
clutter. Each page's content fades and settles in over a moment when it
arrives (`page_in`, keyed on the page); the conversation's list items are
never animated. Floating cards (the composer, the search palette, the
settings panel) lift off the page with `float_shadow`: a wide soft drop in
the ink's own hue and a hairline of contact under it, not a grey halo.
The settings panel is a fixed sheet (`SETTINGS_W` by `SETTINGS_H`, 720 by
520, capped by the window) with a rail of sections on the left
(`SETTINGS_SECTIONS`: Appearance, New sessions, Explanations, Updates,
each with an icon, the chosen one on a plate; `settings_section` is the
one showing and `EMAKI_SETTINGS=<section>` opens the panel on it) and
that section's rows on the right under its title, fading in as the
section changes. The rows scroll under the panel's header with the
toolkit's scrollbar at their edge (`settings_scroll`, the same fading bar
the transcript and tool bodies have); the body is a flex column inside a
flex column, because a block wrapper around it collapsed the panel to
its header. Before the rail the panel was one long sheet of every
setting at 90% of the window's height. Settings choices are segmented controls (`Workbench::segmented`: a muted
track, the choice on a raised plate), where a row of outlined pills was
heavier than the panel needed. The plate is one element under the row
and slides from the old choice to the new one over 220ms: each
segment's bounds are recorded as the row is prepainted
(`on_children_prepainted` into `seg_bounds`, relative to the track), the
control remembers the choice it came from (`seg_state`), and the
animation is keyed on the new choice so a click plays it once. Before
any bounds exist, on the first draw, the chosen segment paints its own
plate so nothing flashes. Probing note: a synthetic click posted to the
pid does not reach a click handler, so the slide was checked with a
probe build that changed the choice on a timer with the animation
slowed. A round in the list is wrapped in an explicit
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
re-apply the list, build. Ten changes so far: the strong weight and the
inline-code family as `TextViewStyle` settings (`md_view` sets 600 and the
theme's mono face, as the Claude app does), the input's Up on the first
line going to the start of the text, Down on the last to the end, the
scrollbar fading a second after the last scroll instead of two, the
rounded plate behind inline code, ranges a textarea can colour, ⇧↑ and
⇧↓ extending a selection by one row on the screen (upstream took a whole
line of the buffer, which in the composer is a paragraph), and the view
following the caret all the way after a paste or dictation (upstream
scrolled one line per change, enough for typing only), and Up or Down
after typing keeping the caret's column (upstream measured the column
at the edit, against the layout from before it, so it came out missing
and the caret went to the start of the row; it is now measured at the
move). The two before that were
checked with `EMAKI_KEYS=shift-up` and a forty-line `EMAKI_TYPE`; a real
paste and a dictation app were not driven from a script.

**Inline code is the accent on a wash of itself, as the Claude app draws
it.** `md_view` sets the letters to `theme.link` (the accent's readable
shade: darker on cream, lighter on charcoal) and `inline_code_chip` to
`theme.primary` at 10% (16% dark) with a border at 18% (22% dark), so
both follow the accent in `config.json`. The plate is the vendored
toolkit's `Inline::paint_code_chips`: a text highlight's own background is
a square box the full height of the line, so the plate is painted before
the text instead, one rounded quad per line of each span, 1.35 times the
font size tall. A text run has no padding either, so the vendored
markdown parser sets each span between two narrow no-break spaces
(U+202F) at each end: the plate takes in the nearer one as its padding,
the farther one is its margin, and gpui's line wrapper counts that
character as part of a word, so they stay with the span at a line's end
(a breaking space left the next line starting with a gap). The view's
Copy action takes the pairs out again; the copy buttons read the model
and never see them. The Claude app sets inline code at 0.9em, and a gpui
text run carries a face but no size, so the smaller size is a font of
its own: `Inline Anthropic Mono`, the same font with a larger em
(`fonts::inline_code_family`; without it inline code is the size of the
paragraph). What still differs from the Claude app: the plate has about
2px of margin where theirs has none, and a span that wraps gets a plate
per line with no padding at the break.

**Code is set in Anthropic Mono when the machine has it.** The Claude
app's live stylesheet (fetched from `assets-proxy.anthropic.com`, not the
copy inside the bundle, which only shows the fallbacks) declares
`@font-face{font-family:anthropic-mono; src:url(….woff2)}` and
`--font-mono: "anthropic-mono", ui-monospace, monospace`. The font is
downloaded when the app runs; the bundle's `Resources/fonts` holds the
serif and the sans and no mono, so there is no file to load it from the
way those two are, and it is Anthropic's, not ours to ship.
`fonts::install` therefore also registers whatever is in
`~/.emaki/fonts`, and `fonts::code_family` takes the first family that
starts with "Anthropic Mono" (the file calls itself "Anthropic Mono
Web"), else on macOS `.AppleSystemUIFontMonospaced` (SF Mono, what
`ui-monospace` is there), else the toolkit's own (Consolas, DejaVu Sans
Mono). `look::install` writes it into both theme configs as
`mono_font_family`, so it is every mono in the window: inline code, code
blocks, tool subjects, the path band. `scripts/anthropic-mono.py` is how
the files get into `~/.emaki/fonts`: given the roman and italic woff2 the
stylesheet names, it unpacks each to TrueType and writes a second copy
under the family `Inline Anthropic Mono` with the em enlarged by 1/0.9,
which is the face inline code is drawn in. The URLs carry a content hash
and change, so read them out of the
stylesheet again. The official rule for inline code, for comparison, is
`code:not(pre code)`: the mono face at `.9em`, `padding: .0625em .25em`,
`border: .5px solid` at about 15%, `border-radius: .4em`, the text in
the danger red on a 5% wash of the text colour. First guess, wrong: SF
Mono, read from the stylesheet inside the app bundle.

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
sees and `emaki-core inbox <id> <text>` delivers by hand. "Driven" is the window's record and the hub's together
(`Workbench::driven`): the hub lets a driver go by itself (idle past its
limit, its child gone) without a word, and the record alone then kept a
terminal session reading as driven, so every message was refused with
"the driver is gone; try again" while its inbox stood by.

**Never splice a list item the reader may be inside.** gpui's `ListState`
moves the scroll anchor to the start of any spliced range that contains it.
A live session's last round is one tall item, so re-splicing it on every
reload sent the reader from wherever they had scrolled to the top of that
round, the middle of the conversation. Items render from the current
session on every frame and are re-measured as they render, so `set_detail`
only tells the list about a change in count (append new rounds, reset on a
rewrite), and toggling a tool, thought, run or thumbnail just notifies.

**A prompt and a reply each show a copy button on hover.** Under a
prompt's bubble, on the right: when it was sent (`format::stamp`, the
clock today, the date with it on another day) and the button. Under a
reply's last line, on the left: the button, then when its last words
were written (the last text item's time). Both lines are gpui
groups (`prompt-<ix>`, `reply-<ix>`) at opacity 0 until the pointer is
over the prompt's row or anywhere in the reply, and both are always laid
out at `HOVER_ROW_H`, so nothing moves when they appear and the list has
nothing to re-measure. The button copies the markdown as written, not
the rendered text: the prompt's words, or `Round::reply_markdown`, every
text item of the round with a blank line between and tool calls and
thoughts left out. The text is read out of the session at the click
(`Workbench::copy_text`), and the icon is a tick for a second and a half.
The line under a prompt used to read "You · Emaki 19:39 · 41.7s · 4 tool
calls · 19.7k tokens" all the time; only the time is kept, and a prompt
that was not the person's own still says whose ("Another session",
"Session"). Probing note: a `mouseMoved` `CGEvent` sent with `postToPid`
moves gpui's hover without moving the real pointer, so a hover state is
one event and a window capture away; mouse-down and mouse-up sent the
same way did not reach a click handler in a background window.

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
by tool, the last subject, the total time, and the agent's turning mark with
"running…" while one is still going. Opening the row shows every call, each
folding on its own. Fewer than three stay inline.

**A tool card names its tool once.** The small badge at the left says the
tool's own name in lower case (`tool_label`: "bash", "read", "write", and
for an MCP tool the last part of its name), then comes the subject. The
badge used to say the kind ("run", "read", "write") with the name in a
larger face beside it ("Bash", "Read", "Write"), the same word twice for
most tools. The run row's tally counts by the same label ("3 bash · 1
write"), and a subagent's calls are drawn the same way.

**The agent's mark moves while it works.** Claude's glyph is Claude's own
starburst (`assets/icons/claude.svg`, the brand mark as simple-icons
carries it; the greeting has the app's icon, below). `agent_glyph` turns it once every 2.8 s while it breathes
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

**The modes, models and effort levels are the agent's own lists.**
Nothing in the window names one. `options.rs` in the core holds what an
agent offers (`Options`: `modes`, `models` each with its `efforts`, and
the key that stands for the agent's default model), an adapter fills it
(`Adapter::catalogue`, which also carries the slash commands; an agent
the app cannot drive offers nothing), and the pills, their lists and the
settings panel draw what came back, so a release that adds a model shows
it with no change here and another agent brings its own. Claude Code
(`driver::options_from`, checked against 2.1.289): the models are the
`models` of the `initialize` reply a headless child gives, twelve here,
each with `value`, `displayName`, `description`, `resolvedModel` and
`supportedEffortLevels` (Haiku takes none, the 4.6 models have no
`xhigh`); the modes are what `claude --help` lists as the choices of
`--permission-mode`, the only place it lists them (no request does:
`set_permission_mode` takes one or refuses it), with `manual` there
being `default` on the wire; the effort levels at large are `--effort`'s.
The wire names modes and levels and gives no words for them, so
`driver::mode_words` and `effort_words` carry Claude Code's own titles
("Manual", "Accept edits", "Extra high") and a line on each for the names
known today, and a name never met is made readable (`options::humanize`)
and offered all the same. `Hub::options_for(agent, cwd)` answers per
folder once that folder has been asked (when a session there is opened,
or a driver starts), else with the last answer from anywhere, which at
launch is `cache/options.json` from the last run; it never starts a
read. `emaki-core options [folder]` asks and prints. Before this the
lists were constants in `driver.rs` (five modes, five model aliases,
five levels) and the settings panel had a segmented control for each;
with a dozen models those are the same picker the composer has.

**A mode and an effort wear a colour, and every value is bold.** On the
pills and in the conversation's lines alike (`PillText`, `render_notice`):
the choice in semibold, the words "mode" and "effort" after it plain. A
mode's colour is the agent's own when it has one (`Choice::color`, light
then dark): Claude Code names each mode in a theme colour at the foot of
its prompt, read out of the 2.1.289 binary into `driver::mode_color`
(grey for manual, teal for plan, purple for accept edits, amber for
auto, red for bypass and don't ask). A mode with none, and any agent
that gives none, gets one by its place on the list (`workbench::ramp`:
blue, green, amber, purple, red from low to high, `RAMP`, each a light
and dark pair in the shades Claude Code's two themes use). Every colour
here is a pair, and the window's own appearance picks: a light window
takes the light theme's shade whatever theme the person's terminal is
on. On 2026-10-06 the colour was read off the terminal instead
(`#FFC107` for "Auto", from a terminal on the dark theme) and used in
both appearances; the person took that back the same day: the dark
gold in a light window was right, and the bright one is the dark
window's. An effort level is Claude
Code's too: its `/effort` slider keeps a table of levels with a theme
colour each (`driver::effort_color`: amber for low, green for medium,
periwinkle for high, purple for xhigh), and draws max as a moving
rainbow, which here is the rainbow's seven colours run through the
letters, still (`Choice::spectrum`, `workbench::tinted`). Those
seven are pastels for a dark ground; a light window takes
`SPECTRUM_LIGHT`, the same seven taken down to shades that read on
cream. The first
version coloured the levels by their place alone, which was not what
the terminal shows. The model is bold and uncoloured.

**The pills have no lists, and what needs the terminal opens it.** Every
choice on a session is made in its terminal, with Claude Code's own
picker or key, so a pill under the composer is a button on every channel
(`Workbench::pill_clicked`); the lists they opened on a driven or
unstarted session are gone (they had read "no driver behind this session"
once the terminal was closed). `Workbench::via_terminal` is the one way
in for anything only the terminal takes (`TerminalAction`: a pick from
`/model` or `/effort`, a typed command, ⇧Tab for the mode, or just going
there): with the session in a terminal it is done at once; with none,
the terminal is opened first the way the top-right button opens it (the
agent's resume command, an idle driver stopped on the way, a driver
mid-reply refused), the row under the composer says "opening the
terminal, one moment…" for as long as it takes, and the action waits in
`pending_terminal`. Nothing in the wait is a length of time, since a
slow machine takes as long as it takes: `terminal_ready` looks, on the
clock and whenever the terminal's status line runs, for two things to be
so. Claude Code has registered there and its registry record says `idle`
(a login or trust screen says `waiting` and is never typed into), and
its prompt is on the screen, which is the mode's footer under it, read
off the terminal where it can be read (`terminal_up`); then the action
is done. The wait ends otherwise only when the person leaves the
session. From there on it is the same as with the terminal already
open: after ⇧Tab the window takes the front back at once, since the key
asks nothing of the person, and a pick leaves them in the terminal to
choose and brings the window back when the pick is made. That, too, is
a state and not a time (`watch_terminal`): the registry record says
`waiting` while the picker is up and something else once it is closed,
chosen or cancelled, with the time of each change, so the pick is over
when the record has been seen `waiting` since the command was typed and
no longer is, or says `idle` as of a time after the typing (a picker
opened and closed between two looks). A change alone is not the sign:
recorded on 2.1.289 with `claude` on a pty, the record goes `busy`,
`waiting` 7 ms later, and `idle` at the choice or at Escape. Two earlier versions were wrong. The first waited a second
and a half after `idle` and gave up after forty-five. And the come-back
was measured by the status line naming another model or effort, or the
transcript moving: after a fresh open the status line's first run can
differ from a stale file, and the resume itself writes rows, so the
window came back before anything was chosen. Before a session exists there is no terminal to
open: the pills show what a new session starts in, and a click opens
Settings on New sessions, where the two lists that remain are (`picker`,
naming each choice with a line on what it does, scrolling when long).
Checked with a second copy and `EMAKI_GO=pill:effort` on a session with
no terminal: the terminal opened, the row said so, and nothing was typed
while the registry read `waiting`. Probing note: `open` hands a newly
launched terminal app the opener's environment, so a probe copy run
with `CLAUDE_CONFIG_DIR` at a scratch tree starts a terminal whose
Claude Code is not logged in; have the terminal app running first.
A pill says the agent's name for the choice
(`mode_name`, `model_name`: a session reports `claude-opus-5-5` and the
list says that is "Opus 5.5"), with "mode" after a one-word mode and
"effort" after a level; `driver::model_label` reads an id that is not on
the list (`claude-3-5-sonnet-20241022` is "Sonnet 3.5"). A switch goes
through `Hub::set_driver_mode` / `set_driver_model`, which answer with the
mode Claude Code actually holds: a refused switch (bypass without the flag)
puts the pill back and says why on the status row. ⇧Tab in the composer
cycles the mode as Claude Code's terminal does; the wrapper captures the
textarea's own `OutdentInline` for it. Checked against 2.1.284:
`set_permission_mode` accepts `auto` and answers `{"mode": "auto"}` but
sends no `system/status` frame for it, unlike the other modes, so the
reply is what the driver trusts.

**The row under the conversation says what the terminal says.** While a
turn runs, Claude Code draws a line over its prompt: a mark that turns, a
word that changes ("Embellishing…", or the task in hand), and the turn's
figures in a bracket ("(13s · ↓ 1.0k tokens)", "(3s · thinking with
medium effort)"). The row shows that line as written and in its colours:
the word in the colour of the terminal's mark, the bracket in the muted
ink where the terminal has grey, and every colour as the window's
appearance has it. The screen is in whichever theme that Claude Code is
set to, which need not be the window's: `driver::theme_pair` holds
Claude Code's light and dark themes side by side (`THEME_PAIRS`, 53
named colours read out of the 2.1.290 binary), a colour found on either
side is answered with the pair, and the window takes its own side
(`workbench::shade`). So the mark's yellow while the agent thinks
(`warning`: 255, 193, 7 in the dark theme) is the light theme's dark
gold (150, 108, 30) in a light window, and the terracotta it has while
writing (215, 119, 87) is the same in both. A colour in neither theme
is only kept readable, and plain text takes the agent's colour. Until
2026-10-06 the colours were drawn as read: a dark theme's bright yellow
on cream, which the person called wrong, asking for the terminal's real
colours in the theme the window is in.
`Workbench::read_working` reads the screen of the session showing once a
second and whenever its status line runs, one read at a time off the main
thread (`sys::terminal_styled`: WezTerm and Kaku write the colour
sequences out with `cli get-text --escapes`, Terminal and iTerm2 give
plain text and the word takes the agent's colour), and
`driver::working_on_screen` finds the line: among the last lines, one of
the spinner's marks, a space, words ending in "…". A finished turn's
line has no ellipsis and a tool call's begins with another mark. The
bracket carries the turn's time, so the row's own clock is left out
then. A screen without the line for a moment keeps the last word three
seconds (`WORKING_KEPT_SECS`). Once a turn has been quiet for a while
the terminal draws the mark in bold behind `ESC ( B`, a sequence that is
not a colour; read as text it hid the line for as long as the quiet
lasted, so every sequence that is not a CSI is skipped. With no line to read (a driven session,
an IDE's terminal, another agent) the row says "<agent> is working…"
with what the transcript shows and the clock, as before. Checked with
`EMAKI_SHOT` on this session in Kaku mid-turn.

**A turn is stopped from the window, and a stop in the terminal shows
here.** Escape in the terminal usually leaves a "[Request interrupted"
row, which `turn_state` reads. Escape during `/compact` leaves nothing:
the `/compact` prompt, a caveat row, and no boundary, so the transcript
read as working for ever. Claude Code's registry record carries `status`
(`idle` or `busy`) and `statusUpdatedAt`, and that is the word on
whether a turn is running: `Hub::settle_stopped` runs on every scan and
`TurnState::settle_idle` turns a working state into your turn, chip
"interrupted", when the registry has been idle since after the rows the
state was read from (idle from before them is the registry not having
caught up with a turn that just began). The row "Claude is working" has
a Stop pill at its right end (`Workbench::interrupt`): a driver takes
an `interrupt` request; a terminal session gets Escape in its terminal
(`sys::key_in_terminal`: WezTerm and Kaku by `cli send-text` to the
pane and iTerm2 by `write text`, neither coming to the front; Terminal
and other hosts by a System Events key press after being brought
forward), then the scan is asked for twice so the row goes within a
second or so. Escape in the window does the same when it has nothing
to close (`Workbench::escape_stops`): the slash list, settings, the
lightbox, the search and the find bar come first, then the running
turn. The textarea keeps its own Escape action, so the composer's
wrapper captures it and passes it on in that order. Until 2026-10-04
the key only closed things. A host
that had to come forward for the key gives the front back to Emaki once
it is pressed. A stopped turn hands its prompt back
(`Workbench::restore_prompt`), as Claude Code's terminal does on Escape:
the words in the composer with the caret after them and the attachments
on the chips again, from the message as it left the window when that
was the last thing sent to the session (`last_sent`, pictures
included), else from the transcript's last round by its paths. It
happens on the Stop click and when the index shows the session going
from working to stopped (`was_working`), which is Escape in the
terminal, and never over something already typed. And only when the
terminal would: a turn stopped before the agent wrote or ran anything
gives the message back, as if it had not been sent; once the agent has
started it has the message, so the stop only stops, the round stays,
and the composer is left empty for what comes next. The test is the
withdrawal's own (no item in the round but notices), applied to
`Session::withdrawn` or, while the stop's marker is not yet in the
transcript, to the last round. Until 2026-10-04 any stop brought the
last prompt back, started on or not. A command that acts by itself
(`/compact`) is never handed back: typed through the hidden terminal it
ran to its end, the registry said idle a moment before the boundary's
rows were in the file, the turn read as stopped for one scan, and
"/compact" came back into the box. The scan now reads the registry
before the transcripts as well, which narrows that moment without
closing it (Claude Code writes its rows late). The conversation lets
go of what the composer got back: "[Request interrupted by user]" is
Claude Code's marker, not something the person said, and is never a
round of theirs: a turn the agent had started on ends on it as a line of
its own, below, and a prompt stopped before the agent wrote or ran anything is taken out
of `rounds` and kept as `Session::withdrawn` (`handle_user` in `build`),
which is what `restore_prompt` reads first; left in, the message showed
twice once it was sent again. A prompt the agent had started on stays
where it is. That round ends on the stop, said in the agent's own
words (`NoticeVariant::Interrupted`, a warm plate at the round's foot):
the text is read out of the transcript, never written here, so it is
"Request interrupted by user" for Claude Code (the marker with its
brackets off; "…for tool use" when that is what it wrote) and for Codex
the reason of its `turn_aborted` event ("Interrupted") or, where only
the next user row says it, the first sentence inside `<turn_aborted>`.
`build::push_interrupted` is the one way in for every adapter and says
a stop once however many rows record it; a new agent's builder calls it
with whatever that agent writes. The terminal's own line on the screen
("Interrupted · What should Claude do instead?") is not what is shown:
it is in no transcript, and is gone at the next prompt. A withdrawn
prompt gets no line, as the round it would sit in is gone and the words
are back in the composer. For Codex the line is also the state:
`turn_state_from_session` reads a round ending on it as your turn,
where a prompt with no reply read as working for ever. No Codex session
on this machine had been stopped, so its two shapes are covered by a
test written from Codex's source, not from a real rollout. The Stop click and the restore were checked by the person
on a session in Positron's terminal; the withdrawal is covered by a
test.

**An IDE's terminal has to be given the focus before keys are sent.**
Terminal, iTerm2, WezTerm and Kaku take text for a tab or pane by its
tty. An IDE built on VS Code (Positron, Cursor, VS Code) is typed into
with System Events, and takes keys wherever its focus was left: an
editor, the file tree. Bringing the app forward does not move that, so
`/compact` sent from the window landed in a file. `Host::
focus_ide_terminal` (any host with `Contents/Resources/app/
product.json`) first asks the IDE's command palette for "Terminal: Focus
Terminal" (⌘⇧P, the words, ↩), which lands in the terminal panel
whatever held the focus and is harmless from the panel itself, where
the toggle on ⌃` would close it. `type_in_terminal` and
`key_in_terminal` both go through it, and before it through
`Host::wait_front`, which asks System Events which process is frontmost
and sends nothing until it is the host: activating an app is a request
the system may grant late, and keys sent early land in whatever is in
front, Emaki's own composer included. It focuses the terminal the IDE
has active, which with several open may not be this session's: nothing
outside the IDE can pick one by its tty. The palette step ran in every
effort and model check below, and six digits typed through it
(`EMAKI_GO=type:123456`) arrived whole; the case it exists for, focus
left in a file, was not reproduced from a script.

**Mode, model and effort are three pills on every Claude conversation.**
A light grey pill with an icon in front (`icons/shield.svg`, `gauge.svg`,
`box.svg`), darker under the pointer, darker again while pressed,
no tooltip and no caret (`composer_pill`; the toolkit draws a custom
button colour at a fifth of its strength, so the resting grey is the
muted ink thinned). A pill is a button, and the choice is made in the
session's terminal with Claude Code's own picker
(`Workbench::pick_in_terminal`, through `via_terminal`, which opens the
terminal when the session has none):
the terminal comes to the front, the person chooses, and the window
comes back by itself. A terminal session takes nothing over its inbox
(it reads everything as prose, and a `control_request` frame sent to it
is dropped without a reply, tried against 2.1.284), so there is no other
way in. The model is a bare `/model` typed there, which opens the list,
and the effort a bare `/effort`, which opens the slider. Done is the
status line naming another model or effort (`state/context/<session>.json`,
read once a second while the person is away, `watch_terminal`), or on an
idle session the row the command leaves in the transcript, which a
cancelled picker also writes ("Kept model as …"); nothing is written
while the picker is open. `/model` on a warm cache asks "Switch model?"
before it switches, and the window comes back after that answer. In
both pickers Enter saves the choice as the default for new sessions and
`s` keeps it to the session; that is Claude Code's and the person's to
decide. The mode has no command and no picker. Only ⇧Tab changes it,
stepping through an order that depends on flags of the session nothing
outside it can read (`isBypassPermissionsModeAvailable`,
`isAutoModeAvailable` and a gate), and Claude Code's key bindings offer
`chat:cycleMode` and nothing that names a mode. So ⇧Tab in the composer
presses the key once in the terminal and stays in the window
(`Workbench::step_mode`, `sys::key_in_terminal` with
`TerminalKey::ShiftTab`, back-tab, ESC [ Z): WezTerm, Kaku and iTerm2
take it for the pane without coming forward, any other host comes
forward for it and the window takes the front back at once. The pill
follows each press, and a click on the pill only says "Use ⇧Tab to
change the mode". Two earlier versions: the click went to the terminal
and came back once the status line had been quiet for a moment after a
press, which worked when it reran and not otherwise, so the person was
sometimes returned and sometimes left; then the click went there and
stayed, which the key in the composer made pointless. What the pills show on a terminal session
is the status line's word for the model and the effort (`terminal_ctx`),
else the transcript's. The mode is read off the terminal's screen
(`sys::terminal_text`: Terminal and iTerm2 by AppleScript, WezTerm and
Kaku by `cli get-text`; `driver::mode_on_screen` looks in the last three
lines, the footer under the prompt, for each listed mode by its own name:
"manual mode on", "plan mode on", "accept edits on") when the session is
opened, when the window comes back, and every time the terminal's status
line runs, and kept in `mode_seen` until a turn starts, whose prompt row
carries the mode. The status line is the signal: the hub watches
`state/context/` beside the transcripts and sends `HubEvent::Context`
for the session whose file was rewritten, which a ⇧Tab causes within
about a third of a second, so the pill follows each press, and the model
and the effort no longer wait for the one-second clock. One read at a
time (`mode_reading`). It was read only on coming back, two seconds
after the last press, with the pill saying "Mode" in between, which read
as slow beside the other two pills. An IDE's terminal cannot be read, so after
a change there the pill says "Mode" until the next turn rather than the
transcript's old mode. Tried before and taken out: a list in the window
for a terminal session, which for the model and the effort typed
`/model <name>` and worked, and for the mode pressed ⇧Tab a counted
number of times, which twice ended in the wrong mode on a live session
(auto to plan and back ended in plan; default to auto passed plan and
landed on default), and then only said which key to press. Do not bring
the counting back: one of the modes after plan can be bypass. One press
for one press, which is what ⇧Tab in the composer sends, aims at nothing
and is not that. Checked on
a session in Kaku with `EMAKI_GO=pill:mode`, `pill:model` and
`pill:effort`: two ⇧Tabs and the window was back within three seconds
with the pill on the footer's mode; a model picked with `s` and the
"Switch model?" answered, back within a second; the slider moved and
confirmed, the same. (`enter_terminal`, which put the keyboard in an IDE's terminal for the mode pill, went with that pill's trip to the terminal.)
A driver has no control request for the effort
(`set_effort` is "Unsupported"), but `/effort <level>` as a user turn runs
as the local command it is, and Claude Code records it as a
`system/local_command` row with `commandRun: {command: "effort", args}`,
which `build` reads into `Session::effort`; `Driver::set_effort` sends that
turn and the pill updates when the file does.

**A change of mode, model or effort is one short line in the
conversation.** `NoticeVariant::Mode`, `Model` and `Effort`, whose text
is what was set and nothing else, drawn as the pill's icon, the word in
the muted ink and the value in the foreground, on no plate ("Effort
High"; `transcript::render_notice`). `/model` and `/effort` answer with
a sentence ("Set effort level to high (saved as your default for new
sessions): Comprehensive implementation with extensive testing and
documentation"), which sat under the command's chip on one plate and ran
out of the column; `build::setting_said` reads what was set out of it
(the model between backticks, the level after "Set effort level to"),
`RoundBuilder::add_output` puts the line in place of the chip, and a
picker closed with nothing changed ("Kept model as …") leaves no line.
Anything else those commands say is shown as it is. A mode has no
command and no row: Claude Code writes nothing when ⇧Tab is pressed (the
2.1.289 binary only sets a field), names the mode on the next prompt row
and restates it in a `permission-mode` row whenever it writes rows. So
`RoundBuilder::saw_mode` says a change where the transcript first has
it, which for a prompt is the foot of the round before, and the first
mode a session names is where it began, not a change. Until then the
window says it: `Workbench::unwritten_mode` is the mode read off the
terminal, or the driver's, when it differs from `Session::mode`, drawn
at the foot of the last round in the same words, and the built notice
takes its place at the next turn. The other notices changed with these:
all are in the window's face, not the reply's serif; a command's output
is quiet text behind a rule that wraps; compaction and errors keep a
tinted plate. A setting changed again straight after itself is one
change, the last (`RoundBuilder::add_notice` replaces a notice of the
same kind when it is the last thing said): effort to high, medium, high
reads "Effort High" once, while effort, model, effort keeps all three.
The line at the foot for a mode not yet written stays when the person
steps away and back to the mode the transcript has (`mode_touched`): it
was dropped as "no change", and the person had changed it. The rule
for all three is one: AAABBB reads A, B and ABAB reads A, B, A, B.
`mode_touched` keeps every change of mode seen since the last turn with
its time, and `render_round` sets each before the first item written
after it; the setting lines the transcript folded
(`Round::superseded`) are put back among them and the folding is done
again over the whole, so effort, mode, effort is three lines and mode,
effort, mode is three. Before that there was one unwritten mode line,
at the foot, and a second change of mode replaced the first across
whatever lay between. Once the next turn writes the mode, the
transcript's order is the only one there is: the mode is recorded with
the prompt, after the efforts, which fold again.
`NoticeVariant::said` is the line as the page reads it,
which is what find, search and the markdown use. Checked with
`EMAKI_SHOT` on a hand-written transcript; the lists under the pills and
the read of a real terminal's footer after ⇧Tab were not run from a
script.

**The limits row is the terminal's status line.** Under the composer, on
the left of the row the notice shares:
`Context 37% (386k of 1M) · 5h 3% (4h26m) · 7d 6% (6d7h)`, coloured at the
same thresholds as `~/.claude/statusline.sh`. The context is the
transcript's (`Session::context_tokens`, the last assistant row's input plus
cache read plus cache creation) over the model's window
(`limits::Limits::context_window`: what a driver's `result` frame reported
in `modelUsage`, else an assumption; `claude-fable-5-1` answered 1,000,000).
The window is the half the transcript cannot give: one model id comes in
two sizes (`claude-opus-5-5` is 200k or 1M by the session's choice), so
after a `/model` to Opus the row took 238k of an assumed 200k and read
119% where the terminal said 23%, and every figure after it was five
times too large. The status line is handed the size, so the script also
leaves `state/context/<session>.json` (model, window, tokens in use) and
`Limits::window_for` takes the session's own window from it
(`limits::session_context`, read with the windows), else the size
last seen for the model (`learn_window`), else the assumption, and never
a window smaller than what is in it: more tokens than the window means
the window is the 1M one. `Session::models` ends on the model in use,
so a `/model` back to an earlier one counts, and `<synthetic>` is not a
model. Files a month old are pruned at launch.
The five-hour and seven-day windows are per account and reach Emaki two
ways, the newer winning: a driver's `rate_limit_event` frames, and the
terminal's status line. Claude Code writes the windows to no file a
terminal session leaves behind; the status-line command in
`~/.claude/settings.json` is the only place it hands them out, on stdin,
once per refresh. So Emaki has a status line of its own,
`scripts/statusline.sh`, built into the binary (`statusline::SCRIPT`): it
prints the terminal's line and, when `~/.emaki/state` exists, leaves the
windows in `state/rate_limits.json`, written whole and renamed into place,
which `Limits::refresh_from_statusline` reads once a minute on the clock
(`LIMITS_SECS`, the countdown's own resolution) and whenever the session
showing is loaded, which is when the file changes.
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

**A question of Claude's is a card, and what channel the session is on
decides who answers it.** `AskUserQuestion` is a tool call, so the
conversation draws it as a card wherever it was asked (`transcript::
render_question_call`): each question with its header chip and its
options, the chosen one ticked once the result's sidecar carries
`answers` (`ToolCall::answers`, kept by question; words typed instead
are quoted under the options; "not answered" when the person declined).
It never folds into a run of tool calls. Who answers depends on the
channel. A driver of ours gets the question as a `can_use_tool` request
like any permission (checked against 2.1.288 with `emaki-core drive`:
the request carries the questions, the reply is allow with the input
completed by `answers`, question text to the label chosen, which is how
Claude Code's own dialog answers; `Driver::answer_question`,
`driver::question_decision`), so the card where permission cards sit
(`Workbench::render_question`) takes a click on an option, a button
when there are several questions or several choices (`picks` holds the
staging), or whatever is typed in the composer (`send_message` routes
words to `answer_question_typed` while a question waits, and the
placeholder says so). A bare ↩ never answers a question, and "Allow
all" skips them. A terminal session's question lives in the terminal's
dialog: the inbox socket takes only `auth` and `user` frames (read out
of the 2.1.288 binary), so no frame from here can answer it; the card
below does, with keys. Where the screen cannot be read, the
status row under the conversation says "Claude is waiting for your
answer · in your terminal" (the same line says "below" for a driver,
and "your approval" or "your go-ahead on the plan" for the other
holds). `emaki-core drive` answers a question with its first option, so
the wire can be checked from a terminal.

**The prompt the terminal suggests is the composer's placeholder.** Once
a turn is over Claude Code sets words in its input that are not typed
yet, dim, and → takes them. The composer offers the same words the same
way: as its placeholder, in the placeholder's lighter ink, with "(→ to
accept, ⌘↩ to send)" after them; → or Tab in the empty box makes them
the text with the caret at the end (`Workbench::accept_suggestion`,
captured before the textarea's own `MoveRight` and `IndentInline`), and
⌘↩ on the empty box sends them as they are. `read_suggestion` reads the
screen once a second while the session showing is idle in its terminal
and the box is empty, and `driver::suggestion_on_screen` finds them: the
line between the prompt's two rules, after "❯", all of it written dim
(`ESC [ 0;2 m`), which typed words are not. That needs the colours, so
WezTerm and Kaku only. A path in the words is a link (`ESC ] 8 ;; url
ESC \`), whose sequences ran into the rule below and hid it;
`escape_at` is the one place that says how long a sequence is, for all
four screen readers. Taking the words here leaves the terminal's own
suggestion where it is. Checked with `EMAKI_SHOT` and `EMAKI_KEYS=tab`
on a session in a Kaku tab; → goes through the same handler and was not
pressed from a script.

**A terminal session's dialog is a card in the window, answered with
the terminal's own keys.** While a question waits in a terminal, the
transcript does not have it: 2.1.289 writes the assistant row with the
`AskUserQuestion` call only once it is answered (checked on a pty: no
such row while the dialog was up), so the window showed "Claude is
working… reading the prompt" and the question appeared after the fact.
What says a dialog is up is the registry (`status: waiting`, with
`waitingFor` "input needed" for a question and "permission prompt" for
an approval), and what says which is the screen. `Workbench::read_dialog`
reads it once a second and whenever the status line runs, for the
session showing, and `driver::dialog_on_screen` parses it: under a rule,
the tabs when there are several questions ("← ☐ Fruit ☒ Colors ✔ Submit
→"), the question, the numbered choices with their lines of description,
"[ ]" and "[✔]" on a question that takes several, and at the foot
"Enter to select · … · Esc to cancel"; the review ("Review your
answers", each answer behind "→", "1. Submit answers / 2. Cancel"); an
approval (the command between dashed rules over "Do you want to
proceed?"). `render_dialog` draws it where the permission cards sit, in
their shape: the questions as chips, ticked once answered, what is
asked, the choices as rows, the review's two choices as buttons. The
terminal's dialog is the one that moves: a click sends the choice's
digit (`sys::text_in_terminal`: WezTerm and Kaku by `cli send-text
--no-paste`, iTerm2 by `write text`, the pane never coming forward),
Next sends Tab, Cancel sends Escape, and the card is the screen read
again. The chips are the terminal's tabs, the review last: the one
showing is ringed, which the terminal says by setting it on a ground of
its own (`Dialog::current`, read from the colours, so the screen is
read with `--escapes`), and a click on another goes there with the
arrow keys, one step at a time, each once the screen shows the tab
before it (`dialog_go`, `DialogUntil::Tab`). Where the colours are not
given (iTerm2) the chips do not take clicks. Words typed in the composer answer a question that offers "Type
something" (`dialog_answer_typed`): the digit, then the words, then
Return, each once the screen shows what the one before did
(`DialogUntil`: the pointer on that choice, then the choice reading as
the words). All three in one write lost the words and answered with the
first choice. Not done: words of one's own on a question that takes
several, whose field works another way, and Terminal or an IDE, where
keys need the app in front; there the line "waiting for your answer · in
your terminal" stands. Not shown while the person was sent to the
terminal for `/model` or `/effort`, whose list is theirs to use there.
Checked on a session in a Kaku tab with `EMAKI_GO=answer:Mango`,
`dialog:1`, `dialog:tab`, `goto:2`, `goto:1` and `EMAKI_SHOT`: the
typed answer, a tick, Next, the review, Submit, and going to the review
and back to a question all landed. `emaki-core screen < text` says
what a screen holds. Probing note: `kaku cli spawn` hands the new pane
the caller's environment, so a session started from inside a Claude Code
session is its child and never registers; start it from a script that
unsets `CLAUDE*` first.

**A slash command runs only where a session is driven from here.** A
headless child runs `/compact` as the command it is (the 2.1.288 binary
marks it `supportsNonInteractive`; checked on the wire: a `status:
compacting` frame, the boundary, then a `result` with no turns, so the
driver's turn ends as usual), and the list of commands it knows comes
with `initialize` (`DriverView::commands`). A terminal session's inbox
cannot: Claude Code wraps every `user` frame on that socket as a message
from a peer, with or without our envelope (checked by sending a bare
`/effort low`: the terminal showed "Message from @…" and the model
answered that it cannot run a command sent by a peer), so there is no
frame that passes for typing and no way to make a message from here
read as the person's own. `send_message` refuses a slash command on the
inbox channel with a notice saying to type it in the terminal, rather
than spend it as a prompt. While a message is a slash command being
typed, a list of the session's commands opens over the composer
(`render_slash_help`: the driver's own, `BUILTIN_COMMANDS` before one
has started, one line of explanation for a terminal session), a click
completing the name.

**What only the terminal can take, the window takes you to.** The
conversation stays in the person's terminal, because keeping that
history is the point of the app, so for a question's dialog, an
approval, a slash command or the mode, Emaki brings that terminal to the
front and comes back when it is done (`Workbench::go_to_terminal`). The
button is wherever the need shows: on the waiting line under the
conversation, on the slash-command card, on the mode, model and
effort pills, and the top-right terminal button, which for a session
already in a terminal goes there instead of refusing. `sys::focus_terminal`
finds the app: the session's pid from the registry, then up the parent
chain until a process is an application the system knows
(`NSRunningApplication`), which is the terminal app or the IDE holding a
terminal (this very session ran under Positron). Within it, where the app
can be asked, the tab or pane on the process's tty is selected first:
Terminal and iTerm2 by AppleScript, WezTerm and Kaku (built on WezTerm)
by the `cli list` / `activate-pane` binary beside the gui, keyed on
`tty_name`; anything else is activated as an app and left as it was.
A slash command on a terminal session goes the same way, typed: the
list over the composer (`render_slash_help`) is the folder's own
catalogue, read once per folder from a headless child's `initialize`
reply (`driver::catalogue`, `Hub::commands_for`: 87 entries here, names,
descriptions and argument hints, no model call, no transcript left, about
half a second; the built-ins stand in until it is in), and a click on a
row, or ⌘↩ on the typed command, sends the text to the terminal
(`Workbench::run_in_terminal`, `sys::type_in_terminal`): Terminal takes
`do script` in the tab on the tty, iTerm2 `write text` in the session,
WezTerm and Kaku `cli send-text --no-paste` with a carriage return to the
pane, and any other host (an IDE's terminal) System Events keystrokes
after the app is in front, which need Accessibility access for Emaki;
refused, the text goes on the clipboard and the row under the composer
says so. On a driven session a click sends the command through the
driver. Checked by hand against Kaku with `EMAKI_GO=type:/status`.
The list is a card of two columns and a foot (`render_slash_help`): the
name with its argument hint in the first, 36% of the card, what it does
in the second, each cut with an ellipsis, because a hint like
`/code-review`'s ran out of the card when it was allowed its own width.
Eight rows show (`SLASH_ROWS`) and the window slides with the choice.
The keys are the composer's while the list is open
(`Workbench::slash_key`, captured before the textarea as ⇧Tab is): ↑ and
↓ move the choice, round the ends, ↩ picks it, ⇥ puts its name in the
text, Escape puts the list away until the text changes.
The choice (`slash_sel`) goes back to the first row on every change to
the text, so typing and ↩ takes the best match (within a rank the
shorter name first: "/co" is /color, /config, /compact before a plugin's
long name). The foot names the keys and what ↩ will do to the row chosen
("run" or "insert"), the place in the list when it is longer than the
card, and on a terminal session "runs in the terminal", a click on which
goes there.

**Two kinds of slash command, told apart by name.** `/compact`, `/model`
and `/clear` act by themselves; `/ph-image` or `/code-review` is a skill,
a prompt for the model, which a person as often names inside a sentence
("use my /ph-image skill to…"), and running one unasked cannot be taken
back. The catalogue on the wire cannot say which is which: it marks
`builtin` (55 of 87 here) and bundled skills are built in too. The
2.1.288 binary can: every command declares `type` `local`, `local-jsx`
or `prompt`, and `driver::LOCAL_COMMANDS` is the first two, read out of
it; `driver::acts_alone` answers, and a name not on the list is taken
for a skill, the harmless side to be wrong on. `Workbench::slash_pick`
(↩ or a click) runs a command only when it acts alone and is the whole
message; anything else goes into the text at the caret with a space
after it (`slash_insert`). ⌘↩ on a message that is only a command still
sends it as that command, a skill with its arguments included. The list
follows the caret, not the start of the message
(`driver::slash_token_at`: a "/" at the start or after a space, and the
name characters around the caret), and inside a sentence it offers
skills only. Every command a message names is coloured in the accent:
in the composer by ranges on the textarea (`Workbench::mark_slash`, set
again on every change; the vendored toolkit's `set_marks`, which is why
this is not the live markdown rendering the composer gave up on), and
in a sent prompt by `driver::mark_commands`, which sets it as inline
code before the markdown view sees it, leaving code spans and fenced
blocks alone. A token counts when `driver::slash_tokens` finds it as a
word of its own (`/usr/bin` and `a/b` are not) and its name acts alone
or is in the folder's catalogue (`Hub::knows_command`, cache only; the
catalogue is read when a Claude session of that folder is opened).
`EMAKI_KEYS` dispatches those actions through the focus, since a
synthetic key does not reach a background window.
Coming back: when the session was waiting or idle at the hand-off, the
next change to its transcript is the interaction done, and the window
activates itself (`come_back`, forgotten after fifteen minutes); when the
agent was working, the next change would be its own, so nothing is
armed. Set aside at the time: a terminal of Emaki's own
(gpui-terminal on alacritty, or Ghostty) and a tmux bridge, as owning
sessions rather than following the terminal. A session with no terminal
at all now does run on one of our own, with no window (the hidden
terminal, below), and so does one that is idle in a terminal of the
person's.

**The window's terminal is a hidden one.** Everything above
that "opens the terminal" used to open the person's terminal app, do
its work there and come back. Since 2026-10-05 the terminal is one of
our own with no window (`pty.rs` in the core): the interactive `claude
--resume <id>` on a pty this process owns (`portable-pty`), its output
fed to a screen model that draws nothing (`vt100`). The child is an
ordinary interactive session: it registers in Claude Code's registry
with an inbox, runs the status line and writes the same transcript, so
it is one more terminal session and the code that follows a terminal
follows it unchanged. What differs is the backend: every `sys` function
that takes a session's pid (`terminal_text`, `terminal_styled`,
`type_in_terminal`, `key_in_terminal`, `text_in_terminal`) asks
`pty::for_pid` first, where the screen is in memory (`Pty::styled`
writes it out one sequence an attribute, the form the readers in
`driver` take) and a key is a write, on every platform, with no app
coming forward and no Accessibility access. The channel is `pty` in
`reply_via_for`, asked after the driver and before the inbox, and
`in_terminal` is either. Checked with `emaki-core pty <cwd>` on
2.1.289: up and registered in 0.7 to 1.4 s, ⇧Tab read back off the
footer 100 ms later, `/effort` and `/model` drawn with the registry
saying `waiting`.

*When it starts.* `Hub::start_terminal`, from three places: a message
sent to a session with no process, or from the new-session page (begun
with `--session-id`, in the mode and model chosen); anything
`via_terminal` is asked for (a pill, ⇧Tab, a command); and the first
character typed in the composer of such a session
(`Workbench::warm_terminal`), so it is up before the message is
finished. Not on opening a session: a resumed Claude Code writes to the
transcript (the file grew on a resume that was given no input), and a
conversation only read would move to the top of every list and read as
live. The person asked for it to be loaded on entering the
conversation; with a start under a second and a half the first
keystroke was chosen instead, for that reason. The headless `claude -p`
driver remains as what a session falls back to when the pty cannot be
started or `driver.hidden_terminal` is false, and for the catalogue and
the explainer.

*A message is typed.* `Hub::send_to_terminal` waits for Claude Code to
have registered and its prompt to be on the screen
(`driver::prompt_on_screen`), pastes each picture's path by itself
(bracketed paste; Claude Code turns the path into the picture, "[Image
#1]", checked: the user row carries an image block) and waits for the
screen to show one more "[Image #" than it did before going on, since
Claude Code reads the file and sets the mark in a moment later, a large
picture later than a small one; it pastes the words,
waits for the prompt to show them and presses Return. So the row is the
person's own, not a peer's: no envelope, no held message under bypass,
no thirty-second repeat drop, and a slash command runs. The wait
for a picture counted the marks on the whole screen from nothing, and
the conversation above the prompt shows the marks of earlier messages:
with one in view the wait was over before it began, Return was pressed
while a 714 KB screenshot was still being read, and the message went
with the smaller of its two pictures (2026-10-06; on a pty the words
pasted straight after two paths came out in front of both marks). It
counts from what the screen showed before each paste now. Not run end
to end from a script: `EMAKI_GO=send:` carries no attachment. A terminal that
shows something else for five seconds is put in front of the person
(`HubEvent::TerminalNeeded`). Not typed while the registry says
`waiting`: the words would answer the dialog.

*When it is seen.* `Workbench::render_terminal` draws the screen on a
card where the dialog cards sit (`pty::panel_rows`: what is under the
line of "▔" Claude Code opens a picker beneath, else the screen without
its blank edges), each row one `StyledText` in the mono face with the
terminal's colours, on a dark ground or a light one by Claude Code's
own `theme`. The card holds the focus (`term_focus`, key context
`Terminal`): `term_bytes` turns each key into what a terminal sends,
Tab and ⇧Tab are bound there so the toolkit's focus traversal does not
take them, and Escape is Claude Code's while the card has the keyboard.
The pointer works on it too, though Claude Code's interface takes no
mouse: `pty::hits` finds what a click can mean on the screen and the
keys that do it, and the card draws each such stretch as an element of
its own, lit under the pointer. Three things: a numbered choice, reached
from the one "❯" is on with that many arrows; a level under a slider,
reached from the one under "▲" with arrows left or right; and a key the
screen names ("Enter to confirm", "s for this session only", "Esc to
cancel", "Tab to toggle"), which is that key. A click on a choice or a
level confirms it too, with Return after the arrows (Return alone on
the one already chosen): at first it only moved there, and the person
asked not to have to press Enter after. Return saves the choice as the
default for new sessions, as it does in the terminal; `s`, for this
session only, is the key named at the foot. The arrows and Return go in
one write, which 2.1.289 takes whole (two lefts and Return set the
effort to low and closed the slider). The mapping is covered by a test; the
click itself was not pressed from a script.
Escape is Claude Code's whenever the card is up, wherever the keyboard
is in the window. The card is asked for at the click, before the picker
is drawn, and is still asked for an instant after it closes; drawn
then, it showed the whole conversation's screen and jumped from tall to
small. So nothing is drawn until a picker is on the screen
(`pty::picker_up`) or the registry says the terminal is waiting.
It shows for the model and effort pills at once, for a typed command
once the registry says `waiting` (`/status`, `/config`), and by itself
for a waiting screen `dialog_on_screen` cannot read (`term_auto`); it
goes when `watch_terminal` sees the wait over, or by its close button,
which sends Escape. ⇧Tab and Stop never show it. A click on the mode
pill is one ⇧Tab (`pill_clicked` to `cycle_mode`): the pill only said
which key to use while the key meant a trip to the person's terminal.

*A terminal of the person's is left alone.* The person's rule: what is
done in the window has nothing to do with any terminal of theirs, and
the hidden terminal is what the window uses, always. Two versions got
this wrong on the same day. The first followed a session into the
terminal it was open in, which for an IDE's terminal is the switch, the
command palette and a mode pill that cannot name its mode. The second
ended the Claude Code in that terminal and resumed the session hidden;
the person's VS Code terminal fell back to its shell, which is the app
reaching into their terminal all the same. Now `reply_via_for` answers
`inbox` for a session in a terminal of theirs only while a turn is
running there; between turns it answers `spawn`, as for a session with
no process, so a typed character, a message, ⇧Tab, a pill or a command
starts a hidden terminal on it beside theirs, and nothing is sent to,
typed in or ended in theirs. `Hub::peer_for` answers with the hidden
terminal's own record once there is one, never the other terminal's,
and `refresh_peers` keeps ours of the two. Mid-turn the window starts
no second Claude Code on the running turn: a message goes to that
terminal's inbox, which queues it, Stop and a dialog's answer go where
the turn is, and a mode, a pick or a command says to wait
(`theirs_busy`). This gives up one writer per transcript when the
person has the session open in a terminal and uses it from the window
too: both processes append to the one file, and the one in their
terminal does not know what was said here until it is resumed. That is
theirs to decide, and they did. Checked with a second copy
(`EMAKI_HOME`), `emaki-core pty --resume` standing in for their
terminal and `EMAKI_GO=step:mode`: the mode stepped on the hidden
terminal, the pill named it, and the other process was still running.

*The terminal button means the default terminal.* "Your terminal" is
the app the system keeps for shell scripts, the one `open` hands the
`.command` file to (`sys::in_default_terminal`: the host app of a
process, by `host_of`, against `NSWorkspace`'s app for the script). The
button looks at every record the registry has for the session
(`peer::registry_all`), ours left out: one in the default terminal,
however it got there, is brought forward and nothing is opened; one in
any other terminal (VS Code's, Positron's) counts as not open, and the
session is opened in the default terminal beside it. Refused onto a
running turn. The hidden terminal is left running: it was let go on
the way at first, and the next ⇧Tab then waited for a new one to come
up, which the person read, rightly, as the button having killed it.
There was a guard here for a minute after the click
(`handed`) under which nothing hidden was started on the session; with
it a ⇧Tab or a pill in that minute ran the terminal's script a second
time, and a message started the headless driver, both on a session the
person had just opened in their terminal. It is gone: with the hidden
terminal on, `via_terminal` never opens the person's terminal. Checked
with a second copy and `EMAKI_GO="step:mode;button:terminal;step:mode;step:mode"`
against a real Kaku: with the session in Kaku the button opened nothing
and Kaku's process outlived every key; with the session only in another
process the button opened it in Kaku, and that process outlived the two
⇧Tabs after it.

*Letting go.* The hub keeps the terminals by session. One that
is not showing and has been idle past `driver.idle_min` is let go, and
all of them at quit; one that dies by itself is said once on the row
under the composer with the last line of its screen. Probes:
`EMAKI_GO=send:<words>` sends a message once the session (or the
new-session page) is up, `EMAKI_TERM_KEYS=left,s` presses keys on the
card a moment after it shows, and `EMAKI_PTY_LOG=<file>` keeps the
child's raw output. Checked with those and `EMAKI_SHOT`: the effort
slider on the card and gone after `s`, ⇧Tab moving the pill, a message
to a session with no process, a new session, `/status` on the card and
gone after Escape. The person ran the effort and model pickers on a
real session. Not run from a script: the hand-over to a real terminal,
a permission prompt through the hidden terminal (it goes through
`read_dialog`, as for any terminal), and Windows beyond the type check.
A probe run from a session that is itself in a hidden terminal must
not quit the running Emaki, which is its parent: use a second copy.

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

**Every session has a composer of its own, and so has the new-session
page.** There is one textarea, so `Workbench::sync_draft` makes it
stand for whichever is showing: on arriving somewhere else (`draft_key`:
the session, or the new-session page) the words and the attachment
chips are put away under where they were typed (`drafts`) and what was
left here is brought back with the caret at its end. It runs at every
draw and before `restore_prompt`, which asks whether the box is empty
on a session's behalf. Setting the value is not a change event, so
bringing a draft back starts no hidden terminal. The board and the
sessions page draw no composer and change nothing. Drafts are in
memory and go with the process. Until 2026-10-06 it was one box for the
window, and words typed for one session followed the person into the
next. Checked with `EMAKI_TYPE` and `EMAKI_GO="open:<id>;page:new"`:
the new-session page's words were gone on two sessions and back on the
page; two sessions each holding words was not driven from a script,
since nothing types into the box mid-run.

**A right click opens a menu of our own.** `Workbench::open_menu`
puts a small card where the pointer is (`render_menu`, kept inside the
window, over a clear sheet any click or Escape puts away), and
`menu_pick` does what was chosen (`MenuDo`). A folder, in the sidebar
or on the new-session page's cards, offers "Open in Finder"
(`sys::OPEN_FOLDER_LABEL`; the sidebar's folder is a project, so its
path is the newest session's `cwd` that is still a directory, and one
with none says the folder is gone). A session in the sidebar offers
Rename and "Reveal in Finder", which shows its transcript. The toolkit
has a context menu too, built on actions; three choices did not need
that. **Renaming a session renames it, for Claude Code too.** Rename opens a
field over a scrim (`render_rename`, `rename_input`; ↩ keeps the name,
Escape or a click outside leaves it, an empty field changes nothing).
Nothing here writes to a transcript, so the rename is Claude Code's to
make: `try_renames` types its own `/rename <name>` into the session's
hidden terminal (starting one when the session has none), and Claude
Code writes a `custom-title` row, which its resume list reads and
`transcript::pick_title` now puts before any AI title (it writes the
same words as an `agent-name` row, which must not disqualify them).
Not typed into a running turn: the name waits in `renames` and goes
when the turn is over. Meanwhile, and for a session that cannot be
asked (another agent, one kept only, a folder that is gone), the name
is ours: `~/.emaki/state/titles.json` by session key, laid over
`SessionRef::title` as each index arrives (`HubEvent::Index`), and
dropped from there once the transcript says the same. Whatever is
still in that file at launch is asked for again. The first version
(2026-10-06, an hour earlier) kept the name in that file only, and the
person, looking at the terminal's resume list still saying the old
title, said a rename means a rename. Checked: `/rename`, typed and
pasted, on a pty writes the `custom-title` row and `emaki-core list`
shows the name; the menu, the field and the name on the row and the
tab with `EMAKI_GO=menu`, `renaming`, `name:<words>` and `EMAKI_SHOT`.
Not driven from a script: a real right click, the ↩ in the field, the
two Finder actions, and the rename from the window end to end (a probe
copy's Claude Code is not logged in).

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

**Three buttons at the top right take a session somewhere else.** In
this order: the terminal (continue it there), the project folder (the
session's `cwd`, opened in the file manager by
`Workbench::open_project_folder`, refused with a notice when the folder is
gone), and the transcript (the JSONL revealed in the file manager, under a
file icon so it does not read as a second folder). The terminal button
used to sit alone at the top left, with the reveal button and an agent
badge at the top right; the split read as two unrelated things, so they
are one group now and the left end holds only the sidebar button. The
agent is named by its mark on the tab and its name over every reply.

**Under the tabs is the session's folder, and nothing else.** A band
across the content pane (`path_line` in `render_detail`): a faint
background between two hairlines, as a file manager's path bar is, with
the absolute path centred in it in the mono face behind a small folder
icon, the parents dimmed and the folder's own name in the foreground. In
a narrow window the parents are what truncates. Clicking the path opens
the folder, the same `open_project_folder` the button above calls. Two
earlier versions failed: a line of plain grey text reading "~/path ·
⎇ main · 2 rounds · 22 tool calls · 200.9k tokens · model · id", too much
to read, and then the path alone on a small rounded plate, which sat
under the active tab's plate and read as two tabs stacked. The model and
the context are under the composer, and the rest is in the rendered
markdown.

**The first of them continues the session in your terminal.**
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
terminal registers, the channel flips to `inbox` on its own. "A terminal
already" is `Workbench::in_terminal`, the inbox channel, not the registry
alone: a driver's child registers an inbox too, and the button took a
driven session for a terminal one, walked up from the child to find the
terminal app, met Emaki itself and said "could not tell which app the
terminal is" (an idle driven session was refused as "Already open in a
terminal" for the same reason). Checked by
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
job, which is why CI leaves the release a draft: `scripts/release-check.sh`
holds a commit to what the tag will be held to before it is tagged
(a Windows type check included), and `scripts/release-publish.sh` uploads
the notarized images, checks them from the outside and publishes. **WORKFLOW.md is the procedure**: the version bump (`Cargo.toml` and
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

**The icon is drawn, on a canvas.** `scripts/icon/icon.html` is the
source: a handscroll (絵巻) seen from the front, a white sheet with a navy
edge between two terracotta rolls on navy axles, on a cream ground. On
the sheet is a conversation, the agent's line with a violet four-point
spark in front of it, the reply in amber indented to the right, the agent
again. The cream, navy, amber and violet are the Alkaline Software
organisation icon's; the terracotta is the first icon's ground. Everything
is in a 100-unit box, and `drawIcon(canvas, icon, px, bleed)` draws it at
any pixel size as the full square (`bleed`) or as the rounded app icon,
inset 9% with a corner radius of 23 units and a drop shadow; the page
itself is the proof sheet, the icon at 280, 128, 64, 32 and 16px with
pixel zooms of the last two. `scripts/icon/render.swift` loads the page
in a `WKWebView` with no window and writes what `exportIcon(size, bleed)`
returns for each file, so every size is drawn on its own pixels and none
is a scaled copy; `scripts/icon/ico.py` packs the Windows `.ico`, and
`scripts/make-icon.sh` runs it all in under two seconds; nothing but a
Mac is needed. The conversation reads down to about 64px; at 16px the
icon is two bars with a pale block between. Two icons came before. Until
2026-10-03 it was cut out of a generated picture (`logo.png`, in git
history), which read as a render and could not be changed without
generating again. Until 2026-10-04 it was `draw.py`, an SVG of a scroll
with a prompt chevron on a terracotta plate, rasterised by Cocoa from
one master. The new one was designed outside this repository, in JS
canvas and again in ggplot2; the canvas version is the one copied here,
whole, and this copy is the app's source. The PNGs are the rounded icon
with its margin, which the Dock (`sys::install_dock_icon` sets the
PNG at start), Windows and Linux want; the `.icns` alone is the full
square, opaque to its corners, because macOS 26 masks every app
icon to its own rounded square over a grey backing and shows anything
transparent, a margin or the icon's own rounder corners, as a grey border
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
in `Workbench::tabs`, and going from one to another is a browser's
switch: the `Detail` left is put away whole (`stashed`: the session,
the list with where it was scrolled, what was unfolded) and the one
arrived at is taken out and drawn in the same frame, then read again
from disk and brought up to date in place. Only a tab never shown since
launch waits for its read, and the pane is empty meanwhile under tabs
that stay put. Until 2026-10-06 there was one `Detail` and every switch
dropped it and read the file again; the read is fast, but the pane drew
"loading…" under a top strip with a title where the tabs were, so the
whole pane and its tabs blinked at each click. A closed tab's `Detail`
is let go. Checked with a line printed for every frame drawn with no
`Detail`, in a second copy switching between two sessions four times:
one such frame per session, at its first open, and none after.

**The tabs share the row, and a change in their number is a move.**
Every tab has the same width (`sync_tab_widths`, at every draw):
`TAB_MAX` (200) while the row has room, the row divided among them once
it has not, down to `TAB_MIN`. A new tab grows in from nothing while the
others give way, and a closed one's room is taken up the same way, over
`TAB_ANIM` (`tab_widths`: from, to, when, a number that is the
animation's id); a change in the row's own width (the window resized,
the sidebar folded) is followed at once. The row's width is measured as
it is painted (`tabs_row_w`, a `canvas` laid over it) and used at the
next draw. Before, each tab was as wide as its title up to 220 and the
flex row did the shrinking: a new tab was drawn at full width for a
frame and the row then snapped narrower, which the person saw as a
blink. A tab is dragged along the row to another place (`on_drag` with
a `DragTab`, the row's `on_drag_move` to `drag_tab_to`, which takes the
place under the pointer as the pointer moves; the order is kept in
`ui.json`): its title follows the pointer on a plate (`TabGhost`) and
its own tab is dimmed meanwhile. The others change places at once, with
no slide. ⌘1 to ⌘9 (Ctrl elsewhere) go to that tab, or to the last one
when there are fewer, as Safari does (`go_tab`). A right click on a tab
is the session's menu from the sidebar, Rename and Reveal. Checked with
`EMAKI_SHOT`: eight tabs sharing the row, and a third tab caught
part-grown and part-faded a moment after `EMAKI_GO="open:<id>;open:<id3>"`
opened it. Not driven from a script: the drag, the keys, the right
click (a synthetic press does not reach a handler in a background
window).

**Empty space along the top moves the window, and the app says which.**
The window is opened with `app_owns_titlebar_drag`, and
`Workbench::drag_region` wraps the top strip and the sidebar's header:
a press there becomes `start_window_move` once the pointer moves with
the button down, and a double click is `titlebar_double_click`. Left to
AppKit, the whole strip under the transparent title bar moved the
window, tabs included, so a tab could not be dragged. A tab and the two
buttons beside the lights take their press first (`press_taken`: in
gpui's bubble phase a child's listener runs before its parent's), so a
press on them never moves the window. Read from gpui's source, as Zed's
own title bar does it; not pressed from a script.

⌘W is one global `CloseTab` binding that closes the showing tab, then
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

**A session begun in the window is listed in the terminal like any
other.** Claude Code stamps every row with its entry point, a `-p`
child's being `sdk-cli`, and the terminal's resume list (`claude
--resume`, `/resume`) leaves out every session whose first rows say
`sdk-cli`, `sdk-ts` or `sdk-py` (2.1.289; the picker reads `entrypoint`
from the head of the file). So a session started here could be resumed
by its id, which is what the terminal button does, and never found from
the terminal itself. The entry point is the environment's to name
(`CLAUDE_CODE_ENTRYPOINT`, as the IDE extension and the desktop app name
theirs; only `cli` is rewritten to `sdk-cli` on a headless run), so the
driver's child is started with `emaki` (`driver::ENTRYPOINT`), its rows
say so, and the picker lists it. The explainer's children keep
`sdk-cli`: they are not conversations. Checked on a pty: a `-p` session
with the variable set was offered in the picker, one without was not,
and `emaki-core drive` ran a turn through the driver unchanged. A
session whose first rows already say `sdk-cli` stays out of the list,
since nothing here writes to a transcript; the terminal button still
opens it.

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
settings panel shows as little as it can: the version, one button that
reads *Check for updates* until a check finds a newer version and *Update
to x* after, in the same place, a small underlined *Release notes* link
under it only then, and a bare tick for the daily check. The detail line
speaks only when there is something to say (downloading, a failure, up to
date after a click). Three pills and two sentences of explanation sat
there before and were asked to go. `emaki-core update` is the check from
a terminal.

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

**A slash command answers its own prompt.** `/compact` typed in the
terminal is written as a user row saying `/compact`, then the rows the
command leaves (`<command-name>` in a user row, or a
`system/local_command` row with `commandRun`), and no assistant row.
`turn_state` would read the prompt as one waiting on a reply and the board
would say working, with the clock running, until the next turn; so a
prompt that is a slash command with a command row of the same name after
it is skipped, and the state is the turn before it. A skill (`/review`)
leaves no command row and is a prompt like any other. In the
conversation the command's own chip is left out when the prompt above
already says it, and `/compact`'s output line, "Compacted (ctrl+o to see
full summary)", is the terminal's instruction and is dropped: the round
reads as the prompt and the boundary's notice, nothing else. Only that
command's output goes (`RoundBuilder::command` is the command the next
output belongs to): a `/model` run later, still in the `/compact`
round because no prompt came between, keeps its "Set model to …" line.
The first version dropped every output in a round that began with
`/compact`.

**The rule over the prompt may carry the session's name.** Once a
session has a name (`/rename`), Claude Code writes it on the rule above
its prompt ("──── My session ─"). The screen readers took a rule to be
a line of "─" and nothing else, so on a renamed session no prompt was
found: `type_message` waited for one that was already there, and a
message sent from the window left the composer and went nowhere (the
day the rename was built, 2026-10-06). `driver::is_rule` is the one
test now, for the prompt, the suggestion and the dialog: a line that
begins and ends as a rule and is mostly one. Covered by a test; found
by resuming the renamed session with `emaki-core pty`.

**A session nothing was said in is listed nowhere.** `/clear` starts a
new transcript under a new id and carries the session's name into it
(`custom-title` and `agent-name` rows, then the `/clear` rows), so the
sidebar showed two sessions of one name, the second empty. `peek` marks
a transcript `blank` (`SessionRef::blank`, `transcript::says_something`)
when the whole file fits in the head it reads and holds no reply,
nothing queued and no prompt; a command that acts by itself (`/clear`,
`/resume`) is not a prompt, a skill is. The index keeps such a session,
so the archive copies it like any other; the window drops it as the
index arrives (`HubEvent::Index`) unless it is the one showing or a
process of ours is behind it (a session begun here is blank for a
moment), and drops its tab with it; `emaki-core list` leaves it out.
Once something is said there it is a session like any other, under the
name Claude Code gave it. Covered by a test; the window itself was not driven from a script.

**A change that lands during a read is read again.** The conversation
showing is reloaded when its file changes (`HubEvent::Changed`), and a
change that came while a load was under way was dropped. A prompt with
a pasted picture is one long row: the load began as it was being
written, read the file without it, the small rows 8 ms later were
dropped, and nothing else was written until the agent's first words
fifteen seconds on, so the person's own message was missing from the
window while "thinking" ran under it. `reload_wanted` now marks such a
change and the load, once back, runs again. Found from the rows' times
in the transcript; not reproduced on purpose.

**A queued message keeps its pictures.** The queue's `enqueue` row has
the words only ("[Image #23]…"), and the picture is in no row until the
message is taken up, so the round at the foot showed no thumbnail.
`Workbench::dress_queued` puts the attachments of the message as it
left the window (`last_sent`) on the queued round when the words match;
a message queued from a terminal still shows without them.

**A message sent mid-turn is never a user row.** Typed in the terminal
or sent from the window while the agent is working, it is absorbed into
the running turn (`queue-operation` `remove`, reason `absorbed_mid_turn`)
and written as an `attachment` row of type `queued_command` with
`commandMode: prompt`, the text or content blocks under `prompt`, and the
`origin` a user row would carry. Seventeen such messages on this machine,
none of them with a user row to match, so a transcript that ignored
attachment rows lost every one. `build::queued_prompt` turns the row into
the user row it stands for and `handle_user` opens a round for it in its
place, with what the agent did next under it; `image_block_bytes` reads a
pasted picture from `attachment.prompt` by the row's uuid. The
`task-notification` rows in the same shape are the harness's and stay
out. The turn state is untouched: the turn it cut into is still running.
That row is written only when the agent takes the message up, at its
next step, which behind a long tool call was half a minute later: the
window showed nothing, and the person sent the message again from the
terminal. What is written at once is the queue: a `queue-operation` row,
`enqueue` with the message as `content`, then `dequeue` (the front one
starts a turn of its own) or `remove` with the same content (absorbed).
`build::Queue` replays them, and whatever is still queued at the end of
the file is drawn as a round of its own at the foot of the conversation
with `Round::queued` set, saying "Queued, Claude will read it at its
next step" under the bubble, until the attachment row lands and the
next build puts it where it was taken up. A harness notification in the
queue is kept for the order and never drawn. Replayed over every
transcript on this machine, 296 enqueues met 222 dequeues and 74
removes, and nothing was left over.

**The compaction summary is not a prompt.** After the boundary, Claude
Code writes the summary it hands the model as a `user` row flagged
`isCompactSummary` (and `isVisibleInTranscriptOnly`: its own view hides
it too). It opened a round of the person's, "This session is being
continued…", and left the board working. `build::machine_authored` says
which user rows are Claude Code's own (that one, and `isMeta` without a
peer origin), and `turn_state`, `handle_user` and `first_prompt_title`
all skip them. The boundary row's `compactMetadata.postTokens` becomes
`context_tokens` at that point, so the row under the composer drops to
the summary's size the moment compaction lands instead of holding the
old figure until the next assistant row.

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
treat a peer row as a prompt or the log shows a reply to nothing. **The same
words twice go with a space after them.** Claude Code drops a peer's
message that is identical to that peer's last one within thirty seconds
(2.1.289: `dedupWindowMs: 30000`, keyed on the sender's name or pid; the
terminal says "Dropped a peer message from @emaki (unknown): identical
to the previous message from this sender"). It is a guard against two
sessions echoing each other, and it also caught a person stopping a turn
and sending the message again. `Hub::send_to_inbox` remembers what last
went to each session (`inbox_last`) and `peer::send` takes `again`,
which adds one trailing space; the envelope keeps it, the message is
still read as ours, and the prompt is trimmed where it is drawn. The
person chose this over a notice, as the case is rare. Checked with
`emaki-core inbox <id> --again <text>` against `claude` on a pty; the
drop itself is keyed on the pid, so two runs of the CLI do not reproduce
it, only the app does. **Do not
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
