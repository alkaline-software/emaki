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

Compaction is *not* a threat: it appends a boundary and keeps writing to the
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
"archived", under its own agent.

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

## Rules that hold everywhere

Each is here because breaking it loses data or reintroduces a bug that was
already fixed once. The doc named after a rule has the full story.

- **Nothing writes to a transcript, and nothing enters a session's
  context.** A rename, a mode, a command: Claude Code makes the change, by
  its own command typed in its terminal. (`docs/window.md` on rename,
  `docs/channels.md`)
- **Copy first.** `archive::sweep` runs before anything renders, and the
  archive is never fed its own file as a source. (`docs/transcript-format.md`)
- **No hooks, ever.** A hook at a path that moved exits 2, and Claude Code
  reads exit 2 as "block every prompt". The one write to Claude Code's
  settings is the status line, whose script never exits non-zero.
  (`docs/core.md`)
- **One writer per transcript where we can choose.** Never start a second
  Claude Code on a running turn. (`docs/channels.md`)
- **Nothing is sent to, typed in or ended in a terminal of the person's.**
  The window uses its own hidden terminal. (`docs/channels.md`)
- **Never `--bare` on a `claude` child** (it skips the keychain and breaks
  subscription users), and strip every `CLAUDE*` variable but
  `CLAUDE_CONFIG_DIR` from a child's environment. (`docs/channels.md`)
- **Never assert `from-mode` on an inbox message.** (`docs/transcript-format.md`)
- **Never press ⇧Tab a counted number of times to reach a mode.** One of
  the modes on the way can be bypass. One press for one press.
  (`docs/modes.md`)
- **Waiting on the terminal is waiting on a state, never a length of
  time.** The registry's status and what is on the screen say when.
  (`docs/channels.md`, `docs/modes.md`)
- **Never splice a list item the reader may be inside.** gpui moves the
  scroll anchor to the start of the spliced range. (`docs/conversation.md`)
- **A click the window swallows goes through `swallow_click`,** or the
  toolkit's text drag never ends. (`docs/window.md`)
- **Every input the window owns sits in a focus wrapper, and the focused
  element must be one the page draws,** or key bindings go nowhere.
  (`docs/window.md`)
- **Every text passes `markdown_is_safe` before the markdown view gets
  it.** The parser panics on some inputs, and a panic in layout aborts the
  app. (`docs/conversation.md`)
- **The composer is plain text.** Live markdown in it was tried three ways
  and dropped. (`docs/composer.md`)
- **One copy of gpui.** The pin is in `Cargo.lock`, not a `rev` of our own.
  Changes to the toolkit go in `vendor/gpui-component/`, marked and listed
  in its `UPSTREAM.md`. (`docs/platform.md`)
- **Everything Unix-only is gated with a stated fallback.** Windows and
  Linux are build targets. (`docs/platform.md`)
- **Whatever appears or disappears does so in motion.** A menu, a card,
  a panel, a list: it comes in over a moment and goes out over one, never
  from one frame to the next, and what is around it moves with it. Use
  the pattern its neighbours use; the person does not ask for this each
  time. (`docs/window.md`, Motion)
- **A shortcut is named as the Mac has it.** When the person says ⌘ and a
  key, bind `secondary-<key>`, which is ⌘ on a Mac and Ctrl on Windows and
  Linux, and write ⌘ in the docs. They do not say this each time.
  (`main.rs`)
- **The explainer's and the outline's model calls are real Claude Code
  calls on the person's account.** Cache the answer, ask for no more than
  is looked at, and never call one from a test. (`docs/core.md`,
  `docs/panels.md`)

## Before you edit, read

Open the doc for a file before changing that file. This is not optional
for a small change: the traps are in the docs and nowhere else. A subagent
handed work on one of these files is told to read the doc too.

| You are about to touch | Read first |
|---|---|
| `emaki-app/src/transcript.rs`; the conversation's list, find, or reload in `workbench.rs`; `emaki-core/src/find.rs` | `docs/conversation.md` |
| The composer, its attachments, the slash and "@" list, drafts, the notice and limits row in `workbench.rs`; `a11y.rs`; `emaki-core/src/files.rs`, `limits.rs`, `statusline.rs`; `scripts/statusline.sh` | `docs/composer.md`; the slash list's keys are in `docs/channels.md` |
| The pills, pickers, mode/model/effort notices; `emaki-core/src/options.rs`; `mode_*`, `effort_*`, `model_*`, `options_from` in `driver.rs` | `docs/modes.md` |
| The working row, Stop, `restore_prompt`, question, dialog, permission and background-command cards; the screen readers in `driver.rs` (`*_on_screen`) | `docs/live-turn.md` |
| `emaki-core/src/driver.rs`, `pty.rs`, `peer.rs`, `terminal.rs`; `emaki-app/src/hub.rs`, `sys.rs`; `reply_via_for`, `via_terminal`, the terminal card and button in `workbench.rs` | `docs/channels.md` |
| `emaki-app/src/term_panel.rs` | `docs/channels.md` (The terminal panel) |
| `emaki-app/src/panels.rs`, `file_icons.rs`; `emaki-core/src/outline.rs`, `git.rs` | `docs/panels.md` |
| The sidebar, sessions page, tabs, settings panel, menus, rename, `route_scroll`, the top strip in `workbench.rs`; `main.rs`; `ui_state.rs`; `format.rs` | `docs/window.md` |
| `look.rs`, `fonts.rs`, `assets.rs`, `assets/`, `themes/`, the avatar; `scripts/anthropic-mono.py` | `docs/look.md` |
| `emaki-core/src/build.rs`, `transcript.rs`, `archive.rs`, `model.rs`; anything that reads a Claude Code row or its registry | `docs/transcript-format.md` |
| `emaki-core/src/adapters/`, `search.rs`, `explain.rs`, `store.rs`, `render_md.rs`, `paths.rs`, `config.rs`, `watcher.rs` | `docs/core.md` |
| `vendor/`, `Cargo.toml`, `.github/workflows/`, release and icon scripts, `update.rs`, any `cfg` for an OS | `docs/platform.md` |
| Checking a change in the window from a script | `docs/probing.md` |

A change that crosses two rows reads both docs. `workbench.rs` is in most
rows: go by what the code you are changing does.

## Layout

`crates/` is all of Emaki: no browser, no daemon.

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
  src/outline.rs             a conversation's outline: an entry a round, and the labels a small model writes for them
  src/options.rs             the modes, models and effort levels an agent offers, as it lists them
  src/explain.rs             opaque tool calls in plain words, via `claude -p`
  src/git.rs                 what git says of a folder's files, as VS Code's explorer shows it
  src/files.rs               a folder's files for "@" in the composer: the list, the match, the tokens
  src/update.rs              the newest release, its installer, and putting it in place
  src/statusline.rs          scripts/statusline.sh built in, installed to ~/.emaki/bin at launch, and the one setting
  src/terminal.rs            the agent's resume command as a script a terminal can be handed
  src/pty.rs                 a terminal of our own with no window: an interactive `claude` on a pty, its screen kept in memory
  src/watcher.rs             notify over every agent's data roots
  src/bin/emaki-core.rs     list | render | build | archive | sync | search | bench | shells | outline | git | files | peers | inbox | options | explain | update | statusline | drive | pty
  tests/core.rs
crates/emaki-app/         the window
  src/hub.rs                 threads: scan -> archive -> index, drivers, watcher
  src/workbench.rs           sidebar, session list, search, composer
  src/transcript.rs          drawing rounds, tool cards, thoughts, subagents
  src/panels.rs              beside a conversation: the folder's files as a tree, and the outline
  src/file_icons.rs          the tree's icons: Catppuccin's, and which a name gets
  src/term_panel.rs          the terminal at a conversation's right: a shell, and the agent's hidden terminal drawn whole
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
docs/                      how each part works and its traps; the index below says which to read
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
```

**Each document has one job.** This file is what Emaki is, the rules that
hold in every session, and where to read next. `docs/` is how each part
works and why. STAGES.md is where the app is going, in three stages; a
change of direction goes there. PLAN.md is the detailed plan under it:
phases, tasks with checkmarks, decisions and their dates. WORKFLOW.md is
how to cut a release, CHANGELOG.md what each release changed for the person
installing it, and README.md how to use the app and how it is built.

## Running it

**One Emaki at a time.** Two copies share `~/.emaki` and both write
`state/ui.json`, the Dock shows two identical icons, and the one the person
looks at is usually the old build, so the change "is not there". The
exception is a probe copy with its own `EMAKI_HOME` (`docs/probing.md`).

**After a change to the app, relaunch it with `scripts/relaunch.sh`.** The
person develops Emaki in Emaki, so at the end of any task that changed the
app, build it (`cargo build -p emaki-app`) and run `scripts/relaunch.sh` as
the last command before the final reply, without being asked. A change to
documents alone needs no relaunch. The script returns at once; fifteen
seconds later, or when the turn is over if that is later, it quits the
running Emaki and starts `target/debug/Emaki`. The agent's session is a
child of the app, so quitting the app ends the session: the script waits
while the session's registry record says `busy`. Never `pkill` by hand from
inside the app, and look at a change before the relaunch with a probe copy.

## Writing things down

- A note on how a feature works, a trap, or a fact learned about Claude
  Code or gpui goes in that feature's file under `docs/`, in its `## Rules`
  when it is something that must not be broken.
- This file gets a line only for a rule every session must follow whatever
  it is working on, or a new row in the table above. Keep it under 300
  lines.
- Write what is so now and why. An approach that was tried and dropped gets
  one sentence, and only when someone might try it again. What was checked
  and how belongs in the reply to the person, not in a doc.
- Do not record what the code says by itself: a constant's value, a pixel
  size, a list of function names.

## Testing

```
cargo test -p emaki-core
cargo build -p emaki-app
```

`tests/core.rs` points `EMAKI_HOME`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME` at
a temp tree (`isolated()`); inherit that for anything touching disk. The
suite must pass before committing, and `rust.yml` runs it on all three OSes.
