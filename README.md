# Emaki

**Every coding-agent session on your machine, kept for good, in one window
you can also talk to.**

Emaki is a desktop app for macOS, Windows and Linux, written in Rust on
[GPUI](https://www.gpui.rs) (the Zed editor's UI framework). It reads Claude
Code and Codex sessions, copies every one of them before the agents' own
expiry sweeps it, searches all of them, and lets you continue a Claude Code
session from the window. Nothing here writes to a transcript, and recording
adds nothing to a session's context window.

## Install

Download the installer for your machine from the
[latest release](https://github.com/alkaline-software/emaki/releases/latest):

```
Emaki-mac-arm64.dmg            Apple silicon
Emaki-mac-x64.dmg              Intel Mac
Emaki-windows-x64-setup.exe    Windows
Emaki-linux-x64.AppImage       Linux (a .deb is there too)
```

The Mac image is signed and notarized. Windows is unsigned, so SmartScreen
asks once: *More info*, then *Run anyway*.

Or build it from a clone with a Rust toolchain from [rustup.rs](https://rustup.rs):

```
cargo build --release -p emaki-app
./target/release/Emaki
scripts/make-app.sh                      # a local Emaki.app under dist/
```

The first launch copies every transcript it can see into the archive, so
nothing you still have is at risk from that moment on. There is no daemon,
no hook and nothing written into Claude Code's settings.

Emaki keeps itself current: it asks GitHub for the newest release once a
day (a tick in Settings turns that off) and says so in the settings panel,
where *Check for updates* asks now and *Update* fetches the installer for
your machine, puts it in place and restarts the app.

---

## Why this exists

Claude Code deletes your session transcripts. `cleanupPeriodDays` defaults to
**30**, the sweep runs quietly in the background, and there is no warning.

On the machine this was built on, the cleanup had already run that morning:
136 MB of transcripts remained, the oldest dated exactly 28 days back, and
everything before it was gone.

There are already several good Claude Code session viewers, among them
[claude-code-viewer](https://github.com/d-kimuson/claude-code-viewer),
[claude-code-trace](https://github.com/delexw/claude-code-trace) and
[claude-code-log](https://github.com/daaain/claude-code-log). Every one of
them reads `~/.claude/projects` and stops there, so every one of them inherits
that expiry. Emaki's reason to exist is that it copies first.

**Compaction is not the threat.** `/compact` appends a boundary marker and keeps
writing to the same file; the earlier rows stay. In a real session that dropped
460,573 tokens from context, all 440 pre-compaction rows were still on disk.
What loses conversations is the 30-day sweep.

---

## What it does

**Archives every session, permanently.** A byte-for-byte copy into
`~/.emaki/archive/`, incremental and append-only. A session is four things on
disk, and all four are captured:

```
<project>/<session>.jsonl                        the conversation
<project>/<session>/subagents/agent-*.jsonl      subagent conversations
<project>/<session>/subagents/agent-*.meta.json  which Task spawned each
<project>/<session>/tool-results/<id>.txt        outputs too large to inline
```

When Claude Code deletes the original, the session keeps working: it stays
listed, searchable and renderable, marked *kept* in the sidebar.

**Shows what needs you on a board.** Live sessions sit in columns by what
they are waiting on: *needs you* (an approval, a question, a plan),
*planning*, *working*, *your turn* (Claude replied). Each card names its
project, branch and agent. Everything without a process behind it is the
collapsed *done* strip. The column is read off the transcript, so it is right
even for a session that started before Emaki did.

**Searches every conversation.** Full-text across the whole corpus, live and
archived, one document per prompt, reply, thought or tool call, so a hit says
where to look. ⌘K opens the palette; a hit opens its session on the round that
matched. ⌘F finds inside the conversation showing.

**Lays the conversation out like the Claude app.** Prompts on the right,
replies as prose in Anthropic Serif when the Claude app is installed, tool
calls as cards that fold, runs of them folded into one row, edits as diffs,
subagents nested inside the call that spawned them, pictures and files shown
as what they are.

**Explains opaque tool calls in plain words.** A `python3 - <<'EOF'` heredoc
or a piped shell chain gets one or two sentences from a small model (Haiku,
through your own Claude Code login, no key of ours), on a permission card
while you decide, and on any tool card's *Explain* button. Simple calls
explain themselves for free, and answers are kept by content, so a command
explained once is annotated everywhere it ever appears.

**Renders readable markdown.** One file per session under `~/.emaki/logs/`,
regenerated from the archive at any time. Greppable, diffable, and readable in
ten years when this program no longer exists.

**Talks back.** See below.

---

## Continuing a conversation from the window

The composer at the foot of a session sends a message into that session.
Every message is something you typed and pressed send on; nothing is injected
on your behalf. How it gets there depends on what is behind the session, and
the line under the composer says which.

**A running terminal session.** Claude Code 2.1 gives every session an inbox,
the same channel one Claude session uses to message another. Emaki writes your
message there and it lands exactly as a prompt typed in the terminal would. The
transcript records it as an ordinary row, so the conversation shows it as
*you · emaki* with Claude's reply underneath. Mode, model and effort show as
the transcript says them and cannot be changed from here: the inbox reads
everything as prose.

One thing Claude Code enforces: a session running with permissions bypassed
holds a message from any other process and asks in the terminal before
delivering it. Emaki does not claim otherwise on your behalf. To let messages
through without the prompt, set `"crossSessionInbound": "accept"` in your
Claude Code settings.

**A finished session.** With no process behind it, sending starts a headless
Claude Code child of Emaki's own (`claude -p --resume <id>`) in the session's
own directory. It appends to the same transcript under the same id, so the
window updates as the turn runs and `claude --resume` in a terminal later
picks up from there. The child stays between turns and closes after
`driver.idle_min` of silence. Because Emaki hosts that process, the window
gets what a terminal has: pictures in the message, the permission mode and
model to pick (⇧Tab cycles the mode), `/effort`, a stop button, and a tool
that needs permission shows as a card you answer (↩ allows the oldest, ⇧↩
denies). If you open the same session in a terminal, Emaki retires its child
after the current turn so two processes never write one transcript.

**A new session.** The home page is a greeting over the composer: pick a
folder, write the first message, and a session starts there under a fresh id.

**Attachments.** The `+` button, a paste, or a drop onto the composer attaches
files; a pasted image is kept under `~/.emaki/uploads/<session>/` and never
enters a repository. On a session Emaki hosts an image goes to Claude as an
image; anything else, and everything on a terminal session, is named by path
so Claude reads it with its own tools.

**The limits row** above the composer is the terminal's status line: context
used against the model's window, and the account's five-hour and seven-day
windows once a session has run through Emaki.

---

## Where it puts things

Everything lives in `~/.emaki`, mode `0700`, outside every repository:

```
~/.emaki/archive/     the permanent byte-for-byte copies: the irreplaceable part
~/.emaki/logs/        rendered markdown, one file per session (regenerable)
~/.emaki/search.db    the search index (regenerable)
~/.emaki/cache/       explanations, by content
~/.emaki/state/       what the window remembers, the account's limits
~/.emaki/uploads/     pictures pasted into the composer
~/.emaki/config.json  settings
```

Only `archive/` holds anything that cannot be rebuilt. Back up that directory
and you have kept everything. `EMAKI_HOME` moves the whole tree elsewhere,
which is also how to run a second copy beside the installed app.

The archive is incremental: an unchanged session costs one `stat`, a growing
one copies only the new bytes. The one case that could destroy data is a
source file rewritten underneath us; rather than overwrite, Emaki rotates the
existing copy to `<session>.gen1.jsonl` and starts fresh, so both survive.

---

## Configuration

The settings panel (⌘, on macOS, Ctrl+, elsewhere) covers the look, the
explainer and what a new session starts with. `~/.emaki/config.json` holds
everything:

| Key | Default | |
|---|---|---|
| `app.appearance` | `system` | `system`, `light` or `dark` |
| `app.accent` | `terracotta` | `terracotta`, `blue`, `green`, `violet`, `teal` or `graphite` |
| `app.chat_font` | `serif` | the conversation's face: `serif` or `sans` |
| `app.chat_size` | `medium` | `small`, `medium` or `large` |
| `app.check_updates` | `true` | ask GitHub for the newest release once a day |
| `explain.scope` | `permission` | `off`, `permission` (cards) or `all` (every new call in the session showing) |
| `explain.model` | `claude-haiku-4-5` | |
| `driver.enabled` | `true` | start a headless Claude Code child for a session with no process behind it |
| `driver.idle_min` | `30` | close that child after this many idle minutes |
| `driver.default_mode` | `""` | permission mode for a started session (`""` = Claude Code's `permissions.defaultMode`) |
| `driver.default_model` | `""` | model for a started session (`""` = the account default) |
| `driver.allow_bypass` | `false` | offer `bypassPermissions` |
| `driver.claude_path` | `""` | where `claude` is, when `PATH` does not say |
| `markdown.tools` | `full` | `full`, `summary`, or `none` |
| `markdown.max_output_chars` | `4000` | per tool call, markdown only |
| `redact.enabled` | `true` | scrub secrets from the markdown and the window |

---

## Privacy

The archive holds complete transcripts, which means whatever the agent read.
Everything lives in `~/.emaki` at mode `0700`, outside every repository; an
in-project log is one `git add -A` away from being published.

`redact.enabled` scrubs the shapes that leak most often: provider key prefixes
(`sk-ant-`, `ghp_`, AWS, Slack, Google), `Authorization` headers, private key
blocks, and `NAME=value` assignments for password/secret/token-ish names. It
applies to the rendered markdown and the window, **not** to the raw archive:
the archive is deliberately verbatim, because a redacted archive is not a
reproducible one. Add patterns with `redact.extra_patterns`. It is a safety
net, not a guarantee.

Explanations are the only feature that sends anything anywhere: the call's
arguments go to Anthropic through your own Claude Code login. Cheap calls
never leave your machine, answers are cached so nothing is sent twice, and
`explain.scope` set to `off` turns it off.

---

## Things worth knowing

- **The markdown is regenerated, not appended to.** Hand-editing a log will be
  overwritten; rename the file to keep annotations.
- **A `Read` records that it happened, not the file's contents.** One session had
  782 of them. The archive keeps the raw record regardless.
- **Token counts exclude cache reads.** Every message re-reads the cached prefix,
  so summing that field reports tens of millions for a session that produced a
  few hundred thousand.
- **The inbox is a Unix socket.** On Windows a terminal session cannot be
  messaged; a finished one still resumes through Emaki's own child.
- **An older Emaki was a Python CLI and web daemon** that registered hooks in
  `~/.claude/settings.json`. The app needs none of that. If those hooks are
  still in your settings, delete the `hooks` entries whose command names
  `emaki-hook`; a stale one blocks every prompt in Claude Code.

---

## Development

```
cargo test -p emaki-core                 # the core, on temp directories
cargo build -p emaki-app && ./target/debug/Emaki
target/debug/emaki-core help             # list, render, archive, search, bench, explain …
```

`EMAKI_HOME` and `CLAUDE_CONFIG_DIR` redirect everything, which is how the
tests stay off your real data. `AGENTS.md` is the map of the code and the
reasoning behind it; `WORKFLOW.md` is how a release is cut.

MIT.
