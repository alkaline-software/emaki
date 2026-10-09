# Probing

How to look at a state of the window from a script, without the person's
hands. Read this before checking a change to the app by eye.

## Rules

- Probe with a second copy, never the running app: `EMAKI_HOME` at a scratch
  directory. The agent's session is a child of the running Emaki (its hidden
  terminal), so quitting that Emaki ends the session.
- Never `pkill` by hand from inside the app. `scripts/relaunch.sh` is the
  only way the running copy is replaced.
- Say what was checked and what was not. A synthetic click does not reach a
  click handler in a background window, so "the click works" is not shown by
  a screenshot of the state the click leads to.

## A second copy

- `EMAKI_HOME=<scratch>/home` gives the copy its own state. It still reads
  the real `~/.claude`, so real sessions can be opened.
- `CLAUDE_CONFIG_DIR=<scratch>/cc` with one hand-written transcript under
  `projects/<mangled-cwd>/<id>.jsonl` makes the copy archive and index only
  that, so a state can be drawn without touching a real session. A copy run
  this way has a Claude Code that is not logged in.
- A copy run with `EMAKI_HOME` leaves Claude Code's settings alone (it would
  otherwise point the status line at the scratch tree).
- What the window remembers is `<home>/state/ui.json`. Edit it before the
  launch to choose a panel (`outline_on`, `files_on`), tabs or the sidebar.

## Launches

```
cargo build -p emaki-app && ./target/debug/Emaki
cargo test -p emaki-core
EMAKI_OPEN=<session-id prefix> ./target/debug/Emaki    # open a session on launch
EMAKI_PAGE=new|sessions ./target/debug/Emaki     # land on a page
EMAKI_FIND=<text> ./target/debug/Emaki                 # open the find bar on that query
EMAKI_SETTINGS=1 ./target/debug/Emaki                  # open the settings panel (=updates: on that section)
EMAKI_TYPE=/co ./target/debug/Emaki                    # put that text in the composer
EMAKI_QUESTION=1 EMAKI_OPEN=<id> ./target/debug/Emaki  # hold a sample question on that session
EMAKI_GO=terminal EMAKI_OPEN=<id> ./target/debug/Emaki  # press "go to the terminal" on it
EMAKI_GO=type:/status EMAKI_OPEN=<id> ./target/debug/Emaki  # type that into its terminal and send
EMAKI_GO=pill:effort EMAKI_OPEN=<id> ./target/debug/Emaki  # click a pill: pill:mode, pill:model, pill:effort; step:mode is ⇧Tab
EMAKI_GO="step:mode;button:terminal" EMAKI_OPEN=<id> ./target/debug/Emaki  # several steps, five seconds apart; button:terminal is the top-right button
EMAKI_GO=effort:high EMAKI_OPEN=<id> ./target/debug/Emaki  # send a value without the picker: effort:, model:
EMAKI_GO="open:<id2>;page:new" EMAKI_OPEN=<id> ./target/debug/Emaki  # go to another session, or to the new-session page
EMAKI_GO=folder:<name> EMAKI_OPEN=<id> ./target/debug/Emaki  # click that folder in the sidebar
EMAKI_GO="sidebar;float" EMAKI_OPEN=<id> ./target/debug/Emaki  # the sidebar's button, then the pointer on it
EMAKI_GO="open:<id2>;tabhints" EMAKI_OPEN=<id> ./target/debug/Emaki  # the tabs' numbers, as holding ⌘ shows them
EMAKI_GO="x;tree:crates;file:README.md" EMAKI_OPEN=<id> ./target/debug/Emaki  # the files panel: open a folder, show a file (file:off closes its pane, file:raw and file:read choose how markdown shows; in a PDF pdf:all selects everything, pdf:sel:<from>,<to> those glyphs, pdf:text prints the selection, pdf:find:<words> finds and pdf:next steps, pdf:pages and pdf:contents are the two buttons, pdf:page:<n> goes to a page; file:menu is a right click in the pane); filemenu:<path> is its right click; files and outline are the two buttons; outline:<n> goes to that round; panelw:<w> drags the panel's edge, panelfit is the double click on it; branches opens the list of branches, branchq:<words> types in its field, branchask:<name> shows the question a switch asks, branchgo:<name>:leave|bring answers it, branchrestore puts left changes back; changes opens the comparison, changes:<path> on that file
EMAKI_GO="page:sessions;sessions:<folder>" EMAKI_OPEN=<id> ./target/debug/Emaki  # the sessions page's folders, then inside one
EMAKI_GO="side:320;sidefit" EMAKI_OPEN=<id> ./target/debug/Emaki  # drag the sidebar's edge to that x; then the double click on it
EMAKI_GO="x;scroll:-3000;x;unread" EMAKI_OPEN=<id> ./target/debug/Emaki  # scroll the conversation by points (less than none is up), then click "New messages"; append rows to a scratch transcript meanwhile to have something new
EMAKI_GO=gitlicence EMAKI_OPEN=<id> ./target/debug/Emaki  # the Files panel's strip as a Mac with the Xcode licence not agreed to has it (the panel must be open: `files_on` in ui.json)
EMAKI_GO=shells EMAKI_OPEN=<id> ./target/debug/Emaki  # open the card of commands running in the background; agents is the subagents' card
EMAKI_GO=menu EMAKI_OPEN=<id> ./target/debug/Emaki  # the session's right-click menu; renaming shows the rename field, name:<words> names it
EMAKI_GO=dialog:2 EMAKI_OPEN=<id> ./target/debug/Emaki  # press that in the terminal's dialog (a digit, or tab); answer:<words> types an answer, goto:<n> goes to that tab
EMAKI_KEYS=down,down,tab EMAKI_TYPE=/mod ./target/debug/Emaki  # press the slash list's keys (up, down, tab, esc)
EMAKI_KEYS=rename,up,caret EMAKI_OPEN=<id> ./target/debug/Emaki  # open the rename field on that session, press keys in it, print where the caret is
EMAKI_KEYS="type:see @cr,down,tab,text" EMAKI_OPEN=<id> ./target/debug/Emaki  # type into that session's composer (no commas), press the list's keys, print what it holds
EMAKI_GO=send:hello EMAKI_OPEN=<id> ./target/debug/Emaki  # send that message once the session is open (or with EMAKI_PAGE=new, start one)
EMAKI_TERM_KEYS=left,s EMAKI_GO=pill:effort EMAKI_OPEN=<id> ./target/debug/Emaki  # press keys on the terminal card (left, right, up, down, enter, esc, tab, or a letter)
EMAKI_SHOT=/tmp/shot.png EMAKI_OPEN=<id> ./target/debug/Emaki  # write a picture of the window there after EMAKI_SHOT_AFTER seconds (5) and quit
```

`EMAKI_GO` steps run five seconds apart, separated by ";". `EMAKI_SHOT`
writes the picture after `EMAKI_SHOT_AFTER` seconds (5) and quits, so set
that past the last step. The picture is taken from inside
(`sys::shoot_window`): a process may capture its own window without Screen
Recording access, which the terminal running the probe often lacks. Crop a
capture with PIL to read a detail.

Debug prints: `EMAKI_TIMING=1` (load and hand-over per open),
`EMAKI_A11Y=1` (gpui's view of the accessibility tree at every draw),
`EMAKI_SCROLL_DEBUG=1` (every wheel event with its phase and owner),
`EMAKI_GLIDE_DEBUG=1` (the outline's move, tick by tick),
`EMAKI_OUTLINE_DEBUG=1`, `EMAKI_PTY_LOG=<file>` (the hidden terminal's raw
output).

## The core from a terminal

`emaki-core` has a subcommand for most of what the window does, and none
needs a window: `list`, `render`, `build`, `archive`, `sync`, `search`,
`bench`, `shells`, `outline` (`--summarize` makes real model calls), `git`,
`files`, `peers`, `inbox`, `options`, `explain`, `update`, `statusline`,
`drive`, `pty`, `screen < text` (what a terminal screen holds).

## What a synthetic event reaches

- A `mouseMoved` `CGEvent` sent with `postToPid` moves gpui's hover without
  moving the real pointer, so a hover state is one event and a capture away.
- Mouse-down and mouse-up sent the same way do not reach a click handler in
  a background window. Check a click's effect through `EMAKI_GO`, or with a
  probe build that changes the state on a timer.
- A synthetic key does not reach a background window either; `EMAKI_KEYS`
  and `EMAKI_TERM_KEYS` dispatch the actions through the focus instead.
- When keys are sent for real, send key codes (System Events `key code`, or
  a `CGEvent` with the right virtual key). A unicode string on virtual key 0
  reaches the composer but not a single-line input.
- A synthetic `CGEvent` scroll carries `CGScrollPhase` values (began 1,
  changed 2, ended 4) and `CGMomentumScrollPhase` (begin 1, continue 2,
  end 3), not the `NSEventPhase` bits gpui reads.
- A terminal without Accessibility access cannot press ⌘F or ⌘, in the
  window, which is why `EMAKI_FIND` and `EMAKI_SETTINGS` exist.

## Environment traps

- `open` hands a newly launched terminal app the opener's environment, so a
  probe copy run with `CLAUDE_CONFIG_DIR` at a scratch tree starts a
  terminal whose Claude Code is not logged in. Have the terminal app running
  first.
- `kaku cli spawn` hands the new pane the caller's environment, so a session
  started from inside a Claude Code session is its child and never
  registers. Start it from a script that unsets `CLAUDE*` first.
- `emaki-core pty --resume <id>` can stand in for the person's terminal.

Each feature's own probing notes are at the end of its doc.
