# Vendored gpui-component

Source: https://github.com/longbridge/gpui-component
Revision: fd3bc2bbb8a2c4dfe268c1475682476ada54cd0c (the rev Emaki pinned before vendoring)
Crates: crates/ui (gpui-component), crates/base (gpui-base), crates/macros
(gpui-component-macros), crates/assets (gpui-component-assets), copied as they
are from that revision, with the upstream LICENSE-APACHE beside them. The
workspace Cargo.toml here is a trimmed copy of upstream's (the four members,
the shared dependency table, the lints) so the crates build unchanged.

Emaki's root Cargo.toml takes these over the git source with a `[patch]`
section, so a `git`+`rev` dependency in a crate manifest still names where the
code came from while the build uses the copies here.

## Changes made here

Each is marked `(Emaki addition.)` in the source, so a diff against the
upstream revision finds them.

1. `crates/ui/src/text/style.rs`: `TextViewStyle` gains
   `strong_font_weight: Option<FontWeight>` (strong text was hard-coded to
   `FontWeight::BOLD`) and `inline_code_font_family: Option<SharedString>`
   (inline code inherited the paragraph's face; only fenced blocks were mono),
   with builders and `PartialEq`/`Default` entries.
2. `crates/ui/src/text/node.rs`: strong text takes the weight from the
   style at both mark-to-highlight sites; `Paragraph::render` collects the
   inline code ranges beside the highlights and hands them to `Inline`.
   The inline-flow path (a paragraph mixing images and text) is left as
   upstream has it, so inline code beside an image keeps the paragraph's face.
3. `crates/ui/src/text/inline.rs`: `Inline::with_mono(family, ranges)`;
   `request_layout` draws a highlighted run in that family when it lies
   inside one of the ranges (`combine_highlights` only splits at mark
   edges, so a run is wholly inside or outside a code span).
4. `crates/base/src/input/base/movement.rs`: Up on the first display row
   moves to the start of the text and Down on the last to the end, as
   inputs generally do; `display_row_of_cursor` is the helper.
5. `crates/base/src/scrollbar.rs`: `FADE_OUT_DELAY` 1.0 and
   `FADE_OUT_DURATION` 1.5 (upstream 2.0 and 3.0), so the scrollbar goes a
   second after the last scroll instead of two.

## Updating

Check out the new upstream revision, copy the four crates over these, re-apply
the changes listed above, update the revision here, and build.
