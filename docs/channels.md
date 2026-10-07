# Channels: how the window reaches a session

Source: `crates/emaki-core/src/driver.rs`, `pty.rs`, `peer`, `terminal.rs`;
`crates/emaki-app/src/hub.rs`, `sys.rs`, and the channel code in
`workbench.rs` (`reply_via_for`, `via_terminal`, `send_message`).

## Rules

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
- Before System Events keys: wait until the host is frontmost
  (`Host::wait_front`), and in an IDE focus its terminal. Activation may
  be granted late, and early keys land in whatever is in front, Emaki's
  composer included.
- The terminal button leaves the hidden terminal running. Letting it go
  made the next ⇧Tab wait for a new one.
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
4. `spawn`: none of those, the session is not kept only, the driver is
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
to a transcript; the terminal button still opens it.

## The hidden terminal

`pty.rs`: the interactive `claude --resume <id>` (`--session-id` for a
new session, in the mode and model chosen) on a pty this process owns
(`portable-pty`), its output fed to a screen model that draws nothing
(`vt100`). The child is an ordinary interactive session: it registers
with an inbox, runs the status line and writes the same transcript, so
the code that follows a terminal follows it unchanged.

Every `sys` function that takes a session's pid (`terminal_text`,
`terminal_styled`, `type_in_terminal`, `key_in_terminal`,
`text_in_terminal`) asks `pty::for_pid` first. There the screen is in
memory (`Pty::styled`, in the form the readers in `driver` take) and a
key is a write: any platform, no app coming forward, no Accessibility
access.

**Start.** `Hub::start_terminal`, from a message sent on `spawn`, from
anything `via_terminal` is asked for, and from the first character typed
in the composer of a `spawn` session (`Workbench::warm_terminal`). It is
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
prompt on the screen. With `driver.hidden_terminal` on it never opens
the person's terminal; with it off it opens theirs as the button does.

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
beneath, else the screen without blank edges), in the terminal's colours
on a ground chosen by Claude Code's `theme`.

- Keys: the card holds the focus (`term_focus`, key context `Terminal`)
  and `term_bytes` turns a key into terminal bytes. Tab and ⇧Tab are
  bound there or the toolkit's focus traversal takes them. Escape is
  Claude Code's whenever the card is up, wherever the keyboard is.
- Pointer: Claude Code takes no mouse, so `pty::hits` maps a click to
  keys. A numbered choice is that many arrows from "❯"; a slider level is
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

## The terminal button

`Workbench::open_in_terminal`. "Your terminal" is the app the system
keeps for shell scripts (`sys::in_default_terminal`). Of the session's
registry records (`peer::registry_all`, ours left out), one in the
default terminal is brought forward and nothing is opened; one in any
other terminal (an IDE's) counts as not open, and the session is opened
in the default terminal beside it. Refused for a session kept only, a
folder gone, a driver mid-reply, and a running turn anywhere; an idle
driver is stopped on the way.

Tried and dropped: a guard after the click (`handed`) under which
nothing hidden was started. A ⇧Tab then ran the terminal's script again
and a message started the headless driver.

`terminal.rs` writes `~/.emaki/run/terminal/<session>.command`: clear
the `CLAUDE*` variables, `cd` to the folder, `exec` the agent's resume
command (`claude --resume <id>`, `codex resume <id>`).
`sys::open_in_terminal` hands it over: macOS `open`, so the system's
choice of app and no setting; Windows a `cmd` window through `start`;
Linux `$TERMINAL`, then the usual emulators.

## Reaching a real terminal app

Used for a terminal of the person's: Stop and a dialog's answer
mid-turn, the button's bring-forward, and everything when
`driver.hidden_terminal` is off.

`sys::focus_terminal` goes from the registry pid up the parent chain to
the first process that is an application (`NSRunningApplication`): the
terminal app, or the IDE holding the terminal. The tab or pane on the
tty is picked where the app can be asked: Terminal and iTerm2 by
AppleScript, WezTerm and Kaku by `cli list` / `activate-pane` keyed on
`tty_name`. Anything else is only activated.

`sys::type_in_terminal`: Terminal `do script`, iTerm2 `write text`,
WezTerm and Kaku `cli send-text --no-paste` with a carriage return. Any
other host gets System Events keystrokes, which need Accessibility
access; refused, the text goes on the clipboard and the row under the
composer says so.

`come_back`: if the session was waiting or idle at the hand-off, the
next change to its transcript brings the window back. If the agent was
working, the next change would be its own, so nothing is armed.

**IDE terminals.** A VS Code based IDE (any host with
`Contents/Resources/app/product.json`) takes keys wherever its focus was
left, and bringing the app forward does not move it: `/compact` landed
in a file. `Host::focus_ide_terminal` asks the command palette for
"Terminal: Focus Terminal" (⌘⇧P, the words, ↩), which is harmless from
the panel itself, where the ⌃` toggle would close it. It focuses the
IDE's active terminal, which may not be this session's: nothing outside
the IDE can pick one by its tty.

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
- `EMAKI_GO=send:<words>` sends once the session or the new-session page
  is up; it carries no attachment. Also `pill:effort`, `pill:model`,
  `step:mode`, `type:/status`, `button:terminal`, joined with `;`.
- `EMAKI_TERM_KEYS=left,s` presses keys on the card after it shows.
- `EMAKI_PTY_LOG=<file>` keeps the pty child's raw output.
