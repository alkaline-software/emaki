# Changelog

Each release has a section here, and `release.yml` puts that section on the
GitHub Release as its notes. Write it for the person installing the app.

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
