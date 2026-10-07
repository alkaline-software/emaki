# What the transcript teaches

Facts about Claude Code's transcript and registry files that
`crates/emaki-core/src/build.rs`, `transcript.rs`, `archive.rs` and `peer`
depend on. The code honours each; keep it that way.

## Rules

- **Never feed the archive its own file as a source.** Once a session
  outlives its original it re-enters the index pointing at the archived
  copy. Without the guards in `archive.rs`, the rotate-then-copy path
  renames the destination aside, fails to read the source it just moved,
  loses the canonical file and rotates again on every sweep. Two
  independent checks exist because the failure is silent and permanent.
- **Subagent rows are all `isSidechain: true`.** They are a sidechain of
  the parent but the whole conversation at their own level, so a nested
  build must not filter on it or it produces zero rounds.
- **Subagents link back explicitly.** `agent-*.meta.json` carries
  `toolUseId`, naming the exact `Task` call that spawned it. Use that, not
  contiguous-run guessing, which cannot tell two parallel subagents apart.
- **`ai-title` is not always a title.** Under a named agent, Claude Code
  overwrites that field with the agent's name. `pick_title` takes the
  newest title that is neither a known `agentName` nor slug-shaped.
- **A rewritten transcript must replace, not append.** `--resume` and
  compaction rewrite in place. `TranscriptTail::restarted` signals it
  (`transcript::file_id` tells a rewrite from an append) and the reader
  drops its accumulated rows; without that the session doubles.
- **Cache-read tokens are not a total.** Every assistant message re-reads
  the whole cached prefix. `Usage::total` excludes `cache_read`.
- **`system` rows with `subtype: local_command`** carry the same wrappers
  as user rows (`<command-name>`, `<local-command-stdout>`). Route them
  through `strip_wrappers` or raw markup lands in the log.
- **Raw HTML in a transcript is content, not markup.** Someone writing
  "maybe the `<aside>` option" means those characters. The markdown view
  must escape it, never render it.
- **User rows Claude Code wrote itself are not prompts.**
  `build::machine_authored` says which; `turn_state`, `handle_user` and
  `first_prompt_title` all skip them. See Compaction.
- **A peer row is `isMeta: true` and still a prompt.** The builder and
  `turn_state` must treat it as one, or the log shows a reply to nothing.
- **Do not ignore `attachment` rows.** A message sent mid-turn exists only
  as one.
- **A rule line on Claude Code's screen may carry text.** Test for one
  with `driver::is_rule` only.
- **Do not assert `from-mode`** in the cross-session envelope.

## Slash commands

A slash command answers its own prompt. `/compact` typed in the terminal
is written as a user row saying `/compact`, then the rows the command
leaves (`<command-name>` in a user row, or a `system/local_command` row
with `commandRun`), and no assistant row. `turn_state` would read the
prompt as one waiting on a reply, and the board would say working until
the next turn. So a prompt that is a slash command with a command row of
the same name after it is skipped, and the state is the turn before it. A
skill (`/review`) leaves no command row and is a prompt like any other.

In the conversation the command's own chip is left out when the prompt
above already says it. `/compact`'s output line, "Compacted (ctrl+o to
see full summary)", is the terminal's instruction and is dropped, so the
round reads as the prompt and the boundary's notice.

Only that command's output goes. `RoundBuilder::command` is the command
the next output belongs to: a `/model` run later, still in the `/compact`
round because no prompt came between, keeps its "Set model to …" line.
Tried and dropped: dropping every output in a round that began with
`/compact`.

## The rule over the prompt

Once a session has a name (`/rename`), Claude Code writes it on the rule
above its prompt ("──── My session ─"). A reader that takes a rule to
be a line of "─" and nothing else finds no prompt on a renamed session:
`type_message` waits for one that is already there, and a message sent
from the window leaves the composer and goes nowhere. `driver::is_rule`
is the one test, for the prompt, the suggestion and the dialog: a line
that begins and ends as a rule and is mostly one.

## Blank sessions

A session nothing was said in is listed nowhere. `/clear` starts a new
transcript under a new id and carries the session's name into it
(`custom-title` and `agent-name` rows, then the `/clear` rows), which
otherwise shows as two sessions of one name, the second empty.

`peek` marks a transcript `blank` (`SessionRef::blank`,
`transcript::says_something`) when the whole file fits in the head it
reads and holds no reply, nothing queued and no prompt. A command that
acts by itself (`/clear`, `/resume`) is not a prompt; a skill is.

The index keeps such a session, so the archive copies it like any other.
The window drops it as the index arrives (`HubEvent::Index`), and its tab
with it, unless it is the one showing or a process of ours is behind it
(a session begun here is blank for a moment). `emaki-core list` leaves it
out. Once something is said there it is a session like any other.

## A change that lands during a read

The conversation showing is reloaded when its file changes
(`HubEvent::Changed`). A change that comes while a load is under way must
be read again: `reload_wanted` marks it, and the load, once back, runs
again. A prompt with a pasted picture is one long row: a load that began
as it was being written read the file without it, the small rows 8 ms
later were dropped, and nothing else was written until the agent's first
words fifteen seconds on, so the person's message was missing meanwhile.

## Messages sent mid-turn

A message sent mid-turn is never a user row. Typed in the terminal or
sent from the window while the agent is working, it is absorbed into the
running turn (`queue-operation` `remove`, reason `absorbed_mid_turn`) and
written as an `attachment` row of type `queued_command` with
`commandMode: prompt`, the text or content blocks under `prompt`, and the
`origin` a user row would carry. Seventeen such messages on the machine
this was built on, none with a user row to match.

`build::queued_prompt` turns the row into the user row it stands for, and
`handle_user` opens a round for it in its place, with what the agent did
next under it. `image_block_bytes` reads a pasted picture from
`attachment.prompt` by the row's uuid. The `task-notification` rows in
the same shape are the harness's and stay out. The turn state is
untouched: the turn it cut into is still running.

The attachment row is written only when the agent takes the message up,
at its next step, which behind a long tool call was half a minute later.
What is written at once is the queue: a `queue-operation` row, `enqueue`
with the message as `content`, then `dequeue` (the front one starts a
turn of its own) or `remove` with the same content (absorbed).
`build::Queue` replays them. Whatever is still queued at the end of the
file is drawn as a round at the foot of the conversation with
`Round::queued` set, saying "Queued, Claude will read it at its next
step", until the attachment row lands and the next build puts it where it
was taken up. A harness notification in the queue is kept for the order
and never drawn. Replayed over every transcript on that machine, 296
enqueues met 222 dequeues and 74 removes, and nothing was left over.

The `enqueue` row has the words only ("[Image #23]…"); the picture is in
no row until the message is taken up. `Workbench::dress_queued` puts the
attachments of the message as it left the window (`last_sent`) on the
queued round when the words match. A message queued from a terminal
shows without them.

## Compaction

After the boundary, Claude Code writes the summary it hands the model as
a `user` row flagged `isCompactSummary` (and `isVisibleInTranscriptOnly`:
its own view hides it too). Read as a prompt it opens a round of the
person's, "This session is being continued…", and leaves the board
working. `build::machine_authored` covers that row and any `isMeta` row
without a peer origin.

The boundary row's `compactMetadata.postTokens` becomes `context_tokens`
at that point, so the row under the composer drops to the summary's size
when compaction lands, not at the next assistant row.

## The inbox

Claude Code 2.1 registers every session in
`~/.claude/sessions/<pid>.json` with a `messagingSocketPath` and
publishes the token a peer must present in `<pid>.<hash>.key` beside it.
The wire is two JSON lines on that socket: an `auth` frame, then a `user`
frame whose content is wrapped in Claude Code's own
`<cross-session-message from-name="emaki">` envelope. The envelope is
what makes the transcript row carry `origin.name` and a clean
`origin.body`. `build` keys on those, so a message from the window
renders as yours and one from another Claude session as a peer.

**The same words twice go with a space after them.** Claude Code drops a
peer's message identical to that peer's last one within thirty seconds
(2.1.289: `dedupWindowMs: 30000`, keyed on the sender's name or pid; the
terminal says "Dropped a peer message from @emaki (unknown): identical to
the previous message from this sender"). It guards against two sessions
echoing each other, and also catches a person stopping a turn and
sending the message again. `Hub::send_to_inbox` remembers what last went
to each session (`inbox_last`) and `peer::send` takes `again`, which adds
one trailing space. The envelope keeps it, the message is still read as
ours, and the prompt is trimmed where it is drawn. Chosen over a notice,
as the case is rare.

**`from-mode`.** Claude Code holds a message that asserts no permission
mode when the recipient runs with permissions bypassed, and asks in the
terminal. That check is what stops a less trusted process steering a
more trusted session, and Emaki is such a process as far as Claude Code
can tell. The person's remedy is `crossSessionInbound: accept` in their
own settings.

## Probing

- `emaki-core pty` resumes a session on a pty, a renamed one included.
- `emaki-core inbox <id> --again <text>` sends with the trailing space.
  The drop is keyed on the pid, so two runs of the CLI do not reproduce
  it; only the app does.
