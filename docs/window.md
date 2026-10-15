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
- A sheet on a scrim that closes at a click occludes (`.occlude()` on
  the sheet, not only on the scrim). gpui counts a click where the press
  and the release are both in an element's bounds and the element is
  under the pointer both times, and a scrim's bounds take its sheet in:
  a press in the sheet let go outside it, or the other way, closed the
  sheet, as a selection dragged past a field's end does. Occluding, the
  sheet keeps the scrim from being under the pointer while the pointer
  is over the sheet. Settings, rename, search, the comparison, the
  branch sheets, the sign-in sheet, the lightbox and the floating
  sidebar all do.
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
- What a caret key does in a field is the toolkit's, in
  `vendor/gpui-component/crates/base/src/input/base`, and so the same in
  every field there is and will be: in one line, up goes to the start
  and down to the end, and ⇧ with either selects to there. A key that
  does nothing in a field is looked for there, in the bindings and the
  `on_action` list (which was missing the selecting pair for one line),
  not answered in the field's own handler.
- The focused element must be one the page draws. gpui dispatches a key
  from the focused node, or from the window root when that node is not in
  the frame, and the root is above every handler in the workbench, so every
  shortcut goes nowhere.
- ⌘W is one global `CloseTab` binding. A context-bound one loses to a
  global binding whenever the focus is in the composer. A part of the
  window that wants ⌘W for itself answers `CloseTab` on its own focused
  element, as the terminal panel does; a second ⌘W binding in its
  context is never reached.
- ⌘W closes the nearest thing, in this order: with the keyboard in a
  shell, that shell's tab and nothing else, even with none left to
  close; otherwise the comparison of changes when it is up (and the
  question over it before that), a file shown beside the conversation,
  then the session's tab, then the window (`close_nearest`).
- A `CloseTab` or `CloseWindow` no view heard is handed to the workbench
  by `main.rs` (`with_workbench`), never answered by closing the window.
  The focus can be on nothing the page draws, and the key then comes
  from the window's root: closing there closed the window with tabs
  open and asked nothing about a file not saved.
- Nothing has to give the focus back when it goes. `keep_focus`, at
  every draw, asks whether the focus is inside what the workbench drew
  (`contains_focused` on its own handle). When it is not at one draw and
  still is not at the next, with the same thing holding it, the focus
  goes to the composer where one shows and to the workbench's handle
  where none does. That covers a file closed with the keyboard in it, a
  field put away, a panel folded, and whatever is added later. It waits
  a draw because a thing focused as it first appears is in no frame yet.
- A file's pane closed with the files tree beside it gives the keys to
  the tree (`tree_focus_back`): the file's row is still lit there, and
  ⌘⌫, ⌘C and the arrows are the tree's only while it has the focus.
  Not when the composer is being typed in.
- Keep `on_reopen` in `main.rs`. The app stays running with no window, and
  without the handler a Dock click does nothing.
- Every scroller in the window, now and later, has the same bar and keeps
  its own wheel. Both halves hold for a list, a box, a card, a menu, a
  page: anything that scrolls up and down.
  - **The bar** is the toolkit's `vertical_scrollbar` on the scroller's
    handle and nothing else. It shows while the scroller moves, stays one
    second after the last movement and fades over half a second (the
    vendored `scrollbar.rs`). No scroller goes without one and none draws
    its own. The file editor's is that bar made as wide as the strip of
    marks it runs over (`docs/panels.md`); nothing else changes its
    width.
  - **A gesture belongs to the scroller it began in**, its momentum
    included, wherever the pointer is carried: it never scrolls anything
    else, and stops at that scroller's edge. So a scroller is known to
    `route_scroll`, one of two ways: as a pane (`Pane`) when it can sit
    beside another, or by naming itself as it is drawn when it is inside
    a pane or on a sheet or menu over the window (`inner_scroller` for a
    handle it has, `kept_scroll` for one it needs; `Inner` says which
    kind). A scroller that does neither leaks: a flick in it scrolls
    whatever the pointer reaches next.
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
- A name asked for is never drawn as the session's title before the
  transcript says it. The title stays the transcript's, with the mark
  beside it. Typed is not taken: only the index puts a name away.
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

A name, three entries and a card (`render_sidebar`).

- The wordmark (the app's icon, served as `icon/app.png`, and "Emaki") is in
  the sidebar's own top strip, right of the two buttons, in
  `fonts::wordmark_family` and the accent (`theme.primary`), so it follows
  Settings. Optima has only regular and bold, so the regular is drawn twice
  half a pixel apart for a weight between. Tried and dropped: serif faces, a
  bold sans, a fixed gold.
- The strip has no line under it: the sidebar ends against the content's
  card (Layout). Its foot is still level with the foot of the card's own
  strip.
- The sidebar stands on the window's frame and draws with a theme of its
  own (`shell_theme`), as do the two buttons at the window's corner while
  they are over it. Beside a light page the frame is near black, so that
  theme has the dark appearance's inks and says it is dark, and whatever
  goes by the mode picks the shade that reads there. A row is lit and a
  line ruled in the ink thinned. Everything the sidebar draws takes its
  colours from that theme: a toolkit button, or a `cx.theme()` inside a
  closure, brings the page's colours onto the frame, which is how the
  open session's row came out near white under white words.
- New session, All Sessions, Agents. The All Sessions entry is still ⌘L
  and `GoSessions`; its icon is a scroll (Tabler's `script`), which is
  what an emaki is, and does not read as a folder row. Agents is ⌘E and goes to the agents page
  (`docs/agents.md`).
- One card (`card`), Projects, under a name that does not scroll, taking
  the height left (`side_scroll`, `Pane::Sidebar` to `route_scroll`).
- The footer is the person's name and a settings gear, nothing else.

The Projects card lists every project, the newest first, and scrolls. It
listed ten and then "N more", which went to the sessions page: the card
stood half empty and the click left the list for another page. A
click opens a folder onto its sessions and closes it again (`folders_open`,
kept in `ui.json`): `FOLDER_ROWS` of them, then "N more".

A project can be pinned (the pin beside the plus under the pointer, or a
right click on its row; `toggle_pin`, `pinned` in `ui.json`). The row
keeps room for the pin and the plus whether they show or not, and a
pinned project wears its pin, filled, at all times: a click lets it go.
The pinned ones are a block of their own over the rest, in the order
they were pinned, with a line at its foot: the rest scrolls under it
and the pinned stay where they are. The block takes what it needs up to
half the card and past that scrolls by itself (`pin_scroll`), so the
rest always has the other half, however many are pinned or open. The
wheel moves the block when a gesture begins over it and it has more
than shows (`scroll_pins`), and the rest otherwise. One dragged onto another takes
its place (`DragPin`, `drag_pin_to`: each pinned row hears the drag and
answers when the pointer is on it). `side_order` is the one place the
order is made, for the list and for the fitted width. A pin only moves
the row: let go, the project is where the index has it.

Several projects are chosen together as the files tree's rows are: ⌘ and
a click adds one or takes it out, ⇧ and a click chooses from the one last
clicked to this one over the rows as drawn (`side_click_more`,
`side_marked`, `side_drawn`), and a rectangle dragged over them from a
press anywhere in the card chooses the ones it lies over
(`side_band_to`). The card has no empty room to press in, so the press
may be on a row: one that has not moved a few points is still that row's
click, and the click that ends a drag does nothing (`side_band_live`). A
press on a pinned project is its own drag, to another place among the
pinned. The rows are not one height here, so each says where it was
drawn (`side_rows_at`), cut to its own list. A plain click opens the
project and lets the chosen go, as Escape and a click on the card's
empty room do. The chosen are lit in the accent, and rows lit one under
another in the same list are one block, round at its two ends, unless
the upper one's sessions show between them. A press anywhere in the
card gives it the keyboard (`side_focus`), and there ⌘A takes every
project showing, whatever was chosen: projects, never sessions. Nothing
has to be chosen first. A session opened by a click gives the keyboard
to its composer, and a letter typed with the card focused goes there
too, the keyboard with it, so typing after a click in the sidebar is
not lost. A right click on one of several is their menu
(`project_menu`), and on any other project its own:

- **Archive Sessions** and **Delete Sessions** ask it of every session
  of the projects, those the sidebar does not list too
  (`MenuDo::Sessions`, `manage_ask_of`): the question and the work are
  the sessions page's (`render_manage_ask`, `manage_now`), and what came
  of it is said on the notice line. Sessions only: nothing in a
  project's folder is touched.
- **Open in Finder** opens each one's folder in a window of its own
  (`projects_do`). More than `OPEN_MANY` at once is asked about first
  (`render_open_ask`).
- **Hide Project** puts a project out of the list (`hidden`, kept in
  `ui.json`); **Show Project** on a hidden one brings it back. A hidden
  project is not among the pinned while it is hidden, and takes its pin up
  again when shown. The sessions page lists every project, hidden or not.

Sessions are chosen together in the same ways (`side_sessions`,
`side_session_more`, `side_sess_drawn`, `side_sess_at`), across every
project that is open: ⇧ and a click reaches over the sessions as drawn,
whichever projects they are in. **Projects are chosen or sessions are,
never both, and what is touched first says which.** With projects chosen,
⌘ or ⇧ and a click on a session does nothing, and the other way round; a
rectangle takes the kind of the row nearest to where it was pressed,
once it reaches one, and keeps to it however far it is dragged
(`side_band_kind`). A right click on one of several sessions is Archive
Sessions and Delete Sessions, and Unarchive Sessions when one is
archived (`side_session_menu`); on any other session it is that
session's own menu, with Archive Session (Unarchive Session on an
archived one) and Delete Session at its foot.

Two sections stand under the last project that shows, each a heading with
its count that opens onto its projects in motion (`toggle_section`):
"Temporary", then "Hidden". Temporary is the projects every session of
which was run in one of the system's temporary folders
(`paths::is_temp_dir`): a scratch run of an agent's or a script's, whose
folder the system clears and nobody goes back to. A pinned one stays
among the pinned and a hidden one is under Hidden. Both sections are shut
at every launch and neither is remembered open: they are where what is
not wanted in the list goes.

A folder with a live session is open without being asked (`sync_folders`):
it opens when one of its sessions goes live and closes when the last stops,
if it was opened that way and not clicked since. A click is the person's
choice and stays until the folder next goes live or quiet.

A session's mark is in the muted ink unless it is live, so many rows do not
read as many colours, and then in its agent's colour (`docs/agents.md`,
Marks) with a dot in its phase's colour. A folder with a live session is
in the accent, whichever agent's it is. A folder
wears the most pressing of its sessions' (needs you, then working, then
your turn).

A folder lists its `FOLDER_ROWS` newest sessions, and under them any
older one that has a tab: opened from further back it is in the list
while its tab is, and goes with it, growing in and shrinking out as any
row does (`side_rows_changed`). The rows have no headings by when
("Today", "Yesterday"); those are the sessions page's.

A session with a tab wears a short bar at its row's left: in the accent
for the one showing, grey for the others. The dot at the right is the
session's phase and says nothing of tabs.

A folder's sessions unfold and fold over `FOLDER_ANIM`: a box whose height
is the sum of its fixed-height rows (`SIDE_ROW_H`, `SIDE_SESSION_H`), which
`folder_anim` runs up or down with the opacity. A closing folder is still
drawn until the time is up.

Narrower than `NARROW_W` the sidebar leaves the row and comes back only as
an overlay over a scrim (the strip button, ⌘B), which any click or Escape
puts away. Each of these is in motion (Motion, below). `sidebar_open` keeps the preference for when the window is wide
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

"All Sessions": a row a project, newest first, as the sidebar has them. A
project is a folder; the window says "project" and keeps the folder icon.
A click opens a row onto its sessions in place and closes it again
(`sessions_toggle`, `sessions_open`, kept for the launch only), by the
sidebar's motion: a box of fixed-height rows (`PAGE_SESSION_H`) run up or
down with the opacity. It went inside the project, to a second level
under an "All Projects › folder" line: a click left the list, and two
projects could not be looked into at once.

Nothing is headed by when. Each row has the day it was last touched at
its right end (`format::day`), a project's before its chevron and a
session's in line with it. The one heading is "Archived", over the
projects whose every session is archived, kept here alone or in
Codex's own archive. A project with a session not archived
is above it, and an archived session in one wears an "archived" tag. The
pills (All, one per agent) narrow the page.

A project opened by a click, here or in the sidebar, has its foot kept in
view as it unfolds (`follow_foot`): where its sessions run past the foot
of the list, the list moves up with them, never by more than leaves the
project's own row showing. One that opens by itself (a session gone
live) moves nothing.

### Choosing sessions

The line over the first project counts the sessions an agent still has
and the archived ones, each with the projects it is in (a project with
both is counted for both), as two labelled figures with the session
counts in the page's ink, with
"Select" at its right. Pressed, the line becomes a plate (`manage`,
`picked`) and every row gets a box before it. A session's row chooses
it; a project's row chooses all of its sessions, and its chevron is
then what opens it. The plate has a box for everything the pills leave,
the count, what came of the last thing done, and Archive, Delete and
Done, drawn as the page's pills are: Archive quiet, Delete in the
danger's ink, Done solid as the pill that is on. It is in the list's
flow, over the first project; scrolled out of view, a copy of it stands
at the page's top (`manage_y`, the "manage-float" presence), so
sessions chosen far down can be acted on. It was a strip fixed over the
page's title, which read as a toolbar of the window's and not of the
list. Each of the two asks first (`render_manage_ask`), and is done off
the main thread (`manage_now`).

- **Archive**: the agent does not continue the session until it is
  unarchived. Claude Code's own copy is let go now, without waiting for
  it to delete it, and Codex archives its own (`Adapter::archive`,
  `docs/core.md`). A session archived in Codex itself is archived here
  too: one word, one meaning, wherever the file is.
- **Delete** moves every copy to the system's trash, Claude Code's and
  the archive's; Codex deletes its own (`adapters::delete_session`).
- **Unarchive** (a session's menu in the sidebar, Unarchive Session, and
  a project's, Unarchive Sessions) puts an archived session back for
  its agent (`Adapter::unarchive`), with nothing asked, since nothing is
  lost, and nothing counted as running: neither agent takes up an
  archived session. An archived session cannot be continued until it is
  unarchived, and the composer says so.
- Both are of sessions. Nothing in a project's folder is touched.
- A session something is running in (`Hub::is_live`, a driver) is left
  as it is and counted in what is said.
- The question is not answered from a script: the probe's steps stop at
  asking.

"N more" under a folder in the sidebar opens that project here and
brings the page to it (`sessions_goto`: the row says where it is once
drawn); the All Sessions entry goes to the top; the row on an agent's
page goes here narrowed to the agent. A right click is Open in Finder on
a project and the session's menu on a session.

## Motion

Nothing in the window appears or disappears from one frame to the next.
The patterns, each in use and to be used again:

- **A hover's label**: `managed_tooltip`, the toolkit's own, which a
  `Button` uses: it comes in, and slides to the next element hovered on
  the same row. Not gpui's plain `.tooltip`, which jumps in and out; a
  row with both kinds read as two different controls.
- A row under the pointer on a card in the dark appearance is a wash of
  the ink (a menu's, a list's, the tree's): the palette's own hover is a
  shade off the card and could not be seen.
- **A menu or a list over the window** (`render_menu`, `render_pick`):
  it comes in over `MENU_IN`, rising a few pixels into place as it
  fades in, and goes out over `MENU_OUT`, fading where it was. What was
  closed is kept for that moment (`menu_gone`, `pick_gone`) and drawn
  once more with no click taken. Each opening has a serial in its
  animation's name, so it plays once.
- **A card at the conversation's end** (`render_terminal`,
  `render_dialog`; `docs/live-turn.md`, The cards): held by its foot,
  its height opens up from the end and closes back to it, with the gap
  under it part of what moves, so the conversation above slides and
  does not jump. What it
  last showed is kept (`TermShown`, `DialogShown`) to draw it closing. A
  card whose height is its content's measures itself as it is drawn
  (`dialog_h`) and takes no room until that is known.
- **A panel at the conversation's side**: its width opens and closes
  (`docs/panels.md`, `docs/channels.md`). One put away for want of room
  goes the same way, through the call its button makes (`term_go`,
  `panel_go`), never by its flag alone: the flag alone left the button
  lit and the panel gone between two frames.
- **The sidebar**: it slides in from the window's edge as its room in
  the row opens, and out as it closes, whoever asked: its button, the
  key, a window dragged narrower or wider, a panel that needed its
  room. `Workbench::render` sees the change (`side_was`) and starts it
  (`side_anim`); nothing that shows or hides the sidebar has to. It is
  drawn off the clock (`side_t`, `shown_t`) and not as one element's
  animation, because the top strip's room for the buttons, the panels'
  left and their edges all stand by the sidebar's width
  (`side_w_now`). Over the scrim in a narrow window it slides the same
  way and the scrim fades (`peek_anim`). A floating sidebar that is
  kept stays where it is while the row makes room under it
  (`float_kept`). Not played: a drag of its own edge, and the first
  draw.
- **A choice that moves** (a segmented control, the strip's buttons): one
  plate slides from the old to the new.

- **A fold in the conversation** (a tool call's body, a run's calls, a
  thought, a subagent's rounds): `fold_state` and `fold_wrap`. The box's
  height runs up from nothing and back, the content fading with it. The
  content keeps its own height, cut by the box, and says how tall it is
  as it is drawn. `fold_state` is asked at every draw, shut or open, so
  a change is seen as one and the first sight of a fold is not played;
  shut, the content is drawn for that moment more. The room between
  a fold and its head is inside the fold (`fold_wrap`'s `gap`), in a
  column with no gap, as it is under a card over the composer: a
  column's gap outlives a box shut to nothing, and the closing stopped
  short and then jumped.
- **A row that comes or goes in a list** (a session in a sidebar
  folder): `side_rows_changed` sets a folder's rows against how they
  were last drawn. One that was not there grows in; one that is no
  longer is drawn a moment more where it stood, shrinking
  (`side_gone`). A folder drawn for the first time plays nothing, and a
  row that only changes place is not moved in motion.
- **Something small in the sidebar that is there or is not** (the bar
  of a session with a tab, the line under the pinned projects):
  `presence`, asked at every draw, which plays a change over
  `FOLD_ANIM` and not the first sight.
- The settings are drawn over every sheet and card, the comparison
  included; only a menu or a pill's list, which they open, is over them.
- **Something that lies over the window and has no way out of its own**
  (the rename sheet, the lightbox, the comparison, the branch list and
  its question, the discard question): `fade_in`, on a static id, which
  plays once each time it appears. These still go at once.
- **A bar in the flow** (the find bar): its height opens with what is
  under it moving, by a `max_h` no less than its own.
- **A line under the conversation** (the agent at work, waiting,
  commands in the background, a card left behind): `foot_line` on its
  `presence`, keyed by the session so a change of tab plays nothing. It
  opens up from the composer and closes back, the air over it part of
  what moves. Going, it is drawn from what it last said, kept for that.
- **A card whose height is its content's** (a permission, a driver's
  question, the background commands' card): `card_fold`, which measures
  the card as it is drawn. One answered is drawn a moment more from the
  request kept, closing, under a layer that takes its clicks.
- **A scroll that a click starts** (the arrows of a dialog's tabs, "Show"
  for a card left behind, "New messages"): run from where the scroller
  is, a share of what is left each frame, so it slows onto the place
  (`tabs_glide_step`, `end_glide_step`, `glide_step`). Never
  `set_offset` or `scroll_to` straight to the place.
- **Something small whose room stays** (an arrow at a strip's end, the
  fade there): `presence`, with only its opacity run.
- **What runs past an edge of a sideways scroller** fades into the
  ground there (a block of code, a dialog's tabs), and the fade comes
  and goes with there being more that way.

Left as they are, on purpose: a tab's page changing (the conversation
is the content, and a cross-fade of two long lists is a blur), text
arriving in a live reply, a hover's ground, and a row changing place in
a list when its session is written to.

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

A phase is shown only for a session with something behind it
(`Hub::is_live`): a driver, a hidden terminal, a registered process, or,
with none of those, a write in the last ten minutes. Two things keep a
dead session from reading as "working" with its clock running, as one
killed while a question waited did. A session whose process was
registered and is gone has ended (`refresh_peers`). And a Claude Code
session that reads as working with no process registered gets a minute
and a half, not ten: an interactive one always registers, so it is a
headless run of someone's, which writes as it goes, or a process that
died before this launch.

## Layout

Laid out like the Claude desktop app.

- The window's ground is the frame (`title_bar.background` in the theme),
  set against the page and not near it: near black beside the light
  page, and lighter than the dark one (`docs/look.md`, The grounds). The sidebar stands on it with no edge of
  its own, and the content is a card on it with rounded corners: `FRAME`
  of the ground shows over, under and right of the card, and left of it
  when the sidebar is away.
- The card's corner is the window's less the frame (`CARD_R`): the two
  curves then run side by side. With a rounder window than card the
  frame thinned to a third of its width at each corner. The window's own
  corner is the system's and not ours to set.
- A clip in gpui is a rectangle, so the card is square and its corners are
  painted over in the frame's colour after what it holds (`card_corners`).
  A panel's ground stood square in a rounded corner otherwise. They are
  one quad's border kept to the card, not paths: gpui gives a path four
  samples a pixel, and the curve stepped beside the window's own.
- Whatever is placed by the window's edges goes by the card's:
  `content_left`, `view_w` (its right edge) and `pane_w`. The card's strip
  is `FRAME` shorter than `TITLEBAR_H` and starts that much lower, so what
  is under it is where it was, and the traffic lights and the two buttons
  are moved down to the strip's middle (`main.rs`, `render_strip`).
- The buttons at the two ends of the card's strip are as far from the
  card's side as from its top (`STRIP_PAD`).
- Everything readable sits in one column of `CONTENT_W`, centred in what is
  left of the window: the conversation, the sessions page, the home page.
- The content pane has a top strip with actions on the right. When the
  sidebar is hidden the strip makes room for the traffic lights.
- The home page is a drawing of a handscroll, an emaki, over a greeting
  in `fonts::display_family` (the chat font, so titles and the
  conversation agree), over the composer. The drawing (`home_scroll`) is the scroll seen from the
  front, as the icon is, with a landscape on its sheet. It is in the
  manner of hairline's figures (github.com/lucasmarkes/hairline): lines
  only, no fill, no words, what is far dimmer, and one bright mark, the
  sun, in the accent, so it follows the appearance and the accent.
  - As hairline's figures do, it answers the pointer, in what a
    handscroll does: the pointer moved across it winds it (`HomeFig`).
    The landscape is longer than the sheet shows and in four layers by
    how far off each is (`scripts/home-scroll.py` writes them): mist, a far range, near hills, water. They pass at four
    speeds. The paper leaving one roll goes onto the other, so one
    thickens as the other thins, each showing its turns; the sun rises
    and sinks a little with the pointer. Tried and dropped: a cord
    hanging from the right axle's foot and swinging with the winding. A
    handscroll's cord is tied to the stave at its cover's edge, which is
    wound inside the right roll while it is read, and nothing hangs from
    an axle.
  - It follows on a spring, a little past and back, and goes to rest
    when the pointer leaves. A frame is asked for while it moves and
    none at rest.
  - The scroll's own lines (rolls, axles, the sheet's edges, the turns,
    the sun) are elements with borders: they change width at any moment
    and are drawn exactly. The landscape is a picture a layer, only
    moved, inside a box that cuts it to the sheet. Not paths painted a
    frame at a time: gpui gives a path four samples a pixel
    (`card_corners`), and lines this thin would step.
  - `EMAKI_GO=homewind:<across>,<down>` (each -1 to 1) winds it as the
    pointer there would, for a picture.
  Tried and dropped:
  hairline's own 2:1 view, which set the picture on the sheet at a
  slant. The app's icon stood beside the greeting until the drawing
  did its work; a dark drawing of the icon for the dark appearance was
  made and dropped, the icon being one thing everywhere.
- The composer's card stands on a tray of its own
  width (`folder_tray`), and the strip of it that shows under the card
  names the folder a new session starts in: its name, then the folders it
  is in. A press anywhere on the strip drops its list down
  (`PickFor::Folder`, the one list that never opens upward): the
  `HOME_FOLDERS` folders worked in last, the chosen one among them even when
  it is none of those, and under a line "Choose a folder…", a row with no
  key, which asks the system for any folder (`choose_folder`). ⌘O is
  that row from the keyboard (`ChooseFolder`), on this page only: on any
  other page it does nothing. Tried and
  dropped: a grid of six cards under the composer, and before it full-path
  pills of every width, which took the page for one choice; a pill beside
  the mode's on the composer's row, where it read as one more setting of
  the agent's; and the panel buttons' small track by itself under the
  card, which belonged to nothing around it.
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

A row is a name and its control, side by side: the names stand in a
column of one width (`SETTINGS_LABEL_W`) and every control starts where
that column ends. A set of choices or a field fills the rest of the row
(`row`), so a page is as full at its right as at its left. What has one
short answer is half of that (`short`): on or off, a button (`press`),
a choice from a list as a field (`settings_pick`, not a pill). One of
those across the whole row was too much for what it says. A control at
the panel's right edge, and a control at its own size beside its name,
were both tried: the first left a short name a panel's width from its
control, the second left the page's right half bare. A new row is made
with `row` or `short`, never laid out by hand.

What is on or off is the two words as a choice (`toggle`), On in the
green and Off in the red, each on a plate washed with its colour. It is
a switch: a press anywhere on it, either word or the track's edge,
turns it the other way (`segmented_sized`, for every Off and On). A
tick is for a list of many, where a choice a row would be too much:
the formatters, each tick before its name.

The spelling's languages are a row of their own, a flag and two letters
each so the five fit, the name on the hover.

A line on a row stands under a control that fills and beside a short
one, in the half it leaves (the version is one, beside its button), and
is there only where the name does not say it all: most rows have none, and one that does is a
few words. A new row gets no line unless a person would otherwise not
know what it does.

An accent's swatch marks the chosen one with a ring inside the disc, so
every disc is one size and the row's ends are the first and last disc.

Settings asks for one text size (Fonts): the conversation's, and the
terminal's and the file editor's go with it (`set_chat_size`,
`term_px`, `editor_px`). The terminal's
own size, its line spacing and its padding were rows once; they are the
middle step now, set at launch whatever `config.json` says. The
terminal's section sends a person to Fonts for its face and size.

A choice is named as the agent names it, less the "(recommended)"
Claude Code writes after its default (`plain_default`).

On the new-session page, a machine with neither agent on it says so
over the composer and sends the person to the agents page
(`no_agent_note`). It waits until the machine has been looked at.

A key is written as the Mac has it and drawn through `shortcut`
(`kbd_hint`), which makes ⌘ "Ctrl+" off a Mac. The tooltips that name a
key still say ⌘ everywhere.

The last section, About, says who makes Emaki. A plain plate of the
fields' ground, one flat shade with no edge, holds the title card, set in the middle as a film's
first one is: "Alkaline Software presents" (the studio at its GitHub
organisation until it has a site), the app's icon, its name in the face
and the accent the sidebar has it in (it and the icon are each a link
to emaki.lol), and the version in small grey words. Under the plate, a sentence
naming its two owners in full, each name a link, and pills to the
site, the source and the release's notes. It has nothing to set. Tried
and dropped: a row a name with a line under it saying what each is,
which read as a form; the card on the bare panel, which was pale; and a plate fading
from the accent, with the version on an accent chip, which the person
did not want: no gradients here.

Dictate is two lanes of models, Speech to Text over Post-processing,
then the key and History. A lane is its name (Post-processing's with
its On and Off)
and the model in use, and it drops down like a list: a press on that
row, or its chevron, folds out the others under it, the most capable
first (`dictate_lane`, `lanes_open`, `fold_under`; the chevron turns).
With none downloaded the lane's first choice stands there with its
Download. At a row's right are two slots of one width (`MODEL_SLOT_W`):
in use, Use or the download's line, then Remove, Download or Stop, so
every button is one size and one column. On for post-processing with
no model here opens its lane and
says to download one. Every row keeps the chevron's room, so Remove and
"In use" stand at one edge down a lane. Any number of models download
at once (`dictate_dl`), each with its own line and Stop. A row is its maker's mark on a
plate (one colour, the ink's, as every icon here is), its name with its
size, under them a few words on what sets it apart and its languages
(the maker is its mark; with the words beside the name they were cut
short once the buttons were one width), and at the right Download (a
line filling as it comes, with Stop), Use, or "In use", with Remove
once it is here. Post-processing turned off fades its name and models
to grey over a moment, the accent and the red gone from them, and its
switch stays as it is (`presence` on `lane-live-clean`).
The one in use wears the accent's edge, and a click anywhere on a row
that is here takes it up. A grid of cards was tried first: three across
left a name two lines deep and the buttons crowding its foot. Under them the key, a field that takes
the next keys pressed in it (`docs/composer.md`, Dictation), at the
row's right, and Reset against the edge, which puts back ⌘⇧D (Ctrl and
⇧ and D off a Mac) and is dimmed while that is the key. Under it History:
how long what is dictated is kept, a field with its list, and Show
History, which opens the dictations on a sheet over Settings, smaller
than it both ways so it reads as lying on it. It rises in as it fades
in, and fades out where it was. The field set
in the page's middle was tried and undone. Off a Mac
the section is one line saying it is a Mac's alone for now.

The three looks in Appearance have an icon each (`choice_icon`).

A language in the Composer section has its flag before its name
(`flag_of`, drawn as a picture so it keeps its colours; the flags are
drawn here, in `assets/icons/flags`).

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

- A folder, in the sidebar or on the new-session page's control, offers "Open
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
  over instead. Each of the four has its icon, as a PDF's Copy and
  Select All have; a spelling's corrections are words only, all of
  them, so they still stand in one column. On a marked word of the composer
  the menu is that word's corrections and nothing else
  (`docs/composer.md`). The input calls the handler from inside its own
  update, so the menu is made a moment later (`window.defer`): reading
  the input there aborts the app.
- A branch, on the pill in the files' head and on a row of the list,
  offers Copy Branch Name. The row stops the press, which would otherwise
  reach the list's sheet and put the list away.

A session's name is written where it was asked for (`RenameAt`,
`rename_input`), in the field a file's name is written in in the tree
(`name_field`): in its row of the sidebar from that row's menu, on its
tab from the tab's. The name there is selected whole so a key replaces
it. ↩ keeps the name and so does leaving the field, Escape leaves it as
it was, an empty field changes nothing. A field opened on a session with
a name on its way holds that name. Giving the name the transcript
already says calls a waiting one off.

- The two are one name. Both draw the session's title, so what is
  written in one is what the other says, as soon as it is the title
  (below: at once for a session that cannot be asked, when Claude Code
  has taken it for one that can).
- Where there is no row and no tab to write in (the sessions page, a
  row in a folder that is closed, a sidebar put away) it is the sheet
  over a scrim it was at first (`render_rename`): a row or a tab says
  it drew the field (`rename_drawn`), and with neither the sheet does.
  There a click outside leaves the name as it was.
- A second click on the row or on the tab writes the name there, on
  every system (the click's count, in the row's and the tab's own
  click). The first has opened the session, as any click does.
- F2 does the same on Windows, wherever the keyboard is (`rename_key`,
  heard at the window's root, after the files tree, which keeps the
  key for its own row): on the tab, or in the row for a session with no
  tab. A Mac has no key for it. Tried and dropped: ↩ on a Mac with the
  keyboard on the sidebar. A click on a session puts the caret in the
  composer, where ↩ is the composer's, so the row had to be clicked a
  second time to take the keyboard before ↩ meant rename, and a click
  on the session showing no longer reached the composer.
- A press in a tab's field says it is taken (`press_taken`) and stops
  there, or it began a drag of the tab or of the window.

A name is a request, kept in `renames` and in
`~/.emaki/state/titles.json` by session key until it is settled, so a quit
does not lose it. For a session Claude Code can be asked about (`can_ask`)
the title everywhere stays what the transcript says, and a pencil beside it
on the tab, the sidebar row and the sessions page says on a hover what the
name will be (`rename_mark`). `try_renames`, run when the index arrives and
on the clock, types `/rename <name>` into the session's hidden terminal
(`Hub::rename_in_terminal`), starting one when the session has none, once
the session is between turns: not during a turn, not while a card waits
for an answer, not while the turn runs in a terminal of the person's.
Claude Code writes a `custom-title` row, which its resume list reads and
`transcript::pick_title` puts before any AI title. It writes the same words
as an `agent-name` row, which must not disqualify them.

`SessionRef::named` is the transcript's own name, and `transcript::naming`
settles a request against it as each index arrives:

- The transcript says the name: it is the session's, the request is put
  away and the mark goes out.
- The transcript took another name since the request was made (a `/rename`
  typed in a terminal): the newer one stands and the request is dropped.
- Neither: it waits. One typed that the transcript still does not say once
  the transcript has moved on is asked for again, a few times in all, and
  then given up with a line under the composer. Nothing shown was wrong
  meanwhile, since the title was the transcript's throughout.

For a session that cannot be asked (another agent, one kept only, a folder
that is gone, the hidden terminal off) the name is ours alone: laid over
`SessionRef::title` at once and as each index arrives, with no mark.

A terminal types one message at a time (`Hub::type_message`), so a rename
and a message sent in the same moment do not arrive as one line.

## A session begun from a folder

A folder's row in the sidebar shows a plus in place of its count while the
pointer is on it (both always there, one clear, so nothing moves). It
begins a session in that folder (`new_session_in`) on a tab like any
other's: the conversation page with nothing said, named "New Session" in
italics, the name being ours and not one anybody gave it (`unnamed`).
⌘N and the home page are as they were. ⌘T is the plus of the folder the
session showing is in (`new_session_here`), and ⌘N where no session is
showing. In the terminal panel ⌘T is a new terminal tab, as it was.

- It is a record of our own (`begun`), put at the head of every index
  until the index has one of its own for it, under a session id made
  here. Nothing is started and nothing is on disk.
- The first message starts Claude Code with that id (`send_message`
  starts where it would resume, `is_draft`), so the transcript that
  follows has the tab's key and the tab goes on as that session, title
  and all, with no hand-over.
- A session started from the home page gets the same record at its
  first message (`show_begun`), so its tab and its row are there at
  once and the message shows in it (`docs/conversation.md`). It waited
  for the index to find the file, on a bare pane.
- Closed with nothing sent, it is gone with its tab. It is not written
  to `ui.json`, since a launch would find nothing for its tab. A
  message typed and not sent is the composer's draft and goes with it.
- A folder's plus pressed twice goes back to the one begun and does not
  make a second.
- It begins with the agent last chosen for a new session, and its
  composer stands on the home page's tray until the first message: the
  folder named and not to be chosen again, the agent to be chosen
  (`begun_agent`). Nothing has been started, so the other agent takes
  the same record: the tab is re-keyed where it stands and what is
  typed stays. Without this a plus could only begin what the home page
  was last set to.
- Until its first message it has no terminal to ask anything of: its
  pills are the home page's (they open Settings), the composer says
  "Start a session…", the terminal panel's agent side says Claude Code
  starts with the first message, and it is not shown as live.
- Nothing starts a hidden terminal for it before that message
  (`terminal_check` refuses it, `warm_now` passes it by). Every other
  start is a resume, and `claude --resume` on an id with no transcript
  ends at once with "No conversation found with session ID".

## Scroll routing

macOS goes on sending wheel events after the finger lifts, addressed to
wherever the pointer is by then, and gpui hands each to the scroll container
under it. Its `touch_phase` tells `Started` and `Ended`, but a momentum
event is just `Moved`. So a flick in one pane followed by a move to another
scrolled the second.

`Workbench::route_scroll` runs in the capture phase from a raw listener a
zero-size `canvas` registers at paint, before any container. Of the
scrollers that name themselves (`inner_live`), the one under the pointer
drawn last is the one on top and takes the gesture: History's list over
the settings', which scrolled the settings while it was first. The pane under
the pointer at `Started`, or at the first event after `SCROLL_GAP` (a mouse
wheel sends no phases), owns the gesture. An event that lands in another
pane is applied to the owner's scroll position (`ListState::scroll_by` for
the conversation, a `ScrollHandle` elsewhere) and stopped. With a single
pane there is nothing to do.

A pane is not the only thing that scrolls: a tool call's body, a box on
the background commands' card, the settings' rows, a pill's list, the
search results, a PDF's page list. Each names itself at every draw
(`inner_live`), and `route_scroll` looks for one under the pointer before
it looks at panes:

- A gesture that begins in one is that scroller's (`scroll_inner`). The
  router moves it itself and stops the event, on the scroller or off it,
  so nothing under or beside it ever takes the rest.
- A gesture begun elsewhere and carried onto one does not move it. In a
  pane the event goes to the gesture's owner; on a sheet or a menu
  (`Inner::Over`) it goes nowhere.
- A tool call's body and a card's (`card_scroll`) are `Inner::Chained`:
  on the body its own handler decides (`chain_wheel`, `Stroke`: a stroke
  that starts at its edge is the conversation's), and off the body the
  gesture stays with it only if it moved it.

Scrollers that only go sideways (the terminal's tabs, a question's
preview) are not named: a sideways flick moves nothing else.

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

A folder's tabs stand together on one pill (`group_tabs`, at every draw):
the segmented control's track, with a folder mark at its left end and the
showing tab on a raised plate. The pills are in the order their first tabs
had, a tab opened lands at the end of its folder's pill, and a session
alone in its folder is a pill of one. A tab whose session is not listed yet
is a pill by itself until it is. The folder is `SessionRef::project`, the
sidebar's.

The plate is one element under a pill's tabs (`TabPlate`). It slides to
another tab of the same pill, and between pills it fades out of one and into
the other: a plate does not leave its track. It is placed by sum, never
measured, since every width in the row is ours, and it reads the same clock
the tabs' widths do (`TabWidth::now`), so it keeps its tab while they move.

Every tab has the same width (`sync_tab_widths`, at every draw): `TAB_MAX`
while the row has room, the row less the pills' own room divided among them
once it has not, down to `TAB_MIN`, which still shows a few letters of the
title. Past that the tabs keep their width and the row scrolls sideways
(`tabs_scroll`), as the shell's tabs do: centred while they fit, from the
left once they do not, since a centred row wider than its box cannot be
scrolled to its start. The tab gone to is brought into the row, and again
when the row's own width changes (the sidebar, the window), and a drag's
places take the scroll into account (`pill_spans`).

The row keeps `STRIP_GAP` clear at both ends, whatever stands there and
whether the sidebar is open: the end of the strip with buttons at it is as
wide as they are, and only an end with nothing keeps the 120px that
centres a title. With the 120px on both, the tabs stood far off the
buttons with the sidebar open and against them with it away. A new tab grows in from nothing while
the others give way, and a closed one's room is taken up the same way, over
`TAB_ANIM` (`tab_widths`). A new pill's track is let out with its tab
(`pill_born`), in a wrapper that clips it and stays until the plate's move
is over: taken off sooner, the plate under it is a new element and plays its
move again. A change in the row's own width is followed at once. The row's
width is measured as it is painted (`tabs_row_w`, a `canvas` over it) and
used at the next draw. Tried and dropped: tabs as wide as their titles with
the flex row shrinking them, which drew a new tab at full width for a frame.

A tab is dragged to another place on its pill and no further (`on_drag`
with a `DragTab`, the row's `on_drag_move` to `drag_tab_to`, which takes the
place under the pointer as it moves). Its title follows the pointer on a
plate (`TabGhost`) and its own tab is dimmed. The pill is dragged by its
folder mark (`DragPill`, `drag_pill_to`), or by its tab when it has one, and
stands after every pill whose middle the pointer has passed. Going by the
middle is what keeps two pills of different widths from changing places back
and forth under a pointer that is still.

Whatever a drag moves slides to its new place (`TabShift`): a tab or a pill
is laid out where it now stands and drawn off it by what is left of the way,
a relative `left` read off the clock, with a frame asked for at every draw
while one runs (`tabs_sliding`). One moved again on its way starts from
where it is. The plate adds its tab's offset, and the pill being moved is
painted after the row (`deferred`, `pill_top`), so it passes over the pills
it changes places with and not under them.

⌘⇧T opens the tab closed last again and shows it, and at each further
press the one closed before it (`closed_tabs`, `reopen_tab`). A session
begun and closed with nothing sent is not kept, one gone since or open
again is passed over, and the list is the window's while it runs, not
`ui.json`'s.

⌘1 to ⌘9 (Ctrl elsewhere) go to that tab, or to the last one when there are
fewer (`go_tab`). ⌘ held by itself for `TAB_HINT_HOLD` shows the digits:
each tab wears an outline and its number on a small plate on its lower edge.
They go when the key is let go or another modifier joins it
(`modifiers_changed`, from `on_modifiers_changed`). The timer from the press
is answered only if that press is still the one held, so a quick ⌘C never
shows them. A release the window never hears (⌘Tab away) is caught at the
next draw, which asks the window what is held. The first nine tabs
have a number, the one `go_tab` takes to them, and any after have none. The tab clips what
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
hands a `CloseTab` no view claimed to the workbench, which does the same.
`on_reopen` (a Dock click, a second launch) opens the window again, and
what was typed in each composer is there (`docs/composer.md`).

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
- `EMAKI_GO=newin:<folder's path>` is the plus on that folder.
- `EMAKI_GO=tabhints` shows the tabs' numbers with no key held.
  `EMAKI_GO="open:<id>;open:<id2>"` opens tabs in turn.
  `tabdrag:<id>:<x>` drags that tab to an x from the row's left edge,
  `pilldrag:<folder>:<x>` a pill. A step before the first index finds no
  pills, so put an `x` step first. `tabslow` before them draws the slide
  forty times slower, and a picture a second after the drag catches it
  partway. `EMAKI_SHOT_AFTER` counts from later than the steps do: 6 lands a
  second after a step at 10.
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
