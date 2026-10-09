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
  one showing and fades in (`file_serial`). Its button and Escape close
  it.
- It is the first to give room up: narrower before the terminal and the
  tree are (`file_pane_w`, and `file_pane_least` in their own widths),
  and in a window with no room for its least it is not drawn
  (`fold_file`), with a line under the composer saying so.
- What is shown (`FileBody`, `read_file`): markdown as the conversation
  draws it; other text in the editor (below), the first `PREVIEW_BYTES`
  and `PREVIEW_LINES` of it; a picture at its own size
  or the pane's width; a PDF's first `PDF_PAGES` pages as pictures; a
  `.csv` or `.tsv` as a table (`files::table`), its first `TABLE_ROWS`
  rows, a column as wide as its longest cell up to a point. Anything
  else, or a file that cannot be read, is a line saying so and "Open
  with default app".
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
  scroller is not drawn around it. Its right click is the pane's menu
  (the vendored `on_secondary_click`). A folded line ends in dots, as
  VS Code's does (the vendored element).
- A file the pane holds whole can be changed in that editor and saved
  with ⌘S, File, Save File, or the Save button the head shows while
  there is something to save (`save_file`).
  - What may be edited is `FileView::source`: the file's text exactly,
    kept when all of it was read and all of it is UTF-8. The text the
    pane shows is not it (`Code` is lines joined, with no last line
    break). A file cut for the preview, or with bytes that are no text,
    has no source and its editor is read only.
  - A `.csv` or `.tsv` has the two segments markdown has: the table,
    and the file as written, which is where it is edited.
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
  - Saving an R file formats it first (`emaki_core::format`), with Air,
    Posit's formatter, built in at its defaults: the editor takes the
    formatted text as one step of undo, then the file does. A file Air
    cannot parse is saved as typed and the row says it was not
    formatted. No other language has a formatter; do not write one by
    hand, take the language's own as Air was taken.
- What shows two ways has two segments in the head, as it reads and as
  it is written (`two_ways`, `file_raw`, one choice for all of them):
  markdown; HTML (`FileBody::Html`, the toolkit's HTML view: words,
  headings, lists, tables, links, pictures, and no style sheet or
  script, since a webview draws over every overlay); an SVG, which is
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
- The palette is VS Code's Dark+ and Light+ on the window's own
  grounds: the `highlight` part of each theme in `themes/emaki.json`.
  Without one the toolkit keeps its light palette in a dark window. The
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
  Escape to close. Escape then drops a selection, then closes the pane.
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
- A right click anywhere in the pane opens the window's menu
  (`file_pane_menu`): what the place offers first (Copy for selected
  words, in markdown either way and in code; Copy Image on a picture;
  Copy Cell and Copy Row on a table's cell; Copy and Select All on a
  PDF's page), then what the file's row in the tree has: Add to
  Message, Open, Reveal, the two paths.
- Not done in a PDF: links, and text set in a font that says nothing of
  its characters, which is drawn and cannot be selected.
- A PDF is drawn by hayro, which is all Rust, so the same on every OS,
  off the main thread (`pdf_pages`), each page to a PNG the window
  draws. The pane says "Drawing the pages…" until they are in, and a
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
