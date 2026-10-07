# Panels beside a conversation

About `crates/emaki-app/src/panels.rs` and `file_icons.rs`, and
`crates/emaki-core/src/outline.rs` and `git.rs`.

## Rules

- One panel shows at most, at the conversation's left, and both have one
  width (`panel_w`), so one takes the other's place without moving the
  conversation. Tried and dropped: both at the right, side by side.
- The tree is not `files.rs`. That is git's flat list for "@", without
  what is ignored; the tree shows what is on disk.
- Nothing here deletes or forces. Move to Trash is the system's trash,
  and a refused `git switch` is reported in git's own words.
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
- A branch switch is refused while a turn runs in the session showing: the
  agent is writing the files a switch would change.
- The comparison only shows. Nothing is staged or committed from it.
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
nothing widens and fades in; one that takes the other's place only fades
in; one put away goes at once. The choice and the width are in `ui.json`.

The edge is dragged as the sidebar's is: `render_panel_grip` holds it at
a press (`panel_drag`), and the raw mouse-move listener that follows the
sidebar's edge sets the width (`panel_drag_to`) between `PANEL_MIN` and
`PANEL_MAX`, never leaving the conversation less than
`CONVERSATION_MIN`. Narrower than `PANEL_FOLD_AT` the panel is put away,
and comes back in the same drag if the pointer does, keeping the width it
had. A double click is `panel_fit`: for the files the widest row showing,
capped at `PANEL_FIT_MAX`; for the outline a fixed `OUTLINE_FIT`, since
its lines are cut at any width.

## The files tree

`Tree` is the session's folder. A folder is read from disk when it is
opened (`list_dir`: folders first, then by name, `.git` and `.DS_Store`
left out, `DIR_MAX` entries and then "N more"). What is open is kept by
absolute path, so two sessions of one folder share it. The root and every
open folder are read again every `TREE_SECS` while the panel shows, which
is how a file the agent writes appears. Rows follow VS Code's explorer.

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
the branches only a remote has. Words that name no branch add a row,
"Create branch x from y". No tags.

The default branch is first in either order. After it the list is ordered
by when each branch was last committed to, the newest first (`Branches::by_recency`, from `Branches::when`, the committer date
`for-each-ref` gives; a branch here and on a remote takes its local
time), with that time at the row's right in words (`format::ago`). Two
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
- `git::diff` is `git diff HEAD` for the file, `--cached` before a first
  commit, and a file git does not track read as all new lines.
- `git::parse_diff` sets a run of lines taken out beside the lines put in
  after it, first with first. Lines within a pair are not compared word
  by word. A file all new or all gone has one side and the whole width.
- The lines are a gpui `list`, read off the main thread and again on the
  tree's clock while the sheet shows, and replaced only when they differ,
  so reading on does not move the reader. `DIFF_LINES` at most; a file
  that is not text says so.

## The file sheet and the menu

A click on a file is `file_preview`: a picture in the lightbox, anything
else on a sheet (`render_file_view`), markdown drawn as the conversation
draws it and other text as a code block in the file's language, the
first `PREVIEW_BYTES` and `PREVIEW_LINES` of it, read again when the file
changes on disk. A file that is not text says so and offers Open.

The file clicked stays marked (`Tree::picked`) until another is, or until
a click on the panel's empty room. A row's click goes through
`swallow_click`, so the room's own click handler does not hear it.

A row under the pointer takes `row_hover`, in both panels: the theme's
muted grey in a dark window, a wash of the ink in a light one, where the
muted grey is all but the sidebar's own ground.

A right click is the window's own menu (`MenuDo::File`, `file_do`). Add
to message puts the path as "@path" at the end of the composer. New file
and New folder are on a folder or the panel's empty room. A name is asked
for in the
field a session is renamed in (`file_prompt`, `commit_file_prompt`): one
name for a rename, a path under the folder for something new, never
"..", and a name already taken is refused with the field left up.

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
900 characters, the reply a line per message, "n: label". A child
sometimes answers its messages instead of labelling them; the tags, the
prompt's wording and the retry are for that. Labels are kept for good in
`cache/outline.json` by a hash of the words they were made from
(`key_for`), so a message is asked about once.

The window asks in `ask_labels`, off the main thread: the entries in the
outline's view and `LABEL_AHEAD` above and below it, once the scroller
has rested `LABEL_REST` and nothing is being asked already. A scroll
through a long conversation asks about where it stops.

An entry with no label is two breathing bars; the head says
"Summarizing…" while a call is out; a label fades in as it lands
(`outline_fresh`). A line that does not read as a label (markdown in it,
more than sixteen words) is dropped and asked for once more. What still
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
- `branchask:<name>` goes to that branch as a click does, held until git's
  status is in, so with changes it shows the question (or the refusal); `branchgo:<name>:leave`
  or `:bring` answers it, and `branchrestore` is the strip's button. Put a
  dummy step first (`x;branchgo:…`): the first step can run before the
  session's folder is known, and then does nothing.
- `EMAKI_GO=outline:<n>` chooses that round's entry.
  `EMAKI_GLIDE_DEBUG=1` prints the list's place and what is left at every
  tick of a glide.
- `emaki-core git <folder> [path...]` prints what a path wears.
  `emaki-core outline <id>` prints an outline; `--summarize` asks for the
  labels and is how the timings in Rules were measured.
- Code on the file sheet came out in one colour, unhighlighted, in a
  probe; not looked into.
