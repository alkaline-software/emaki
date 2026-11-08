# Performance

About keeping Emaki fast and light: how an optimization pass is done, the rules passes have left, and the log of every pass, which is what makes the next one incremental. The frame rules (the window's clock, kept views) are in `docs/window.md`, Frames.

## Rules

- **A pass has three aims:** the best speed; the least use of the computer (CPU, memory, power, wakeups, disk, network, processes); and nothing the app does lost. The third is never traded for the other two: a change that saves time and changes what a person sees or can do is not made.
- **Measured before and after.** A stopwatch in a copy or a throwaway test, taken out when done, timing the real call the same way twice. The first guess is often wrong. The numbers go in the log.
- **Incremental.** A pass starts from the log's last entry and looks only at what changed since the commit that entry names (`git diff <commit>..`), plus anything an entry left for later. A product with no log gets a pass over everything. A pass that is not logged did not happen as far as the next one knows.
- **Nothing slow is asked while a frame is built** (`docs/window.md`, Frames), and that includes what a row asks for: a row's call runs once a row a frame, so a list of 300 rows at 120 frames a second makes it 36,000 times a second.
- **A lookup a row makes at every frame is answered from memory.** A small file the window reads per row (`state/remotes.json`, a remote's `probe.json`) is held in memory (`remote::kept`, `remote::probed`): this process's own writes go into the memory as they go to the disk (`remote::save`, `remote::keep_probe`), and the disk is looked at no more than every half second (`LOOK_AGAIN`), for another process's write. A read that answers with a shared list (`Arc`) copies nothing; `find` copies one remote, not the list.
- **A path is a name, not a look at the disk.** `paths::root` starts every path the app names, many times a frame; the move from the old folder name is looked for once a run, not at every call.
- **A picture belongs to the place that shows it.** gpui keeps every picture it decodes from a path or from bytes (`ImageSource::Resource`, `ImageSource::Image`), and its copy on the GPU, for as long as the app runs, and a day of pasted screenshots looked at held 2.4 GB. So each place that shows pictures has a `pictures::Kept` (a conversation's `Detail::pictures`, the composer's, the lightbox's, the file pane's, the comparison's), and a picture is drawn through it (`pictures::through`, or an `image_cache` of it around a markdown view, which catches the pictures it shows). When the place goes, its `Kept` goes, and every picture in it is let go with `drop_image`, its GPU copy too; one made from bytes goes as soon as nothing else holds those bytes (`Kept::sweep`), so a PDF's pages drawn again at a new width leave no copies behind. A thumbnail is decoded no larger than `THUMB_PX` on its longer side, twice the largest box one is drawn in: handed the whole picture, the GPU kept all of it and drew it jagged. `remove_asset` is not a way to let a picture go: it frees the decoded copy and leaves the GPU's.
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
