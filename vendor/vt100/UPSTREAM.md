# vt100

The terminal screen model, copied from crates.io at 0.16.2 (https://github.com/doy/vt100-rust, MIT). The root `Cargo.toml` `[patch]`es crates.io with this copy. `emaki-core/src/pty.rs` feeds it the hidden terminal's and the shell terminal's output.

## Changes

1. `src/row.rs`, `Row::resize`: a wide character (CJK, an emoji) the new right edge cuts from its second half is cleared. Upstream left it, and the next character drawn in that column unwrapped the missing half (`screen.rs`, `text`) and panicked, on the thread that reads the pty and with the screen's lock held. A terminal narrowed under a line of Chinese, or the agent's terminal going between the CLI layout's size and its own, then took the app down. Marked `(Emaki change.)`.

2. `src/grid.rs`, `Grid::reflow`, with `Screen::set_size_reflowed` and `Row::from_cells` and `Row::all_cells` for it: a resize that reflows the main screen and its scrollback, as a terminal window's does, keeping the bottom of what was written at the bottom, and optionally the cursor's line as its rows, for a shell to draw again. Upstream has none: a resize cut or padded each row. `Screen::set_size` is upstream's still. Marked `(Emaki addition.)`; the reasons are in `docs/channels.md`, The terminal panel.
3. `src/perform.rs`, `src/screen.rs`, `src/grid.rs`: the prompt's marks, OSC 133 `A`, `B` and `C`, kept on the main screen as rows counted from the first that ever was (`Grid::scrolled`), so they ride the scroll and a reflow; `Screen::typing_start` says where what is typed begins. Upstream hands OSC 133 to `unhandled_osc`. Marked `(Emaki addition.)`.

## Taking a newer one

Copy the new release over this one and apply the changes again: the first unless the release fixes it (a resize that leaves a wide character in the last column, then a write to that column), the second unless the release reflows, the third unless it keeps the prompt's marks.
