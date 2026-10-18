# Emaki

*Every coding-agent session on your machine, kept for good, in one window you can read, search and talk to.*

[Website](https://emaki.lol) · [Download](https://github.com/alkaline-software/emaki/releases/latest) · [Changelog](CHANGELOG.md)

## Why

Emaki (絵巻) is a Japanese picture scroll: a long roll, read from one end to the other. A coding-agent session is the same thing, and Emaki is where it is kept and read.

Claude Code deletes its transcripts after 30 days, with no warning. Every other tool that reads them inherits that expiry. Emaki copies each session byte for byte before anything else touches it, so if rendering fails, the bytes are already safe. Codex keeps its own sessions and never expires them, so Emaki reads those in place.

Emaki never writes to a transcript and adds nothing to a session's context. The agent runs as it always does; Emaki watches, keeps and shows.

## Quick Start

Download the [latest release](https://github.com/alkaline-software/emaki/releases/latest) for your system:

| File | System |
| :--- | :--- |
| `Emaki-mac-arm64.dmg` | Mac, Apple silicon |
| `Emaki-mac-x64.dmg` | Mac, Intel |
| `Emaki-windows-x64-setup.exe` | Windows |
| `Emaki-linux-x64.AppImage`, `Emaki-linux-x64.deb` | Linux |

On a Mac, open the image and drag Emaki into Applications. The Mac image is signed and notarized. Windows is unsigned, so SmartScreen asks once: *More info*, then *Run anyway*.

Open the app. The first launch copies every Claude Code session on the machine into `~/.emaki/archive` and lists it beside your Codex sessions; every later launch catches up. Emaki needs at least one agent installed: the Agents page (⌘E) walks through installing Claude Code or Codex and signing in. The app updates itself from Settings.

## Features

- **Kept for good**: Every Claude Code session is archived before Claude Code can delete it, and a deleted one stays listed, searchable and readable, marked as kept
- **One place**: Sessions by project in the sidebar, by day on All Sessions, and in tabs; pin a project, rename a session, archive or delete several at once
- **Readable**: Prompts, replies, tool calls, thoughts and subagents laid out like the Claude app, with an opaque tool call explained in one plain sentence
- **Search**: ⌘K searches every session on the machine; ⌘F finds inside the one showing
- **Talk back**: Reply from the composer with attachments, `@` for a file, `/` for a command, and the agent's mode, model and effort on pills; questions, approvals and plans come up as cards
- **Two agents**: Claude Code and Codex, each started, continued and stopped from the window, each in its own color
- **Four panels**: The files panel (the project as a tree, with branches, changes, commits and history) and the outline panel at the left; the shell terminal and the agent terminal at the right
- **Files open**: Code with an editor and format on save, markdown, CSV as a grid, Word, PowerPoint, Excel, PDF, HTML, Parquet and pictures, beside the conversation
- **A record you own**: Every session also renders to markdown under `~/.emaki/logs`

## Usage Guide

| Action | Shortcut |
| :--- | :--- |
| Search every session | `⌘ K` |
| Find in this session | `⌘ F` |
| New session | `⌘ N` |
| New session in this project | `⌘ T` |
| Choose a folder | `⌘ O` |
| All Sessions | `⌘ L` |
| Agents | `⌘ E` |
| Sidebar | `⌘ B` |
| Files panel | `⌘ ⇧ E` |
| Outline panel | `⌘ ⇧ O` |
| Shell terminal | `⌘ ⇧ S` |
| Agent terminal | `⌘ ⇧ A` |
| Send | `⌘ ↩` (or `↩`, from Settings) |
| Save the file showing | `⌘ S` |
| Switch tab | `⌘ 1` to `⌘ 9` |
| Close tab, reopen tab | `⌘ W`, `⌘ ⇧ T` |
| Settings | `⌘ ,` |

Off a Mac, Ctrl stands in for ⌘.

## FAQ

- **Does Emaki change my sessions?** No. Nothing is written to a transcript and nothing enters a session's context. The one setting Emaki touches is Claude Code's status line, which feeds the context and rate limits under the composer; `emaki-core statusline restore` puts the old one back
- **Does it have to be running?** No. Every launch catches up on every session since the last one. A Claude Code session deleted before Emaki first saw it is gone, though, so open the app once before the 30 days are up
- **Where is my data?** In `~/.emaki`. Only `archive/` cannot be rebuilt, so that is the folder to back up
- **Does anything leave my machine?** There is no server. The explanations and the outline's labels are calls to your own agent on your own account, Claude Code's Haiku or Codex's light model, and the update check reads GitHub's releases page
- **Why are Codex sessions not in the archive?** Codex never deletes a session by age, so its files are the only copy. Emaki reads them where they are, and archives or deletes one through Codex itself
- **Windows and Linux?** Both build and ship, and both are less tested than the Mac. On Linux an HTML file is drawn as a picture, since no web view can be set in the window
- **Where did dictation go?** Into [Kotoba](https://github.com/alkaline-software/kotoba), the studio's dictation app, which types into Emaki as into any field

## Docs

- [AGENTS.md](AGENTS.md) - what Emaki is, the rules that hold everywhere, and the layout of the code
- [docs/](docs/) - how each part works and its traps, one file per part
- [WORKFLOW.md](WORKFLOW.md) - how a release is cut
- [CHANGELOG.md](CHANGELOG.md) - what each release changed

## Background

Emaki began as a Python tool that rebuilt conversations from Claude Code's hooks, and its own README conceded the result was not a full audit trail. The JSONL transcript Claude Code writes is complete, so the app was rewritten around it: copy first, render second, and get everything from the transcript. The Python tool and its web daemon were removed in September 2026.

The window is Rust on [GPUI](https://www.gpui.rs), the Zed editor's UI framework, with [gpui-component](https://github.com/longbridge/gpui-component). There is no daemon, no browser and no hook. The terminal's look is [Kaku](https://github.com/tw93/Kaku)'s.

## Development

```bash
cargo test -p emaki-core
cargo build -p emaki-app && ./target/debug/Emaki
```

Run one Emaki at a time: two share `~/.emaki`. CI tests and builds on macOS, Windows and Linux on every push, and a `v*` tag builds the installers.

## License

MIT.
