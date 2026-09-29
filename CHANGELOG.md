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

**Installing**

- macOS: open the disk image and drag Emaki to Applications. The app is
  signed and notarized.
- Windows: run the installer. It is not yet signed, so SmartScreen will ask
  once; choose *More info*, then *Run anyway*.
- Linux: the AppImage runs as is (`chmod +x` first); the `.deb` installs on
  Debian and Ubuntu.

Emaki reads the transcripts Claude Code and Codex already write. It never
modifies them, installs no hooks, and adds nothing to a session's context.
