# Changelog

Each release has a section here, and `release.yml` puts that section on the
GitHub Release as its notes. Write it for the person installing the app.

## v0.1.4

- **A new icon.** A handscroll on the terracotta plate: cream paper
  between two rollers, a chevron and three lines of ink on the sheet. It
  is drawn, not generated, so it reads at every size and matches the
  window's colours.
- **The home page starts in a folder you can see.** The six most recent
  folders are cards with the folder's name in front and its parents
  dimmed, three to a row, where a cloud of full-path pills used to wrap
  every which way. The greeting is set in the Claude app's serif, with
  the date and how many sessions are live and kept under it.
- **The sidebar's recents are headed by when.** Today, yesterday, this
  week, this month, earlier. The agent's mark is quiet unless the session
  is live, so the list reads as titles, not as a column of marks. A
  settings gear sits in the footer.
- **The sessions page is headed the same way**, and a live row carries
  its state as a chip at the right (working, your turn, needs you)
  instead of a few cut-off words from its last message.
- **The board has no grey slabs.** Four open columns under hairlines,
  side by side when the window has room, two by two or one under another
  when it does not, never scrolling sideways. An empty column says so
  inside a dashed outline. Done is a row under them that opens into a
  grid.
- **`/compact` no longer leaves the board working.** The summary Claude
  Code writes after compaction showed as a message of yours with a clock
  running under it, and the context figure kept the pre-compaction number
  until the next turn. The summary is Claude Code's own and is not shown;
  a slash command typed in the terminal counts as answered by its own
  output; the context drops to the summary's size at the boundary. The
  round reads as the command and one line, "Context compacted", not the
  command twice and a terminal hint.
- **Claude's questions are cards, and a session started here answers
  them from the window.** A question Claude asks shows in the
  conversation with its options and, once answered, the choice ticked.
  On a session Emaki is driving, the question also sits above the
  composer like a permission card: click an option, or type an answer.
  On a terminal session the line under the conversation says Claude is
  waiting for your answer in the terminal, instead of nothing.
- **The terminal is one click away, and the window comes back.** For
  what only the terminal can take on a terminal session, a question's
  dialog, an approval, a slash command, the mode, the window brings that
  terminal to the front, selecting the right tab or pane in Terminal,
  iTerm2, WezTerm and Kaku, and returns to the front once the transcript
  shows it done. The button sits on the waiting line, the command card,
  the mode and effort pills, and the top-right terminal button.
- **Slash commands, on every session.** Typing "/" opens the list of
  commands the session's folder knows, with their descriptions, read from
  Claude Code itself. A click runs one: through Emaki on a session it
  drives; on a terminal session, by typing it into that terminal and
  sending it, since Claude Code takes a command only at its own prompt.
  Terminal, iTerm2, WezTerm and Kaku are typed into directly; an IDE's
  terminal needs Accessibility access for Emaki, and without it the
  command is put on the clipboard. ↑ and ↓ move through the list, ↩ runs
  the one chosen (the first, after every keystroke), ⇥ completes its name
  and Escape closes the list; long argument hints and descriptions are
  cut with an ellipsis.
- **Commands and skills are told apart.** `/compact` or `/model` chosen
  from the list runs; a skill such as `/ph-image` goes into the message
  instead, so it can be asked for in a sentence. The list opens wherever
  a "/" is typed, not only at the start, and every command a message
  names is coloured, while typing and once sent.
- **A command's result shows after a compaction.** A `/model` run after
  `/compact` showed its name and not what it answered.
- **The context percentage is of the session's real window.** One model
  comes with a 200k or a 1M window, and Emaki guessed, so a session
  switched to Opus read 119% where the terminal said 23%. The status
  line now tells Emaki each session's window, and a context larger than
  the guess corrects it.
- **Stop a turn from the window, and get your message back.** "Claude
  is working" has a Stop button, which does what Escape does in the
  terminal, and a turn stopped in the terminal shows as stopped here,
  `/compact` included. Either way the message you sent returns to the
  composer with its pictures and files, ready to edit, and leaves the
  conversation when Claude had not started on it, so sending it again
  does not show it twice. "[Request interrupted by user]" is no longer
  drawn.
- **Commands reach an IDE's terminal wherever its focus was.** In
  Positron, Cursor or VS Code, a command sent from the window went into
  whatever had the focus, a file as often as not. The terminal panel is
  focused first.
- **Mode, effort and model are pills with lists, on terminal sessions
  too.** Each is a grey pill with an icon that darkens under the pointer
  and again when pressed. On a terminal session a model or an effort
  picked from the list is sent to the terminal as its command; plan mode
  goes as `/plan`, and the other modes take you to the terminal to press
  ⇧Tab. The pills show the terminal's model and effort as they change,
  and plan mode as soon as `/plan` has run.
- **Lighter while something is moving.** Every frame read the project
  registry off disk once per session listed; the names are now kept in
  memory. The status line's files are read once a minute and when the
  conversation moves, not every second.
- **A message sent while Claude is working shows at once, then in its
  place.** Claude Code folds such a message into the running turn and
  records it apart from the conversation's rows, so the window never
  showed it. It now appears the moment it is sent, marked as queued, and
  once Claude takes it up it is a prompt where it was sent, with what
  Claude did next under it.
- **Settings are segmented controls.** Each choice sits on a raised plate
  in a muted track, in both appearances, and the plate slides to the
  choice you click instead of jumping.
- **Softer depth.** The composer, the search palette and the settings
  panel lift off the page with a wide, warm shadow instead of a grey
  halo. The search palette is centred in the window; it sat at the left
  edge.
- **Pages settle in.** Home, sessions and the board fade and rise over a
  moment as they arrive. The conversation itself never animates.
- **The composer says what it does.** "Start a session…" on the home
  page, "Reply…" on a conversation.
- **Settings in sections.** The panel is a fixed-size sheet with a rail
  on the left: Appearance, New sessions, Explanations, Updates. One
  section shows at a time, and the rows scroll with a scrollbar when they
  need to.
- **Updates, in one button.** *Check for updates* becomes *Update to x*
  in the same place when there is one, with a small *Release notes* link
  under it, and the daily check is a plain tick. The sentences that
  explained all this are gone.

## v0.1.3

- **Three buttons at the top right take a session somewhere else.** The
  terminal button moved from the top left to sit with the other two: open
  the session in your terminal, open its project folder, show its
  transcript file. The project folder button is new.
- **Under the tabs, the session's folder and nothing else.** The long
  grey line of branch, rounds, tool calls, tokens, model and id is gone.
  In its place is a band with the folder's full path, its own name
  picked out, and a click on it opens the folder.
- **The app's icon and name in the sidebar.** Emaki's icon and its name
  have a row of their own above "New session", where a plain asterisk
  and a small name sat beside the window buttons.
- **A copy button on every prompt and reply.** Hover a prompt or a reply
  and a copy button appears under it, with the time. It copies the
  markdown as it was written: your words, or the whole reply without its
  tool calls.
- **Only the time under a prompt, and only on hover.** The line that read
  "You · Emaki 19:39 · 41.7s · 4 tool calls · 19.7k tokens" under every
  prompt is now the time alone, shown with the copy button.
- **Inline code looks like the Claude app's.** `Inline code` is drawn in
  your accent colour on a rounded plate tinted with it, with room around
  the letters, and it follows the accent you pick in settings.
- **Code is set in the Claude app's face.** On a Mac, code is drawn in SF
  Mono, the system's monospaced face, where it was Menlo. The Claude app
  itself uses Anthropic Mono, a font it downloads as it runs; Emaki does
  not ship it, and uses it when you have put it in `~/.emaki/fonts`.
- **A tool card names its tool once.** The label at the left of a card
  now says the tool itself (`bash`, `read`, `write`), followed by the
  command or the file. It used to say "run Bash", "read Read", "write
  Write".

## v0.1.2

- **The status line is Emaki's own.** The five-hour and seven-day limits
  under the composer came from a script outside the app, patched by hand;
  the patch went missing and the row froze. Now the script ships inside
  the app: at launch it lands at `~/.emaki/bin/statusline.sh` and Claude
  Code's `statusLine` is set to run it, with nothing else in your settings
  touched. The line in the terminal reads as before, `Context 18% | 5h:
  12% (3h20m) | 7d: 42% (4d6h)`, and the row in the app follows it from
  the next Claude Code session. `emaki-core statusline restore` puts the
  old setting back.
- **No tooltip on the limits row.**
- **Open in your terminal.** A button at the top left of a conversation
  continues that session in a terminal window of your own: the session's
  folder, `claude --resume` (or `codex resume`), in the terminal app your
  Mac keeps for shell scripts. It refuses when the session is already open
  in a terminal, so two processes never write one transcript.
- **Find hits wear one ring.** Whatever ⌘F matched, a prompt, a reply
  paragraph, a tool card or a folded run, it is outlined the same way: a
  faint accent ring for a hit, a full one for the hit the bar is on.
- **Bold is a semibold, and inline code is monospaced.** Strong text
  in a conversation is set at weight 600, as the Claude app sets it,
  instead of the font's heavier true Bold, and `inline code` is drawn in
  the mono face like a code block. The UI toolkit is now vendored under
  `vendor/` to make both possible.
- **The conversation follows the reply.** Sitting at the end, the view
  stays at the end while a reply streams in, and comes back to following
  once you scroll to the bottom again.
- **The scrollbar goes sooner.** It fades a second after the last scroll,
  not two.
- **A session opens at its end.** Opening a session, or coming back to
  its tab, lands on the newest turn instead of wherever it was left.
- **Claude's own mark, and the way it moves.** Claude is drawn with the
  starburst from the Claude app wherever the agent is named, and while
  Claude works the mark on the status row turns and breathes, as the first
  Emaki's did. Emaki's own asterisk stays on the wordmark.
- **A flick stays where it started.** A trackpad scroll's momentum no
  longer follows the pointer into the other pane: a flick in the
  conversation keeps scrolling the conversation while the pointer crosses
  to the sidebar, and the other way round.
- **The arrow keys reach the ends of the composer.** Up on the first line
  puts the caret at the start of the text, Down on the last line at the
  end, as in any input box.
- **The conversation is set at the Claude app's size.** Medium is now
  16px at line height 1.5, the same as the Claude desktop app's replies;
  small and large moved up with it.
- **Bold is bold again.** Bold and semibold text in a conversation drew
  in the regular weight, because the Claude app's fonts are variable
  fonts and only their regular instance had been loaded. On macOS every
  weight is available now.
- **Messages under the composer, not in the sidebar.** What a send or a
  click did ("opened in your terminal") and why it did not (in red) now
  appear at the right end of the limits row under the composer, and fade.
  The sidebar footer no longer blinks "indexed 1 session" after every
  turn; it is your name alone.

## v0.1.1

The app stands alone now. The Python command-line tool and web daemon Emaki
grew out of are gone from the repository, and nothing the app does ever
needed them.

- **Emaki updates itself.** Settings (⌘,) shows the version, checks for a
  newer release on a click or once a day (a tick turns the daily check
  off), and *Update* fetches the installer for your machine, puts it in
  place and restarts the app with your tabs as they were.
- **Opaque tool calls explained in plain words.** A tool row has an
  *Explain* button; click it and one short sentence on what the call does
  appears under the row, written by Haiku through your own Claude Code
  login. A permission card shows the same while you decide. Simple calls
  explain themselves for free, and an answer is kept by content, so a
  command explained once is annotated everywhere it appears. The settings
  panel chooses whether cards, every new call, or nothing gets explained
  unasked.
- **The status line, under the composer.** `Context 32% | 5h: 5% (4h45m)
  | 7d: 11% (6d2h)`, the same numbers and colours as the terminal's status
  line, fed by it: `~/.claude/statusline.sh` leaves a copy of the windows
  for Emaki.
- **Long tool output scrolls inside its card**, capped at a comfortable
  height with its own bar. A scroll stroke that reaches the card's edge
  stops there; the next stroke moves the conversation.
- **Opening a tool call keeps its row where it is**, also at the very end
  of a conversation, where the new content used to push the row up.
- **Pictures show their names.** A pasted picture is captioned with the
  `[Image #n]` the terminal gave it, in a square tile; the preview shows
  the whole path, selectable, with *Open* and *Reveal in Finder* when there
  is a file. Text pasted into the terminal shows without its wrapper tags.
- **⌘↩ sends the message as typed**, with no line break left where the
  caret stood.
- **A Codex session that quoted YAML front matter no longer crashes the
  app.** The markdown parser the text view uses aborts on that shape; such
  a text is now shown as a code block instead.
- **No hooks, ever.** The old daemon registered hooks in Claude Code's
  settings by absolute path; renaming the folder broke them and Claude Code
  blocked every prompt. The app installs nothing. If those entries are still
  in your `~/.claude/settings.json`, delete the ones naming `emaki-hook`.
- **Removed:** the `emaki` command, the web viewer, and the hook. The archive,
  the markdown and `~/.emaki` are unchanged and the app reads them as before.

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
