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
- The cards that come up while a session runs are drawn under the last
  round, in the list's last item (`render_list_item`;
  `docs/live-turn.md`, The cards). They are not an item of their own:
  the list's count is the rounds', and everything that goes by it stays
  as it is.
- The hover lines under a prompt and a reply are always laid out
  (`HOVER_ROW_H`) and hidden by opacity, so nothing moves when they appear
  and the list has nothing to re-measure.
- Every text goes through `markdown_is_safe` before the markdown view gets
  it. The `markdown` crate the toolkit's `TextView` parses with (1.0.0)
  panics on some inputs ("Cannot push to non-parent"), and a panic inside
  an element's layout aborts the app.
- Each place that draws the moving agent mark passes its own animation id.
- A conversation is read as markdown through `chat_md_view`, never
  `md_view`, which reads a file's or a page's as GitHub does. A sign
  plain text uses for something else is taken out of what the
  conversation reads, as a whole construct (`chat_parse_options`), never
  as a fix for one text. (What markdown a conversation reads, below.)
- The markdown extensions a view is given are made once
  (`CHAT_MARKDOWN`), never at a draw. A set made anew takes a new
  revision, and the view parses its text again at every new revision:
  every text was parsed again every frame, and a selection was gone the
  moment the mouse let go.

## The end, and what is new

A conversation opens at its end and follows it (`FollowMode::Tail`): the
list pins to the end at each layout, and stops when the reader scrolls
up. Two things are ours, for prompts and replies alike:

- **Near the end counts as at it.** When more arrives and the reader is
  within `NEAR_END` of the end and not mid-scroll (no wheel for
  `SCROLL_REST`), the list is set following again (`set_detail`). gpui
  alone resumes only at the very end, so a reader a line short of it was
  left behind. How far off they are is the list's last layout, from
  before what arrived.
- **Further up, a pill says so.** `Detail::seen` is how much of the
  conversation the reader has been at the end of (`shape_of`: the rounds,
  and the items of the last), kept up while the list follows. With more
  than that and the list not following, "New messages" stands over the
  conversation's foot (`unread_pill`, rising in and fading out). A click
  goes to where the new part begins (`unread_show`) and counts it seen.
  A new round is an item the list can go to. An item part way down a
  round is not: a mark of no height is drawn before that item
  (`render_round`, `unread_go`), which says at every frame where it is.
- **The way there is run from where the reader is** (`UnreadGlide`,
  `glide_step`): each frame the list moves a part of what is left, so
  the conversation is seen going down and slows onto the place. A way
  longer than `UNREAD_CAP` is skipped down to that first. A round not
  drawn yet has no place: the list runs toward it a few frames, which
  draws what it passes, and is put a little short of it if that does not
  find it. The move ends at the place, at the list's end when the place
  cannot come up to the top, or when the reader takes the wheel. Tried
  and dropped: putting the list a stretch short of the place in one
  frame and running only that stretch, which read as a jump.

## A message sent, before it is written

An agent writes a prompt to its transcript a moment after it takes it,
and one that has to be started first takes a second or more. Until then
the message stands at the conversation's end as it left the window
(`Workbench::outgoing`, a message a tab; `show_outgoing`).

- It is a round put on the session in `set_detail` (`wear_outgoing`),
  with `OUTGOING` for its `uuid`, and never on disk. It goes when the
  transcript has a round written since it was sent, past the rounds
  there were then, or a prompt withdrawn since. The transcript's round
  is then the same item of the list, so nothing is spliced.
- A session started from the home page has no file to read yet: it gets
  a record of our own and an empty conversation to stand the message in
  (`show_begun`; `docs/window.md`, A session begun from a folder).
- A message that did not go (`HubEvent::Sent` with an error,
  `DriverFailed`) keeps its bubble, and the line under it says "Not
  delivered" in the danger colour, all the time, with the reason under
  the pointer and the copy button beside it. The next message sent on
  the tab takes its place.
- `restore_prompt` passes it by: it is not a round the agent was
  stopped on.
- A slash command gets none. It has no round to wait for.

## The two voices

A prompt's bubble has a tail at its foot on the right, as a message
sent has in iMessage: `icons/bubble-tail.svg` in the bubble's colour,
an absolute child hanging past the bubble's edge.

The bubble stops short of the column's left edge and a reply as short
of its right (`REPLY_INSET`), a share of the column, which is as narrow
as the panels beside the conversation leave it. A long prompt fills the
width it is given, and without the inset it sat edge to edge like a
reply. The bubble has no most of its own: it had one, and in a wide
column stood further from its edge than a reply from its own.

The bubble is the one item of a row, which shrinks it to the room there
is (`min_w_0`). As an item of a column aligned to its end it took the
width of its words up to its most, and in a column narrower than that ran
off the left edge, with its height still that of the narrow wrap.

## Code blocks

A block of code does not wrap its lines: one wider than its room scrolls
sideways (the vendored `node.rs`, listed in `UPSTREAM.md`). Wrapped,
anything set out in columns (a table, a mock of a screen) came apart, and
the terminal was where it could be read. The scroller is the toolkit's
own, the one a table has, and takes a sideways gesture only, so a wheel
over a block still moves the conversation. Where more lies past an edge
the text fades out there: cut off clean, a line read as all there was. A
selection dragged to a block's edge scrolls it that way, as a field does
(`code_drag`, vendored `node.rs`).

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

- An Explain button's change between off and on is drawn in motion on
  the button pressed and no other (`explain_flip`). gpui plays an
  animation whenever its element is first drawn, so with the animation
  on every button, each one in a run of calls flashed the accent as the
  run opened or shut.

- The badge at a card's left is the tool's own name in lower case
  (`tool_label`; for an MCP tool the last part of its name), then the
  subject. Its colour is what the call does (`tool_badge`, by
  `ToolKind`): a command gold, a read blue, a search teal, an edit
  violet, a written file green, the web rose, an agent or a skill the
  accent, each one of code's inks (`docs/look.md`) on a wash of itself.
  Anything else is the muted grey. A subagent's calls are drawn the same way. Tried and dropped: a
  kind badge ("run") beside the name ("Bash"), the same word twice.
- `tool_has_body` says yes whenever a result came back, a Read included.
  Text shows clipped in the file's language. A picture is read back out of
  the transcript as a pasted one is (`ToolCall::result_uuid` and
  `result_index` name the result row and block;
  `transcript::image_block_bytes` takes the first image inside a
  `tool_result` block) and drawn as a tile that opens the lightbox.
- A call that is running says what the agent says it is for, in the
  page's own face (`ToolCall::saying`: a Bash call's `description`, a
  subagent's), on its card, on a folded run and as the session's
  activity (`build::turn_state`). Claude Code's terminal sets that line
  over a running command and takes it away when the command is done, and
  so does the card: it is then the command again.
- Three or more consecutive tool calls, thoughts between them included,
  fold into one row (`render_run`): the count, a tally by `tool_label`, the
  last subject, the total time, and the turning mark with "running…" while
  one is going. Opened, each call folds on its own. Fewer stay inline.

## Opening a card brings it into view

A tool call, a run or a thought opens over a moment (`docs/window.md`,
Motion), and may run past the foot of the conversation. It is moved up
until its end is in view, and no further than brings its head to the
top, so one taller than the view is read from its start
(`Workbench::reveal_now`, `reveal_mark`, `reveal_arrive`).

- The click names what was opened, and for as long as it opens a canvas
  over it takes its bounds at each draw and the list is moved by what
  is over. So the conversation moves with the card, at the card's own
  pace, and there is no second animation to keep in step.
- At the conversation's very end the list hangs from its foot and goes
  on following it while the card grows a little at a time, so the card
  grows upward. Its head is then held at the top, by the same step.
- Closing moves nothing, and neither does a card opened by the find
  bar, which goes to its hit itself.

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
anywhere, Escape closes, and so does ⌘F with the keyboard in the bar.
⌘F opens it from the composer too: the toolkit's field binds the key to
a search of its own and, with none, passes it on (a change of ours in
`vendor/`; upstream ended the key there).

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

## What markdown a conversation reads

A prompt is typed as plain text (the composer is plain text) and a reply
is read in the agent's terminal too, where markdown is not drawn. So
the conversation reads GFM less what text sets off by accident
(`chat_parse_options`, given to the toolkit's view through its
`parse_options_with`):

- No setext headings. A line of `-` or `=` under a line is a line of
  its own: a person splitting a list with a dash turned the lines above
  into headings. `---` after text is a rule, as a model writing it
  means.
- No indented code. Four spaces in is text; code is fenced.
- No raw HTML. `<Input>` in a sentence was parsed as a tag and drawn
  as nothing.
- One tilde is a tilde. "~30% … ~40%" struck the words between;
  strikethrough takes two.

A construct is turned off here only where prose sets it off by
accident and what was meant can still be written another way. Emphasis
needs no rule: CommonMark already leaves an underscore inside a word or
after a space unclosed.

## The markdown safety check

The smallest panicking input found is a paragraph followed by two `---`
lines, which YAML front matter quoted in a Codex tool result produces.
`markdown_is_safe` runs the same parser under `catch_unwind`, once per
distinct text and with the options the view parses with (a
conversation's, or a document's), and `md_view` shows an unsafe text as
a code block.
`markdown` is a direct dependency of the app at the toolkit's version for
this one call.

## Probing

- `EMAKI_GO=toolopen` opens the last `Bash` or `Write` call that is not
  in a folded run, as a click does; with `EMAKI_SCROLL_DEBUG=1` it
  prints where the card stood and how far the view was moved.

- A `mouseMoved` `CGEvent` sent with `postToPid` moves gpui's hover without
  moving the real pointer, so a hover state is one event and a window
  capture away.
- Mouse-down and mouse-up sent the same way do not reach a click handler in
  a background window.
