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
   In a field of one line ⇧↑ selects to the start of the text and ⇧↓ to
   its end, as Up and Down alone go there (4); upstream does nothing.
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

15. `crates/ui/src/input/input.rs`, `textarea.rs`, `editor.rs`: `on_secondary_click`
    hands a right click to the application and shows no native menu, so
    an input's menu can be the window's own. `secondary_click_at` in
    `crates/base/src/input/base/state.rs` makes that click from code,
    through the same path the mouse takes, for a probe.

16. `crates/ui/src/tooltip.rs`: `ManagedTooltipExt` is public, so an
    element of the application's own gets the tooltip a `Button` has
    (`managed_tooltip`). gpui's plain `.tooltip` appears and goes from
    one frame to the next and does not slide along a row.

17. `crates/base/src/input/base/element.rs`, `state.rs`: a folded line
    ends in dots ("⋯", in the text's colour at less than half strength,
    after the last word of its last wrapped row), as VS Code's does;
    upstream marks a fold in the gutter only. `toggle_fold_at` folds a
    line from code, for a probe.

18. `crates/base/src/input/base/state.rs`: `set_next_line_indent` hands
    the application the line up to the caret and the last line with
    words above it at Enter in a code editor, and takes the new line's
    indent from it (`asked_indent`). Upstream repeats the indent of the
    line above, which is wrong after Python's `:` or an opening bracket.

19. `crates/base/src/input/base/state.rs`, `element.rs`,
    `crates/ui/src/input/mod.rs`: `GutterMark` and `set_gutter_marks`, a
    bar or a wedge in a code editor's margin between the line numbers
    and the fold marks, in a colour the application gives, and the same
    marks small along the editor's right edge. A click on one is handed
    to the application with the mark's `id` (`set_gutter_click`), taken
    in the capture phase so the margin's own click does not also run.
    Upstream's margin has numbers and folds only.

20. `crates/base/src/input/base/element.rs`, `state.rs`,
    `editor/display_map/display_map.rs`, `crates/ui/src/input/input.rs`:
    a code editor's margin and right edge, as VS Code has them.
    - `MARK_LANE`: room between the line numbers and the fold marks, so
      a `GutterMark` stands clear of both. The margin is otherwise
      drawn in: `LINE_NUMBER_RIGHT_MARGIN` 2 and the fold mark's box 14
      (upstream 10 and 18), so the text starts 29 points after its line
      number, as VS Code's does; an editor with no fold marks keeps
      upstream's ten (`PLAIN_GUTTER_GAP`).
    - The fold marks are plain chevrons (upstream: ghost buttons with a
      plate under the pointer and on a folded one), shown while the
      pointer is in the margin and not on the cursor's line, and they
      fade in and out (`fold_fade`, `FOLD_FADE`).
    - A click on one moves the fold (`toggle_fold_moving`, `FoldAnim`,
      `FoldMove`) over `FOLD_MOVE`. Opening, its lines are uncovered
      from their head while the lines under them slide down. Closing,
      its lines go at once and the lines under them slide up from where
      they were: the editor lays out only what is in view, so the lines
      a long fold brings into view are not there to draw until it is
      made. The cursor, the selection and the indent guides are not
      drawn for that moment.
    - `RULER_WIDTH`: a code editor's scrollbar is that wide and square
      with eased corners, and under it, in the same strip and placed as
      its thumb is, the marks are drawn by row as wrapped, with the
      cursor's line a thin line across.
    - `crates/base/src/scrollbar.rs`: a click on the track puts the
      thumb's middle on the click all the way down (upstream scaled by
      everything that scrolls, not by what there is to scroll, and
      overshot more the lower the click), for every scrollbar; and
      `click_when_hidden`, which the code editor sets, takes that click
      when the bar has faded too.
    - A markdown `list` node is not a fold candidate
      (`crates/ui/src/highlighter/input_adapter.rs`): it starts on the
      line its first item does, and upstream's one-candidate-a-line
      folded the whole list from that item's mark.
21. `crates/ui/src/highlighter/delimited.rs` (new), `mod.rs`,
    `input_adapter.rs`: the languages `csv` and `tsv` get a highlighter
    with no grammar behind it, which gives each column one of ten of the
    theme's syntax colours in turn, as the Rainbow CSV extension does.
    It reads the whole text again at each change, since a quoted field
    may hold the separator and line breaks.
22. `crates/base/src/input/base/state.rs`: `undo_step` and `redo_step`,
    undo and redo for an owner that edits the value from outside the
    field with `replace_bytes` (a table's cell).
23. `crates/base/src/input/editor/search.rs`: a field that is not
    `searchable` passes ⌘F on (`cx.propagate()`), where upstream ended
    it there. The window's find is bound to the same key, and with the
    keyboard in the composer it never heard it. ⌘⇧F, Replace, likewise:
    the window's files panel is bound to it.
24. `crates/ui/src/highlighter/input_adapter.rs`: a text parsed for
    the first time gets 60 ms before the first draw (upstream: the 2 ms
    a keystroke gets) and, past that, a background parse with no wait
    before it (upstream: the 150 ms that follows typing). A file opened
    was drawn in one colour for a moment first.
25. `crates/base/src/input/base/state.rs`: `select_word_at`, the word
   under a point selected as a double click selects it, for a right
   click in the file's editor to offer Copy for the word under it, as a
   conversation's does.
26. `crates/base/src/text_selection.rs`: `WindowSelectionState::end`
   ends the press too (`mouse_down_prepared`). A click handler that
   swallows the mouse-up calls `TextSelection::end` in its place, and
   the press stayed "prepared": every later press skipped the clearing,
   and a word selected by a double click in such a pane stayed selected.
   The test is in `crates/ui/src/text/window_selection.rs`
   (`click_clears_double_click_selection_where_clicks_are_swallowed`).
   The crates' tests are run from this folder with the app's
   `Cargo.lock` copied beside its `Cargo.toml`, for the pinned gpui,
   and a `themes/aurora.json` holding `{}`, which a test reads and the
   copy does not have; neither is kept.

27. `crates/ui/src/text/node.rs`: a code block's lines are not wrapped
   (`whitespace_nowrap` on the block). The text is in the toolkit's own
   `horizontal_scroll_area`, as a table is, so a block wider than its
   room scrolls sideways and a wheel that is not sideways passes through
   to what the block is in. The area is a row and the text an item of it
   that does not shrink: as a block's child the text was as wide as the
   block and there was nothing to scroll. Its handle is kept with `use_keyed_state`
   under the parsed block's own address, since a node is made off the
   main thread and cannot hold one. Where more lies past an edge the text
   fades into the block's ground, and the block has a horizontal
   scrollbar, on the block itself so it lies in the padding under the
   text and not over the last line. Wrapped, anything set out in columns came apart.
28. `crates/ui/src/text/node.rs` (`render_wrap_table`): a column is never
   narrower than its longest word, up to a number of letters that is
   less with more columns. Columns are sized by the length of their text
   alone, so a column of short words beside one of long text was given
   so little that a word broke in its middle.
29. `crates/base/src/input/base/state.rs` (the input's `on_action` list):
   `select_up` and `select_down` are listened for in a field of one line
   too, not a textarea only. Both had a branch for one line (change to
   `select_up`, above) that no key reached, so ⇧ with up or down did
   nothing in any name, search or find field.
30. `crates/base/src/tooltip.rs` (`TooltipOverlay::claim`, the canvas in
   its `render`), `crates/ui/src/tooltip.rs` (`managed_tooltip`'s
   prepaint): a trigger drawn with the pointer on it claims the tooltip
   each frame, and one nobody claimed, with no hide on its way, goes.
   gpui sends a hover's end only to an element it still draws, so a
   tooltip whose trigger went (a sheet closed under the pointer, or a
   show that landed after its trigger had gone) stayed for good.
31. `crates/ui/src/text/node.rs` (`code_drag`, `CodeDrag`): a selection
   dragged to a code block's left or right edge scrolls the block that
   way, a step a frame, longer the further past the edge the pointer is.
   It begins only with a press on the text and a move with the button
   held, so the copy button and the scrollbar are left alone. The text
   view keeps a selection as points in the window and reads them
   against the text where it stands, so the selection follows the
   scroll with nothing else changed.
32. `crates/ui/src/highlighter/highlighter.rs`
   (`MAX_INLINE_INJECTION_PARSES`): markdown's inline content is parsed
   a block at a time, up to a budget of its own, past which the rest are
   one combined layer as upstream has them all. Combined, an emphasis
   opened in one paragraph closed in another (two Quarto includes,
   `_a.html` and `_b.html`, set the page between them in italics), which
   CommonMark never allows. The test is
   `test_markdown_emphasis_does_not_cross_blocks`.
33. `crates/ui/src/text/markdown_ext.rs`
   (`MarkdownExtensions::parse_options_with`): a view's parse options
   can be changed over GFM's, for the conversation, which turns off what
   chat text sets off by accident (`docs/conversation.md`).
34. The code editor's find. `crates/ui/src/input/overlay.rs`: the panel is shown anew when it opens (`SearchSession::opened`), not whenever the query changes; the panel's own typing changed it, the field was selected again at each letter, and the next letter replaced it. It is still drawn while it closes (`closing`, `closed_at`). `crates/base/src/input/editor/search.rs`: the matcher is a regular expression (`SearchOptions`: case, whole word, regex), escaped unless regex is on, case folded beyond ASCII, and a replacement fills `$1` from its match (`replacement_for`); upstream had one literal through AhoCorasick. `crates/ui/src/input/search.rs`: the panel drawn as the window's find bar, after VS Code's (a chevron for the replace row, the toggles in the field, ↩ and ⌘↩ in the replace field, ⌥⌘C, ⌥⌘W, ⌥⌘R), moving as it opens and closes over its measured height, two rows in a narrow pane; ⌘F in it closes it. Its room around the card is inside the box that closes: padding is the least a box shrinks to, and the close stood still at it before the panel went. `crates/base/src/input/base/state.rs`: Replace is ⌥⌘F, since ⌘⇧F is the window's. Escape in a field whose card is open closes the card and is not passed on (`escape`); passed on, it reached the window, which took it to stop the turn running.
35. `crates/ui/src/tooltip.rs` (`set_label`, `label`): the application
   says how a hover's label is drawn, so the toolkit's own labels (the
   find panel's) draw their keys as the application's do.
36. `crates/ui/src/text/node.rs` (a heading's size): a
   `heading_base_font_size` of zero is the text's own size where the
   view stands, so headings grow with the words under them; from a
   fixed base they stayed as they were when the text was set larger.
37. `crates/ui/src/text/find.rs` (`TextFind`), `text_view.rs`
   (`TextView::find`), `inline.rs`: words found in a text view. The
   application hands the view a shared `TextFind`; each run paints its
   matches behind its letters as it paints (the current one in the
   selection's colour, the others in a grey of it), numbered in the
   order the view paints, and records where the current one stands.

38. `crates/base/src/input/editor/pairs.rs` (`AutoPairs`, `Typed`),
   `base/state.rs` (`set_auto_pairs`, `auto_closed`), `base/movement.rs`:
   in a code editor with the language's pairs set, an open typed puts
   its close after the caret, as VS Code does and only where it does
   (at a line's end or before what ends a term, a quote not after a
   word nor inside a string begun on the line, a bracket not where a
   close further on lacks its open); a close typed where the editor put
   that close steps over it; Backspace between the two takes both; and
   an open typed over a selection puts the pair around it, the
   selection kept. The closes the editor put in are tracked by offset,
   moved with each edit, and let go for good once the caret leaves the
   pair, as VS Code lets its go, so a close typed by hand is never
   stepped over and never taken with Backspace. Typed only: a paste,
   the editor's own edits and an input method composing go in as they
   are.

39. `crates/ui/locales/ui.yml` (`Input`): Cut, Copy, Paste and Select All in Japanese, Korean, French, German and Spanish, the words of a field's menu, so it speaks every language Emaki's window does (`emaki-app/src/i18n.rs`). Upstream has English and Chinese. Kotoba's copy has the same words (its change 31).

40. `crates/ui/src/highlighter/input_adapter.rs` (`hides_words`): a fold is offered only where it hides a line with something on it. A fold keeps its first and last lines and hides those between, and a node that ends at the start of a line (markdown's HTML block, a paragraph) takes the blank line after it along, so a one-line HTML block's chevron hid that blank line and nothing else.

41. `crates/base/src/scrollbar.rs` (prepaint): off a Mac the thumb's place and length come from the offset held to the scroll range, so the thumb stands at its end with its length while the content runs past (it shrank and grew there at every step, and shook); on Windows the thumb is a thin line until the pointer is on the bar or it is dragged, as Windows 11's own bar is.

42. `crates/ui/src/highlighter/registry.rs` (`LanguageRegistry::language`): a name in braces is a Quarto or R Markdown chunk's (`{r setup, echo=FALSE}`, `{python}`, `{=html}`), and its language is the first word inside, lowercased. Looked up as written, a chunk had no colors in the editor or in the markdown view.

43. `crates/ui/src/text/find.rs` (`HeadingSpots`), `text_view.rs` (`TextView::headings`), `state.rs`, `node.rs` (the heading's paint): a view handed a `HeadingSpots` keeps where each heading was painted, in the order the document paints them, as `TextFind` keeps where its match was; an outline goes to a heading by it.

44. `crates/base/src/text_selection.rs` (the mouse-move listener in `paint_text_selection`): a move with the left button up ends the drag, as a release would. The release does not always come: on Windows a press on the window's frame is handed to the window as a mouse-down and the system's resize or move keeps the release, and every later move went on selecting until a click. A test (`a_move_with_the_button_up_ends_a_drag_whose_release_was_lost`) holds it.

45. `crates/base/src/input/base/state.rs` (`InputState::line_top`): how far down the text a buffer line begins, soft wraps and folds counted as scrolling to the caret counts them, and the view's height, so a caller can scroll a line to a place of its own choosing. Emaki's structure glides a heading to a third of the way down the file's editor with it.

46. `crates/base/src/input/base/state.rs`, `movement.rs` (`previous_start_of_word`, `next_end_of_word`): a word step goes from the place it is handed. ⌥⇧← and ⌥⇧→ (Ctrl off a Mac) go from the caret, so a selection made rightward is taken back a word at a time; upstream went from the selection's start, and the first ⌥⇧← dropped the selection and took the word before it. ⌥← lets a selection go from its start and ⌥→ from its end, as the Mac's fields do. A test (`word_steps_go_from_the_caret`) holds it.

47. `crates/ui/src/text/style.rs` (`TextViewStyle::front_matter`), `node.rs` (`render_fields`): a view told its text opens with a file's header of fields, written by the caller as a two-column table, draws that table as rows: a field a row, the key in a quiet column as wide as the longest, keys and values in the mono face, a hairline between rows, and no card of its own (the band around them is the caller's). The cells are the table's own, so a selection runs through them.

48. `crates/ui/src/text/style.rs` (`TextViewStyle::chunk_words`, `chunk_icons`), `node.rs` (`CodeBlock::render`, `ChunkFold`, `chunk_label`, `chunk_icon`): a Quarto or R Markdown chunk (a fence whose language is in braces) has a head row, a chevron, its language's mark from the caller's icon folder and its name, and folds by itself at a click on it: what is under the head closes to it over a moment off the chunk's own clock, by its height as last drawn, kept by keyed state named for what the chunk holds. The hover labels are the caller's words.

49. `crates/ui/src/text/format/quarto.rs` (new), `markdown.rs` (`blocks_of`, `reparse`, `parse_inlines`), `markdown_ext.rs` (`MarkdownExtensions::quarto`), `node.rs` (`BlockNode::Div`, `DivAttrs`, `Heading::attrs`, `ImageNode::align` and `caption`, `TextMark::attr`, `render_div`), `inline.rs` (`with_attr_chip`), `Cargo.toml` (`regex`): Quarto's syntax over markdown, read when the extensions say so. A paragraph is cut at fence lines (`::: {.class}`, `:::`), the pieces between read again on their own with their spans where they stand, and what an open and its close hold nested into a `Div`, drawn by its class (callouts, columns, `layout-ncol`, centering, `.hidden`, else a chip and a hairline). Attributes after a heading go on the heading, after a picture set its width and place, after a link or span are dropped, the span unwrapped. A picture alone in a paragraph is a figure with its alt text as caption, placed by its margins in a block. A raw `{=html}` block or inline is parsed as HTML. Math (`$…$`, `$$`) is read when Quarto's syntax is (`MarkdownExtensions::parse_options`) and drawn as Unicode (`quarto::tex`), display math in a centered block. A `.panel-tabset` is tabs (`render_tabset`, `TabSet`). A heading's attributes and a div's are paragraphs of their own (`Heading::attrs`, `Div::chip`), selectable, drawn as quiet chips. A shortcode left over, and an attribute set, is a quiet chip: inline code in muted ink on a plate of the muted color. Tests at the module's foot (`quarto::tests`) hold it; `cargo test -p gpui-component --lib quarto` in a copy of this folder with Emaki's `Cargo.lock` runs them.

50. `crates/ui/src/text/inline_flow.rs` (`request_layout`): the paragraph's text style and line height are taken as the layout is requested and handed to the measure closure. The closure runs when the window lays the whole tree out, where the style on the stack is the root's: measured in that font and painted in the paragraph's, a wider face (the conversation's serif) wrapped every line's last words onto a line of their own, drawn over the next, in any paragraph with a picture among its words.

51. `crates/ui/src/text/node.rs` (`LinkMark::raw`, `NodeContext::source`), `format/markdown.rs`, `inline.rs` (`right_clicked_link`), `state.rs` (`TextViewMultiClickKind::Link`): a link keeps the markdown it was written as, and a right click on one selects the whole link and keeps it, with its words, for the caller's menu to take.

52. `crates/ui/src/text/math.rs` (new), `markdown_ext.rs` (`MarkdownExtensions::math`), `format/quarto.rs` (`math_node`, `inline_math`, `display_math`), `format/markdown.rs`, `node.rs` (`InlineNode::math`, the paragraph's display line, `selected_run_text`, `emit_run`), `inline.rs` (`with_math`, `paint_math`), `inline_flow.rs` (`MathRooms`): math set as pictures by a typesetter the caller hands the markdown extensions (`MathFace`), asked while the text is parsed. A formula stays a piece of its line's text, a room of no-break spaces in the caller's math face with its picture painted over it on the line's baseline, so it wraps, selects, finds and copies (as its source) with the words; on a line of its own it is a line as tall as its picture with its `\tag{n}` at the right. Without a face it is the Unicode it was before.

53. `crates/ui/src/text/node.rs` (`render_callout`, `render_div`), `style.rs` (`TextViewStyle::callout_icons`): a Quarto callout's options (`appearance`, `icon`, `collapse`, folding on the chunk's clock) and its kind's icon; theorem-like divs, proofs and column classes drawn as their words with no chip.

54. `crates/ui/src/text/format/quarto.rs` (`margin_rows`, `MARGIN_ROW`, `is_margin`, `is_column`), `format/markdown.rs` (`ast_to_document`), `node.rs` (`render_div`): a Quarto document with margin notes set in two columns, each block of the text a row with the notes beside it, the notes wrapping under it where the pane is narrow.

55. `crates/ui/src/text/inline.rs` (`Spots`, `layout_selections`, `text_line_bounds`, `paint_chips`, the mouse-move listener): where each character of a run is, found in one walk of its lines for indices in rising order. `TextLayout::position_for_index` walks the line from its start for each index, and the selection's band, the text's boxes for the selection layer and the code plates asked it twice a character every frame, so a paragraph cost its length squared: 70% of a frame with a long document open. A run wholly above or below what shows has no text boxes. A run draws again under the pointer only when the link under it changes, not at every letter crossed. A line whose glyphs are not in the text's order asks `position_for_index` as before. `the_walk_puts_each_index_where_the_layout_does` holds it.

56. `crates/ui/src/text/node.rs` (`CodeBlock::render`): a code block's actions are made for the block under the pointer only (`on_hover`, keyed state), not for every block at opacity 0: a document of a hundred chunks built a copy button view each every frame.

57. `crates/ui/src/text/find.rs` (`KeptViews`, `TextFind::case_sensitive`), `text_view.rs` (`TextView::kept`), `selection_adapter.rs` (`keep`), `crates/base/src/text_selection.rs` (`TextSelectionHandle::keep_registered`): text views inside a view the caller draws from gpui's cache (`AnyView::cached`) stay in the window's selection. A view registers as it paints and one that did not register in a frame was taken for gone, its selection cleared and its state notified, which threw the cache away every other frame and left nothing to start a selection on. The caller holds a `KeptViews`, hands it to each view, and calls `keep` in a frame the cache is drawn from.

58. `crates/ui/src/text/text_view.rs` (`TextView::text`): the text of a view made from a state the caller keeps (`TextView::new`), set as it is laid out and after its markdown extensions, so the state is parsed once with them.

59. `crates/base/src/input/editor/search.rs` (`InputBaseState::close_search`): the find closed with a match current selects that match, so the caret is where the find came to, as VS Code and Sublime Text leave it. Upstream leaves the caret where it was before the find opened.

60. `crates/base/src/input/editor/search.rs` (`InputBaseState::find_from_selection`), `crates/base/src/input/base/state.rs` (`fold_at_caret`, `unfold_all`, `extend_selection_to`), `crates/ui/src/input/search.rs` (`FindNext`, `FindPrevious`, ⌘G and ⇧⌘G, F3 and ⇧F3 off a Mac, in `SearchPanel`): what Sublime Text's keys need of the editor (Emaki's `sublime.rs`): the query's next or previous match from the selection, the panel open or not, the matches found again in the text as it is; the section around the caret folded or opened as a click on its mark does; every fold opened; the selection's moving end moved with the other kept.

61. `crates/base/src/input/editor/search.rs` (`open_search`, `set_search_query`): the find begins at the selection, or the caret, and the match it is on is the first from there as the query changes, as VS Code's and Sublime Text's are. Upstream began at the top of the view, and a query typed went back to the first match, so a word selected lower down was not the match the find was on.

62. `crates/base/src/input/base/state.rs` (`cursor_wrap_row`, `start_of_line`, `end_of_line`), `movement.rs` (`update_preferred_column`, `end`, `vertical_offset`): ⌘→ on a row that wraps stops after its last word, the spaces it broke at left; and a caret at a soft wrap put there as a row's end is on that row, as it is drawn, for ⌘←, ⌘→, and the column and row ↑ and ↓ start from. Upstream went past the spaces to the next row's start, where the caret was drawn a space past the word, ⌘← went to the start of the whole line and ↓ two rows down.

63. `crates/base/src/scrollbar.rs` (`SETTLE`, `born`, the state's first value): a bar drawn anew starts from its scroller's offset, and for `SETTLE` after it takes a change of the offset as where it starts, not as a scroll. A scroller kept between draws (Emaki's outline, in a drawer that comes and goes) showed its bar each time it came, and again as it moved itself to the entry in view.

64. `crates/base/src/input/base/movement.rs` (`move_to`, `vertical_offset`): the caret is put on a character's first byte, whatever offset it is moved to. ↑ or ↓ onto a line not laid out kept the line before's column in bytes, which could fall inside a character (CJK, an emoji, an accent), and the next letter typed or Enter sliced the text there and the app aborted.

65. `crates/ui/src/text/state.rs` (`parse_content`, `parse_unguarded`, `SYNC_PARSE_MOST`, `ParseMode::Replace`): the parse runs under `catch_unwind`, and a text the markdown crate panics on is parsed again as one block of code; a full replacement longer than `SYNC_PARSE_MOST` is parsed by the background task and shown when done, not on the main thread. A panic in the parse aborted the app from either thread, and a long table in a markdown file held the window for seconds as it opened.

66. `crates/base/src/input/base/state.rs` (`replace_text_in_range`, the input method's `replace_and_mark_text_in_range`, `replace_bytes`, `on_chars`): before any text goes in, the selection and the marked range are put on characters' starts, and so is the range a caller replaces. A caret inside a character (one left by ↑ or ↓ onto a line not laid out, number 64) made the next letter typed slice the text there; the panic could not unwind out of AppKit's `insertText:`, and the app aborted. This was the crash a v0.3.2 user hit while typing in the file's editor.

67. `crates/base/src/input/base/state.rs` (`scroll_to`): the row an offset is scrolled to is its wrapped row from the display map. Upstream added the row within its line from the last layout, which has only the lines in view, so an offset deep in a long paragraph out of view scrolled to the paragraph's first row: the find's first step to a single match stopped short of it, and the second step found it. A test (`test_scroll_to_offset_in_wrapped_line_out_of_view`) holds it.

68. `crates/ui/src/text/node.rs` (`render_list_item_row`): an item of a list that is not numbered is marked by a hairline, a short thin line at the middle of its first line in the quiet ink, shorter below the first depth, in place of upstream's "•", "◦" and "▪". A numbered list keeps its numbers (`list_item_prefix`).

## Updating

Check out the new upstream revision, copy the four crates over these, re-apply
the changes listed above, update the revision here, and build.
