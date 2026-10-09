# Vendored air_r_parser

Source: https://github.com/posit-dev/air, `crates/air_r_parser`
Revision: 8d20826ccf65a0ccc1d6a82e78d308213271667c
Licence: MIT (LICENSE beside this file).

Air is Posit's formatter for R. Emaki formats an R file with it when the
file is saved from the file's pane (`crates/emaki-core/src/format.rs`).
The rest of Air (`air_r_formatter`, `air_r_syntax`, `air_r_factory`, and
its fork of Biome) is taken by git revision as it is. This one crate is
copied because of its manifest alone.

## Changes made here

`src/` is upstream's, untouched. `Cargo.toml` is written for one crate
outside Air's workspace, and differs in two dependencies:

- `tree-sitter = "0.26"` (upstream 0.24.7). The app's syntax colours are
  on 0.26, and the `tree-sitter` crate declares `links`, so cargo refuses
  two versions in one program.
- `tree-sitter-r = "1.3"` from crates.io (upstream a git revision of
  1.2.0), the same copy the file's pane colours R with: two copies of
  the grammar would define the same C symbol twice.

The grammar is therefore one minor release newer than the one Air's
parser was written against. `an_r_file_is_formatted_as_air_formats_it`
in `crates/emaki-core/tests/core.rs` formats a file with most of the
language in it and checks the result formats to itself; run it after any
change here.

## Updating

Copy `crates/air_r_parser/src` from the new revision over `src/`, set
the revision here, in `Cargo.toml` and in `crates/emaki-core/Cargo.toml`
(the three must agree, or two copies of `air_r_syntax` are built and
their types do not match), and run the test. If Air has moved to the
tree-sitter the app uses, delete this copy and take the crate by git as
the others are.
