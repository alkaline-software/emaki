# block

The Objective-C blocks crate, copied from crates.io at 0.1.6 (https://github.com/SSheldon/rust-block, MIT). The root `Cargo.toml` `[patch]`es crates.io with this copy. It is the last release of a crate that is no longer maintained, pulled in by `cocoa`, which gpui's Mac backend uses.

## Changes

1. `src/lib.rs`, `Class`: the opaque type for the runtime's block class is a zero-sized `#[repr(C)]` struct, not an empty enum. Upstream's `extern` static of an uninhabited type is being phased out of Rust (rust-lang/rust#74840) and cargo reports it as a future incompatibility on every build. Marked `(Emaki change.)`.
2. `src/lib.rs`: every `extern` block and `extern fn` names its ABI, `"C"`. The copy is built as a path crate, so rustc's lints are not capped as they are for a registry crate, and the 46 deprecations for an `extern` with no ABI would otherwise be printed on every build.

## Taking a newer one

There is none expected; the crate's successor is `block2`, which cocoa does not use. Drop this copy and the `[patch]` line when gpui no longer depends on cocoa.
