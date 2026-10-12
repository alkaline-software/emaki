# stylo_derive

The derive crate of Stylo, Servo's style engine, copied from crates.io at
0.20.0 (https://github.com/servo/stylo, MPL-2.0). The root `Cargo.toml`
`[patch]`es crates.io with this copy. Blitz, which draws an HTML file in
the file's pane, is built on Stylo.

## Changes

1. `to_css.rs`, `derive_variant_arm`: the block a `#[css(function)]`
   variant writes is given its type, `std::fmt::Result`, before `?`.
   Upstream writes `{ ...; Ok(()) }?`, which leaves the block's error to
   be worked out from the conversion `?` makes, and that is one of two in
   a build where `log` has `kv_serde` on (`serde_fmt` converts into
   `std::fmt::Error` too). gpui turns that feature on, so Stylo did not
   compile in this workspace: 32 "type annotations needed" errors, all
   from this line. Marked `(Emaki change.)`.

## Taking a newer one

Copy the new version's files over these, make the change again if
upstream still writes the block that way, and drop this folder and the
`[patch]` if it does not.
