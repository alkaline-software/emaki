# Performance

About keeping Emaki fast and light: how an optimization pass is done, the rules passes have left, and the log of every pass, which is what makes the next one incremental. The frame rules (the window's clock, kept views) are in `docs/window.md`, Frames.

## Rules

- **A pass follows every round of changes.** Once a set of updates is in, a pass looks at them before the next set begins, since what is new is where waste comes in. Its measure is the computer's work: as fast as it can be, and nothing computed, copied, read or woken for that is not used.
- **A pass has three aims:** the best speed; the least use of the computer (CPU, memory, power, wakeups, disk, network, processes); and nothing the app does lost. The third is never traded for the other two: a change that saves time and changes what a person sees or can do is not made.
- **Measured before and after.** A stopwatch in a copy or a throwaway test, taken out when done, timing the real call the same way twice. The first guess is often wrong. The numbers go in the log.
- **Incremental.** A pass starts from the log's last entry and looks only at what changed since the commit that entry names (`git diff <commit>..`), plus anything an entry left for later. A product with no log gets a pass over everything. A pass that is not logged did not happen as far as the next one knows.
- **Nothing slow is asked while a frame is built** (`docs/window.md`, Frames), and that includes what a row asks for: a row's call runs once a row a frame, so a list of 300 rows at 120 frames a second makes it 36,000 times a second.
- **A lookup a row makes at every frame is answered from memory.** A small file the window reads per row (`state/remotes.json`, a remote's `probe.json`) is held in memory (`remote::kept`, `remote::probed`): this process's own writes go into the memory as they go to the disk (`remote::save`, `remote::keep_probe`), and the disk is looked at no more than every half second (`LOOK_AGAIN`), for another process's write. A read that answers with a shared list (`Arc`) copies nothing; `find` copies one remote, not the list.
- **A path is a name, not a look at the disk.** `paths::root` starts every path the app names, many times a frame; the move from the old folder name is looked for once a run, not at every call.
- **A picture belongs to the place that shows it.** gpui keeps every picture it decodes from a path or from bytes (`ImageSource::Resource`, `ImageSource::Image`), and its copy on the GPU, for as long as the app runs, and a day of pasted screenshots looked at held 2.4 GB. So each place that shows pictures has a `pictures::Kept` (a conversation's `Detail::pictures`, the composer's, the lightbox's, the file pane's, the comparison's), and a picture is drawn through it (`pictures::through`, or an `image_cache` of it around a markdown view, which catches the pictures it shows). When the place goes, its `Kept` goes, and every picture in it is let go with `drop_image`, its GPU copy too; one made from bytes goes as soon as nothing else holds those bytes (`Kept::sweep`), so a PDF's pages drawn again at a new width leave no copies behind. A thumbnail is decoded no larger than `THUMB_PX` on its longer side, twice the largest box one is drawn in: handed the whole picture, the GPU kept all of it and drew it jagged. `remove_asset` is not a way to let a picture go: it frees the decoded copy and leaves the GPU's.
- **A picture that moves is played a frame at a time.** gpui decodes every frame of a GIF or an animated WebP before it draws one, and keeps on the GPU each frame it has drawn: a GIF of 360 frames of 1600 × 840, 750 KB on disk, held 1.8 GB decoded and as much again on the GPU. Through a `Kept`, a thread of the picture's own decodes its frames in turn, the next two ahead (`pictures::play`); one frame is held and drawn, and the one before goes from the GPU as the next comes in (`pictures::advance`). A GIF's frames are laid on its canvas where each frame covers (`GifFrames`), not built again whole, which the image crate's reader does at every frame, and a thumbnail of a moving picture is kept and worked out again only over what a frame changed (`Small`). The window draws the picture again when a frame's time is up, through the clock's view when the workbench draws it (`pictures::Clock`), so the views it keeps are not drawn with it; it stands still in a window in the background and with less motion asked for, as gpui's own do.
- **An image cache stands over what is inside it in every phase.** gpui's `image_cache` element was over its children's layout and paint and not their prepaint, where a markdown view measures its pictures, so each picture there was read a second time through gpui's own cache, decoded whole and kept until the app quit (`vendor/gpui/UPSTREAM.md`).
- **A layout asked for again is answered from what it was.** gpui's layout asks an element's size several times a frame, and once with no width while it measures what the element would take. The live editor keeps the key of all its layout reads (`Reader::inputs_key`: its reading of the text, the selection, the look, the width, the marks, issues and pieces) and returns at once when that key is unchanged; asked with no width, it answers with its last layout's size, never a layout at a stand-in width. Under an edit, a line's row is kept by a key of what the row is made from, in the line's own bytes, never the whole text's offsets, which shifted every row under a letter typed; and a row, a code block's colors or a table's grid is found by its key where lines came or went above it.
- **Work whose result is thrown away is not done.** A branch that returns early returns before the work only the other branch uses: a LaTeX or Typst file's pieces are its folds, so it is not written as markdown and diffed against its text at every key to be dropped.
- **What runs at every draw compares in place or by a count, never by a copy.** The composer's sync asks its count of readings (`version`) first; the file's spelling sync compares the field's text where it stands.
- **A caret blinks only in the window in front**, as the toolkit's fields do: behind another app's it is not drawn, and it does not draw the window twice a second.
- **Work leaves the main thread** (reading files, git, ssh, a model's answer), and a clock runs only while it has something to move (`docs/window.md`, Frames).

## A pass

1. Read the log's last entry: the commit it covers to, and what it left.
2. List what is new since (`git log` and `git diff` from that commit): the files, and the parts of the window they draw.
3. Find the hot paths in what is new. What `render` and each row call: a disk read, a system call, a lock, a copy of a list, a parse. Clocks and timers: does anything wake while nothing moves. The main thread: does it wait. Loops off it: how often they poll, and what each turn costs here and on another computer. What grows with use: memory, a list drawn whole, files kept.
4. Time each suspect as it is now.
5. Fix the cause, in the place every caller goes through.
6. Time it again, in the same way.
7. Check that nothing is lost: `cargo test -p emaki-core`, `cargo test -p emaki-app`, and a probe copy of each part changed, in each state it has (`docs/probing.md`).
8. Write the log entry.

## Log

Newest first. Each entry: the version and date, the commit range covered, what was looked at, what was found and done with numbers, and what was left for later.

### v0.5.4, 2026-11-13

Covers `c781dcb..` the commit that adds this entry: links and carets in the live editor, the folders held at the top of the tree, blank lines, and LaTeX and Typst read lightly in the rendered editor with their code folded. Counts from a stopwatch in a throwaway test (debug), the second of two runs, on a Typst file of 900 lines and a text of 240 KB, taken out when done; a probe copy on a scratch project in `/private/tmp` (`docs/probing.md`) checked a `.typ` file typed in, a markdown file's spelling marks and blank lines, and the composer's marks for text set from outside.

| | Before | This pass |
|---|---|---|
| A key in a `.typ` file of 900 lines, the pane's pieces | 9.2 ms: written as markdown (5.2 ms) and diffed against the text (4.0 ms), then dropped, and the folds (0.2 ms) | the folds, 0.2 ms |
| A markdown file with no raw block of Typst or LaTeX, a key | the whole text copied and walked for folds | nothing |
| The file's spelling sync, each draw, 240 KB open | two copies of the text, 33 µs | one walk in place, 15 µs |
| The composer's spelling sync, each draw | its text joined and copied | a count compared |
| Blank lines' heights, a run of k | k² looks | k |

Measured and left as they are: the folders held at the top of the tree walk the rows at most five times a draw of the files panel; the reading of a LaTeX or Typst file at a key, 0.25 ms for 900 lines, and of markdown, 0.7 ms.

Left for later:
- From the last pass, still open: the picture paths of a markdown file parsed at every key (`pictures_from`); a Quarto file's includes read from disk at every key (`quarto::expand`); the rendered editor drawn afresh at every frame, and the structure's heads hashed (`file_heads_now`); a change of width laying every line out again, and `piece_h` keeping a height for every key a piece has had.
- The file's spelling sync still walks the whole text at each draw while it is unchanged: a count of changes from the field would make it one comparison.

### v0.5.3, 2026-11-12

Covers `8196125..` the commit that adds this entry: the live composer, the rendered editor, custom fonts with their styles, and the smaller cut for code among words. A debug build, a probe copy on a scratch project in `/private/tmp` (`docs/probing.md`), a session open and a markdown file of 240 KB (837 lines, one picture and two `<img>` tags) in the rendered editor, the window behind the terminal; counts from a stopwatch taken out when done. The live editor's and the rendered editor's parts were read for this pass by two reviewers, whose list the checks below went by.

| | Before | This pass |
|---|---|---|
| Window drawn, a session open, nothing moving | about 20 a second | one in five seconds |
| The file's layout, each draw | asked 16 times; three of them the whole file again (5 to 9 ms each) | asked 16 times, each a check of one key (0.1 ms) |
| The file's layout, a letter typed at its top, or a line added | about 1 ms a call, 16 calls a frame | 0.35 to 3 ms, once |
| The smaller monospace cuts, at launch | 21 faces, 8.7 ms, 4.2 MB kept (release) | the family in use, 1.4 ms |

Found and fixed:
- The toggles under the composer asked the window's clock for its phase at every draw, for a dot drawn only while something runs; so did the outline's summary. Every open session kept the clock going (`docs/window.md`, Frames). This predates the range.
- gpui's layout measured the live editor with no width, and the editor laid the whole text out at 600 points and again at its own width, three times a frame. Now that question is answered from the last layout, and a layout whose reads are unchanged is not done (Rules).
- A line's key hashed its spans, marks and issues by the whole text's offsets, so a letter typed made every line under it new: 1,577 of 4,052 lines in a reviewer's count. Keys are by the line's bytes; when lines come or go, rows, code colors and table grids are found by key. Whether the selection touches a span summed every line before it, at every span of every line; it is one addition now.
- The caret's blink drew the window twice a second behind another app's, and the workbench redrew whenever the composer did, throwing the conversation's list and the sidebar out of their kept views at each blink. The blink stops in the background; the composer redraws by itself.
- The rendered editor copied the whole file at every frame to see whether it had changed; it compares in place now. Each key counted the lines before each block from the file's start (quadratic in the blocks), found each block's place by a walk through the runs, and kept its blocks' keys in a list; it counts on, halves, and keeps a set. The picture paths are read through the markdown parser only in a file that has a picture or an `<img>`.
- The inline cuts were made for all 21 monospace faces at launch, on the main thread; now for the family `look::install` sets, once.

Measured and left as they are: the custom font list built again at each frame of its slide (29 µs for 285 families), and the system's families read again (125 ms off the main thread) only when the list opens after the settings window comes back to the front.

Left for later:
- A key in the rendered editor still costs 14 to 22 ms (debug) in a file with a picture: `pictures_from` parses the whole file with the `markdown` crate. Move it off the main thread, or to pulldown-cmark's offsets with a test that the edits are the same.
- A Quarto file reads its includes, `_quarto.yml`, `_variables.yml` and its bibliography from disk at every key (`quarto::expand`): keep them by path, looked at again on the watcher's word.
- The rendered editor is drawn afresh at every frame, not as a kept view as the reading view was, and the hidden raw editor's text is copied at each frame to sync its marks (`file_writing_sync`), as the structure's heads are hashed (`file_heads_now`).
- A change of width lays out every line again (a pane opening, a resize), and `piece_h` keeps a height for every key a piece has had.

### v0.5.3, 2026-11-10

Covers `v0.5.2..` the commit that adds this entry: what memory a moving picture takes, from a memory report written at 6.3 GB, two days into a run, with two tabs open: one picture in the file's pane held 1845 MB, and the GPU 1.8 GB. The picture was mado's `.github/readme.gif`, 360 frames of 1600 × 840 in 750 KB; 360 × 1600 × 840 × 4 bytes is the 1845 MB to the megabyte.

Found and fixed: gpui decoded every frame of a GIF before drawing one and kept each frame it drew on the GPU; now a picture that moves is played a frame at a time, and a thumbnail of one is worked out again only where a frame changed it (Rules). Found with it: a markdown view's pictures were read a second time through gpui's cache and kept until the app quit, since the image cache was not over its children's prepaint; and a GIF in the file's pane did not move unless something else drew the window, while gpui asked for every frame the display has to move it.

Release builds, a probe copy of each with its own bundle identifier on a scratch project in `/private/tmp` holding the GIF and mado's README (`docs/probing.md`), the window in front; CPU is the share of a core over 20 seconds, from the CPU time the process used:

| | v0.5.2 | This pass |
|---|---|---|
| Nothing open | 252 MB, 13.7% | 254 MB, 13.3% |
| The GIF in the file's pane | 2455 MB, 13.8%, the picture standing still | 305 MB, 28.2%, playing |
| mado's README, which shows it | 6264 MB, 2.3 GB of it on the GPU, 30.4%, playing | 315 MB, 23 MB on the GPU, 30.2%, playing |
| The GIF in the composer's chip | 882 MB | 284 MB, 22.5%, playing |
| Any of them, the window in the background | 10 to 12% | 10 to 12%: the picture stops |

The chip brought down whole at every frame, as the first version of this pass did, took 33.5% in the same build: the thumbnail's work went from about 20 points to about 9 over the copy with nothing open, and 6.7 ms a frame to 0.12 ms (`cargo test`), since this GIF's frames change 5.6% of it on average; the thumbnails are the same to the byte. The 360 frames decoded once took 794 ms with the image crate's reader and 202 ms with `GifFrames`, the same to the byte. What is left while one plays is the window drawn at each of its 30 frames a second, as a window is while an agent's mark turns, and for one at its own size the frame put on the GPU.

Left for later: nothing.

### v0.5.2, 2026-11-08

Covers `v0.5.1..` the commit that adds this entry: what memory a picture takes, from a memory report written at 3 GB after 23 hours with four tabs open: 1.3 GB of the heap in about 126 blocks of ten megabytes, and 1.1 GB on the GPU. The window's own counts were small; the pictures were in gpui's caches, which they do not see.

Found and fixed: every picture the window drew was decoded whole and kept by gpui, on the heap and on the GPU, until the app quit: a conversation's thumbnails, the composer's chips, the lightbox's pictures, the file pane's pictures, PDF pages and HTML strips, the comparison's pictures and a markdown file's. Closing the tab let none of them go. Now each place's pictures go with it (Rules, a picture belongs to the place that shows it), a thumbnail is decoded at 640 pixels at most, and two pictures are decoded at a time (`pictures::DECODING`): twenty decoded together left some 400 MB of scratch the allocator gave back only over a minute or more (`vmmap`, Malloc Large (empty)). A probe copy (`docs/probing.md`), a session of one prompt with twenty pasted pictures of 2000 × 1288, measured with `footprint`:

| | Before | After |
|---|---|---|
| A session without pictures open | 257 MB | 258 MB |
| The twenty-picture session opened | 831 MB | 367 MB, 303 MB a minute later |
| Its tab closed | 737 MB | 268 MB |
| The lightbox opened on the twenty, then closed | not measured | +225 MB while open, 218 MB of it back on closing |

What the open session holds is the twenty thumbnails, twenty blocks of about a megabyte (`heap`), and as much again on the GPU.

Left for later: nothing.

### Before the log

Passes before this log were not written down by what they covered. Together they cover the app as it stood at `v0.4.3`, the first logged pass's start:

- v0.4.1: reading files, git and spell checking left the main thread.
- v0.3.2: a fraction of the CPU and power while an agent works (the window's clock, kept views: `docs/window.md`, Frames); a long session or a large file no longer slows the window.
- v0.2.6: long sessions open about four times faster.
