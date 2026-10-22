# Changelog

Each release has a section here, and `release.yml` puts that section on the
GitHub Release as its notes. Write it for the person installing the app, as
a numbered list: a topic in bold, then one sentence on what changed for
them, with small fixes gathered under **Fixes**. Ten items at most. The
reasoning belongs in AGENTS.md and `docs/`.

## v0.2.9

1. **Settings**: A window of its own, with Text Size as a section of whole-point presets and a Custom size per place; Escape closes it.
2. **Trash**: Deleted sessions wait in a Trash at the foot of the sidebar and of the sessions page for 30 days, with Put Back, Delete Now and Empty Trash; Delete takes any session, a running turn stopped first, and every confirmation says one line or none.
3. **Repositories**: A folder of checkouts shows the one in use on a button before the branch, with a list to switch; the count, the branch list, the comparison and the commit are that checkout's, each keeping its own commit words.
4. **Branches**: Delete Branch on a branch's right click, never for the default branch, with a tick that also deletes it on the remote; View on GitHub at the foot of the branch list.
5. **Editor**: Pairs close as their open is typed and wrap a selection, as in VS Code; find with case, whole word, regex and replace; ⌘F finds in a file as it reads.
6. **Spelling**: Markdown, Quarto and plain text files are checked in the editor as the composer is, in the same language, with corrections on a right click.
7. **Panels**: ⌘⇧F for the files panel and ⌘⇧B for the sidebar; a panel put away for a narrow window comes back when it is wide again, and the panels give way by rank.
8. **Attachments**: Thumbnails for TIFF, HEIC and other pictures, and attachments as even rows of pictures and file tiles by kind.
9. **Fixes**: The figures over the sessions page fit a narrow width; question cards stack their buttons when narrow; chat markdown no longer reads setext headings, indented code or HTML; a selection in the transcript stays on release; drafts belong to the Emaki that wrote them.

## v0.2.8

1. **Accents**: Periwinkle, the blue of emaki.lol, with orchid, rose, amber, moss, jade and teal beside it in Settings, Appearance; terracotta and graphite are gone, and the icon's rolls are periwinkle.
2. **Settings**: Groups of rows on cards, a switch for what is on or off, and every section fits without scrolling.
3. **Artifacts**: The pages an agent publishes are listed over the composer, each a click from opening, and a background agent sent more work shows as running again.
4. **Changes**: The Changes sheet shows a changed picture before and after, side by side, and making or switching a branch, pulling, reverting and restoring are no longer refused while an agent works.
5. **Sessions**: A conversation Claude Code moves into a background session stays one session in the sidebar, with the composer reaching it, and the agent terminal pastes screenshots.
6. **Dictation**: Moved out of Emaki into Kotoba, the studio's dictation app, which types into the composer as into any field.
7. **Fixes**: A selection dragged to the edge of a wide code block scrolls it, and the light terminal's diff and Copied note are soft on the cream.

## v0.2.7

1. **Dictation Anywhere**: The shortcut starts and stops dictation in any app, pastes the words where you type and puts your clipboard back, with a pill at the top of the screen while it listens; allow Accessibility from Settings, Dictate.
2. **Dictation in Emaki**: Writes into the field you are in, a file's editor included, not only the composer.
3. **Preferences and Terms**: Your own post-processing instructions in plain words, and names spelled exactly as you save them, capitals and all, added several at once.
4. **Post-processing**: Numbers said in words become digits, and lists become lists, when asked.
5. **History**: The folder is a click away, and a shorter time trims it at once.
6. **Performance**: Long sessions open about four times faster.
7. **Fixes**: Settings, Composer shows the send keys as keys, and Esc cancelling dictation no longer plays the stop sound.

## v0.2.6

1. **Post-processing**: A small model on this Mac (Qwen3.5 4B, 2B or 0.8B) removes hesitations, applies corrections, mends grammar and makes lists, in any language, without translating; turn it on in Settings, Dictate.
2. **Dictation Settings**: Two lists, Speech to Text and Post-processing, each showing the model in use, with several downloads at once.
3. **History**: What was said is kept on this Mac for as long as you choose, with a window to look back and copy it, as heard or post-processed.
4. **Dictation Keys**: A modifier tapped alone, such as Right ⌘ or Fn, starts and stops it, Esc cancels, and the chimes are softer.
5. **Agents Page**: Steps for this machine only, and GitHub in place of GitHub Desktop.
6. **Controls**: Every button has an icon, and a switch turns at a click anywhere on it.
7. **Fixes**: Sessions order by when they last did something, a tooltip no longer outlives its sheet, text stops selecting after a click in the Changes sheet, and a window over Settings scrolls.

## v0.2.5

1. **Dictation**: Talk into the composer with the microphone button or ⌘⇧D; a model downloaded in Settings, Dictate (Qwen3-ASR or Whisper) writes it down on this Mac, Chinese and English mixed, Mac only.
2. **Changes**: The Changes sheet commits the files you tick and has a History tab to browse, undo or revert commits, with Push, Pull and Fetch beside the counts.
3. **Settings**: A page a section, Off and On for every switch, a field for dictation's key with Reset, and an About page.
4. **Codex**: Sessions are archived, unarchived and deleted through Codex itself, and an archived one is unarchived from its menu in the sidebar.
5. **Agents Page**: Walks through installing git and each agent and signing in, a numbered step at a time, for the system you choose.
6. **Cards**: Cards that come up while a session runs stand at the conversation's end and scroll with it, and a dialog's tabs sit in a strip with arrows.
7. **Left Running**: A Claude Code left running after its terminal closed is named as such, with a button to stop it and continue here.
8. **Fixes**: Square corners past rounded panels, sheets, menus and cards, and Codex's usage limits.

## v0.2.4

1. **Every Question Shown**: A question Emaki has no card for, such as Claude Code asking whether a new folder is trusted, comes up on the terminal card and no longer waits unseen.
2. **Selection**: Sessions in the sidebar are selected together with ⌘-click, ⇧-click or a dragged rectangle, across projects, and archived or deleted from their menu; a project's menu does the same for its sessions.
3. **Changes**: The Changes sheet selects files the same ways, has a right-click menu, discards several files at once and closes with ⌘W.
4. **Files**: A markdown file's YAML header shows as a table of its fields over the text.
5. **Fields**: ⇧↑ and ⇧↓ select to the start and the end in every one-line field.
6. **Fixes**: A question card's buttons are pills with an icon, and a table column no longer breaks a word in its middle.

## v0.2.3

1. **Sidebar**: Every project is listed and scrolls, pinned projects stay above the rest, and projects are selected together with ⌘-click, ⇧-click, a dragged rectangle or ⌘A, then opened in Finder or hidden; hidden projects and temporary folders sit under their own headings.
2. **All Sessions**: Projects unfold in place with their dates, archived projects have a section, and sessions are selected by project or one by one, then archived or deleted; Archive checks Emaki's copy first, Delete moves every copy to the Trash, and nothing in a project's folder is touched.
3. **Conversation**: An agent's progress notes show as part of the reply, a running command says what it is for, and a wide code block scrolls sideways instead of wrapping.
4. **Navigation**: "New messages" scrolls to the new part, a project opened near the foot of a list scrolls into view, and a session begun by `/clear` opens with it.
5. **Fixes**: Stray lines around the logo in About, and a `/clear` session no longer flashes under a stray folder.

## v0.2.2

1. **Word Documents**: Open as pages and can be written in: type, select, style, format, undo and save with ⌘S, on WordCraft's engine; try it on a copy first, since a save rewrites the file.
2. **More Files**: PowerPoint slides drawn as they look, Excel number formats, PDF pages sharp at any width, HTML in the system's web view, and Parquet as a table with each column's type and the row count.
3. **Files Panel**: Works like a file manager: drag files in from Finder, drag rows to move them, cut, copy, duplicate, remove with ⌘⌫, and take it all back with ⌘Z; files, folders and sessions rename in place.
4. **File's Pane**: Takes over the conversation's room with the enlarge button, opens a file as it reads rather than as source, and tables keep their heads in view and select blocks of cells.
5. **Explanations**: Explanations and outline labels come from the session's own agent, Haiku for Claude Code and Codex's light model for Codex.
6. **Home**: ⌘↩ or the send button with nothing typed starts a session, each agent's page links to its sessions, and the composer's draft survives closing the window.
7. **Fixes**: The keyboard is no longer lost after a file closes, ⌘W closes a file or a tab before the window, pages no longer blink when the pane is resized, and Settings has an About section.

## v0.2.1

1. **Look**: The sidebar and a thin frame in graphite around a page near white, and in the dark appearance a darker page in a lighter frame.
2. **Fonts**: Eight chat fonts and four monospace fonts built in, and one font size for the conversation, the file editor and the terminal.
3. **Format on Save**: Ten languages: R, Python, Rust, Go, C and C++, TypeScript, JavaScript, JSON, Markdown and TOML, off until you turn it on.
4. **Conversation**: A message shows the moment it is sent, a prompt's bubble has a tail, tool badges are in color, and `/clear` is followed to the session it starts.
5. **Projects**: Pin a project to the top of the sidebar, start a session in the current project with ⌘T, and choose a folder with ⌘O.
6. **Files Panel**: ⌘A selects everything, a drag over the rows selects the ones it covers, and a right click in the editor selects the word under it.
7. **Codex Alone**: With Claude Code not installed, a light Codex model writes the explanations, and the home page points to the Agents page when no agent is installed.
8. **Settings**: Shorter: one Spelling Check entry with its languages, flags on the languages, icons on the themes, fewer notes.
9. **Fixes**: "New messages" glides to the end, buttons and menu rows show in the dark appearance, and a small window shows one panel at a time.

## v0.2.0

1. **Codex**: Codex joins Claude Code: its sessions are kept, read and searched, and you can start, continue and stop one from the window, with its approvals, questions and plans on the same cards.
2. **Agent Terminal**: Codex's own terminal is on the terminal panel, joined to the same session, so a turn typed there shows in the conversation and the other way round.
3. **Agents Page**: ⌘E says which agents are installed and signed in, with the commands to set one up and a shell beside them.
4. **New Sessions**: The agent is chosen on the strip under the composer, with its mode, effort and model as lists there and in Settings, New sessions; the mode pill opens a list for Claude Code too, in each mode's color.
5. **Usage**: The row under the composer shows context and usage for Codex, with whatever windows its plan has, and each agent has its color, Claude's terracotta and Codex's green.
6. **Tabs**: Keep a readable width and scroll once they no longer fit, the one you are on stays in view, and ⌘ shows 1 to 9 on them.
7. **Branches**: Fetch, make a branch from the list, publish a local one, and see how to sign in when the remote refuses.
8. **Motion**: A plan is shown on its approval card and stays in the conversation, and tool calls, thoughts and rows open and close in motion.
9. **Fixes**: The archive is a folder an agent, moved into place at launch; a session begun from a folder no longer ends with "No conversation found"; lists in the dark appearance show the row under the pointer.

## v0.1.9

1. **Editing**: Edit a file in the pane beside the conversation and save with ⌘S; unsaved changes are never lost without asking, and git's changes are marked in the margin, each with a card to see or revert it.
2. **CSV**: Opens as a grid with lettered columns and numbered rows; edit a cell in place, or the file as written, where each column has its own color.
3. **Office Files**: Word, PowerPoint and Excel files open to read.
4. **Tabs**: Grouped by project into pills; drag a tab within its pill, or a pill by its folder mark.
5. **Home**: The greeting and the composer, with the folder to start in on a strip under it, and a plus on a project in the sidebar starts a session there on a tab of its own.
6. **Files Panel**: Copy and Paste on the menu, ⌘C and ⌘V on a row (to and from Finder too), ⌘Z to take a paste back, and a file's changes discarded from the Changes panel after a confirmation.
7. **Settings, Files**: Formatting R files on save (Air), on or off.
8. **Fixes**: A rename made while the agent works waits its turn, copying from the agent terminal reaches the clipboard, a session stays in the folder it started in, and Emaki no longer asks any terminal app for anything.

## v0.1.8

1. **File's Pane**: Click a file in the files panel to read it beside the conversation: code with line numbers and colors, markdown as it reads or as written, pictures, tables, and PDFs with search, selection, page previews and a table of contents; click again, or ⌘W, closes it.
2. **Writing**: The composer checks spelling and grammar as you type (English US or UK; Spanish, French and German spelling on a Mac) and capitalizes a new sentence; right-click a marked word for corrections, Learn Spelling or Ignore.
3. **Menus**: Every text field has Emaki's own right-click menu.
4. **Following**: The conversation follows new content when you are near the bottom, and a "New messages" button takes you to what arrived while you were reading higher up.
5. **Terminal**: One look for the shell terminal and the agent terminal, after Kaku, with a Terminal section in Settings; ⌘⇧T and ⌘⇧A show and hide them, and in the shell ⌘T opens a tab and ⌘W closes it.
6. **Pills and Code**: The model and effort pills open a list in place, and code blocks in a conversation are colored.
7. **Archive**: Sessions are copied as clones where the disk allows, and a session its agent deleted stays listed as archived.
8. **Look**: New icons (Tabler), the board is gone, Projects is now All Projects, and a folder inside a repository shows git's marks for its own files only.
9. **Fixes**: A question with previews reads correctly and can be answered, a session whose process is gone stops showing as working, and a stopped prompt is not sent twice.

## v0.1.7

1. **Terminal Panel**: A terminal inside Emaki at the conversation's right: shells in tabs, and the session's own agent as it runs; nothing opens a terminal app any more.
2. **Mouse and Keys**: Select with the mouse (copied on release), click and scroll in programs that take the mouse, and the line keys of a Mac terminal (⌘←, ⌘→, ⌘⌫, ⇧↩, ⌘K).
3. **Files Panel**: The project's folder as a tree with git's marks, a file's contents, what changed, and a switch of branch that carries your changes along or leaves them behind to restore later.
4. **Outline Panel**: A short label per prompt; click one to go there.
5. **Menus**: Right-click menus on sessions, tabs, files, the conversation, attachments and the branch name, a copy button on code blocks, and a right click on a word selects it.
6. **Composer**: Send with ⌘↩ or with ↩ (⇧↩ is always a new line), running subagents in a row over the composer, and "Type something" on a question card for your own answer.
7. **Tabs**: Hold ⌘ to see each tab's number.

## v0.1.6

1. **Hidden Terminal**: Sessions run in a terminal of Emaki's own; nothing done in Emaki touches a terminal of yours.
2. **Modes**: ⇧Tab or the mode pill changes the mode in place, and the model and effort pills show Claude Code's picker on a card.
3. **Composer**: Type `@` to pick a file or folder from the project, with a path that exists in the accent color, and a command left running in the background shows in a row over the composer.
4. **Sidebar**: Agents and Projects on cards, an edge to drag or double-click to fit, and the sidebar button and search beside the traffic lights.
5. **Tabs**: Switch instantly, share the row, drag to reorder, and answer to ⌘1 to ⌘9.
6. **Sessions**: Renaming a session renames it for Claude Code too, and every session keeps its own draft.
7. **Look**: Your own picture and name in Settings, Appearance, new icons (Phosphor), and scrollbars that fade after a second.

## v0.1.5

1. **Icon**: The scroll on cream, with a conversation on its sheet.
2. **Terminal Sessions**: A question or approval waiting in a terminal shows as a card in the window, the row under the conversation shows the terminal's own line in its colors, and a session started in Emaki shows in the terminal's resume list.
3. **Suggestions**: The prompt Claude Code suggests after a turn shows in the composer; → or Tab takes it.
4. **Modes**: Read from Claude Code and colored as it colors them, a change is one short line in the conversation, ⇧Tab steps a terminal session's mode, and the effort and model pills open the terminal's picker.
5. **Composer**: ⇧↑ and ⇧↓ select one line at a time, a paste scrolls to the caret, and Stop gives your message back only when Claude had not started on it.
6. **Fixes**: Durations read in whole seconds.

## v0.1.4

1. **Icon**: A handscroll on the terracotta plate.
2. **Home**: Recent folders as cards under a serif greeting; recents and the sessions page grouped by day, with a live row showing its state.
3. **Board**: Four open columns that reflow with the window and never scroll sideways.
4. **Settings**: Sections and segmented controls, and updates as one button.
5. **Composer**: Type "/" for the folder's slash commands; a command runs, a skill goes into the message.
6. **Questions**: Claude's questions show as cards, a session started in Emaki answers them from the window, and a terminal session takes you to the terminal and brings the window back when you are done.
7. **Turns**: Stop a turn from the window, with the message returned to the composer, and a message sent while Claude works shows at once, marked queued.
8. **Fixes**: `/compact` no longer leaves the board on "working", the context percentage uses the right window size, and softer shadows, pages that fade in, less work while idle.

## v0.1.3

1. **Top Strip**: The terminal, project folder and transcript buttons sit together at the top right, and the session's folder path is under the tabs in place of the stats.
2. **Look**: The app's icon and name in the sidebar, inline code in the accent color on a tinted plate, code in SF Mono (or Anthropic Mono from `~/.emaki/fonts`), and a tool card naming its tool once.
3. **Conversation**: A copy button and the time under a prompt or reply on hover.

## v0.1.2

1. **Status Line**: The script ships with the app and feeds the limits row; `emaki-core statusline restore` puts the old setting back.
2. **Open in Terminal**: Continue a session with `claude --resume` in a terminal window.
3. **Conversation**: Opens at its end and follows the reply as it streams, with text as the Claude app sets it and Claude's own mark turning while it works.
4. **Notices**: Shown under the composer, not in the sidebar.
5. **Fixes**: Bold drew as regular, scroll momentum crossed panels, and arrow keys stopped short of the composer's ends.

## v0.1.1

1. **Standalone**: The app stands alone; the Python command-line tool and web daemon are gone, and there are no hooks (delete any `emaki-hook` entries left in `~/.claude/settings.json`).
2. **Updates**: Emaki updates itself, from Settings or a daily check.
3. **Explain**: One plain sentence on what an opaque tool call does, on tool and permission cards.
4. **Limits**: Context and rate limits under the composer.
5. **Fixes**: Long tool output scrolls inside its card, pasted pictures show their names, a crash on some Codex sessions, and ⌘↩ leaving a line break.

## v0.1.0

The first release: every coding-agent session on your machine, read, searched and continued in one window.

1. **Kept for Good**: Every Claude Code and Codex session archived byte for byte, staying after Claude Code deletes it.
2. **Board**: Live sessions by state: needs you, planning, working, your turn.
3. **Search**: Full text across every session (⌘K), and find inside one (⌘F).
4. **Conversation**: Laid out like the Claude app, with a reply from the window, attachments, permission cards, and mode, model and effort pills.
5. **Markdown**: Every session also renders to a markdown file.
6. **Settings**: Appearance, accent, font and text size.
7. **Install**: On macOS drag Emaki to Applications (signed and notarized); on Windows SmartScreen asks once; on Linux run the AppImage or install the `.deb`.
