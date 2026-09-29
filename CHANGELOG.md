# Changelog

Each release has a section here, and `release.yml` puts that section on the
GitHub Release as its notes. Write it for the person installing the app.

## v0.1.0

The first release of Emaki, a desktop app that keeps every coding-agent
session on your machine and lets you read, search and continue them in one
window.

**Why it exists.** Claude Code deletes its transcripts after thirty days,
quietly. Emaki copies every session as it is written, so a conversation from
last spring is still there, whole, after the sweep.

**What you get**

- **Every session, kept.** Claude Code and Codex sessions are archived
  byte-for-byte under `~/.emaki`, subagents and large tool results included.
  A session Claude Code has since deleted stays listed, marked *kept*.
- **A board of what needs you.** Live sessions sit in columns: needs you,
  planning, working, your turn. Each card names its project, branch and agent.
- **Search across everything.** Full-text search over every prompt, reply,
  thought and tool call, live or archived, opening on the round that matched.
- **A conversation view laid out like the Claude app.** Prompts on the right,
  replies as prose, tool calls as cards that fold, runs of them folded into
  one row, pictures and files shown as what they are.
- **Talk back.** Type into a session from the window. A terminal session gets
  the message through Claude Code's own inbox; a finished one resumes as a
  headless child and the reply appears in the same transcript. Attach files
  and pictures by paste, drop or the picker.
- **Markdown on disk.** Every session also renders to a CommonMark file,
  regenerated from the archive, readable without Emaki.

**After the first day of use**

- **Find in the conversation.** ⌘F opens a find bar over the session you
  are reading. Every prompt, reply, thought and tool call is searched, the
  hits are marked in the transcript, and ↩, ⇧↩, ⌘G and ⌘⇧G step through
  them, unfolding whatever hides one. A hit in the search palette (⌘K) now
  opens its session with the find bar on that query, on the matched round.
- **Every permission mode, and its name.** The pills under the composer
  open a list with all four modes (and bypass, when config allows it),
  each with a line on what it does. Auto mode used to show as "Default
  permissions", so it looked as if it could not be reached. ⇧Tab in the
  composer cycles the mode, as in Claude Code's terminal. A switch Claude
  Code refuses now says why in the status row instead of pretending.
- **Model names with their version.** The model pill reads "Opus 5.5",
  "Fable 5.1", "Haiku 4.5" once Claude Code has said which it is running,
  not "Opus".
- **Permission cards from the keyboard.** With the composer empty, ↩ allows
  the oldest card waiting on the session and ⇧↩ denies it; the card says
  so on its buttons. Several cards at once get an "Allow all".
- **More settings.** ⌘, now has the appearance (follow the system, light,
  dark), six accent colours, the chat font, three text sizes, and the
  permission mode and model a session started from the window begins in.
  Everything is drawn at once and kept in `config.json`. The panel sits in
  the middle of the window.
- **Mode, model and effort on every conversation.** The pills under the
  composer show them for a terminal session too, read from the transcript;
  a session run through Emaki can change all three, effort included.
- **Context and limits, as in the terminal.** A line above the composer
  reads `Context 37% · 5h 3% (4h26m) · 7d 6% (6d7h)`: the context the next
  request carries against the model's window, and the account's five-hour
  and seven-day windows as the last session run through Emaki saw them.

**Installing**

- macOS: open the disk image and drag Emaki to Applications. The app is
  signed and notarized.
- Windows: run the installer. It is not yet signed, so SmartScreen will ask
  once; choose *More info*, then *Run anyway*.
- Linux: the AppImage runs as is (`chmod +x` first); the `.deb` installs on
  Debian and Ubuntu.

Emaki reads the transcripts Claude Code and Codex already write. It never
modifies them, installs no hooks, and adds nothing to a session's context.
