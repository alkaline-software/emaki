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
- `EMAKI_GO` step `press:<keystroke>` (`press:shift-up`) presses a key as
  the keyboard does, through every listener and binding on the way. The
  `EMAKI_KEYS` names send an action to the focus instead, which finds
  nothing wrong when a key never becomes its action. `treecaret` prints
  the selection in the tree's name field.
- `EMAKI_GO` step `attach:<path>` puts that file in the composer, as a
  drop does.
- `EMAKI_GO` step `size:<small|medium|large|custom>` picks Text Size's
  preset; `textsize:<place>:<points>` sets one place's size
  (conversation, composer, preview, editor, terminal).
- `EMAKI_GO` step `pointer:<x>:<y>` moves the pointer there, in points
  from the window's top left, through the window as the mouse does, so a
  hover and its tooltip can be drawn; `wheel:<x>:<y>:<dy>` scrolls that
  many points there; `clickat:<x>:<y>` presses and lets
  go there as well.
- A new home copies the whole archive (over a gigabyte here) and reads
  every session before the ones after the newest can be opened. Keep one
  scratch home for a run of probes and write its `ui.json` again before
  each, and remove it when done.
- A probe variable whose value holds a space is not handed to the copy
  (`scripts/dev-app.sh` passes only those without one), so every step
  after it is lost without a word: `say:hello`, not `say:hello there`.
- `EMAKI_GO` step `termstate` prints whether the terminal card is up,
  whether by itself, what the registry says and the top of the hidden
  terminal's screen. In the comparison of changes, `changeclick:<path>`
  is a click on a file (`changeclick:cmd:<path>`, `changeclick:shift:<path>`
  with that key), `changeband:<y>:<y2>` a rectangle dragged down the list
  and `changeband` its release, `changeoff` a click on the list's empty
  room, `changemenu:<path>` a right click, and
  `changesel` prints the files chosen and the one shown.
- In the sidebar, `smark:<id>` is ⌘ and a click on a session and
  `smarkto:<id>` ⇧ and one (the id's start; a session the sidebar does
  not draw is no end of a range), `sidemenu:<project>` and
  `sidemenu:session:<id>` are right clicks, `sideask:archive` and
  `sideask:delete` ask it of what is chosen (nothing answers), and
  `sidesel` prints what is chosen. `sideband:<y>` counts from the first
  project's row, not from the card's heading.
- The Trash is drawn from `<home>/state/trash.json`: write
  `{"claude-code:<id>": {"at": <unix seconds>}}` before the launch to
  have sessions in it without deleting any (the question is not answered
  from a script). An `at` thirty days back is removed for good by the
  copy at launch. A copy without `CODEX_HOME` reads the real Codex
  sessions: point it at an empty folder when anything is deleted.
- A folder Claude Code asks about trusting is a new git repository
  beside others (not one under `/tmp`, nor an empty folder). Nothing
  answers the question from a script.
- A copy run with `EMAKI_GO` keeps its window over every other app's
  (`sys::keep_on_top`): under the window in use it draws nothing, opens
  no session and runs no step, and the system no longer lets a copy
  step in front of the app in use.
- A new home has the outline on, and an outline over a real session asks
  a model for its labels, on the person's account. Before a copy opens a
  real session, write `{"outline_on": false}` to `<home>/state/ui.json`,
  unless the outline is what is being looked at.

## Launches

```
cargo build -p emaki-app && ./target/debug/Emaki
cargo test -p emaki-core
EMAKI_OPEN=<session-id prefix> ./target/debug/Emaki    # open a session on launch
EMAKI_PAGE=new|sessions|agents ./target/debug/Emaki     # land on a page
EMAKI_FIND=<text> ./target/debug/Emaki                 # open the find bar on that query
EMAKI_SETTINGS=1 ./target/debug/Emaki                  # open the settings window (=updates: on that section)
EMAKI_GO=x EMAKI_SETTINGS_SHOTS=<folder> ./target/debug/Emaki  # every section of the settings window, light then dark, a picture each in that folder, then quit
EMAKI_GO="x;unarchive:<id>" EMAKI_OPEN=<id> ./target/debug/Emaki  # Unarchive Session from that session's menu, as the sidebar does it (it asks nothing; Archive and Delete ask, and no step answers)
EMAKI_GO="x;lightbox:<picture>" EMAKI_OPEN=<id> ./target/debug/Emaki  # that picture shown large, as a click on an attachment shows it; press:cmd-k opens the search palette
EMAKI_TYPE=/co ./target/debug/Emaki                    # put that text in the composer
EMAKI_QUESTION=1 EMAKI_OPEN=<id> ./target/debug/Emaki  # hold a sample question on that session
EMAKI_GO=terminal EMAKI_OPEN=<id> ./target/debug/Emaki  # press "go to the terminal" on it
EMAKI_GO=type:/status EMAKI_OPEN=<id> ./target/debug/Emaki  # type that into its terminal and send
EMAKI_GO=pill:effort EMAKI_OPEN=<id> ./target/debug/Emaki  # click a pill: pill:mode, pill:model, pill:effort; step:mode is ⇧Tab
EMAKI_GO="step:mode;button:terminal" EMAKI_OPEN=<id> ./target/debug/Emaki  # several steps, five seconds apart; button:terminal is the top-right button
EMAKI_GO=effort:high EMAKI_OPEN=<id> ./target/debug/Emaki  # send a value without the picker: effort:, model:, mode:
EMAKI_GO="page:new;newfolder:<path>;newagent:codex;say:<words>" EMAKI_OPEN=<id> ./target/debug/Emaki  # a new Codex session: the folder, the agent, a message (a real model call); allow, deny and stop answer the oldest card and stop the turn; pick:mode and pick:agent open those lists
EMAKI_GO="open:<id2>;page:new" EMAKI_OPEN=<id> ./target/debug/Emaki  # go to another session, or to the new-session page
EMAKI_GO="page:new;pick:folder" EMAKI_OPEN=<id> ./target/debug/Emaki  # the new-session page's list of folders (steps wait for a session to be open, so name one)
EMAKI_GO=folder:<name> EMAKI_OPEN=<id> ./target/debug/Emaki  # click that folder in the sidebar; mark:<name> is ⌘ and a click on it and markto:<name> ⇧ and one, projects:open, projects:hide and projects:show what the menu of those chosen does, hidden a click on the Hidden heading and temporary one on Temporary, and sideband:<y>:<y2> a rectangle dragged down the Projects card; project:<name> is a click on a project on the sessions page; manage is its Select button, pickall:<project> chooses a project's sessions, manage:archive and manage:delete ask (and nothing answers)
EMAKI_GO="sidebar;float" EMAKI_OPEN=<id> ./target/debug/Emaki  # the sidebar's button, then the pointer on it
EMAKI_GO="pin:<project>;pin:<other>;pindrag:<other>:<project>" EMAKI_OPEN=<id> ./target/debug/Emaki  # pin two projects (pin: again unpins), then drag the second onto the first
EMAKI_GO="open:<id2>;tabhints" EMAKI_OPEN=<id> ./target/debug/Emaki  # the tabs' numbers, as holding ⌘ shows them
EMAKI_GO="x;tabdrag:<id2>:0;pilldrag:<folder>:500" EMAKI_OPEN=<id> ./target/debug/Emaki  # drag a tab along its pill, or a folder's pill along the row, to an x from the row's left edge
EMAKI_GO="x;tree:crates;file:README.md" EMAKI_OPEN=<id> ./target/debug/Emaki  # the files panel: open a folder, show a file (file:off closes its pane, file:raw and file:read choose how markdown or a table shows; file:fold:<line> folds the section that line heads, file:type:<words> puts words at the start of the editor and file:save saves it; file:peek:<n> opens the nth change against git on its card and file:revert reverts it; with changes not saved, file:off or quit brings the question up and file:ask:save, file:ask:discard or file:ask:cancel answers it; in a PDF pdf:all selects everything, pdf:sel:<from>,<to> those glyphs, pdf:text prints the selection, pdf:find:<words> finds and pdf:next steps, pdf:pages and pdf:contents are the two buttons, pdf:page:<n> goes to a page; in a Word document word:type:<words> types, word:compose:<words> is an input method still composing, word:run:<id> runs one of the engine's commands, word:style:<name> gives the paragraph a style, word:all selects everything and word:style opens the list of styles; file:menu is a right click in the pane; file:spell prints the marks on a file of prose, file:spellmenu opens the first mark's menu and file:spellfix takes its first correction); filemenu:<path> is its right click; files and outline are the two buttons; outline:<n> goes to that round; panelw:<w> drags the panel's edge, panelfit is the double click on it; branches opens the list of branches, branchq:<words> types in its field, branchask:<name> shows the question a switch asks, branchgo:<name>:leave|bring answers it, branchrestore puts left changes back; branchmenu:<name> is a right click on a branch, branchdel:<name> the question Delete asks (branchdel:<name>:remote with its tick on) and branchdelgo its Delete; changes opens the comparison, changes:<path> on that file; in a folder of checkouts repolist opens the list of them and repo:<name> chooses one
EMAKI_GO="page:sessions;sessions:<folder>" EMAKI_OPEN=<id> ./target/debug/Emaki  # the sessions page, then a project opened on it and the page brought to it
EMAKI_GO="side:320;sidefit" EMAKI_OPEN=<id> ./target/debug/Emaki  # drag the sidebar's edge to that x; then the double click on it
EMAKI_GO="x;scroll:-3000;x;unread" EMAKI_OPEN=<id> ./target/debug/Emaki  # scroll the conversation by points (less than none is up), then click "New messages"; append rows to a scratch transcript meanwhile to have something new
EMAKI_GO="x;term:shell;term:w:300;winw:800" EMAKI_OPEN=<id> ./target/debug/Emaki  # term:w:<w> drags the terminal panel's edge to that width; winw makes the window that wide, as a drag of its edge would: here with the files and a terminal both showing, to see which is put away
EMAKI_GO="x;term:shell;termfocus;panes;winw:970;panes" EMAKI_OPEN=<id> ./target/debug/Emaki  # panes prints the window's width, whether the sidebar, the panel at the left and the terminal panel show, and where the keyboard is; termfocus puts the keyboard in the terminal, as a click on it does
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

`EMAKI_KEYS=send` is ⌘↩, `blur` takes the focus off everything, and
`astray` puts it on an element no frame holds, as a thing closed leaves
it (`EMAKI_FOCUS_DEBUG=1` prints where it is at every draw, and whether
the native keyboard is at the window's own view; `EMAKI_GO=web:keys`
gives it to the page in the web view as a click there does, and
`EMAKI_KEYS_STAY=1` stops the window taking it back): `EMAKI_KEYS=tabs,blur,closetab,tabs`
shows what ⌘W does then.

`EMAKI_GO` steps `treeall` (⌘A in the files tree) and `treeband:<y>:<y2>`
(a rectangle dragged down it, in points from the tree's top); `file:menu`
with the editor showing is a right click on its first line's word.

`EMAKI_NO_AGENTS=1` shows the new-session page as a machine with no agent
on it has it.

Debug prints: `EMAKI_TIMING=1` (load and hand-over per open),
`EMAKI_A11Y=1` (gpui's view of the accessibility tree at every draw),
`EMAKI_SCROLL_DEBUG=1` (every wheel event with its phase and owner),
`EMAKI_GLIDE_DEBUG=1` (the outline's move and the move to the new
messages, tick by tick),
`EMAKI_OUTLINE_DEBUG=1`, `EMAKI_PTY_LOG=<file>` (the hidden terminal's raw
output).

A variable that steers one launch (`EMAKI_PAGE`, `EMAKI_GO`, `EMAKI_SHOT`
and the rest above) is not handed to the agent's child
(`driver::child_env`). Set on a relaunch, one used to reach the session's
shell and every relaunch from there, so the window opened on the same
page from then on.

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
