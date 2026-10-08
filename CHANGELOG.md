# Changelog

Each release has a section here, and `release.yml` puts that section on the
GitHub Release as its notes. Write it for the person installing the app, and
keep it short: one line per change, a dozen lines at most, small fixes
grouped into one. The reasoning belongs in AGENTS.md.

## v0.1.8

- Click a file in the files panel to read it in a pane beside the conversation; click it again, or press ⌘W, to close it.
- That pane shows code with line numbers and colours, markdown as it reads or as written, pictures, tables, and PDFs with search, selection, page previews and a table of contents.
- The composer checks spelling and grammar as you type (English US or UK; Spanish, French and German spelling on a Mac) and capitalizes a new sentence. Right-click a marked word for corrections, Learn Spelling or Ignore.
- Every text field has Emaki's own right-click menu.
- The conversation follows new content when you are near the bottom, and a "New messages" button takes you to what arrived while you were reading higher up.
- One terminal look for the shell and the agent's screen, after Kaku, with a Terminal section in Settings.
- ⌘⇧T and ⌘⇧A show and hide the two sides of the terminal panel; in the shell, ⌘T opens a tab and ⌘W closes it.
- The model and effort pills open a list in place, and code blocks in a conversation are coloured.
- The archive copies sessions as clones where the disk allows, and a session its agent deleted stays listed as archived.
- New icons (Tabler); the board is gone, and Projects is now All Projects.
- A folder inside a repository shows git's marks for its own files only.
- Fixes: a question with previews reads correctly and can be answered, a session whose process is gone stops showing as working, and a stopped prompt is not sent twice.

## v0.1.7

- A terminal inside Emaki, at the conversation's right: shells in tabs, and the session's own agent as it runs. Nothing opens a terminal app any more.
- In it: select with the mouse (copied on release), click and scroll in programs that take the mouse, and the line keys of a Mac terminal (⌘←, ⌘→, ⌘⌫, ⇧↩, ⌘K).
- A files panel beside the conversation: the project's folder as a tree with git's marks, a file's contents, and what changed.
- Switch branches from the files panel, carrying your changes along or leaving them behind to restore later.
- An outline of the conversation, a short label per prompt; click one to go there.
- Right-click menus on sessions, tabs, files, the conversation, attachments and the branch name.
- Code blocks have a copy button, and a right click on a word selects it.
- Settings, Composer: send with ⌘↩ or with ↩; ⇧↩ is always a new line.
- Running subagents show in a row over the composer; click it to see them.
- Hold ⌘ to see each tab's number.
- A question asked in a named session shows as a card, and "Type something" takes your own answer.

## v0.1.6

- Sessions run in a hidden terminal of Emaki's own; nothing done in Emaki touches a terminal of yours.
- ⇧Tab or the mode pill changes the mode in place; the model and effort pills show Claude Code's picker on a card.
- Type `@` in the composer to pick a file or folder from the project; a path that exists takes the accent colour.
- A command left running in the background shows in a row over the composer; click it for its output.
- Set your own picture and name in Settings, Appearance.
- Drag the sidebar's edge to resize it, or double-click the edge to fit it.
- The sidebar lists Agents and Projects on cards; the Projects page lists projects first, then their sessions.
- Tabs switch instantly, share the row, drag to reorder, and answer to ⌘1 to ⌘9.
- Renaming a session renames it for Claude Code too, and every session keeps its own draft.
- The sidebar button and search sit beside the traffic lights; the terminal button opens your default terminal.
- New icons (Phosphor), and scrollbars that fade after a second.

## v0.1.5

- A new icon: the scroll on cream, with a conversation on its sheet.
- A question or approval waiting in a terminal shows as a card in the window; answer it and move between its questions there.
- While Claude works in a terminal, the row under the conversation shows the terminal's own line ("Embellishing… (13s · ↓ 1.0k tokens)") in its colours.
- The prompt Claude Code suggests after a turn shows in the composer; → or Tab takes it.
- A session started in Emaki shows in the terminal's resume list.
- Modes, models and effort levels are read from Claude Code and coloured as it colours them.
- A change of mode, model or effort is one short line in the conversation.
- ⇧Tab in the composer steps a terminal session's mode, and the pill follows.
- The effort and model pills open the session's terminal when it has none, then the picker there.
- Stop, or Escape, gives your message back only when Claude had not started on it, and sending it again works.
- In the composer, ⇧↑ and ⇧↓ select one line at a time, and a paste or dictation scrolls to the caret.
- Durations read in whole seconds.

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
