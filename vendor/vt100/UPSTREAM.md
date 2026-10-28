# vt100

The terminal screen model, copied from crates.io at 0.16.2 (https://github.com/doy/vt100-rust, MIT). The root `Cargo.toml` `[patch]`es crates.io with this copy. `emaki-core/src/pty.rs` feeds it the hidden terminal's and the shell terminal's output.

## Changes

1. `src/row.rs`, `Row::resize`: a wide character (CJK, an emoji) the new right edge cuts from its second half is cleared. Upstream left it, and the next character drawn in that column unwrapped the missing half (`screen.rs`, `text`) and panicked, on the thread that reads the pty and with the screen's lock held. A terminal narrowed under a line of Chinese, or the agent's terminal going between the CLI layout's size and its own, then took the app down. Marked `(Emaki change.)`.

## Taking a newer one

Copy the new release over this one and apply the change again, unless the release fixes it (a resize that leaves a wide character in the last column, then a write to that column).
