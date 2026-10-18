# Emaki

*Every coding-agent session on your machine, kept for good, in one window you can read, search and talk to.*

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

### Project Management

- **Every session, by project**: The sidebar lists each project with its sessions; pin one to the top, hide one, or open its folder
- **By day**: All Sessions (⌘L) groups every session on the machine by day, with archived projects in their own section
- **Kept for good**: A session Claude Code has deleted stays listed, searchable and readable, marked as kept
- **Search**: ⌘K searches the full text of every session
- **Tabs**: Open sessions sit in tabs grouped by project; switch with ⌘1 to ⌘9, close with ⌘W, reopen with ⌘⇧T
- **Housekeeping**: Rename a session, or select several with ⌘-click, ⇧-click or a dragged rectangle and archive or delete them at once; a project's folder is never touched
- **Two agents**: Claude Code and Codex, each in its own color; the Agents page (⌘E) says which is installed and signed in
- **New sessions**: From the home page, from a project's plus, with ⌘T in the current project or ⌘O in any folder, with the agent, mode, model and effort chosen on the strip under the composer

### Conversation (Main Panel)

- **Readable**: Prompts, replies, tool calls, thoughts and subagents laid out like the Claude app, with code colored and wide blocks scrolling sideways
- **Explained**: An opaque tool call gets one plain sentence on what it does, written by the session's own agent
- **Talk back**: Reply from the composer with attachments, `@` for a file, `/` for a command, and the mode, model and effort on pills; ⌘↩ sends
- **Cards**: Questions, approvals, plans and background commands come up as cards at the conversation's end, answered there; Stop gives your message back
- **Live**: A session running in a terminal is followed as it writes, with its context and rate limits under the composer and "New messages" when you are reading higher up
- **Find**: ⌘F finds inside the session showing
- **Artifacts**: The pages an agent publishes are listed over the composer, a click from opening
- **A record you own**: Every session also renders to markdown under `~/.emaki/logs`

### Files Panel

- **The project as a tree**: The session's folder with git's marks, kept current as the agent writes
- **Branches**: Switch, carrying your changes along or leaving them to restore later; make a branch, publish one, fetch
- **Changes**: A sheet of what changed, with a diff per file; commit the files you tick, push, pull, browse history, undo or revert a commit
- **A file manager**: Drag files in, drag rows to move, cut, copy, duplicate, rename in place, remove with ⌘⌫, and take it back with ⌘Z
- **Files open beside the conversation**: Code in an editor with ⌘S and format on save, markdown as it reads or as written, CSV as a grid, Word as pages you can write in, PowerPoint, Excel, PDF with search and selection, HTML, Parquet and pictures
- **Room**: Drag the panel's edge, double-click it to fit, or enlarge a file over the conversation

### Outline Panel

- **An entry a round**: What you asked, in a short label, with the time
- **Go there**: Click an entry and the conversation glides to that round
- **Labels by a small model**: Written by the session's own agent, Claude Code's Haiku or Codex's light model, and cached

### Shell Terminal Panel

- **A shell in the session's folder**: At the conversation's right, in tabs; ⌘T opens one, ⌘W closes it
- **A Mac terminal's keys**: ⌘←, ⌘→, ⌘⌫, ⇧↩ and ⌘K, with mouse selection copied on release and programs that take the mouse
- **Kaku's look**: Its dark and light schemes, with the font and size set in Settings, Terminal

### Agent Terminal Panel

- **The session's own agent, drawn whole**: Claude Code or Codex as it runs, in a terminal of Emaki's own; nothing opens a terminal app
- **One session, two views**: A turn typed in the terminal shows in the conversation, and a message sent from the composer shows in the terminal
- **Whatever the agent asks is shown**: A question with no card of its own comes up on the terminal card and never waits unseen
- **Copy and paste**: Copying reaches the clipboard, and a pasted screenshot goes to the agent

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

## License

MIT.
