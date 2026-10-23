# proc-macro-error2

The proc-macro error helper, copied from crates.io at 2.0.1 (https://github.com/GnomedDev/proc-macro-error-2, MIT or Apache-2.0). The root `Cargo.toml` `[patch]`es crates.io with this copy. Biome's derive crates use it, and Air's R formatter is built on Biome.

## Changes

1. `src/lib.rs`, `__export`: `proc_macro` is declared there as `pub extern crate` rather than re-exported from the root, where the crate is a private extern crate. Re-exporting a private extern crate is being phased out of Rust (rust-lang/rust#127909, E0365) and cargo reports it as a future incompatibility on every build. Marked `(Emaki change.)`.

## Taking a newer one

Copy the new release over this directory, apply the change above if the release still has `pub use proc_macro;` in `__export`, and keep this file.
