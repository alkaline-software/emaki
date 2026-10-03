# Changelog

Each release has a section here, and `release.yml` puts that section on the
GitHub Release as its notes. Write it for the person installing the app, and
keep it short: one line per change, a dozen lines at most, small fixes
grouped into one. The reasoning belongs in AGENTS.md.

## v0.1.4

- A new icon: a handscroll on the terracotta plate.
- The home page shows recent folders as cards under a serif greeting.
- Recents and the sessions page are grouped by day, and a live row shows its state.
- The board is four open columns that reflow with the window and never scroll sideways.
- Settings has sections and segmented controls. Updates are one button.
- Type "/" for the folder's slash commands. A command runs; a skill goes into the message.
- Claude's questions show as cards, and a session started in Emaki answers them from the window.
- Stop a turn from the window. The stopped message returns to the composer.
- On a terminal session, questions, approvals, commands, mode, model and effort take you to the terminal, and the window comes back when you are done.
- A message sent while Claude is working shows at once, marked as queued.
- Fixed: `/compact` left the board on "working", and the context percentage used the wrong window size.
- Softer shadows, pages that fade in, less work while idle.

## v0.1.3

- The terminal, project folder and transcript buttons sit together at the top right.
- The session's folder path is under the tabs, in place of the long line of stats.
- The app's icon and name are in the sidebar.
- A copy button and the time appear under a prompt or reply on hover.
- Inline code is drawn in the accent colour on a tinted plate, as in the Claude app.
- Code is set in SF Mono, or Anthropic Mono when it is in `~/.emaki/fonts`.
- A tool card names its tool once.

## v0.1.2

- The status line script ships with the app and feeds the limits row. `emaki-core statusline restore` puts the old setting back.
- Open in your terminal: continue a session with `claude --resume` in a terminal window.
- A session opens at its end and follows the reply as it streams.
- Text matches the Claude app: 16px, semibold for bold, monospaced inline code.
- Claude's own mark, turning while it works.
- Notices show under the composer, not in the sidebar.
- Fixed: bold drew as regular, scroll momentum crossed panes, arrow keys stopped short of the composer's ends.

## v0.1.1

The app stands alone. The Python command-line tool and web daemon are gone.

- Emaki updates itself, from Settings or a daily check.
- Explain: one plain sentence on what an opaque tool call does, on tool cards and permission cards.
- Context and rate limits under the composer.
- Long tool output scrolls inside its card.
- Pasted pictures show their names.
- Fixed: a crash on some Codex sessions, and ⌘↩ leaving a line break.
- No hooks. If `~/.claude/settings.json` still has entries naming `emaki-hook`, delete them.

## v0.1.0

The first release. Emaki keeps every coding-agent session on your machine and
lets you read, search and continue them in one window.

- Every Claude Code and Codex session is archived byte for byte, and stays after Claude Code deletes it.
- A board of live sessions: needs you, planning, working, your turn.
- Full-text search across every session (⌘K), and find inside one (⌘F).
- A conversation view laid out like the Claude app.
- Reply from the window, with attachments, permission cards, and mode, model and effort pills.
- Every session also renders to a markdown file.
- Settings for appearance, accent, font and text size.

Install: on macOS drag Emaki to Applications (signed and notarized). On
Windows SmartScreen asks once: *More info*, then *Run anyway*. On Linux run
the AppImage or install the `.deb`.
