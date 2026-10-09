# Channels: how the window reaches a session

Source: `crates/emaki-core/src/driver.rs`, `pty.rs`, `peer`, `terminal.rs`;
`crates/emaki-app/src/hub.rs`, `sys.rs`, and the channel code in
`workbench.rs` (`reply_via_for`, `via_terminal`, `send_message`).

## Rules

- Where a session was started makes no difference. One begun in Emaki,
  in a terminal app or in an IDE's terminal is the same session: it is
  read, replied to and taken up here the same way, and it can be resumed
  in any terminal with its own resume command (the session's menu copies
  it). The one thing that depends on another process is a turn running
  there now, which is never joined by a second Claude Code.

- Between turns, nothing is sent to, typed in or ended in a terminal of
  the person's. The window uses its hidden terminal. Ending their Claude
  Code to resume it hidden dropped their IDE terminal to its shell.
- Start no second Claude Code on a running turn. Mid-turn in their
  terminal a message goes to that inbox; a mode, pick or command says to
  wait (`theirs_busy`).
- `reply_via_for` asks the driver, then our pty, then the registry. Both
  of our children register an inbox of their own.
- "Driven" is the window's record and the hub's together
  (`Workbench::driven`). The hub lets a driver go without a word, and the
  record alone refused every message with "the driver is gone".
- No hidden terminal on opening a session. A resumed Claude Code writes
  to the transcript, so a conversation only read would read as live.
- Never type a message while the registry says `waiting`: the words would
  answer the dialog.
- Empty Claude Code's prompt before typing a message into it
  (`Hub::type_message`: ^E, ^U and Backspace, a line at a time, until
  `prompt_on_screen` says empty). A prompt stopped at once is put back
  into Claude Code's own input as well as into the composer, and the
  next message went out with the old words in front of it, doubling at
  every stop.
- Count "[Image #" marks from what the screen showed before each paste.
  Earlier messages' marks are in view; counted from zero, Return went
  before a large picture was read and the picture was lost.
- Every child clears each `CLAUDE*` variable but `CLAUDE_CONFIG_DIR`
  (`driver::child_env`; the resume script too). The app may be a
  grandchild of a session: the child would take its parent's id and
  inbox, and a resumed session stops writing its transcript.
- Never `claude -p --bare`: it reads auth only from `ANTHROPIC_API_KEY`,
  never the keychain, which breaks subscription users.
- Only `claude -p --resume <id>` appends to the transcript. `claude --bg
  --resume` forks under a new id.
- The driver's child runs with `CLAUDE_CODE_ENTRYPOINT=emaki`
  (`driver::ENTRYPOINT`), or the terminal's resume list hides the session.
- No terminal app is asked anything: no command line of one (`kaku cli`,
  `wezterm cli`), no AppleScript, no System Events keys. Emaki has its own
  terminal, most people have neither Kaku nor iTerm2, and a terminal's
  command line can start the terminal. `sys`'s functions answer for a
  hidden terminal of ours and for nothing else.
- No terminal app of the person's is opened, for anything: what needs a
  terminal is done in the hidden one and shown in the terminal panel.
- Never draw the terminal card on a screen with Claude Code's prompt on
  it. The registry's `waiting` is up to a second old, and the whole
  conversation flashed on the card after a pick.
- Never walk up from a driver's child to find a terminal app: its parent
  is Emaki. `in_terminal` asks the channel, not the registry alone.
- A probe run from a session in a hidden terminal must not quit the
  running Emaki, which is its parent. Use a second copy (`EMAKI_HOME`).

## The channels and their order

`Workbench::reply_via_for`, for a Claude Code session (other agents are
read-only):

1. `driver`: a headless child of ours is behind the session, or starting.
2. `pty`: a hidden terminal of ours is behind it (`Hub::terminal_for`).
3. `inbox`: the registry (`~/.claude/sessions/<pid>.json`, read by
   `emaki_core::peer`) shows a process with an inbox. With
   `driver.hidden_terminal` on (the default), only while that record is
   not `idle`: a turn running in a terminal of the person's.
4. `spawn`: none of those, the session is not archived, the driver is
   enabled and the folder is there.

`in_terminal` is `inbox` or `pty`. `Hub::is_live` trusts the driver, the
hidden terminal and the registry before the mtime grace. `Hub::peer_for`
answers with the hidden terminal's own record once there is one, never
another terminal's on the same session; `refresh_peers` keeps ours of
the two.

On `spawn`, `send_message` starts a hidden terminal and the channel is
`pty`. The headless driver is the fallback when the pty cannot start or
`driver.hidden_terminal` is false, and serves the catalogue and the
explainer.

## The inbox

`Hub::send_to_inbox`: one connection per message, wrapped in Claude
Code's `<cross-session-message>` envelope, no permission mode asserted.
The row is a peer's and is read as prose, so no slash command runs
there. A running turn queues the message.

## The headless driver

`claude -p --input-format stream-json --output-format stream-json
--permission-prompt-tool stdio`, kept alive between turns. `--resume`
appends to the same transcript, and the window shows the reply by
reading the file. Permission prompts arrive as `can_use_tool` control
requests and are answered from a card; ten minutes of silence is a deny.
`Hub::reap_drivers` stops one idle past `driver.idle_min` or whose child
is gone. The docstring at the top of `driver.rs` records what each
Claude Code release does on the wire.

The slash catalogue is read once per folder from a headless child's
`initialize` reply (`driver::catalogue`, `Hub::commands_for`): no model
call, no transcript; the built-ins stand in until it is in.

## The entry point name

Claude Code stamps every row with its entry point, `sdk-cli` for a `-p`
child, and the terminal's resume list leaves out sessions whose first
rows say `sdk-cli`, `sdk-ts` or `sdk-py` (2.1.289). The environment
names it (only `cli` is rewritten to `sdk-cli` on a headless run), so
the driver's child is started with `emaki`. The explainer's children
keep `sdk-cli`: they are not conversations. A session that already
begins with `sdk-cli` stays out of the list, since nothing here writes
to a transcript.

## The hidden terminal

`pty.rs`: the interactive `claude --resume <id>` (`--session-id` for a
new session, in the mode and model chosen) on a pty this process owns
(`portable-pty`), its output fed to a screen model that draws nothing
(`vt100`). The child is an ordinary interactive session: it registers
with an inbox, runs the status line and writes the same transcript, so
the code that follows a terminal follows it unchanged.

Every `sys` function that takes a session's pid (`terminal_text`,
`terminal_styled`, `type_in_terminal`, `key_in_terminal`,
`text_in_terminal`, `focus_terminal`) asks `pty::for_pid` and answers
only there. The screen is in memory (`Pty::styled`, in the form the
readers in `driver` take) and a key is a write: any platform, no app
coming forward, no Accessibility access. For a session in a terminal of
the person's the answer is nothing, or a line saying the thing is done in
that terminal.

**Start.** `Hub::start_terminal`, from a message sent on `spawn`, from
anything `via_terminal` is asked for, and at the first sign the person means to do
something on a `spawn` session and not only read it: a click in the
composer, the first character typed there, the terminal panel opened
(`Workbench::warm_now`, `warm_terminal`). It is
up and registered in 0.7 to 1.4 s (2.1.289), which is why the first
keystroke is early enough and opening the session is not used.

**`via_terminal`** is the one way in for what only a terminal takes
(`TerminalAction`: a pick from `/model` or `/effort`, a typed command,
⇧Tab, going there). In a registered terminal it is done at once. Else an
idle driver is stopped (one mid-reply is refused, `terminal_check`), the
hidden terminal is started and the action waits in `pending_terminal`; a
wish asked again during the wait replaces the older one. The wait is a
state, not a time: `terminal_ready` wants the registry record `idle` (a
login or trust screen says `waiting` and is never typed into) and the
prompt on the screen. It never opens the person's terminal: with
`driver.hidden_terminal` off, or for another agent, it says so and does
nothing.

**A message.** `Hub::send_to_terminal` waits for the registry and the
prompt (`driver::prompt_on_screen`), pastes each picture's path by
itself (bracketed paste; Claude Code turns it into "[Image #n]") and
waits for one more mark on the screen, pastes the words, waits for the
prompt to show them, then presses Return. Each step waits on the screen:
words pasted straight after two paths came out in front of both marks.
The row is the person's own: no envelope, no held message under bypass,
no thirty-second repeat drop, and a slash command runs. A screen that is
something else for five seconds is put in front of the person
(`HubEvent::TerminalNeeded`).

**Letting go.** `reap_terminals` drops one whose child is gone, or that
is not showing and idle past `driver.idle_min`; all go at quit. One that
dies by itself is said once under the composer with the last line of its
screen.

## The terminal card

`Workbench::render_terminal` draws the pty's screen where the dialog
cards sit: `pty::panel_rows` (what is under the "▔" line a picker opens
beneath, else the screen without blank edges), drawn as the terminal
panel draws a screen (`term_card_look`: the same face, rows, shapes and
colour rules, in letters small enough for every column to fit). Its
scheme is the window's own appearance, light or dark, whatever theme
Claude Code is set to and whatever the panel's scheme is set to: the card
sits in the conversation.

- Keys: the card holds the focus (`term_focus`, key context `Terminal`)
  and `term_bytes` turns a key into terminal bytes. Tab and ⇧Tab are
  bound there or the toolkit's focus traversal takes them. Escape is
  Claude Code's whenever the card is up, wherever the keyboard is.
- Pointer: Claude Code takes no mouse, so `pty::hits` maps a click to
  keys, each a stretch of a row laid over the cells it covers. A numbered choice is that many arrows from "❯"; a slider level is
  arrows from "▲"; a key the screen names is that key. A choice or level
  is confirmed too, the arrows and Return in one write, which 2.1.289
  takes whole. Return saves the default for new sessions; `s` is session
  only.
- Drawn only with a picker on the screen (`pty::picker_up`) or the
  registry `waiting`. The card is asked for before the picker is drawn
  and just after it closes; drawn then it jumped from tall to small.
- Shown at once for the model and effort pills, for a typed command once
  the registry says `waiting` (`/status`, `/config`), and by itself for
  a waiting screen `dialog_on_screen` cannot read (`term_auto`). It goes
  when `watch_terminal` sees the wait over, or by its close button
  (Escape). ⇧Tab and Stop never show it; a click on the mode pill is one
  ⇧Tab.
- Animation (`TERM_ANIM`, `TermShown`): held by its foot, opening up
  from the composer. While closing it is drawn from its last rows and
  takes no click. The gap under the card is an element inside the height
  that runs, in a column with no gap. Tried and dropped: the column's
  gap (a stop at the end) and a negative margin with padding (a shake).

## The terminal panel

`term_panel.rs`: a terminal at the conversation's right, under the top
strip. It shows one of two things, one at a time, each with a button at
the strip's right end (`term_buttons`, the same two-segment control as
the files and the outline at the left: the shell's first, then the
agent's) and a key (⌘⇧T for the shell, ⌘⇧A for the agent; Ctrl off a
Mac). A press on the other swaps the side and slides the plate, a
press on the one showing puts the panel away (`toggle_term`, `term_go`).
`side_term_anim` keeps the side before the press and the one after, and
the sides come, go and take each other's place as the two panels at the
left do (`render_term_panel`; the side that goes is drawn for that moment
as it was, with nothing started, sized or measured for it). The side and
the width are in `ui.json`. A double click on the panel's edge puts the
width back to the one it began with.

The conversation keeps `panels::CONVERSATION_MIN` between the panels at
its two sides, the width at which the composer's pills and send button
still fit their card. Both panels are drawn no wider than leaves it
(`term_panel_w`, `panel_w_now`): the terminal gives way first, down to
its least, then the files or the outline. The conversation is never
squeezed under that width (`Workbench::render`): where the panels asked
for do not fit beside it at their least, the sidebar folds away as it
does in a narrow window, and in a window too narrow even so a panel is
not drawn until there is room again, the files or the outline first and
then the terminal (`fold_left`, `fold_term`). What was asked for is kept.

A side's head is as tall as the path bar over the conversation, so the
heads read as one line across the window, and it is a row of tabs on the
terminal's own ground, as Kaku's tab bar is: the agent's side has the one
tab, with its mark in its colour, and the shell's a tab a shell. It has
no close button: the strip's button puts the panel away.

**The look is Kaku's** (tw93/kaku, MIT), the same for both sides:

- The scheme is Kaku Dark or Kaku Light by the window's appearance
  (`scheme`): ground, ink, cursor, selection, tabs, toast and the sixteen
  named colours. A span keeps which named colour it asked for
  (`Span::fg_ix`, `bg_ix`), so those are drawn from the scheme and not
  from the screen model's own table.
- The grounds and inks Kaku swaps (`grounds`, `inks`) are swapped, and an
  ink is taken toward black or white until it stands 3 to 1 against its
  ground (`legible`, Kaku's `text_min_contrast_ratio`). That is what makes
  Claude Code's colours, chosen for its own theme, readable on either
  scheme; the panel no longer asks which theme Claude Code is set to.
- A grey ground from the other side of the scale is the scheme's quiet
  ground (`Scheme::wash`), and a grey ink that was to stand out on it is
  the scheme's ink. Claude Code set to a dark theme draws a near-black
  band behind each prompt, which on the light scheme was a black bar
  across the screen, as it is in Kaku.
- The face is JetBrains Mono with Kaku's fallbacks, built in
  (`fonts::TERM_FAMILY`, `assets/fonts`), no ligatures, no italic, plain
  and bold a weight heavier on the light scheme, a row 1.28 of the font's
  own line. The size is 13, smaller than Kaku's 17: the panel is a part
  of a window.
- Those are Kaku's own; ours begin at the middle step of the row's
  height and of the room around the screen. `terminal` in `config.json`
  (`config::Terminal`), changed in Settings, Terminal, chooses the scheme
  (the window's, or one kept), the face (`fonts::term_family`), the
  size, the row's height, the room around the screen, the cursor's shape
  and whether it blinks, ligatures, and whether a selection is copied
  when the button is let go. `set_terminal` saves a change and the panel
  reads the settings at each draw, so nothing is restarted.
- Kaku's room around the screen (40 of the screen's pixels at the sides
  and the top, 26 on a screen that is not dense), its bar of a cursor
  that is on and off by half seconds from when it last moved and still
  without the keyboard, and its "Copied" toast.

- **Shell.** The person's login shell (`pty::shell_argv`) in the
  session's folder, a tab each in the side's head (`Workbench::shells`, a
  `Shells` a session; `render_shell_tabs`). The first is made when the
  side is first looked at; the + after the last tab makes another
  (`shell_new`), a tab's own button closes it and lets its shell go
  (`shell_close`), and with none left the side says so and stays empty.
  A press shows a tab (`shell_pick`), its screen fading in while the one
  before fades out over it (`shell_swap`), and a drag moves it along the
  row as the window's own tabs move. A new tab grows in from nothing.
  Tabs share the head down to a least width; past that the row scrolls
  and the + is held at the head's right end, with the tab that shows
  brought into view (`shell_reveal`). A tab's name carries one more than
  the largest number in the row when it was made, so a closed tab's
  number is given again; what tells tabs apart in the code is `id`,
  which is never given twice.
  Shells are kept until closed, exited, or the app quits. Their
  environment is `pty::child_env`, so no `CLAUDE*` variable of ours is in
  it and a `claude` typed there is nobody's child.
- **The agent.** The session's hidden terminal, the same `Pty` the window
  reads and types into, drawn whole. With none behind the session it
  is started as soon as this side is looked at (`start_hidden_terminal`:
  the checks of `terminal_check`, an idle driver let go, then
  `Hub::start_terminal`), and the panel says "Starting…" until the agent
  has drawn something. Once a session (`side_term_tried`, forgotten when
  one has run thirty seconds): an agent that ends straight away is not
  started in a loop, and the panel then says it has stopped and offers
  Start. Where it may not be started (a turn running in a terminal of the
  person's, a driver mid-reply, an archived session) the panel says why.
  Claude Code only.

How it works:

- The screen is `Pty::rows_back`, a row a line in the mono face. Under
  the letters each row draws its own cells: a stretch's ground the whole
  height of the row, then the selection, then the block characters
  (U+2580 to U+259F) and the common line-drawing ones as the shapes they
  are (`block_shape`, `line_shape`). Left to the font, those are as tall
  as a letter and not as a row: Claude Code's mark came out in stripes
  and its frames with a gap between every two rows. The pty is resized
  to the rows and columns that fit (`side_term_bounds`, measured by a canvas at the last draw).
- The agent's pty takes the panel's size only while the panel shows it
  (`fit_agent_pty`, at every draw of the conversation) and goes back to
  `pty::ROWS` by `COLS` otherwise: the terminal card and the screen's
  readers were made for that size.
- Scrolling back is the panel's own count of rows (`side_back`), applied
  for the one look and taken off again inside `rows_back`. Set on the
  screen model itself, the readers in `driver` would read an old screen.
- A pty writes from its own thread and tells the window nothing, so while
  the panel shows a task looks every 33 ms and redraws when the screen
  changed in the last moments (`term_panel_watch`).
- Keys are `term_bytes`, plus what a Mac's terminals add (Kaku's list was
  the model): ⌥←, ⌥→ and ⌥⌫ as words; ⌘←, ⌘→ and ⌘⌫ as the line's
  start, its end and all of it before the cursor (^A, ^E, ^U); ⌘↩ and
  ⇧↩ a new line that sends nothing (ESC and Return); the arrows in their
  application form when the program asked for it; ⌘V a paste and ⌘C the
  selection. ⌘K forgets the screen and what has left it and asks for a
  redraw (`Pty::clear_all`, then ^L), not where a program has the screen
  to itself. On the shell's side, with the keyboard in the panel, ⌘T
  or ⌘N is a new tab, ⌘W closes the one showing, and ⌘⇧[ and ⌘⇧] go to the tab before and after. ⌘K, ⌘T and ⌘N
  are bound in the terminal's key context, because a binding is
  answered before any key listener and the window has its own for two of
  them; where the panel does not take one (the agent's side, no tab left)
  it passes it on and the window's is next. ⌘W is not bound there: the
  window's `CloseTab` is a global binding, which outranks one bound to a
  context (`docs/window.md`), so the panel answers that action itself,
  first, being where the keyboard is. On the shell's side ⌘W is the
  shell's alone: with no tab left it does nothing, and never reaches the
  file shown or the session's tab (`docs/window.md`). Any other ⌘ key is
  the window's.
- The pointer is Kaku's. A drag selects, by the letter, by the word after
  a double click and by the row after a triple (`Sel`); with ⌥ it takes
  the same columns of every row, and a press with Shift runs the
  selection there is on to the press. The selection is copied when the
  button is let go, with "Copied" shown for a moment. Rows that run on
  into the next are copied as one line (`Pty::wraps_back`). The
  selection is in the screen's rows as they were drawn, so it is dropped
  on a key, a paste, the wheel, another tab or side; output that moves
  the rows leaves it where it was. ⌥ and a click that does not move, on
  the cursor's row, takes the cursor to that column with arrow keys. The
  middle button pastes, and the right one opens a menu: Copy, Paste,
  Clear, and on the shell's side New Tab and Close Tab.
- A link is underlined under the pointer, which becomes a hand, and ⌘
  and a click opens it, in a program that hears the mouse too
  (`link_at`): an address with a scheme, one that begins "www.", a mail
  address, a bare domain under a well-known ending, and a path to a file
  that is there, relative to the session's folder. The pointer is an
  arrow over a program that hears the mouse.
- The wheel is routed (`Pane::Terminal`): the panel has no scroller to
  hear it. Where a program has the screen to itself (an editor, a pager)
  it is that program's arrow keys.
- The mouse goes to a program that asked to hear it (`Pty::mouse`, the
  modes 1000, 1002 and 1003 as the screen model keeps them): a press, the
  button let go, the pointer moving a cell at a time with a button held
  or, where asked, without, and the wheel as the program's own, each in
  the form it asked for (`pty::mouse_bytes`). So a click, a drag, a
  double click and the wheel in Claude Code's screen are Claude Code's to
  answer. With Shift held the press is the panel's and selects, as in any
  terminal. Nothing is told while the panel is scrolled back.
- Both sides are one terminal: everything above holds for the shell and
  for the agent alike.
- Not done, of Kaku's: its keys beyond the ones above, a link that runs
  over two rows, a tab named by its shell's folder, the scroll bar and
  the split panes. Also not done: a drag followed past the panel's edge
  (no scrolling while selecting), a search, and typing through an
  input method (the keys arrive one at a time, so composed text does
  not).

Embedding a terminal app was looked at and dropped: Kaku is WezTerm with
its own window and renderer, not a view another app can hold.

"Open in Terminal" on a session's menu is this panel on the agent's side
(`open_term_panel_agent`), and there the agent is started when it is not
running and may be: the person asked for it by name.

## A terminal of the person's

Between turns such a session answers `spawn`, so anything done in the
window starts a hidden terminal beside theirs. Mid-turn it answers
`inbox`: a message is queued there, and Stop and a dialog's answer go
where the turn is.

This gives up one writer per transcript: both processes append to the
one file, and theirs does not know what was said here until resumed.
That is the person's decision. Tried and dropped: following the session
into its terminal (in an IDE: an app switch, the command palette, a mode
pill that cannot name its mode).

## No terminal app is opened

Emaki opens no terminal app of the person's, for anything. What needs a
terminal is done in the hidden one, and the person sees it in the
terminal panel. Until 2026-10-07 a button continued a session in the
default terminal app (a `.command` script handed to `open`), and
`via_terminal` fell back to that with `driver.hidden_terminal` off; both
are gone, and with the setting off or for another agent the action says
it cannot be done. `terminal.rs` keeps the resume command, which the
session's menu copies.

A session the person started in a terminal themselves is another matter:
it is where it is, and the next section is how it is reached.

## A session in a terminal of the person's

It is listed, read from its transcript, and sent messages through its
inbox while a turn runs there. Nothing else reaches it: its screen is not
read (so no working row's word, no dialog card, no mode off its footer),
no key is pressed in it (Stop and ⇧Tab say to press it there), and its
app is not brought forward. Between turns the session is taken up on a
hidden terminal like any other. Reaching into terminal apps was done
once, by `kaku cli`, AppleScript and System Events, and was removed: it
served a few terminals on one OS, and the hidden terminal serves all.

`come_back`: if the session was waiting or idle at the hand-off, the
next change to its transcript brings the window back. If the agent was
working, the next change would be its own, so nothing is armed.

## The slash list

`render_slash_help` shows the folder's catalogue over the composer. A
click or ⌘↩ sends the command through the driver on a driven session and
types it into the terminal otherwise (`run_in_terminal`). Name with
argument hint and description are two columns, each cut with an
ellipsis: a long hint ran out of the card at its own width.
`Workbench::slash_key` captures the keys before the textarea (↑ ↓ wrap,
↩ picks, ⇥ inserts the name, Escape hides the list until the text
changes). `slash_sel` returns to the first row on every change, so
typing and ↩ takes the best match, the shorter name first within a rank.

## Probing

- `emaki-core peers` lists the registry; `emaki-core inbox <id> <text>`
  delivers by hand; `emaki-core drive` runs a turn through the driver.
- `emaki-core pty <cwd>` (or `--resume`) runs a hidden terminal from a
  shell, and can stand in for a terminal of the person's.
- `EMAKI_GO=term` opens the terminal panel, `term:shell` and `term:agent`
  go to that side, `term:off` puts it away, `term:start` starts the
  agent's hidden terminal, `term:tab+` makes a shell tab, `term:tab-`
  closes the one showing, `term:tab:<n>` shows the nth from 0, `term:w:<w>` drags the edge to that width, `term:clear` is ⌘K, `term:sel:<unit>,<row>,<col>,<row>,<col>` selects and prints the words,
  (a unit of 4 or more is a block of columns), `term:hover:<row>,<col>` puts the pointer on that cell for a link, `term:copied` shows the toast, `term:text:<size>` sets the letters' size,
  and `termtype:<words>` types them there with Return. Several probe
  copies at once run their steps late: one at a time for a sequence.
- `EMAKI_GO=send:<words>` sends once the session or the new-session page
  is up; it carries no attachment. Also `pill:effort`, `pill:model`,
  `step:mode`, `type:/status`, `button:terminal`, joined with `;`.
- `EMAKI_TERM_KEYS=left,s` presses keys on the card after it shows.
- `EMAKI_PTY_LOG=<file>` keeps the pty child's raw output.
