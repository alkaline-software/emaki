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
   second after the last scroll instead of two. The fade's opacity is
   taken over its own length (`1 - t²`, `t` from 0 to 1 across the half
   second): upstream's `1 - (seconds past the delay)^10` only fades when
   the fade is a second long, and over half a second held the bar at
   full strength and then cut it.

6. `crates/ui/src/text/style.rs`, `inline.rs`, `node.rs`,
   `format/markdown.rs`, `text_view.rs`: the rounded plate behind inline
   code. `TextViewStyle::inline_code_chip: Option<(Hsla, Hsla)>` is its
   fill and border; with it set, `inline_code_highlight` leaves the flat
   background off, `Paragraph::render` hands the code ranges to
   `Inline::with_chip`, and `Inline::paint_code_chips` paints one rounded
   quad per line of each span before the text, 1.35 times the font size
   tall and centred on the line. A text run has no padding, so the
   markdown parser sets every inline code span between two `CODE_PAD`
   characters (U+202F) at each end, outside the code mark: the plate takes
   in the nearer one as its padding and the farther one is its margin.
   The parser runs before the view's style is known, so the spaces are
   set whether or not a plate is asked for. The view's Copy action takes
   each pair out of the copied text again.

7. `crates/base/src/input/base/kind.rs`, `input/textarea/mod.rs`: a
   textarea can colour ranges. `TextareaMode::Extras` is `TextareaExtras`
   (upstream `()`), one layer of `TextDecoration`s the renderer already
   composes for every mode, and `TextareaState::set_marks` replaces them.
   Not tracked across edits: the application sets them again on change.

8. `crates/base/src/input/base/movement.rs`, `state.rs`: ⇧↑ and ⇧↓ extend
   the selection by one display row in the cursor's column. Upstream's
   `select_up` went to the end of the line before and `select_down` past
   the end of this one, lines of the buffer, so in a wrapped text one
   press took a whole paragraph. `move_vertical` is split so the step it
   computes (`vertical_offset`) serves both; `column_for_selecting` fixes
   the column when a selection starts.
9. `crates/base/src/input/base/element.rs`, `state.rs`: the view follows
   the cursor as far as it went. `layout_cursor` stepped the scroll one
   line per change of selection, which keeps up with typing and leaves
   the cursor out of sight after a paste or a dictated paragraph; it now
   scrolls by what it takes, and places a cursor on a line that is not
   laid out by the wrap map's row instead of its paragraph's first row.
   `paste` no longer calls `scroll_to`, which measured against the layout
   from before the paste and overrode the element's answer.

10. `crates/base/src/input/base/state.rs`, `movement.rs`: Up and Down
    after typing keep the cursor's column. `replace_text_in_range`
    measured the preferred column right after the edit, against the
    layout of the text before it, which gave none (or a stale one) and
    sent the next Up to the start of the row above. The column is left
    unknown at an edit and measured when a vertical move needs it
    (`ensure_preferred_column`).

11. `crates/base/src/input/base/state.rs`, `movement.rs`: Up and Down in
    a single-line input go to the start and the end of the text. Upstream
    registers the two actions only on a textarea and returns early for a
    single line, so the keys did nothing in a field.

12. `crates/ui/src/text/inline.rs`: a right click on text selects the
    word under it, as a double click does, unless the press is inside
    what is selected already. Upstream's handler takes the left button
    only, so a menu opened at a right click had nothing to copy.

13. `crates/ui/src/text/node.rs`: a code block's actions
    (`code_block_actions`) show while the pointer is on the block.
    Upstream draws them all the time.

14. `crates/base/src/input/base/state.rs`: three small doors for the
    composer's spelling and capitals. `replace_bytes` replaces a byte
    range as one undo step and sets the cursor; `offset_at` is
    `index_for_mouse_position` made public; `composing` says whether an
    input method has marked text.

15. `crates/ui/src/input/input.rs`, `textarea.rs`: `on_secondary_click`
    hands a right click to the application and shows no native menu, so
    an input's menu can be the window's own. `secondary_click_at` in
    `crates/base/src/input/base/state.rs` makes that click from code,
    through the same path the mouse takes, for a probe.

## Updating

Check out the new upstream revision, copy the four crates over these, re-apply
the changes listed above, update the revision here, and build.
