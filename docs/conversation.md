# Drawing a conversation

About `crates/emaki-app/src/transcript.rs`, the conversation list in
`crates/emaki-app/src/workbench.rs`, and `crates/emaki-core/src/find.rs`.

## Rules

- Never splice a list item the reader may be inside. gpui's `ListState`
  moves the scroll anchor to the start of any spliced range that contains
  it, and a live session's last round is one tall item, so a re-splice on
  reload throws the reader to the top of that round.
- `set_detail` tells the list only about a change in count (append new
  rounds, reset on a rewrite). Toggling a tool, thought, run or thumbnail
  just notifies: items render from the current session on every frame and
  are re-measured as they render.
- The hover lines under a prompt and a reply are always laid out
  (`HOVER_ROW_H`) and hidden by opacity, so nothing moves when they appear
  and the list has nothing to re-measure.
- Every text goes through `markdown_is_safe` before the markdown view gets
  it. The `markdown` crate the toolkit's `TextView` parses with (1.0.0)
  panics on some inputs ("Cannot push to non-parent"), and a panic inside
  an element's layout aborts the app.
- Each place that draws the moving agent mark passes its own animation id.

## Copy buttons and times

A block of code has a copy button at its top right while the pointer is on
the block: `md_view` gives the toolkit's `code_block_actions` its own
`Clipboard` button, and the vendored `node.rs` shows the actions under the
pointer only.

Under a prompt's bubble, on the right: when it was sent (`format::stamp`)
and a copy button. Under a reply's last line, on the left: the button, then
the time of the round's last text item. Both are gpui groups
(`prompt-<ix>`, `reply-<ix>`), shown while the pointer is over the prompt's
row or anywhere in the reply.

The button copies the markdown as written, not the rendered text: the
prompt's words, or `Round::reply_markdown` (the round's text items with a
blank line between, tool calls and thoughts left out), read out of the
session at the click (`Workbench::copy_text`). A prompt that was not the
person's own says whose on the same line ("Another session", "Session").
Tried and dropped: a permanent line under each prompt with duration, tool
count and tokens.

## Tool cards and runs

- The badge at a card's left is the tool's own name in lower case
  (`tool_label`; for an MCP tool the last part of its name), then the
  subject. A subagent's calls are drawn the same way. Tried and dropped: a
  kind badge ("run") beside the name ("Bash"), the same word twice.
- `tool_has_body` says yes whenever a result came back, a Read included.
  Text shows clipped in the file's language. A picture is read back out of
  the transcript as a pasted one is (`ToolCall::result_uuid` and
  `result_index` name the result row and block;
  `transcript::image_block_bytes` takes the first image inside a
  `tool_result` block) and drawn as a tile that opens the lightbox.
- Three or more consecutive tool calls, thoughts between them included,
  fold into one row (`render_run`): the count, a tally by `tool_label`, the
  last subject, the total time, and the turning mark with "running…" while
  one is going. Opened, each call folds on its own. Fewer stay inline.

## The agent's mark

`agent_glyph` turns Claude's starburst (`assets/icons/claude.svg`) and
makes it breathe inside a fixed box, so nothing around it moves; Codex's
glyph only breathes. It moves whenever `is_working` holds (the phase is
working or planning): in the status row under the transcript, the top bar,
the session rows and the sessions page. The round header's
mark is still. Tried and dropped: a mark at the foot of the conversation,
one too many beside the status row.

## Find (⌘F)

`find.rs` lowers every prompt, reply, thought and tool call (arguments,
output, a subagent's rounds counted against the Task call) once per session
load. A query is a substring scan over that, so the answer is what the page
shows and needs no index. ↩ and ⇧↩ step from the field, ⌘G and ⌘⇧G from
anywhere, Escape closes.

- Stepping scrolls the hit's round to the top (`ListState::scroll_to`, item
  offset zero; the list cannot address a point inside an item) and unfolds
  what hides the item: tool card, thought, folded run
  (`transcript::run_start`).
- Every hit wears one accent ring (`find_wrap`), faint for a hit and full
  for the one the bar is on, a folded run with a hit inside included. Tried
  and dropped: tints and edge bars.
- A live reload recomputes hits without moving the reader (`compute_hits`).
  Typing lands on the first hit at or after the round in view (`run_find`).
- A search-palette hit opens its session through `open_with_find`, which
  fills the bar and lands on the round once loaded (`find_pending`). The
  search index numbers rounds from one, the model from zero.

## The markdown safety check

The smallest panicking input found is a paragraph followed by two `---`
lines, which YAML front matter quoted in a Codex tool result produces.
`markdown_is_safe` runs the same parser under `catch_unwind`, once per
distinct text, and `md_view` shows an unsafe text as a code block.
`markdown` is a direct dependency of the app at the toolkit's version for
this one call.

## Probing

- A `mouseMoved` `CGEvent` sent with `postToPid` moves gpui's hover without
  moving the real pointer, so a hover state is one event and a window
  capture away.
- Mouse-down and mouse-up sent the same way do not reach a click handler in
  a background window.
