# Emaki

*The agentic development environment.*

[Website](https://emaki.lol) · [Download](https://github.com/alkaline-software/emaki/releases/latest) · [Changelog](CHANGELOG.md)

## Motivation

Emaki (絵巻) is a Japanese picture scroll: a long roll, read from one end to the other. A coding-agent session is the same thing, and Emaki is where it is kept and read.

Claude Code deletes its transcripts after 30 days, with no warning. Every other tool that reads them inherits that expiry. Emaki copies each session byte for byte before anything else touches it, so if rendering fails, the bytes are already safe. Codex keeps its own sessions and never expires them, so Emaki reads those in place.

Emaki never writes to a transcript and adds nothing to a session's context. The agent runs as it always does; Emaki watches, keeps and shows.

## Get Started

Download the [latest release](https://github.com/alkaline-software/emaki/releases/latest) for your system:

| File | System |
| :--- | :--- |
| `Emaki-mac-arm64.dmg` | Mac, Apple silicon |
| `Emaki-mac-x64.dmg` | Mac, Intel |
| `Emaki-windows-x64-setup.exe` | Windows |
| `Emaki-linux-x64.AppImage`, `Emaki-linux-x64.deb` | Linux |

On a Mac, open the image and drag Emaki into Applications. The Mac image is signed and notarized. Windows is unsigned, so SmartScreen asks once: *More info*, then *Run anyway*.

Open the app. The first launch copies every Claude Code session on the machine into `~/.emaki/archive` and lists it beside your Codex sessions; every later launch catches up, so Emaki does not have to stay running. It needs at least one agent installed: the Agents page (⌘E) walks through installing Claude Code or Codex and signing in. The app updates itself from Settings.

Everything Emaki keeps is in `~/.emaki`, and `archive/` is the one folder that cannot be rebuilt. The one setting it touches outside is Claude Code's status line, which feeds the context and rate limits under the composer; `emaki-core statusline restore` puts the old one back.

## Features

- **Project Management**: Every session on the machine, by project in the sidebar and by day on All Sessions, in tabs, searched in full with ⌘K; one Claude Code has deleted stays, marked as kept, and several are renamed, archived or deleted at once without touching a project's folder
- **Conversation (Main Panel)**: Prompts, replies, tool calls, thoughts and subagents laid out like the Claude app, an opaque call explained in one sentence, and a composer to reply from, with attachments, `@` for a file, `/` for a command, the mode, model and effort on pills, and the agent's questions, approvals and plans answered on cards
- **Files Panel**: The project as a tree with git's marks, branches to switch, make and publish, a Changes sheet to commit, push, pull and revert from, and every file open beside the conversation: code in an editor, markdown, CSV, Word, PowerPoint, Excel, PDF, HTML, Parquet and pictures
- **Outline Panel**: An entry a round, labeled in a few words by the session's own agent; click one and the conversation glides there
- **Shell Terminal Panel**: A shell in the session's folder at the conversation's right, in tabs, with a Mac terminal's keys and Kaku's look
- **Agent Terminal Panel**: The session's own Claude Code or Codex drawn whole, in a terminal of Emaki's own, so a turn typed there shows in the conversation and whatever the agent asks is never missed

## Keys

| Action | Key |
| :--- | :--- |
| Search every session | `⌘ K` |
| Find in this session | `⌘ F` |
| New session | `⌘ N` |
| New session in this project | `⌘ T` |
| Choose a folder | `⌘ O` |
| All Sessions | `⌘ L` |
| Agents | `⌘ E` |
| Sidebar | `⌘ ⇧ B` |
| Files panel | `⌘ ⇧ F` |
| Outline panel | `⌘ ⇧ O` |
| Shell terminal | `⌘ ⇧ S` |
| Agent terminal | `⌘ ⇧ A` |
| Send | `⌘ ↩` (or `↩`, from Settings) |
| Save the file showing | `⌘ S` |
| Switch tab | `⌘ 1` to `⌘ 9` |
| Close tab, reopen tab | `⌘ W`, `⌘ ⇧ T` |
| Settings | `⌘ ,` |

Off a Mac, Ctrl stands in for ⌘.

## License

MIT.
