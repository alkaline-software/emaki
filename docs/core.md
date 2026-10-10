# Core

About `crates/emaki-core/src/`: `paths.rs`, `adapters/`, `archive.rs`,
`search.rs`, `explain.rs`, and the `bench` command.

## Rules

- Nothing may write a hook into Claude Code's settings. The Python daemon
  wrote seven, each an absolute path into the checkout; when the folder was
  renamed, Python exited 2 on the missing file, and Claude Code reads exit
  2 as "block", so it refused every prompt.
- If a hook is ever needed, it is a binary at a stable path outside the
  repository that never exits 2. The status line, the one thing the app
  writes into Claude Code's settings, is held to the same two rules.
- The archive's old shape is still taken in. One made by the Python
  Emaki, or by any Emaki before v0.2.0, is moved into today's by renames
  (`archive::settle_layout`); nothing in it is deleted or copied.
- Nothing above the adapters (archive, search, the window) branches on the
  agent except to label it.
- Nothing is read from or copied into an archive until
  `settle_layout` says it is in shape. In the old shape a project of
  Claude Code's could be named `codex`, and would be taken for Codex's
  folder.
- `fts_query` quotes every term. People type `rm -rf`, `a:b` and a lone
  `"`, all FTS5 syntax that would raise mid-keystroke.
- The explainer's child is never `--bare`, and its `cwd` stays under
  `~/.emaki/run/explain`: it is a real Claude Code session and leaves a
  transcript, which that folder keeps out of the index.
- The tests never call a model.

## What is on disk

`~/.emaki`, in the layout the Python Emaki used but for the archive:
`archive/<agent>/<project>/<session>.jsonl` plus sidecars, `logs/`, the project
registry in `state/projects.json`, `config.json`,
`cache/explanations.json`. `~/.emaki/index.db` was the daemon's search
index; nothing reads it now.

## The archive

`archive.rs`. Every session is in it from the first time it is seen, not
from when its agent is about to delete it: the deletion is silent, the app
may not be running that week, and the thirty days is a setting.

- **A copy the agent still has is a clone** where the filesystem can
  (`clone_over`, APFS): a second name for the same blocks, made again
  whenever the source has grown. It takes next to no room, and it is a
  picture of the source and not a link to it, so nothing done to the
  source reaches it. Elsewhere the bytes are copied and what is new is
  appended. Hard links were not used: the archive would be the source's
  own file, and whatever cut the source short would cut the archive.
- **A source that grew is taken only when the archive is still how it
  begins** (`still_begins_with`). Claude Code rewrites a transcript in
  place at times, and one rewritten and longer was once taken as one that
  had grown: its new end went after the old copy, which then held rows
  twice or lacked some. A copy made before this is compared with its
  source once (`Entry::shared`) and taken again when it is not the same.
- **Nothing is set aside that holds nothing.** A copy that is not the
  source is kept as a generation file (`<name>.genN.jsonl`, listed as a
  session of its own) only when it holds a row the source lacks
  (`nothing_lost`); one that is merely behind is replaced.
- **A copy the agent no longer has is packed** (`pack_stale`), a day after
  it was last written: it is the only copy, takes its full size and never
  changes again. The filesystem does it (`ditto --hfsCompression` on a
  Mac, `compact` on Windows, nothing on Linux), so the file keeps its
  name and every reader, ours and any other program, gets the same bytes.
  On a Mac the packed copy is made beside the file and compared byte for
  byte before it takes its place. A `.gz` beside the file was not used:
  a dozen readers go by a transcript's name and seek in it.
- **When** is the hub's (`spawn_scanner`): every session at launch and at
  quit; after that a session whose file changed, once its turn is over,
  and one still at work every five minutes. The scan every few seconds is
  the window's and only reads sizes and times.

The markdown a session renders to is byte-identical to the Python
renderer's except JSON key order inside tool arguments and the `You (web)`
label, now `You (emaki)`.

## Adapters

The shape is borrowed from Wake (`iAmCorey/Wake`): an `Adapter` knows an
agent's data roots, lists sessions cheaply, peeks one, and parses one into
the shared model. Claude Code and Codex exist. A new agent is a new file
under `adapters/` and a variant of `AgentId`.

The agents a machine can have, read or not, are `agents.rs`
(`docs/agents.md`).

The archive is a folder an agent, `archive/claude/` and `archive/codex/`,
and in each a folder a project. Until v0.2.0 Claude Code's projects were
at the archive's top and Codex was in `_codex`, the underscore keeping it
apart from a project's name. `settle_layout` moves that shape into this
one in two steps, so that stopping anywhere leaves something the next
call finishes: first every folder without an underscore goes into
`_claude` and `.layout` is written, then the underscores come off. A
project an older Emaki leaves at the top afterwards is taken into
`claude/`; a file both have comes in as a generation unless its bytes are
the same.

Gemini CLI had an adapter for one commit and lost it: in June 2026 Google
closed that program to personal accounts, so most people could not sign in
to it.

### Codex

`adapters/codex.rs` reads a rollout
(`~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`).

- **A rollout says most things twice, and the items are the source.**
  What the model was sent and what it answered are `response_item` rows,
  which every Codex writes. A newer one also writes what happened as
  `event_msg` rows of type `item_completed`: the prompt, each thing the
  agent said (commentary and the final answer, both shown), each thought,
  each command run, each file changed. A response row that says what an
  item already said is dropped; one with no item to match is read as it
  always was. This is decided row by row, not once a file: a rollout
  moved up from an old Codex has the prompt's row before the turn starts
  and its item after, and a session imported from Claude Code has items
  for some turns only.
- **A tool call is drawn as Claude Code's would be.** A command
  (`CommandExecution`) is a `Bash` call; a patch (`FileChange`, or the
  `apply_patch` call of an older Codex) is a call a file, `Write` for one
  added, `Edit` for one changed, with hunks in the shape of
  `structuredPatch`; `request_user_input` is an `AskUserQuestion` call
  with its answers; a plan is an `ExitPlanMode` call. The window and the
  markdown need nothing of their own for Codex.
- **Code mode.** Codex 0.162 has the model write a script
  (`custom_tool_call` named `exec`) and runs its tools from it. The items
  after the script say what it ran, and the script's own call is dropped
  once one has. What only the script has is what it printed: where it ran
  one command and that command's own output is empty, the print is the
  command's output. A command the sandbox refused leaves no item, so a
  script with none is shown itself: as the command, when all it does is
  run one, else as the script under its first line.
- **A command that outlasts its script's wait is one call.** The
  script that starts it gives up waiting after ten seconds and a second
  script only waits on it (`write_stdin` with nothing typed); the
  command's item comes under that one. The item takes the place of the
  first script's call, where the command began, and a script that only
  waits is no call.
- **A setting's change is written when it is made**
  (`thread_settings_applied`), not with the next prompt as Claude Code
  has it, so the window says no mode of its own for Codex
  (`Workbench::unwritten_mode`). A model is named as Codex's list
  writes them (`driver::model_label`).
- **A stop is written before the command it cut short.** The item of a
  command stopped mid-run comes after `turn_aborted`; it is put above the
  stop's line, which stays the last thing the round says.
- **A plan has no answer in the rollout.** The last one waits
  (`Pending`). The next prompt settles it: sent out of plan mode the plan
  reads as approved, sent in plan mode as not approved. That is read off
  the mode, not off anything Codex records.
- **What a session is set to** is in `turn_context` and
  `thread_settings_applied` rows. `Session::mode` is one key: `plan` in
  plan mode, else the permission profile's id without its colon
  (`read-only`, `workspace`, `danger-full-access`, or a profile of the
  person's by its name), else the same three from the sandbox policy an
  older row names. The first value is where the session began; a later
  one that differs is a line at the foot of the round before, as for
  Claude Code (`docs/modes.md`).
- **A turn's ends are explicit**: `task_started`, then `task_complete` or
  `turn_aborted`, kept as `Session::turn_open`. A `task_complete` with an
  `error` is a turn that failed, and the round ends on the error.
  `adapters::turn_state_from_session` goes by `turn_open` where there is
  one, since words said in an open turn are commentary and not the reply;
  a rollout without those rows is read off the model's tail as before.
- **Tokens**: `token_count` rows are running totals with cached tokens
  inside `input_tokens`. The last request's `input_tokens` is the context
  in use, and `model_context_window` the size it is in. The same row
  carries the account's usage windows (`rate_limits`: `primary` and
  `secondary`, each with its length in minutes, either of which can be
  null), kept as `Session::usage_windows` for the row under the composer
  (`docs/composer.md`). A row can have the windows with `info` null.
- **The title** is the thread's name when the person gave one. Names are
  not in the rollout: `session_index.jsonl` has a row a naming, the
  newest last. Nothing else in Codex's home is opened.

## Presence, without hooks

There is no daemon. A session is live by the registry, by a driver of our
own when we started the process, and by an mtime grace for anything else.
A fresh file with neither is a session that just ended (an interactive
Claude Code always has an inbox), so the composer offers to resume it.

A `statusLine` command cannot block anything: if it fails, the terminal's
line goes blank. The status line is described with the limits row, in
`docs/composer.md`.

## Search

`~/.emaki/search.db`, FTS5, one document per item (a prompt, a reply
paragraph, a thought, a tool call): coarser and a hit in a long session
says nothing about where to look, finer and snippets lose context. A hit is
a matching document, not a term occurrence. Sync is incremental on (size,
mtime), on its own thread. A trailing `*` survives the quoting as a prefix
match.

## Opening time

Tried and dropped: a tail-first reader. The largest transcript on the
machine this was built on (137 MB) reads in about 80 ms and builds in about
80 ms more. Bring it back only if bench shows a session over about half a
second.

## The explainer

`explain.rs`: opaque tool calls in plain words.

- Asked about: a `Bash` heredoc, a piped chain, anything with an opaque
  shape (`OPAQUE_MARKERS`) or longer than `explain.min_chars`. Simple calls
  get a canned line from their own arguments (`explain::canned`) and never
  reach a model.
- The call: `claude -p --model claude-haiku-4-5` with a fixed system
  prompt, no settings, no MCP servers, no tools.
- Answers are keyed by a hash of the call's name and sorted arguments and
  kept in `~/.emaki/cache/explanations.json`, bounded. `Explainer::lookup`
  is cache-only and `attach` folds cached answers into a loaded session, so
  a command explained once is explained wherever it appears.
- `paths::is_explainer_cwd` makes the index drop the child's sessions, and
  `prune_transcripts` sweeps their files hourly.
- The hub owns one `Explainer` and asks it the moment a `Permission` event
  arrives, so the card has the answer while the person decides. A tool
  card's *Explain* button asks with `force`. Scope `all` also asks for the
  tail of the session showing on each live reload, never on a first open.
- Answers come back as `HubEvent::Explained` into
  `Workbench::explanations`, an overlay by call id that the next load makes
  redundant.

## Probing

- `emaki-core bench [<id>...]` times read, build and render for a
  transcript (the five largest by default).
- `EMAKI_TIMING=1` makes the app print load and hand-over time per open.
- `emaki-core explain <command>` makes one real model call from a terminal.
