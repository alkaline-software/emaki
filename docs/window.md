# The window's chrome

About `crates/emaki-app/src/workbench.rs`, `main.rs` and `ui_state.rs`: the
strip buttons, the sidebar, the sessions page, the layout, the
settings panel, menus and rename, tabs, focus, scroll routing, what the
window remembers.

## Rules

- Every click the window swallows goes through `swallow_click`. The
  toolkit's selection layer begins a text drag on every left mouse-down and
  ends it on the bubble-phase mouse-up; a handler that stops propagation
  eats that mouse-up and every later mouse move extends a selection.
- A hover style that changes the text's colour is set whatever state the
  element is in, never under a `when`. gpui works the text's colour out
  at layout from a flag it keeps per element, and that flag hears the
  pointer leave only while a hover style is set. Taken off an active
  segment, the flag stayed "over", and the segment came back to rest
  lit with the pointer elsewhere. A hover that changes a ground or a
  border is painted from where the pointer is and has no such trap.
- Every input the window owns sits in a `div` that `track_focus`es the
  `InputState`'s handle and carries the text role. The toolkit's input
  frame tracks a handle of its own, so focusing a state from code lands on a
  handle with no dispatch node: no key bindings, nothing for assistive apps.
- The focused element must be one the page draws. gpui dispatches a key
  from the focused node, or from the window root when that node is not in
  the frame, and the root is above every handler in the workbench, so every
  shortcut goes nowhere.
- ⌘W is one global `CloseTab` binding. A context-bound one loses to a
  global binding whenever the focus is in the composer. A part of the
  window that wants ⌘W for itself answers `CloseTab` on its own focused
  element and passes it on when it has nothing to close, as the terminal
  panel does; a second ⌘W binding in its context is never reached.
- Keep `on_reopen` in `main.rs`. The app stays running with no window, and
  without the handler a Dock click does nothing.
- A new scroller takes the toolkit's `vertical_scrollbar` and nothing else,
  so every scrollbar in the window fades with the same timing.
- A scroller that can sit beside or over another is a pane of its own to
  `route_scroll` (`Pane`). Otherwise momentum from a flick scrolls whatever
  the pointer is carried over.
- Rows inside a scrolling flex column go in a column of their own and never
  shrink. As direct children each gave up height once the list was longer
  than the sidebar, and the whole list changed its spacing at a click.
- A tab and the two strip buttons take their press before `drag_region`
  does (`press_taken`), or a press on them moves the window.
- Read `sidebar_w`, not the constant `SIDEBAR_W`, which is only the width
  before any drag.
- A rename is Claude Code's to make (`/rename` in the hidden terminal).
  Nothing here writes to a transcript, and a name kept only in our own file
  leaves the terminal's resume list on the old title.
- Never type a rename into a running turn; it waits in `renames`.
- The settings panel's body is a flex column inside a flex column. A block
  wrapper around it collapsed the panel to its header.
- A round in the conversation list is wrapped in an explicit centring
  parent. The list lays each item out alone, so an auto margin has nothing
  to push against.
- The lightbox backdrop is `occlude`d so nothing beneath it hears the mouse.

## The two buttons beside the traffic lights

The sidebar button and search (`Workbench::render_strip`) are drawn once
over the window's top left corner, not inside the sidebar or the top strip,
so neither moves whether the sidebar is there or not. The sidebar's header
is only room for them, and the top strip leaves room when the sidebar is
away (`strip_right`). The strip is set so the buttons' centres are on the
traffic lights'.

With the sidebar folded away, the pointer on its button floats the sidebar
in over the content (`set_float`, `render_sidebar_float`, no scrim). It goes
when the pointer leaves it: `float_follow` is a raw mouse-move listener that
asks where the pointer is, not what is hovered. A click on the button while
it floats keeps it. The click that folds the sidebar leaves the pointer on
the button, which is not a hover: `float_block` holds until the pointer has
left the button once.

Tried and dropped: back and forward arrows after search. The tabs are the
way between sessions.

## The sidebar

A name, three places and two cards (`render_sidebar`).

- The wordmark (the app's icon, served as `icon/app.png`, and "Emaki") is in
  the sidebar's own top strip, right of the two buttons, in
  `fonts::wordmark_family` and the accent (`theme.primary`), so it follows
  Settings. Optima has only regular and bold, so the regular is drawn twice
  half a pixel apart for a weight between. Tried and dropped: serif faces, a
  bold sans, a fixed gold.
- The hairline at the strip's foot meets the hairline over the folder's band
  in the content pane, so one line crosses the window. Tried and dropped: a
  row of its own for the wordmark, which put the two lines at different
  heights.
- New session, All Projects. The All Projects entry is still ⌘L and
  `GoSessions`; its icon is a briefcase so it does not read as a folder row.
- Two cards (`card`), Agents and Projects, each under a name that does not
  scroll. The agents' card shows `SIDE_AGENTS` rows and scrolls for the rest
  (`agents_scroll`); the projects' takes the height left (`side_scroll`).
  They are `Pane::Agents` and `Pane::Sidebar` to `route_scroll`.
- The footer is the person's name and a settings gear, nothing else.

The Projects card lists the `SIDE_FOLDERS` newest projects, then "N more". A
click opens a folder onto its sessions and closes it again (`folders_open`,
kept in `ui.json`): `FOLDER_ROWS` of them, headed by when (`format::bucket`,
as on the sessions page), then "N more".

A folder with a live session is open without being asked (`sync_folders`):
it opens when one of its sessions goes live and closes when the last stops,
if it was opened that way and not clicked since. A click is the person's
choice and stays until the folder next goes live or quiet.

A session's mark is in the muted ink unless it is live, so many rows do not
read as many accents, with a dot in its phase's colour. A folder
wears the most pressing of its sessions' (needs you, then working, then
your turn).

A folder's sessions unfold and fold over `FOLDER_ANIM`: a box whose height
is the sum of its fixed-height rows (`SIDE_ROW_H`, `SIDE_SESSION_H`), which
`folder_anim` runs up or down with the opacity. A closing folder is still
drawn until the time is up.

Narrower than `NARROW_W` the sidebar leaves the row and comes back only as
an overlay over a scrim (the strip button, ⌘⇧S), which any click or Escape
puts away. `sidebar_open` keeps the preference for when the window is wide
again.

### The sidebar's edge

A strip over the edge (`render_side_grip`, only while the sidebar is beside
the content) takes a press (`side_drag`); from then a raw mouse-move
listener in the capture phase sets the width to the pointer
(`side_drag_to`) and stops the event, so nothing under the pointer hovers or
selects. The width (`sidebar_w`, in `ui.json`) stays between `SIDEBAR_MIN`,
the least that holds the two buttons and the wordmark, and `SIDEBAR_MAX`.
Left of `SIDEBAR_FOLD_AT` the sidebar folds, and comes back in the same
drag if the pointer does; the width it had is kept.

A double click goes to the fitted width (`sidebar_fit`): the widest row the
Projects card lists, measured with the text system, capped at
`SIDEBAR_FIT_MAX`. A row that needs more is a prompt standing in for a
title and is cut short at any width; without the cap one long title always
asked for the maximum.

## The sessions page

Two levels, as the sidebar has. A project is a folder; the window says
"project" and keeps the folder icon. The top level (`sessions_folder` none)
is a row per folder with a chip for the most pressing of its live sessions.
A click goes inside (`show_sessions_in`), where sessions are headed by when
under an "All Projects › folder" line that goes back up. The pills (All, one per
agent) narrow either level and stay as the level changes; inside
a folder they count that folder's own. An archived session, one its agent no
longer has, is listed with that agent's and wears an "archived" tag. It
has no category of its own: a row a kind of session beside a row an agent
read as a second sort of agent.

"N more" under a folder in the sidebar goes inside that folder; "N more" at
the foot of the folders and the All Projects entry go to the top; an agent's
row goes to the top narrowed to that agent. A right click is Open in Finder
on a folder and the session's menu on a session.

## Motion

Nothing in the window appears or disappears from one frame to the next.
The patterns, each in use and to be used again:

- **A menu or a list over the window** (`render_menu`, `render_pick`):
  it comes in over `MENU_IN`, rising a few pixels into place as it
  fades in, and goes out over `MENU_OUT`, fading where it was. What was
  closed is kept for that moment (`menu_gone`, `pick_gone`) and drawn
  once more with no click taken. Each opening has a serial in its
  animation's name, so it plays once.
- **A card in the column over the composer** (`render_terminal`,
  `render_dialog`): held by its foot, its height opens up from the
  composer and closes back to it, with the gap under it part of what
  moves, so the conversation above slides and does not jump. What it
  last showed is kept (`TermShown`, `DialogShown`) to draw it closing. A
  card whose height is its content's measures itself as it is drawn
  (`dialog_h`) and takes no room until that is known.
- **A panel at the conversation's side**: its width opens and closes
  (`docs/panels.md`, `docs/channels.md`).
- **A choice that moves** (a segmented control, the strip's buttons): one
  plate slides from the old to the new.

The toolkit's own popover has no way out but at once, which is why a
pill's list is ours (`PickMenu`) and not a `Popover`.

A measuring `canvas` laid over an element is `.absolute().inset_0()`.
With `.size_full()` alone it stands after the element in the flow, and
reports a place one element lower.

## A session's phase

There was a board, a page of live sessions in four columns; it was removed
in v0.1.8 as of little use. What it sorted by is still what the sidebar's
dots, the status row and the working mark go by (`Column`, `card_for`):
needs you, planning, working, your turn, done.

The phase is `build::turn_state`, a function of the transcript's tail:
`stop_reason: end_turn` is your turn, a trailing `tool_use` is working, a
trailing `AskUserQuestion` or `ExitPlanMode` needs you, and the latest
permission mode says whether working is planning. `peek` computes it for the
index from the same tail slice it reads the title from, cached on (size,
mtime), so a rescan of every transcript costs nothing.

## Layout

Laid out like the Claude desktop app.

- Everything readable sits in one column of `CONTENT_W`, centred in what is
  left of the window: the conversation, the sessions page, the home page.
- The content pane has a top strip with actions on the right. When the
  sidebar is hidden the strip makes room for the traffic lights.
- The home page is a greeting in `fonts::display_family` (the Claude app's
  serif when present, so titles and the conversation agree) behind the app's
  icon, over the composer, with the most recent folders as a grid of cards
  (`FOLDER_COLS` to a row). Tried and dropped: full-path pills of every
  width.
- Each page's content fades in when it arrives (`page_in`, keyed on the
  page). The conversation's list items are never animated.
- Floating cards (composer, search palette, settings) lift with
  `float_shadow`, a drop in the ink's own hue, not a grey halo.
- The composer's textarea grows with its content, which also keeps the caret
  laid out, so the caret rectangle macOS asks for (dictation, the
  input-source badge) is real.
- Replies stop `REPLY_INSET` short of the prompts' right edge so the two
  voices sit at different widths.
- The palette is `crates/emaki-app/themes/emaki.json`, one config per
  appearance, loaded by `install_theme` in `main.rs`. `assets.rs` wraps the
  toolkit's icon set and serves our own files; nothing draws `mark.svg` now.

## The settings panel

A fixed sheet (`SETTINGS_W` by `SETTINGS_H`, capped by the window) with a
rail of sections on the left (`SETTINGS_SECTIONS`; `settings_section` is the
one showing) and that section's rows on the right, fading in as the section
changes. The rows scroll under the header (`settings_scroll`).

Choices are segmented controls (`Workbench::segmented`): a muted track, the
choice on a raised plate. The plate is one element under the row and slides
from the old choice to the new. Each segment's bounds are recorded as the
row is prepainted (`on_children_prepainted` into `seg_bounds`, relative to
the track), the control remembers the choice it came from (`seg_state`), and
the animation is keyed on the new choice so a click plays it once. On the
first draw, before any bounds exist, the chosen segment paints its own plate
so nothing flashes.

The slide belongs to the change: `seg_state` keeps when the choice changed,
and a control drawn later than the slide's length has no "from". Without
that, a control drawn anew (the panel opened again) was a new element
whose animation started over, and the plate slid once more from a choice
made long before. `segmented_sized` is the same control in a small size,
used for the branch list's Recent and Name.

The scrollbar's fade lives in the vendored `scrollbar.rs` (`FADE_OUT_DELAY`,
`FADE_OUT_DURATION`). The fade curve must fit its length: upstream's curve,
made for a longer fade, held the bar at full strength and then cut it.

## Menus and rename

A right click opens a menu of our own. `Workbench::open_menu` puts a small
card at the pointer (`render_menu`, kept inside the window, over a clear
sheet any click or Escape puts away) and `menu_pick` does what was chosen
(`MenuDo`). The toolkit's action-based context menu was more than a few
choices needed. Every menu is made the same way, and a new one follows it:
each choice has an icon in front of its words (`MenuDo::icon`, by what
the choice does), and choices of one kind stand together with a line
between the groups (`MenuDo::Rule`): what opens the thing, what it gives
the clipboard or the message, what changes it. The card is as wide as its
longest choice and no wider, the words measured before it is placed, so
the room at its right is the room at its left. It settles
in from just above (`MENU_IN`, keyed on the menu's serial so each opening
plays once) and, put away, fades where it was (`close_menu`, `menu_gone`,
`MENU_OUT`), taking no click meanwhile.

- A folder, in the sidebar or on the new-session page's cards, offers "Open
  in Finder" (`sys::OPEN_FOLDER_LABEL`). The sidebar's folder is a project,
  so its path is the newest session's `cwd` that is still a directory; with
  none, the menu says the folder is gone.
- A session offers one menu wherever it is right-clicked, in the sidebar,
  on the sessions page and on its tab (`session_menu`): Rename; Copy
  Session ID and Copy Resume Command (the agent's own, after a `cd` to
  its folder); Open in Terminal, which shows the session in the terminal
  panel; Open Folder and Reveal Transcript in
  the file manager; and, when it has a tab, Close Tab and Close Other
  Tabs. `MenuDo::Rule` is a line between groups.
- The conversation offers Copy for what is selected in it
  (`conversation_menu`). A right click on a word selects that word first:
  the toolkit's text view does word selection itself (`inline.rs`, its
  double click), not the window's selection layer, whose participants
  hold no text for a rendered document, so the right click is handled
  there too (`UPSTREAM.md`). The menu is opened a moment after the press
  (`window.defer`), once the word is selected. A right click on no text
  with nothing selected opens nothing.
- An attachment, as a tile in the conversation, a picture a tool read, or
  a chip in the composer, offers Copy Image for a picture
  (`attachment_menu`, `copy_image`: the file's bytes in its own format, or
  the bytes read back out of the transcript) and, when the file is on
  disk, Copy Path and Reveal. Its handler stops the press, so the
  conversation's own menu does not open over it.
- A text input (the composer, search, find, rename, the name, the
  branch field) offers Cut and Copy with a selection, Paste with
  something on the clipboard, and Select All (`input_menu`, `EditDo`,
  dispatched to the input's own focus). The toolkit would show the
  system's menu there; the vendored `on_secondary_click` hands the click
  over instead. A menu over text has no icons, none of its choices, so
  its words still stand in one column. On a marked word of the composer
  the menu is that word's corrections and nothing else
  (`docs/composer.md`). The input calls the handler from inside its own
  update, so the menu is made a moment later (`window.defer`): reading
  the input there aborts the app.
- A branch, on the pill in the files' head and on a row of the list,
  offers Copy Branch Name. The row stops the press, which would otherwise
  reach the list's sheet and put the list away.

Rename opens a field over a scrim (`render_rename`, `rename_input`): ↩ keeps
the name, Escape or a click outside leaves it, an empty field changes
nothing. `try_renames` types `/rename <name>` into the session's hidden
terminal, starting one when the session has none. Claude Code writes a
`custom-title` row, which its resume list reads and
`transcript::pick_title` puts before any AI title. It writes the same words
as an `agent-name` row, which must not disqualify them.

Meanwhile, and for a session that cannot be asked (another agent, one kept
only, a folder that is gone), the name is ours:
`~/.emaki/state/titles.json` by session key, laid over `SessionRef::title`
as each index arrives (`HubEvent::Index`) and dropped once the transcript
says the same. Whatever is still in that file at launch is asked for again.

## Scroll routing

macOS goes on sending wheel events after the finger lifts, addressed to
wherever the pointer is by then, and gpui hands each to the scroll container
under it. Its `touch_phase` tells `Started` and `Ended`, but a momentum
event is just `Moved`. So a flick in one pane followed by a move to another
scrolled the second.

`Workbench::route_scroll` runs in the capture phase from a raw listener a
zero-size `canvas` registers at paint, before any container. The pane under
the pointer at `Started`, or at the first event after `SCROLL_GAP` (a mouse
wheel sends no phases), owns the gesture. An event that lands in another
pane is applied to the owner's scroll position (`ListState::scroll_by` for
the conversation, a `ScrollHandle` elsewhere) and stopped. With a single
pane there is nothing to do.

## The top strip of a session

One control at the top right, of two segments: the shell and the agent's
terminal, which open the panel at the conversation's right
(`term_panel.rs`, `docs/channels.md`). The project
folder and the transcript are on the session's right-click menu, and the
folder is also the path line under the strip
(`Workbench::open_project_folder`); their two buttons stood here until
the menu had them. The agent is named by its mark on the tab and
its name over every reply, not by a badge here.

Under the tabs is a band with the session's folder and nothing else
(`path_line` in `render_detail`): the absolute path, parents dimmed, and
the parents are what truncates. A click opens the folder. Tried and
dropped: a line with branch, rounds, tool calls, tokens, model and id, too
much to read; and the path on a small rounded plate, which under the active
tab read as two tabs stacked.

## What the window remembers

`ui_state.rs` keeps `~/.emaki/state/ui.json`: bounds, the sidebar and its
width, the page, the open tabs in their order and the active one, the open
folders, the panels. It is written when any of that changes (bounds at most
every two seconds) and at quit. The bounds are reused only when their centre
is still on a screen.

The page is saved but not restored: the window opens on the new-session
page, and the tabs come back. `EMAKI_PAGE` and `EMAKI_OPEN` pick something
else.

Scroll positions are not saved (an old `scroll` key is ignored): every open
lands at the end of the conversation, which the list's bottom alignment does
by itself. The list is in gpui's `FollowMode::Tail`, so it stays at the end
while a reply streams in. Following pauses when the reader scrolls up, when
a find hit is scrolled to, or when `pin_scroll` gives the list a real top so
an opened card extends downward, and resumes once the view is back at the
bottom. Without it, the list stayed anchored to the prompt while the reply
grew out of sight.

## Tabs

Tabs are keys in `Workbench::tabs`. A switch is a browser's: the `Detail`
left is put away whole in `stashed` (the session, the list with its scroll,
what was unfolded), the one arrived at is taken out and drawn in the same
frame, then read again from disk and brought up to date in place. Only a tab
never shown since launch waits for its read, and the pane is empty meanwhile
under tabs that stay put. A closed tab's `Detail` is let go. Tried and
dropped: one `Detail`, dropped and reread at every switch, which blinked the
pane and its tabs.

Every tab has the same width (`sync_tab_widths`, at every draw): `TAB_MAX`
while the row has room, the row divided among them once it has not, down to
`TAB_MIN`. A new tab grows in from nothing while the others give way, and a
closed one's room is taken up the same way, over `TAB_ANIM` (`tab_widths`).
A change in the row's own width is followed at once. The row's width is
measured as it is painted (`tabs_row_w`, a `canvas` over it) and used at the
next draw. Tried and dropped: tabs as wide as their titles with the flex row
shrinking them, which drew a new tab at full width for a frame.

A tab is dragged to another place (`on_drag` with a `DragTab`, the row's
`on_drag_move` to `drag_tab_to`, which takes the place under the pointer as
it moves). Its title follows the pointer on a plate (`TabGhost`) and its own
tab is dimmed. The others change places at once.

⌘1 to ⌘9 (Ctrl elsewhere) go to that tab, or to the last one when there are
fewer (`go_tab`). ⌘ held by itself for `TAB_HINT_HOLD` shows the digits:
each tab wears an outline and its number on a small plate on its lower edge.
They go when the key is let go or another modifier joins it
(`modifiers_changed`, from `on_modifiers_changed`). The timer from the press
is answered only if that press is still the one held, so a quick ⌘C never
shows them. A release the window never hears (⌘Tab away) is caught at the
next draw, which asks the window what is held. The last tab says 9 once
there are more than eight, and one between has no number. The tab clips what
it holds, so the plate is beside it in a box of the tab's size, and the
outline is always there, clear until then, so nothing shifts.

## The drag region and ⌘W

The window is opened with `app_owns_titlebar_drag`, and
`Workbench::drag_region` wraps the top strip and the sidebar's header. A
press there becomes `start_window_move` once the pointer moves with the
button down; a double click is `titlebar_double_click`. Left to AppKit, the
whole strip under the transparent title bar moved the window, tabs included,
so a tab could not be dragged. `press_taken` works because in gpui's bubble
phase a child's listener runs before its parent's. This is how Zed's own
title bar does it.

`CloseTab` closes the showing tab, then the last tab (which lands on the
new-session page with the caret in its composer), then the window. `main.rs`
answers a `CloseTab` no view claimed by closing the window. `on_reopen` (a
Dock click, a second launch) opens the window again.

Clicking a session puts the caret in the composer. On the sessions page,
which draws no composer, `Workbench::render` moves the focus
to the workbench's own handle whenever the composer still holds it.

## Probing

- `EMAKI_SETTINGS=<section>` opens the settings panel on that section.
- `EMAKI_GO=folder:<name>` is the click on a sidebar folder. `side:<x>`
  drags the sidebar's edge to that x and lets go; `sidefit` is the double
  click on it.
- `EMAKI_GO=menu` opens the session's menu, `renaming` the rename field,
  `name:<words>` names it.
- `EMAKI_GO=tabhints` shows the tabs' numbers with no key held.
  `EMAKI_GO="open:<id>;open:<id2>"` opens tabs in turn.
- `EMAKI_SCROLL_DEBUG=1` prints every wheel event with its phase and owner.
- A `mouseMoved` `CGEvent` posted to the pid moves gpui's hover, which is
  how the sidebar's float is driven. A synthetic click or press posted to
  the pid does not reach a click handler in a background window. The
  segmented plate's slide was checked with a probe build that changed the
  choice on a timer.
- Send real key codes (System Events `key code`, or a `CGEvent` with the
  right virtual key). A unicode string on virtual key 0 reaches the composer
  but not a single-line input.
- A synthetic `CGEvent` scroll carries `CGScrollPhase` values (began 1,
  changed 2, ended 4) and `CGMomentumScrollPhase` (begin 1, continue 2,
  end 3), not the `NSEventPhase` bits gpui reads.
- A probe copy's Claude Code is not logged in, so a rename cannot be run end
  to end from one.
