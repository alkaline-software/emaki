# Panels beside a conversation

About `crates/emaki-app/src/panels.rs` and `file_icons.rs`, and
`crates/emaki-core/src/outline.rs` and `git.rs`.

## Rules

- One panel shows at most, at the conversation's left, and both have one
  width (`panel_w`), so one takes the other's place without moving the
  conversation. Tried and dropped: both at the right, side by side.
- The tree is not `files.rs`. That is git's flat list for "@", without
  what is ignored; the tree shows what is on disk.
- Nothing here deletes or forces without being asked first. Move to Trash
  is the system's trash, a refused `git switch` is reported in git's own
  words, a paste never writes over a file, and Discard in the comparison
  asks on a card of its own before it does anything.
- A change of branch is all or nothing (`git::switch_with`): either the
  branch is changed and every change is where it was asked to go, or
  nothing has moved and the reason is shown. No path leaves a conflict
  behind and none throws a change away. A bring that did not fit once left
  five files with conflict marks on `main` and a stash to recover from.
- Changes are brought to a branch only when git has said they fit there
  (`git::misfits`: `stash create` and `merge-tree --write-tree`, which
  touch no file). Do not replace the check with trying it and looking.
- A refused switch is said on a dialog (`BranchAsk::stop`), not only on
  the row under the composer, which a sheet can cover.
- A call to a remote may never wait on a question (`git::net`): no
  prompt in a terminal, no window of a credential helper's, a time
  limit. Nobody is at one, and the list of branches would hang. A
  remote that refuses comes back as words, and `git::why_net` says
  whether to check the network or to sign in.
- Nothing is pushed, published or merged without a click on a button
  that says so. A fetch is the one call to the remote made unasked: it
  changes what is known of the remote and nothing checked out. The
  default branch is brought up to the remote's only by a fast-forward
  (`git::fast_forward`); where that takes a merge it is left alone.
- Nothing here signs in. The sign-in sheet gives commands to paste, as
  the agents page does, and "Check again" asks the remote.
- A branch switch is refused while a turn runs in the session showing: the
  agent is writing the files a switch would change.
- Nothing is staged or committed from the comparison. The one thing it
  changes is Discard, on the file showing (`ask_discard`, `discard_now`,
  `git::discard`): a file the last commit has goes back to what that
  commit has, staged or not, after what it held is written into git's
  object store, where it stays until git prunes; a file the commit does
  not have is unstaged and put in the system's trash. The question's
  card says which of the two it is. What was typed in the file here and
  not saved goes with it.
- Dim in the tree means ignored by git, nothing else. A name beginning
  with a dot is not dimmed.
- Labels are asked only for outline rows in or near the view, once the
  scroller has rested, and never while the outline is hidden. Each child
  is a real Claude Code call on the person's account. Tried and dropped:
  asking about every message of the conversation at once.
- The summarizer's children run with thinking off
  (`MAX_THINKING_TOKENS=0`). With it on, nineteen labels in one child took
  24.6 s and a batch of twenty-four did not finish in thirty; with it off
  and six to a child, 19 labels took 8.4 s, 29 took 4.2 s, 52 took 5.6 s.
- An outline row's place is its laid-out bounds plus the scroller's
  offset: `ScrollHandle::bounds_for_item` answers before the offset is
  taken. Without the offset, an outline scrolled down asks about the rows
  at its top and leaves the ones in sight unlabelled for good.
- The wheel over the outline scrolls the outline and nothing else; a
  click chooses an entry. Tried and dropped: the wheel moving the mark an
  entry at a time with the conversation following.
- Go to the outline's last entry by the foot (`scroll_to_bottom`), not by
  the row's place: the place comes from the layout before the row was
  there, or before its label took a second line, and leaves it cut.
- At its end the conversation list says its top is past the last round
  (`logical_scroll_top` is the item count, `bounds_for_item` answers for
  nothing), so "the top is the round chosen" never holds there. That is
  what `outline_pick_end` is for.
- The wrapper `panel_in` puts around a panel is there at rest too, under
  the name of the last press. gpui keeps an animation by the names above
  it, so a wrapper that went when the arrival was over began every
  animation inside the panel again: the outline blinked 200 ms after it
  opened.
- A label fades in once, for `LABEL_FADE` after it lands
  (`outline_fresh` keeps when). Kept as a set with no time, every label
  made since launch faded in again at each opening of the outline.
- A child's reply is labels only when every line of it is "n: label"
  (`parse_summaries`); anything else in it and the whole reply is
  dropped. A child that answered a message with a numbered list once gave
  the outline "Copy or clone your project files into the working
  directory, or" as a label.
- A change to `SUMMARY_PROMPT` raises `SUMMARY_VERSION`, which is part of
  a label's name in the cache, so labels made by the old wording are
  asked for again as they come into view.
- Every scroller in a panel, card or sheet is a pane of its own to
  `route_scroll` (`Pane::Branches`, `ChangeFiles`, `ChangeLines`, and
  `Nowhere` for the rest while a card is up), or momentum from it scrolls
  what lies beneath.

## The control and the edge

The panel is chosen with one control of two segments at the top strip's
left end (`panel_buttons`): a press on the other segment swaps the panel
and slides the plate, a press on the one showing puts the panel away
(`toggle_panel`, ⌘⇧E and ⌘⇧O). `panel_anim` keeps what showed before the
press and what after. The segments are a fixed size, so the plate's place
is known without measuring, unlike `segmented`. A panel that comes beside
nothing widens and fades in; one that takes the other's place fades in
while the other fades out over the same place, out of the row's layout;
one put away narrows and fades out. A panel that goes is drawn for that
moment though it is no longer chosen (`panel_leaving`), with a timer's
redraw to drop it.
While it goes the outline neither scrolls itself nor asks for labels. The
choice and the width are in `ui.json`.

The edge is dragged as the sidebar's is: `render_panel_grip` holds it at
a press (`panel_drag`), and the raw mouse-move listener that follows the
sidebar's edge sets the width (`panel_drag_to`) between `PANEL_MIN` and
`PANEL_MAX`, never leaving the conversation less than its least
(`Workbench::conv_min`: what the composer's row of pills and its send
button take, measured where they are drawn from the room left between
the pills, and never under `CONVERSATION_MIN`; an agent with longer
names on its pills has a wider least). Narrower than `PANEL_FOLD_AT` the panel is put away,
and comes back in the same drag if the pointer does, keeping the width it
had. A double click is `panel_fit`: for the files the widest row showing,
capped at `PANEL_FIT_MAX`; for the outline a fixed `OUTLINE_FIT`, since
its lines are cut at any width.

## A narrow window

The window is never dragged narrower than the conversation at its least
with one pane beside it (`main::MIN_W`, the conversation's
`CONVERSATION_MIN` and the terminal's least, the wider of the two
panes). So the files, the outline or the terminal, asked for, always
has room, and its button never lights over nothing.

Under the width at which the panel at the left and the terminal both
fit (`room_for_both`), there is one of them at a time: asking for one
puts the other away, in motion (`toggle_panel`, `term_go`). A window
dragged narrower with both showing puts the terminal away, always the
terminal, so it is known beforehand which goes (`Workbench::render`,
through `term_go` so its button goes out with it). Either way it is put away for
good, not kept for a wider window: what the buttons say is what is
there. The sidebar has given its room up before any of this.

- What puts a pane away goes by `conv_need`, the conversation's need as
  last measured with room to spare, never by `conv_min`. `conv_min` is
  raised whenever the composer's row is squeezed, which it is for a
  moment while the sidebar and a pane are both in motion, and a pane
  put away on that reading stayed away: which of the two went came out
  differently from one time to the next.
- The room of a terminal put away for the window's width still counts
  against the sidebar (`term_folded`), until the window is `FOLD_SLACK`
  wider than where both fit, or a panel's button is pressed. Without
  it, a window dragged narrower lost the sidebar, then the terminal,
  then got the sidebar back in the terminal's room, then lost it again.
- A terminal put away takes the keyboard to the window with it, whoever
  put it away (`Workbench::render`). Left in a terminal not drawn, no
  key binding was heard until a click.

The file shown is the one thing still not drawn for want of room
(`fold_file`), with a line under the composer saying so.

Tried and dropped: a pane with no room drawn over the conversation. It
covered the composer, and two of them lay on each other.

## The files tree

`Tree` is the session's folder. A folder is read from disk when it is
opened (`list_dir`: folders first, then by name, `.git` and `.DS_Store`
left out, `DIR_MAX` entries and then "N more"). What is open is kept by
absolute path, so two sessions of one folder share it. The root and every
open folder are read again every `TREE_SECS` while the panel shows, which
is how a file the agent writes appears. Rows follow VS Code's explorer.

**The head row.** The session's folder is the tree's first row, over the
scroller so it stays in view, open to begin with, and a click closes
or opens it (`Tree::shut`, by folder, not kept between launches); what
is in it stands one step in. Something new made in a closed tree, or
pasted into it, opens it. At its right are New File, New Folder
and Collapse All Folders (`tree_new`, `tree_collapse`). The two new ones
go in the folder chosen, the folder the chosen file is in, or the
session's folder with nothing chosen. A click on the head row also
lets go of what was chosen, and its right click is the empty room's
menu. Collapse All Folders closes the folders under it and leaves it
open.

**Motion.** A row that comes or goes does so over `TREE_ANIM`
(`Tree::seen_now`, as the sidebar's `side_rows_changed`): the rows about
to be drawn are set against the rows last drawn, by path. One that was
not there grows in; one that is no longer is drawn a moment more where
it stood, shrinking (`TreeGhost`). That one rule is a folder opened or
closed, the head row closed, Collapse All, a file made, renamed, put in
the trash or written by the agent, and the row a name is typed in. A
folder drawn for the first time plays nothing, and the name's row gives
its place to what it named with neither moving.

**Choosing rows.** One row is the file showing (`Tree::picked`), or
the one last right-clicked, which is lit while its menu is up and after.
Several are chosen together (`Tree::marked`) by ⌘A, which takes every
row showing, or by a rectangle dragged over them from a press anywhere
in the panel (`tree_band_to`). Every row is one height and the list
begins at the scroller's top, so the rows under the rectangle are found
by number, with nothing measured. A press that has not moved a few
points is still a click, and the click that ends a drag chooses nothing
(`tree_band_live`). A right click on one of several is their menu
(`files_menu`: Add to Message, the paths a line each, the trash); on any
other row it chooses that row alone. A folder chosen with what is in it
is asked of once. Rows lit one under another are drawn as one block,
round at its two ends: a round plate a row read as stripes.

**Icons** are Catppuccin's (`file_icons.rs`). `assets/catppuccin/` holds
the VS Code icon theme's icons (`catppuccin.catppuccin-vsc-icons` 1.26.0,
MIT, copied from the installed extension's `dist/`), Latte for a light
window and Mocha for a dark one, and `theme.json`, the theme's table cut
down to what is used. Lookup: a whole file name, then the extensions from
the longest, a folder by its name, open or closed. Built in with
`rust-embed`, served under `catppuccin/`. An icon has its own colours, so
it is an `img` and not the one-colour `Icon`. A newer release of the
theme is the same copy again.

**Git marks** are `emaki_core::git`: one `git status --porcelain=v1 -z
--untracked-files=all --ignored=matching` for the repository the folder
is in, read off the main thread on the tree's clock (`read_git`, and at
the first draw of a folder not asked about yet), into a state per path.

- Which git (`git::binary`, found once): the first on `PATH` that
  answers `--version`, then Homebrew's and the system's places, or Git
  for Windows' own. The app opened from the Finder has the system's
  `PATH`, with no Homebrew on it, and the Mac's `/usr/bin/git` fails
  every call without Xcode's tools and their licence; with plain `git`
  such a machine had no branch, no marks and no changes in a real
  repository, and nothing said why. Every call goes through
  `git::command`, `files.rs`'s too. A search that finds none is made
  again a few seconds on, so a licence agreed to in a terminal is seen
  without a relaunch.
- A checkout git says nothing of says why (`git::trouble`, the strip
  under the panel's head, `git_trouble_strip`): a folder with a `.git`
  in it or above it, and no git that runs (none installed, the Xcode
  licence) or a git that refuses it (someone else's folder, a damaged
  repository). The words are git's or the system's own, whole, since
  they say what to do. A folder that is no checkout by its files, or
  one its repository ignores, gets no strip.
- The strip offers a command to copy for one trouble only, the Xcode
  licence, and only when it is that for certain (`git::xcode_licence`:
  a Mac, and Apple's message naming `xcodebuild -license`). Its words
  are then ours and one line (`Trouble::licence`), since Apple's three
  lines spell out the command the box already holds. Every other
  trouble gets its own words and no advice of ours: a guess at the cause
  would send someone to run `sudo` for nothing.
- Which repository, as VS Code's explorer has it (`git::inside`): the
  one the folder is in, when the folder is its top or a tracked part of
  it. A folder that repository ignores (`target/x` under a checkout) is
  in none: no branch, no marks, no count. A folder that only holds
  repositories is in none either, and nothing is said of its children's.
- A folder part way down a repository is asked about by itself (`git
  status -- .`): the marks, the count and the comparison are of what is
  under it. The branch is the repository's, and so is the question a
  switch asks: `Status::dirty` says whether anything in the repository
  is uncommitted, in the folder or outside it.
- A file wears its letter (M, U, A, D, R, T, "!" for a conflict). A
  folder wears a dot for the most pressing thing under it: a conflict,
  then what is new or gone, then what is changed. Ignored is dimmed with
  no mark.
- Git names an untracked or ignored folder once and not what is in it, so
  a path under one takes its state.
- Paths are spelled as the folder is, not as git resolves it (a temp
  folder on macOS is behind a symlink).
- Colours (`git_rgb`) are the GitHub VS Code theme's, light or dark with
  the window, and VS Code's defaults for the three that theme leaves out
  (staged and changed, staged and deleted, renamed).

## Branches

`branch_pill`, straight after "Files" in the head, names the branch
checked out: the commit when the head is detached, nothing outside a
repository. It is read with the status (`git::branches`). A click opens
`render_branch_menu`, laid out as GitHub's list is: a field that narrows
the list, the local branches with the default first (what `origin/HEAD`
names, else `main` or `master`) and a tick on the one checked out, then
the branches only a remote has. No tags.

Each row is the name, a column for its one tag, and the time, so the
tags start under one another: "default", "remote" for a branch only the
remote has, and "local", in the accent, for a branch made here that no
remote has (`Branches::unpublished`). A tag says what a branch is and is
never a button; beside "local" an up arrow, in the accent too, sends the
branch to the remote and has it follow its copy there (`branch_publish`,
`git::publish`). Tried and dropped: a "publish" tag that was the button,
which named an action in a column of states. The branch checked out says the same on a strip under
the head (`publish_strip`).

**The remote.** "remote" means the remote had the branch at the last
fetch; nothing of it is checked out here. Opening the list fetches when
the last word is more than a minute old (`branch_fetch`), and the line at
the list's foot says when the remote last answered or why it did not. A
switch to a remote's branch fetches first, so what is checked out is what
the remote has now; when the remote cannot be asked the branch is checked
out as last fetched and the row under the composer says so.

**Making a branch** is one sheet (`render_branch_new`, `BranchNew`),
reached three ways: the New Branch button beside the field, words that
leave no branch (the list says it cannot find one and offers to make it)
or leave some and name none exactly (a row at the foot), and ⌘⇧N
anywhere. It asks the name (spaces become hyphens, as GitHub Desktop
has it) and what to start from: the default branch, chosen to begin
with, or the branch checked out. On the default branch there is nothing
to choose. With the default branch chosen the remote is asked how it
stands (`BaseSync`): behind, a box, ticked to begin with, has it brought
up first; with commits of its own not pushed, a button pushes them;
gone separate ways, it is left as it is and the sheet says why. A
remote that cannot be asked is said, and the branch is made from what is
here. The making is `git::create_from`, all or nothing as a switch is.

**Signing in** (`open_gate`, `render_git_gate`): when the remote refuses
who is asking, a sheet says what was refused in git's words and what to
type, by how the machine stands (`git::account`): GitHub's `gh` to
install, to sign in with, or to hand git its sign-in when it is signed
in already; a key to check for a remote reached over ssh; and where to
make an account. GitHub Desktop's own sign-in does not reach git
elsewhere, and the sheet says so.

The default branch is first in either order. After it the list is ordered
by when each branch was last committed to, the newest first (`Branches::by_recency`, from `Branches::when`, the committer date
`for-each-ref` gives; a branch here and on a remote takes its local
time), with that time at the row's right in words (`format::ago`), on
one line: the column holds the longest of them. Branches at one commit
have one time, as a branch just made from the default one and the one
last merged into it do; the one made later is first then
(`Branches::made`, the first line of the branch's reflog, the only
record git keeps of when a branch was made, and only for a branch
here). Where that does not say, the later name is first, a number in a
name read as a number. Two
words in the head, Recent and Name, change the order (`branch_by_name`,
kept in `ui.json`); by name it is the default branch, the rest by name,
then the branches only a remote has.

A click on a row, or ↩ in the field (the branch the words name, else the
first left, else the new one), is `branch_go`: `git switch` or `git
switch -c`, off the main thread. The row under the composer says what
came of it.

With changes not yet committed (`Tree::changed`), `branch_go` asks first
(`BranchAsk`, `render_branch_ask`), as GitHub Desktop does, and
`branch_run` does what was chosen through `git::switch_with`:

- **Leave** (`Carry::Leave`, the choice to begin with): `git stash push
  --include-untracked` under the name `!!GitHub_Desktop<branch>`, then
  the switch. That name is GitHub Desktop's own for the same thing, so
  each app sees what the other left. A detached head has no branch to
  leave them on, and the choice is greyed.
- **Bring** (`Carry::Bring`): offered only once `misfits` has answered
  with no file, which `branch_go` asks off the main thread while the
  question is up; until then, and when a file would conflict, the choice
  is greyed and names the files. A new branch starts from here, so the
  changes always fit. The switch itself is a plain one first, since git
  carries changes along when none is to a file the branches differ in;
  refused, it is stash, switch, pop. Should the pop still not fit,
  `switch_with` clears the half-applied files, goes back to the branch
  left and pops there, and answers with an error.

With a conflict already open (`State::Conflict` in `Tree::changed`,
`git::unmerged`) no switch is tried: git goes nowhere with one and a
stash cannot hold it, so the dialog says which files to resolve first.

`Branches::stashed` counts the sets left on the branch checked out. While
there is one, `stash_strip` under the head says so with a Restore button
(`branch_restore`, `git::restore`: `git stash pop` of the newest). Nothing
here drops a stash; discarding one is `git stash drop` in a terminal.
Restore, like a switch, is refused while a turn runs in the session
showing.

## Changes

`changes_pill` at the head's right counts the files with a change
(`Status::changed`, kept as `Tree::changed`) and opens the comparison
(`render_changes`, `Changes`), laid out as GitHub Desktop's Changes tab:
a sheet with the changed files at its left and the picked file's lines at
its right, the last commit beside what is there now. "Show changes" in a
changed file's menu opens it on that file.

- The file list is a `uniform_list`, since a commit of icons is a
  thousand rows.
- Files are chosen in it as rows are in the tree (`Changes::marked`,
  `change_click`, `changes_band_to`): a click is one, ⌘ and a click
  adds or takes one, ⇧ and a click reaches from the last one clicked,
  and a rectangle dragged over the list takes the rows under it. Rows
  chosen one under another are one lit block. A click on the list's
  empty room lets them all go (`changes_let_go`); the file whose lines
  show (`picked`) stays shown with no row lit, and Discard, Open and
  Reveal at the head are then asked of it.
- A right click is the menu (`changes_menu`): of one file, Open, Reveal,
  Add to Message, the two paths and Discard Changes; of several chosen
  together, what can be asked of them all. A file taken out has nothing
  to open. The choices are the tree's own (`MenuDo::File`, `Files`) but
  for Discard (`MenuDo::Discard`).
- Discard is asked of every file chosen (`ask_discard`, `discard_now`),
  the pill at the head saying how many, and the question says which of
  them go to the trash because the last commit has no such file.
- ⌘W closes the sheet (`close_nearest`), the question over it first.
- `git::diff` is `git diff HEAD` for the file, `--cached` before a first
  commit, and a file git does not track read as all new lines.
- `git::parse_diff` sets a run of lines taken out beside the lines put in
  after it, first with first. Lines within a pair are not compared word
  by word. A file all new or all gone has one side and the whole width.
- The lines are a gpui `list`, read off the main thread and again on the
  tree's clock while the sheet shows, and replaced only when they differ,
  so reading on does not move the reader. `DIFF_LINES` at most; a file
  that is not text says so.

## What the tree does, taken back and done again

⌘Z in the panel takes the last thing the tree did to the disk back, and
⌘⇧Z (Ctrl+Y too, off a Mac) does it again (`tree_undo`,
`emaki_core::fsops`). Everything the tree does is in it: a file or
folder made, a rename, a move, a copy (pasted, duplicated, dropped in
from the file manager), a removal.

- Each is a list of moves, a path before and a path after, and one of
  the two may be in the holding folder (`~/.emaki/state/held`). Taking
  it back is the moves the other way, last first; doing it again is the
  moves as they were. So nothing deletes anything: a file removed is
  moved to the holding folder, and a file made or copied in has a place
  there to go to when it is taken back.
- A file removed is therefore not in the system's trash at once. It
  goes there when ⌘Z can no longer bring it back: when something new is
  done after a ⌘Z, when it is the hundred-and-first thing done, at
  Quit, and at the next launch for what a crash left. The Trash is not
  read back from: an app is not let into it, so a file put there could
  not be put back by ⌘Z at all.
- A move is a rename and nothing else, so a thing is never half
  copied. What is on another volume than the holding folder cannot be
  held: it goes to the trash at once, and the notice says so.
- A thing is taken back whole or not at all. When one of its moves
  cannot be made (a name since taken, a file since gone), those already
  made are put back, the thing is forgotten, and the reason is said.
- The window follows the files (`tree_follow`, `tree_forget`): the
  folders open under a path, the file showing, what was typed in one
  and not saved, the rows chosen and cut.
- A file's own edits are not here: ⌘Z in the file's pane is the
  editor's.

## Moving, and the tree's keys

- A row is dragged to move it, with the other rows chosen when it is
  one of them (`TreeDrag`, `tree_move`); ⌥ held as it is let go copies
  instead. Where it would go is lit and said as for files dragged in,
  below, and a closed folder held over opens. A row already in that
  folder stays, a folder is not moved into itself, and a name taken
  there is not written over.
- A press on a row is that row's click or drag (`tree_press_row`); the
  rectangle that chooses rows starts from the room around them.
- ⌘ and a click adds a row to those chosen or takes it out; ⇧ and a
  click chooses from the row last clicked to this one
  (`tree_click_more`, `tree_anchor`).
- The keys (`tree_key`): ⌘X cuts, and the paste that follows moves
  what was cut (drawn faint until then); ⌘D makes a copy beside the
  row; ⌘⌫ removes on a Mac, and Delete elsewhere; ↑ and ↓ go from row
  to row, → opens a folder or goes into it, ← closes it or goes up.
  Cut and Duplicate are on a row's menu, and on the menu of several.

## Files dragged in

Files dragged from the file manager over the tree are copied in where
they are let go (`tree_drop_over`, `tree_drop_done`, and `tree_put`,
which a paste and a duplicate go through too): into the folder under them, the folder
of the file under them, or the session's own over the room around the
rows. Nothing is written over; a name taken gets a free one.

- Where they would go is lit as it changes: the folder's row and, more
  faintly, the rows it holds, so the place reads as one; a line round
  the whole panel for the session's folder. At the panel's foot the
  same is said in words, coming up as the files arrive and fading as
  they leave.
- A closed folder held over for `TREE_SPRING` opens, so what is in it
  can be reached without letting go.
- Every row and the panel hear every move of a drag, in no promised
  order, so the panel yields a move a row has already taken
  (`tree_drop_probe`).
- A drag that leaves the window says nothing as it goes. What it lit is
  put out at the next draw with no drag about.
- It is a copy, never a move: the file manager's own file stays.

## The file's pane and the menu

A click on a file shows it in a pane between the tree and the
conversation (`render_file_pane`, `file_pane`), with a width of its own
(`file_w`, in `ui.json`) and an edge to drag (`render_file_grip`; a
double click puts the width back). Tried and dropped: a sheet over the
window, which read as something passing, and the pane in the terminal's
place at the right, which kept the two from showing together and stood
far from the tree.

- A click shows the file, and a click on the file showing puts it away
  (`file_clicked`). Opening it in the system's app is on the row's menu
  and in the pane's head. Tried and dropped: two clicks to open it
  there, which made every single click wait to see whether a second
  followed.
- The pane widens in and narrows out as the panels at its sides do
  (`file_anim`, `file_gone`), and another file takes the place of the
  one showing and fades in (`file_serial`). Its button and ⌘W close it.
  Escape does not: it closed the pane at first, and a press meant for
  something else put the file away. With the keyboard in the pane it
  lets go of what is selected there, as an editor's does
  (`file_unselect`): the editor's selection, words selected in a file
  as it reads, a block of a table's cells.
- The head's enlarge button gives the pane the conversation's room too
  (`file_enlarge`, `file_big`), and its next press or closing the file
  gives it back. The width runs from one to the other
  (`file_pane_w`), and on the way the conversation keeps the width it
  has beside the pane, cut by the pane's edge and fading
  (`beside_file`), so it is not laid out again a frame at a time. Under
  an enlarged pane the conversation and its composer are not drawn, and
  the focus is the pane's. The pane has no edge to drag then. It lasts
  for the window, not between launches. "Add to message" was in the
  head and was removed; it is on the tree's menu.
- It is the first to give room up: narrower before the terminal and the
  tree are (`file_pane_w`, and `file_pane_least` in their own widths),
  and in a window with no room for its least it is not drawn
  (`fold_file`), with a line under the composer saying so.
- What is shown (`FileBody`, `read_file`): markdown as the conversation
  draws it, and a YAML header it opens with as a table of its fields over
  the text (`files::front_matter`, `front_matter_table`; markdown itself
  reads a header as a rule and a heading of every field). The table is
  part of the same markdown text, since a selection does not run from
  one text view into another: a card of its own, drawn first, could not
  be selected or copied; other text in the editor (below), the first `PREVIEW_BYTES`
  and `PREVIEW_LINES` of it; a picture at its own size
  or the pane's width; a PDF's first `PDF_PAGES` pages as pictures; a
  `.csv` or `.tsv` as a grid (`files::table`, `render_table`), its first
  `TABLE_ROWS` rows, a column as wide as its longest cell up to a point;
  a Word, PowerPoint or Excel file as `emaki_core::office` reads it
  (below). Anything else, or a file that cannot be read, is a line
  saying so and "Open with default app".
- A Word document is written in (`word.rs`, `FileBody::Word`,
  `Workbench::word`). The pane is WordCraft's engine with a front of
  ours: its `Session` holds the document, the selection and the undo
  history, and every change is one of its commands run by id
  (`WordPane::run`), the same commands WordCraft's own window runs.
  Its layout says where the caret and the selection are, and its
  renderer draws a page.
  - It opens `.docx`, `.docm`, `.rtf` and `.odt` to write in, and the
    binary `.doc` and the templates to read (`word::opens`): the engine
    does not write those, so a change in one is refused and said.
  - A page is a picture drawn on the main thread, only when it is in or
    near view and the document or the width has changed
    (`draw_pages`). It is given to the toolkit as pixels
    (`RenderImage`), not as a PNG: a PNG is read over later frames, and
    the page was blank until it was. The picture it replaces is let go
    with `drop_image`, or the toolkit keeps it. While the pane's width
    is moving the picture there is stretched, and drawn again when the
    width has stood still.
  - The caret and the selection are drawn over the picture and are not
    in it, so moving the caret draws no page.
  - Text comes through the toolkit's input handler
    (`EntityInputHandler`), which is what an input method needs. What
    it is still composing stands in the document; before each change
    of it the document goes back to what it was (`EditSnapshot`), so
    the composing letters leave no step to undo.
  - A key is the engine's own key for a command (`KeyMap`), with two
    exceptions. On a Mac ⌘ and ⌥ with an arrow move as a Mac moves,
    since the engine's keys are Windows'. And ⌘B, ⌘L, ⌘E and ⌘R are
    bound in the pane's own context (`WORD_CONTEXT`), since the window
    binds them: a binding in a deeper context wins, and a key listener
    never runs before a binding. ⌘S is the window's and saves it
    (`save_word`).
  - The strip over the pages is the Home tab's most used: undo, the
    style, the size, bold to strikethrough, alignment, lists, indent.
    The rest of the engine's commands have no button; those with a key
    run by it. A command that opens one of WordCraft's dialogs is
    passed over: there is no dialog here.
  - Saving writes the document as WordCraft writes it. What it does not
    model in a file made by Word may not come back out.
  - With changes not saved it is in `unsaved`, so closing, quitting and
    showing another file ask as they do for the editor. The file
    changing on the disk reads it again only when nothing is unsaved.
- A PowerPoint or Excel file is shown and nothing more. It has no
  source, so no second way to show and nothing to save. The binary
  `.ppt` is not read.
  - A `.pptx` is its slides as pictures, drawn by DeckCraft
    (`pptx_pages`). It is a PDF to the pane (`FileBody::Pages`,
    `drawn_pages`): the same pages, the small ones beside them, the
    same fit to the pane's pixels. A slide's words are not kept yet.
  - An `.xlsx` or `.xlsm` is its sheets in the grid (`FileBody::Sheets`),
    their names in a row over it when there are several (`sheet_at`),
    each cell as the sheet shows it: its number format applied, by
    GridCraft (`office::shown_sheets`). A formula's cell is the result
    the file kept. calamine reads an `.xls`, and a zip GridCraft
    refuses, as values. Not shown: bold, fills, widths, merged cells.
  - `office::read` still reads a `.docx` and a `.pptx` as markdown; the
    pane no longer asks it to.
- A Parquet file is a table (`emaki_core::parquet_file`, Apache's
  reader by rows, without Arrow): its columns' names as the first row,
  as a comma-separated file has them, and its first `TABLE_ROWS` rows.
  What each column holds is over it, where the letters are for other
  tables (`FileView::kinds`), and the row count under the grid is the
  file's own, from its foot (`rows_all`). Read only.
- The grid (`render_table`) has columns named by letter and rows by
  number, both staying in view (`render_table_heads`). The heads are
  not in the scroller: the letters are a strip over it and the numbers
  a strip at its left, each cutting what it holds, and what it holds is
  moved along the strip by the scroll the cells have that way
  (`file_view_scroll.offset()`, the pane drawn again at every wheel).
  So the letters cannot move up or down and the numbers cannot move
  sideways. Tried and dropped: both laid over the cells inside the
  scroller and moved against both scrolls, where they shook and stood
  off their place.
- Every other row is a shade darker. One cell is chosen and outlined
  (`table_at`); with `TableAt::to` the block from it to another is
  chosen and washed, and its rows' and columns' heads are lit
  (`table_range`). A press chooses a cell; a drag over the cells
  (`table_drag`, ended at a release anywhere), a ⇧ press or ⇧ and an
  arrow takes the choice to another (`table_extend`, `table_step`).
  The arrows and Tab move. ⌘C copies what is chosen, a tab between
  cells and a line a row (`table_range_text`), and Delete empties it
  (`table_clear`, a step of undo a cell).
- A cell of a table the pane holds whole is written in the grid: a
  second press, ↩ or a letter opens a field in the cell's place
  (`table_edit`, `table_input`), ↩ keeps it and goes down a row, Escape
  leaves it. A letter takes the place of what the cell held; a second
  press keeps it and puts the caret where the press was
  (`table_caret`, the words measured as the field sets them). The
  field is the toolkit's input at a size of its own, so its words stand
  where the cell's did, and its outline is laid over it, not set around
  it as a border.
  - The second press says the press is taken (`prevent_default`). The
    pane takes the keyboard at every press in it that nobody claims,
    and took it from the field as it opened: the field was up and a
    third press was needed to write in it.
  - What is written goes into the file's text in that cell's place and
    nowhere else (`table_write`, `files::cell_at`, `cell_written`): the
    rest of the file keeps its quotes and its line ends. Do not write
    the table out again from its cells; that requotes every cell and
    the comparison with git shows the whole file changed. A cell keeps
    its quotes if it had them and gets them when it needs them; a row
    too short for the column is made long enough.
  - The text is the editor's. A table has its editor whichever way it
    shows (`file_editor_want`), so one cell is one step of its undo
    (⌘Z and ⇧⌘Z in the grid are the editor's, by the vendored
    `undo_step`), the dot and Save are as for any file, and what is not
    saved is kept in the drafts. The grid is read again from the
    editor's text after each (`table_refresh`).
  - A workbook's sheet, or a table shown in part, has no editor and
    says so when a cell is opened.
- What is wider than the pane (a grid) keeps its own width in the
  scroller (`items_start`). Stretched across it, as a column's child
  is, it measured as wide as the pane and never scrolled sideways.
- The file shown is the folder's, not the window's (`sync_file_root`,
  `file_root`, `file_for`): going to a session of another folder puts
  it out of sight, and coming back to any session of this folder shows
  it again. Kept as one file for the window, it stayed up over another
  project's conversation.
- Code, and markdown or a table as it is written, are drawn by the
  toolkit's code editor (`sync_file_editor`, `FileEditor`): line
  numbers, folding, the language's colours, and its own selection, copy
  and find (⌘F). The editor is made again when the file, or
  what it holds, is another, and scrolls by itself, so the pane's own
  scroller is not drawn around it. Its right click is Copy and Select
  All and no more, each with its icon (the vendored `on_secondary_click`): what is asked of
  the file is on its row's menu in the tree, and stood here too until
  it was one list said twice. With nothing selected, the right
  click selects the word under it first (the vendored `select_word_at`),
  as one in a conversation does, so the menu has Copy for it. A folded line ends in dots, as
  VS Code's does (the vendored element).
- A file the pane holds whole can be changed in that editor and saved
  with ⌘S, File, Save File, or the Save button the head shows while
  there is something to save (`save_file`).
  - What may be edited is `FileView::source`: the file's text exactly,
    kept when all of it was read and all of it is UTF-8. The text the
    pane shows is not it (`Code` is lines joined, with no last line
    break). A file cut for the preview, or with bytes that are no text,
    has no source and its editor is read only.
  - A `.csv` or `.tsv` has the two segments markdown has: the grid,
    and the file as written. It is edited in either.
  - The save writes the file in place, so it keeps its permissions,
    then reads it again; the editor is kept (`FileEditor::key` is set
    to the new file's) so the caret and the undo history stay.
  - The agent writes files too. A dirty editor is never replaced by
    what the disk now holds, and a save that finds the file changed
    since it was read says so under the composer and writes nothing; a
    second save writes over it (`file_conflict`).
  - A file with changes not saved shows an orange dot where its close
    button is, as an editor's tab does, and the cross again under the
    pointer; Save stands where its size was. Nothing takes the file
    away without asking (`file_guard`, `render_file_ask`): closing the pane, showing another file in its
    place, closing the window (its button, ⌘W, the menu) and quitting
    (⌘Q, the menu) each bring up the question an editor asks, in the
    window's own card: Save, Don't Save, Cancel; ↩ saves and Escape
    cancels. The card sits over the file's pane and is no wider than
    it, since the question is that pane's; for a file not showing it is
    in the middle of the window. Quitting and closing the window ask of
    every such file, one after another, showing or not. The answer is followed by what
    was being done (`FileThen`).
  - Going to a session of another folder, or to markdown as it reads,
    asks nothing: the file is still open, only out of sight, and its
    dot is there when it is back.
  - An editor that goes out of sight is kept whole (`file_parked`), so
    what was typed is there when the file shows again and ⌘Z still
    walks back through it. A few with nothing to save are kept too,
    for their histories; one whose file has changed on disk since is
    made again.
  - What shows two ways reads, as it reads, the way its editor has it
    while that has changes not saved (`edited_body`), not the way the
    disk does. A file renamed keeps what was typed in it under the new
    name; one put in the trash takes it along.
  - What is not saved is also written to `state/file_drafts.json` on
    the clock (`keep_drafts`, a `Draft`: the text and the time on the
    file it was typed over, so a save still knows a file changed
    since) and read at launch. The app can be ended with no moment to
    ask (`scripts/relaunch.sh`, a crash, the system shutting down), and
    nothing typed is lost to that; the undo history is.
  - A new line starts where the language says (`emaki_core::indent`,
    asked through the vendored `set_next_line_indent`): a step in after
    Python's or YAML's `:` and after an opening bracket, a step out
    after Python's `return`, `pass`, `break`, `continue`, `raise`, and
    in R a step in after a pipe or an operator left open, once for the
    chain. It reads the line's words and parses nothing, since it
    answers at a keystroke in a file that does not parse yet. One step
    is the file's own (`indent::unit`: the smallest indent its lines
    show, a tab if they use tabs), else the language's habit.
  - Saving a file formats it first (`emaki_core::format`) when Settings,
    Files has the switch on (`app.format_saved_files`, off to begin with;
    the older `format_on_save` in a file is not read, having been R's
    alone and on)
    and the file's language is ticked there (`app.format_langs`, all
    ten to begin with): the editor takes the formatted text as one step
    of undo, then the file does. A file its formatter cannot parse is
    saved as typed and the row says it was not formatted.
  - No formatter is written here: each is the language's own, at its
    defaults. R is Air's, Posit's formatter, built in. TypeScript,
    JavaScript, JSON, Markdown and TOML are dprint's plugins, built in
    as the libraries they are. Python, Rust, Go and C and C++ have a
    formatter that is a program and not a library (`ruff format`,
    `rustfmt`, `gofmt`, `clang-format`): it is run with the text on its
    standard input when the machine has it. Settings names each
    language's formatter after it, and says so when one is not installed.
  - Markdown is formatted in `.md` files only. Quarto's and R
    Markdown's are markdown to the editor and more than it, and a
    formatter that does not know their blocks would rewrite them.
  - Settings lists the ten in two lists that are always there, dimmed
    and not to be ticked while the switch is off (`format::LANGS`; a
    language's icon is its file's in the tree).
  - Air and the Biome crates under it are built without
    debug assertions in the dev profile too (`Cargo.toml`): with them a
    6,700-line file took half a second to format, without them 27 ms.
- The editor's margin marks what differs from git's copy of the file,
  as VS Code's does (`sync_file_marks`, the vendored `GutterMark`): a
  green bar beside lines git does not have, a blue one beside lines
  that differ, a red wedge where lines were taken out.
  - They compare what the editor holds, saved or not, so a line is
    marked as it is typed, with the index's copy (`git::base_text`:
    `git show :./name`), which is the last commit's until something is
    staged. A file git does not track has no marks.
  - `git::line_marks` is `similar`'s line diff. A run that stands where
    a shorter one stood is as many changed lines as there were, and the
    rest new; marked "changed" whole, one line edited over two put in
    read as three changed.
  - Worked out off the main thread, one at a time, when the text
    changes (the editor's `Change`, not its every notify: setting the
    marks notifies too) or git's copy does, which is read again on the
    tree's clock and after a save.
  - The same marks run small down a strip at the editor's right edge:
    where in the whole file the changes are, by row as wrapped, with
    the cursor's line a thin grey line across. A file shorter than the
    view is not stretched to fill it. The editor's scrollbar runs in
    that strip and is as wide as it, over the marks, as VS Code's is.
    It is the one scrollbar in the window that is not the usual width;
    its fade is the same.
  - In the margin the marks have a lane of their own between the line
    numbers and the fold marks. The fold marks are plain chevrons with
    no plate under the pointer, shown while the pointer is in the
    margin and fading in and out, and a click on one opens or closes
    the fold in motion: the lines under it slide to where they are
    going. All of it is in the vendored editor (`UPSTREAM.md`, 20).
  - A click on a mark opens that change on a card at the click
    (`open_file_peek`, `render_file_peek`): git's lines in red over the
    lines there now in green, and Revert, which puts git's lines back
    as one step of undo (`git::lines_of` cuts both out whole). VS Code
    opens it inside the text; the toolkit's editor has no room between
    two lines to open, so it is a card over the pane. A mark's `id` is
    its place among `FileEditor::hunks`, and a click while the marks
    are behind the text opens nothing.
- What shows two ways has two segments in the head, as it reads and as
  it is written (`two_ways`, `file_raw`, one choice for all of them).
  Every launch begins with files as they read: the choice lasts for the
  launch and is not in `ui.json`, where a source looked at once was what
  every file opened as from then on. They are: markdown; HTML
  (`FileBody::Html`, below); an SVG, which is
  a picture with a source; a table; a Jupyter notebook
  (`FileBody::Notebook`, its cells as markdown by
  `files::notebook_markdown`, its source the JSON). The written side is
  the editor, so each is edited there.
- An SVG the pane holds whole is drawn from its text and not by its
  path. gpui keeps a picture read by path under that path, so one saved
  here, or rewritten by the agent, went on showing as it was.
- The colours are the toolkit's tree-sitter grammars
  (`tree-sitter-languages` on `gpui-component`, off until v0.1.8, which
  also colours a conversation's code blocks). `render_md::lang_for_path`
  names a file's language and `editor_language` is the toolkit's name
  for it. R is not in the toolkit's set: `look::install_languages`
  registers the `tree-sitter-r` crate's grammar and queries.
- A file has its colours at its first draw. The toolkit parses in the
  editor's own thread for a moment and, past that, in the background
  after a wait, both sized for a keystroke; a text with no tree yet (a
  file just opened) is given 60 ms in the vendored `input_adapter.rs`,
  which a file of some tens of kilobytes takes a quarter of, and its
  background parse starts with no wait. A file past 256 kB is parsed in
  the background from the start and is still one colour for a moment.
- A `.csv` or `.tsv` as written has each column in a colour of its own,
  as the Rainbow CSV extension does: the vendored `delimited.rs`, a
  highlighter with no grammar behind it for the languages `csv` and
  `tsv`, which gives the columns ten of the theme's syntax colours in
  turn. A quoted cell keeps its column across the separators and line
  breaks inside it.
- The palette is the window's own inks (`docs/look.md`, Code's
  colours), named in the `highlight` part of each theme in
  `themes/emaki.json`. Without one the toolkit keeps its light palette
  in a dark window. The
  names it reads are a fixed list (`SyntaxColors`); note `comment_doc`,
  with an underscore.
- Markdown has two segments in the pane's head, as the files and the
  outline have in the strip: as it reads, which is how it opens, and as
  it is written, in the editor (`file_raw`, `set_file_raw`).
- A PDF's page is a picture, so its text is kept beside it: each glyph
  with its place on the page, collected by the same interpreter that
  draws it (`PdfText`, `PdfGlyph`), in the order the file draws them,
  with a space or a line break where the page leaves room for one. A
  selection is glyphs from and to (`pdf_sel`), drawn as bands over the
  picture by a `canvas` that also keeps where each page is
  (`pdf_bounds`). A drag selects by the letter, a second click the
  word, a third the line, ⇧ and a click reaches from the selection; ⌘C
  copies, ⌘A selects all, and a right click offers both (`PdfDo`).
  The pane has a focus of its own (`file_focus`) for those keys.
- ⌘F with the keyboard in a PDF opens that file's own find row
  (`pdf_find_open`, tried first in `open_find`): every place marked,
  the one it is on darker and brought into view, ↩ and ⇧↩ to step,
  Escape to close. Escape then drops a selection.
- A PDF's head has two segments for what stands beside its pages
  (`pdf_side`, `pdf_side_toggle`, kept in `ui.json`): the pages small,
  or the file's table of contents, and neither at a second press on the
  one showing. A click goes to the page (`pdf_go_page`), the page in
  view is marked in either list, and the head says "page n of m"
  (`pdf_page_now`, from where the pages were last drawn; the pane draws
  again at a wheel so it keeps up). The small pages are drawn small by
  the renderer (`PdfPage::thumb`): the large picture scaled down by the
  GPU is jagged.
- The table of contents is read out of the file (`pdf_contents`): the
  tree under `/Outlines`, each heading with the page its destination
  names. A destination is an array beginning with the page, or a name
  for one in the catalog's `/Dests` or the name tree under `/Names`,
  which is how LaTeX writes them. hayro has no reader for this.
- A right click in the pane opens the window's menu (`file_pane_menu`),
  the editor's own apart, with what the place offers and nothing else:
  Copy for selected words in markdown, HTML, a notebook or a document as
  it reads; Copy Image on a picture; Copy Cell and Copy Row on a table's
  cell; Copy and Select All on a PDF's page. Where the place offers
  nothing there is no menu. What is asked of the file itself (Add to
  Message, Open, Reveal, its paths) is on its row in the tree and in the
  pane's head; it stood under these too, and read as being about the
  words.
- Not done in a PDF: links, and text set in a font that says nothing of
  its characters, which is drawn and cannot be selected.
- A PDF is drawn by hayro, which is all Rust, so the same on every OS,
  off the main thread (`pdf_pages`), each page to a PNG the window
  draws. A page is drawn as wide in pixels as the pane shows it, and
  drawn again when that width has changed and stood still a moment
  (`pdf_fit`, `pdf_want`), the old pictures showing until the new are
  in. In is not when they are made: the toolkit reads a PNG over the
  frames after, and a page is blank until its own is read. So the old
  pictures stay under the new for a moment (`pdf_under`), and are then
  let go with `remove_asset`, or the toolkit keeps them. At one fixed width a page was scaled on its way to the screen and
  its letters were soft. PdfCraft was looked at for this and draws its
  pages with hayro too.
- HTML as it reads is the system's own web view (WKWebView, WebView2)
  set over the pane's room as a child of the window (`html_web.rs`,
  `html_web_sync`): the page as a browser has it, scripts run. A page
  that lays itself out by script is nothing without one, and Blitz,
  which drew it first, runs none.
  - The view is not gpui's and is drawn over whatever gpui draws where
    it stands, so it is put out of sight while the window draws over
    its pages (`over_page`: a sheet, a menu, a list, the sidebar
    floated). A new thing drawn over the pages goes in `over_page`, or
    the page shows through it. A tooltip under the pane's head is behind
    the page.
  - The page's files come through a scheme of ours and not `file:`, so
    it reads the session's folder and nothing else on the disk. What it
    asks of the network it gets, as in a browser. A link out of its own
    files opens in the system's browser, and the pane stays on the file.
  - It shows the file as saved, not as the editor has it, and loads it
    again when the file changes on the disk.
  - A click in the page gives it the keyboard, and the window's
    shortcuts that are not in the menu bar wait for a click outside it.
    The keyboard has to be taken back, since AppKit does not give it
    back: `sys::keys_to_window` makes the window's own view the first
    responder again at a press on anything the window draws, when the
    view is put out of sight, and when it is let go. Without it the
    keyboard stayed on a view that was hidden or gone: no letter
    reached the composer and an input method's candidates stood at the
    screen's corner, while Return and Delete, which AppKit offers to
    every view first, still worked.
  - It is put where the pane's room is as that room is laid out
    (`Web::placer`, a canvas over the room), in the same frame, so it
    does not trail the pane. The page's own drawing at a new size is
    the web view's, in another process, and comes a moment after.
  - Its lower corners are cut round where they are the card's
    (`Web::round`): it is drawn over the card's own round corners.
  - It is let go with the file.
  - On Linux no web view can be set in a gpui window (WebKitGTK wants a
    GTK one), and the page is a picture drawn by Blitz
    (`html_shot::shoot`, `html_fit`): its style sheets, no script,
    nothing fetched, light on white, its links to the web places over
    the picture, and before the first drawing the toolkit's text view.
    Blitz is built there only. The pane says "Drawing the pages…" until they are in, and a
  file the renderer stops on is said to be one it could not draw.
- The file is read again when it changes on disk (`tick_files`).
- The pane's scroller is a pane of its own to `route_scroll`
  (`Pane::File`).

The file clicked stays marked (`Tree::picked`) until another is, or until
a click on the panel's empty room. A row's click goes through
`swallow_click`, so the room's own click handler does not hear it.

A row under the pointer takes `row_hover`, in both panels: the theme's
muted grey in a dark window, a wash of the ink in a light one, where the
muted grey is all but the sidebar's own ground.

A right click is the window's own menu (`MenuDo::File`, `file_do`). Add
to message puts the path as "@path" at the end of the composer. New file
and New folder are on a folder or the panel's empty room, and the empty
room's menu has Collapse All Folders.

A name is typed in the tree, in the row it is for (`file_prompt`,
`tree_input`, `commit_file_prompt`), as VS Code's explorer has it. Tried
and dropped: the sheet a session is renamed in, far from the row.

- A rename's field stands where the name did, with the name selected up
  to its extension. A new folder's row is the first in its folder and a
  new file's the first after the folders there, where the sorted list
  will have them (`Row::Edit`); the folder is opened for it.
- The row's icon is the one the name typed so far would get, at every
  letter.
- ↩ makes it and Escape leaves it. Leaving the field for somewhere else
  keeps a name typed, and the keyboard stays where it went
  (`file_prompt_blur`); an empty field makes nothing.
- One name for a rename, a path under the folder for something new,
  never "..". A name already taken is refused with the field left up
  after ↩, and dropped with the reason said when the field was left.
- What is made or renamed is the row chosen afterwards.
- ↩ with a row chosen renames it on a Mac, F2 elsewhere, as each
  system's file manager has it (`tree_rename_key`).
- While the field is up the panel's own keys (⌘A, ⌘C, ⌘V, ⌘Z) are the
  field's.

Copy on a row's menu, or ⌘C with the row chosen, puts the file or folder
itself on the clipboard (`tree_copy`, `sys::copy_file`: on a Mac as a file,
so the Finder pastes it too; elsewhere its path, with the path kept in
`file_clip`). Paste, or ⌘V, copies every file the clipboard holds
(`clipboard_files`: what any app copied as files, else the one copied
here while the clipboard still names it) into the folder chosen, the
folder the chosen file is in, or the session's folder with nothing chosen
(`tree_paste`, `files::copy_into`). A name already there is not written
over: the copy is "name copy.ext", then "name copy 2.ext", as the Finder
has it. Paste is on the menu only while there is a file to paste. The
panel has a focus of its own for its keys (`tree_focus`), taken at a
click on a row or on its empty room, and handed to the window when the
panel is put away with the keyboard in it. Opening a file from the tree
leaves the keyboard in the tree (`file_preview` hands it to the window only
when it was elsewhere): the row is still what ⌘C and ⌘V are about until a
click in the file's pane takes it there. A click on a folder chooses it
as well as opening it, so that it can be copied or pasted into.

The trash is `sys::trash_path` (the `trash` crate). On macOS it uses the
file manager's own call, since the crate's default asks Finder by
AppleScript.

## The outline: entries and labels

`emaki_core::outline` makes an entry per round, saying what the person
asked there and nothing of the reply, once per load (`Detail::outline`).
An entry is drawn as its time on a small plate, then the words, two lines
at most, with nothing in front of the time. Tried and dropped: a rail
down each day with a dot per entry. The entry also carries `title` and
`gist` (first lines of the prompt and of the last reply), which only the
CLI prints now.

A command (`/compact`) and a message of `SHORT_MAX` characters or less
are shown as they are. A longer one is shown by a label a small model
writes (`outline::summarize`): the explainer's model and isolation
(`explain::run_child`: Haiku, no settings, no tools, the scratch folder
whose transcripts are swept), `BATCH` messages to a child and the
children side by side, each message inside `<message n>` tags and cut to
900 characters, the reply a line per message, "n: label". A label is a
headline of three to five words for the main thing asked, never a list of
all of it. A child sometimes answers its messages instead of labelling
them; the tags, the instruction before and after the messages, the check
on the reply and the one retry are for that. Labels are kept for good in
`cache/outline.json` by a hash of the words they were made from
(`key_for`), so a message is asked about once.

The window asks in `ask_labels`, off the main thread: the entries in the
outline's view and `LABEL_AHEAD` above and below it, once the scroller
has rested `LABEL_REST` and nothing is being asked already. A scroll
through a long conversation asks about where it stops.

An entry with no label is two breathing bars; the head says
"Summarizing…" while a call is out; a label fades in as it lands
(`outline_fresh`). A reply with a line that does not read as a label (no number,
markdown in it, more than eight words) is dropped and asked for once more. What still
has none shows the message's own first words and is asked again at the
next launch (`outline_failed`).

## The outline: days and the mark

Entries sit under a head for each day. The last head above the view is
drawn pinned at the panel's top, so a time is always under its day. The
pinned head changes when the next head's own words reach where the pinned
ones are drawn (`PIN_LEAD`). Changed when the head's box left the view,
the words stood a few pixels apart and jumped.

The entry in view wears the accent: the round at the top of the
conversation list, or the last once the list is at its end (a short last
round never reaches the top), and an entry just clicked for as long as
the view is where the click put it (`outline_pick`). The outline brings
the marked entry into sight when the mark changes (`outline_at`). An
outline at its foot stays at its foot as it grows.

## The outline: the glide

Choosing an entry moves the conversation to its round, and the move is
drawn (`outline_go`, `Glide`): a task steps the list every `GLIDE_TICK`
until the round is at the top. Tried and dropped: setting the list's
place in one step, which read as abrupt.

The list knows where a round is only once it is laid out, so the way is
felt out:

- Place known (the round at the top, or one below that `bounds_for_item`
  answers for): what is left falls away over `GLIDE_EASE`, and more than
  `GLIDE_CAP` of it is skipped first, so a far round arrives over the
  same moment as a near one.
- Place unknown (any round above, or one below never drawn): the list
  runs that way for `GLIDE_RUN_TICKS`, which lays out what it passes. A
  round above still not reached is then gone to `GLIDE_FROM` under its
  top and come up onto; one below is gone to directly.
- A list held at its end has nothing to run from and skips the run.

The move is over when nothing is left, when the list's end holds it short
(no nearer for four ticks, or the view back on the end's own anchor), or
after `GLIDE_MOST`. A newer choice takes it over (`outline_glide`, its
number), and a wheel in the conversation ends it.

While it runs the mark stays on the entry chosen (`outline_gliding`). A
move that ends with the list on its end records that, with the count of
rounds (`outline_pick_end`), and the mark is kept while the list is still
there with that many; a wheel in the conversation lets it go. Without
this the mark fell back to the last entry when the move ended.

## Probing

- `EMAKI_GO=panelw:<w>` drags the panel's edge to that width and lets go;
  `panelfit` is the double click on it.
- `EMAKI_GO=branches` opens the branch list, `branchq:<words>` types in
  its field; `changes` opens the comparison, `changes:<path>` on a file.
- `branchnew` opens the sheet a branch is made on, `branchnew:<name>`
  with that name, and `branchnewgo` presses Create Branch. `branchpublish`
  publishes the branch checked out, `branchfetch` fetches, and `gitgate`
  shows the sign-in sheet as after a refusal. Use a scratch repository
  whose remote is a bare one beside it: these steps push for real.
- `branchask:<name>` goes to that branch as a click does, held until git's
  status is in, so with changes it shows the question (or the refusal); `branchgo:<name>:leave`
  or `:bring` answers it, and `branchrestore` is the strip's button. Put a
  dummy step first (`x;branchgo:…`): the first step can run before the
  session's folder is known, and then does nothing.
- `EMAKI_GO=file:big` is the pane's enlarge button.
- `EMAKI_GO=treedrag` holds files from the file manager over the
  panel's room and `treedrag:<path>` over that row, with no drag there
  is; `treedrop:<a file's full path>` lets that file go there.
- `treetrash:<path>` removes a row, `treemove:<path>:<folder>` moves one
  (an empty folder is the session's), and `treeundo` and `treeredo` are
  ⌘Z and ⌘⇧Z. A probe that removes and does not take it back leaves the
  file held, and the next launch puts it in the real trash.
- `EMAKI_GO=outline:<n>` chooses that round's entry.
  `EMAKI_GLIDE_DEBUG=1` prints the list's place and what is left at every
  tick of a glide.
- `emaki-core git <folder> [path...]` prints what a path wears.
  `emaki-core outline <id>` prints an outline; `--summarize` asks for the
  labels and is how the timings in Rules were measured.
- `EMAKI_GO=treecopy:<path>` copies that file or folder, `treepaste` or
  `treepaste:<path>` pastes into the folder or at that row;
  `discard:<path>` opens the comparison on a file with the question up,
  and `discard:yes` answers it. `treefocus` puts the keyboard in the files
  panel, and `EMAKI_FOCUS_DEBUG=1` prints where it is at every draw.
- `EMAKI_GO=treenew:file:<folder>:<words>` opens the field for a new
  file in that folder ("." is the session's) with those words typed;
  `treenew:folder:…` and `treenew:rename:<path>` are the other two, and
  `treecollapse` closes every folder. Nothing is made: the copy quits
  with the field up. Use a scratch project all the same.
- `EMAKI_GO=renaming:side` and `renaming:tab` show the field a
  session's name is written in, in its row or on its tab, with nothing
  in it; `EMAKI_KEYS=rename:side` and `rename:tab` are the menu's Rename
  there, with the name and the keyboard in the field.
- `EMAKI_GO=cell:<row>,<col>` chooses a cell of the table showing (from
  0), `cell:to:<row>,<col>` takes the choice to another, `cell:type:<words>` writes in it and goes down a row, `cell:open`
  opens its field and leaves it up, `cell:undo` is ⌘Z.
- Code on the file sheet came out in one colour, unhighlighted, in a
  probe; not looked into.
