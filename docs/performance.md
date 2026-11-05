# Performance

About keeping Emaki fast and light: how an optimization pass is done, the rules passes have left, and the log of every pass, which is what makes the next one incremental. The frame rules (the window's clock, kept views) are in `docs/window.md`, Frames.

## Rules

- **A pass has three aims:** the best speed; the least use of the computer (CPU, memory, power, wakeups, disk, network, processes); and nothing the app does lost. The third is never traded for the other two: a change that saves time and changes what a person sees or can do is not made.
- **Measured before and after.** A stopwatch in a copy or a throwaway test, taken out when done, timing the real call the same way twice. The first guess is often wrong. The numbers go in the log.
- **Incremental.** A pass starts from the log's last entry and looks only at what changed since the commit that entry names (`git diff <commit>..`), plus anything an entry left for later. A product with no log gets a pass over everything. A pass that is not logged did not happen as far as the next one knows.
- **Nothing slow is asked while a frame is built** (`docs/window.md`, Frames), and that includes what a row asks for: a row's call runs once a row a frame, so a list of 300 rows at 120 frames a second makes it 36,000 times a second.
- **A lookup a row makes at every frame is answered from memory.** A small file the window reads per row (`state/remotes.json`, a remote's `probe.json`) is held in memory (`remote::kept`, `remote::probed`): this process's own writes go into the memory as they go to the disk (`remote::save`, `remote::keep_probe`), and the disk is looked at no more than every half second (`LOOK_AGAIN`), for another process's write. A read that answers with a shared list (`Arc`) copies nothing; `find` copies one remote, not the list.
- **A path is a name, not a look at the disk.** `paths::root` starts every path the app names, many times a frame; the move from the old folder name is looked for once a run, not at every call.
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

### v0.5.0, 2026-11-05

Covers `v0.4.3..` the commit that adds this entry: Connected Devices (`remote.rs`, `remote_files.rs`, Settings' saved computers in `workbench/settings.rs`, the sidebar's computer cards and `side_unlinked` in `workbench.rs`, the hub's links).

Found and fixed: every remote session row in the sidebar and on the sessions page asked `remote::is_there`, `find` (its computer's name) or `probed` (its home) at every frame, and each call looked at the file's stamp on the disk and copied the whole list or probe. `paths::root` looked at the disk at every call as well. Now held in memory as the Rules say. Timed 20,000 calls each, release build, the real `HOME`:

| Call | Before | After |
|---|---|---|
| `is_there` | 3.4 µs | 0.25 µs |
| `find` | 2.5 µs | 0.33 µs |
| `probed` | 2.4 µs | 0.38 µs |

Looked at and left as they are: the link's watch (a `find` there once a second, a line back; the work is on the remote), the files engine's watch (the same), the sign-in check (one ssh command a connected computer every 30 seconds, only while Settings shows the section), one shared ssh connection a computer (`ControlMaster`), a copied change waking the scan as a local session's write does, and Settings' entries, drawn only while the window shows them.

Left for later: nothing.

### Before the log

Passes before this log were not written down by what they covered. Together they cover the app as it stood at `v0.4.3`, the first logged pass's start:

- v0.4.1: reading files, git and spell checking left the main thread.
- v0.3.2: a fraction of the CPU and power while an agent works (the window's clock, kept views: `docs/window.md`, Frames); a long session or a large file no longer slows the window.
- v0.2.6: long sessions open about four times faster.
