# Emaki

**Every coding-agent session on your machine, kept for good, in one window
you can also talk to.**

Claude Code deletes its transcripts after 30 days, with no warning. Emaki
copies every Claude Code and Codex session before that happens, renders and
searches all of them, and lets you continue a Claude Code session from the
window. It never writes to a transcript and adds nothing to a session's
context.

## Install

Download the installer from the
[latest release](https://github.com/alkaline-software/emaki/releases/latest):

```
Emaki-mac-arm64.dmg            Apple silicon
Emaki-mac-x64.dmg              Intel Mac
Emaki-windows-x64-setup.exe    Windows
Emaki-linux-x64.AppImage       Linux (a .deb is there too)
```

The Mac image is signed and notarized. Windows is unsigned, so SmartScreen
asks once: *More info*, then *Run anyway*. The app updates itself from
Settings.

## Use

Open the app. The first launch copies every transcript it finds into
`~/.emaki/archive`, and every later launch catches up.

- **Home.** Pick a folder, type a message, and a new Claude Code session
  starts there.
- **Board.** Live sessions in four columns: needs you, planning, working,
  your turn.
- **Sessions.** Every session, grouped by day. One that Claude Code has
  deleted is still there, marked *kept*.
- **Search.** ⌘K searches every session. ⌘F finds inside the one showing.
- **Conversation.** Prompts, replies, tool calls and subagents, laid out
  like the Claude app. Type in the composer to reply: a session running in
  a terminal gets the message through Claude Code's inbox, and a finished
  one resumes under Emaki. Attach files by paste, drop or the "+" button.
- **Terminal.** The button at the top right opens a terminal beside the
  conversation: a shell in the session's folder, or the session's own
  Claude Code. A session's right-click menu opens its folder and shows
  its transcript file.
- **Settings.** ⌘, sets the appearance, accent, font, text size and what a
  new session starts with.

Emaki sets one thing in Claude Code's settings: its status line, which
feeds the context and rate limits shown under the composer.
`emaki-core statusline restore` puts the old one back.

Everything is in `~/.emaki`. Only `archive/` cannot be rebuilt, so that is
the directory to back up.

## How it is built

Rust, with [GPUI](https://www.gpui.rs) (the Zed editor's UI framework) and
[gpui-component](https://github.com/longbridge/gpui-component) for the
window. There is no daemon, no browser and no hook.

```
crates/emaki-core    everything without a window, plus the emaki-core CLI
crates/emaki-app     the window
vendor/              the gpui-component crates, with a few changes of ours
scripts/             packaging, the icon, the status line
```

The core works in this order:

1. **Adapters** (one per agent) find each agent's sessions on disk.
2. **Archive** copies them byte for byte, before anything else touches them.
3. **Build** parses the JSONL transcript into a model of rounds, items and
   tool calls. The transcript is the only source of truth.
4. **Render** writes that model as markdown under `~/.emaki/logs`.
5. **Search** indexes every item in SQLite FTS5.

The app reads the same model and draws it. A file watcher says when to look
again. To reply, it either writes to a terminal session's inbox socket or
runs a headless `claude -p --resume` child that appends to the same
transcript.

CI tests and builds on macOS, Windows and Linux on every push. A `v*` tag
builds the installers with cargo-packager.

## Development

```
cargo test -p emaki-core
cargo build -p emaki-app && ./target/debug/Emaki
target/debug/emaki-core help
```

`AGENTS.md` explains the code and the reasoning behind it. `WORKFLOW.md` is
how a release is cut.

MIT.
