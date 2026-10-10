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
- Keep the `~/.emaki` layout. An archive made by the Python Emaki is
  picked up as it is.
- Nothing above the adapters (archive, search, the window) branches on the
  agent except to label it.
- Another agent's archive goes under a leading underscore. A project slug
  is `[a-z0-9-]`, so an underscore can never collide with one.
- `fts_query` quotes every term. People type `rm -rf`, `a:b` and a lone
  `"`, all FTS5 syntax that would raise mid-keystroke.
- The explainer's child is never `--bare`, and its `cwd` stays under
  `~/.emaki/run/explain`: it is a real Claude Code session and leaves a
  transcript, which that folder keeps out of the index.
- The tests never call a model.

## What is on disk

`~/.emaki`, in the layout the Python Emaki used:
`archive/<project>/<session>.jsonl` plus sidecars, `logs/`, the project
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
the shared model. Claude Code, Codex and Gemini CLI exist. A new agent is a new file
under `adapters/` and a variant of `AgentId`.

The agents a machine can have, read or not, are `agents.rs`
(`docs/agents.md`).

Claude Code keeps the flat `archive/<project>/` for compatibility. Codex
lives in `archive/_codex/<project>/`, and `iter_archived(ClaudeCode)` skips
the underscore directories.

Gemini CLI's file is a log to be replayed, not rows to be read in order
(`adapters/gemini.rs`): a message written again takes the place of the
one before it, `$set` changes what the session is, and `$rewindTo` takes
messages back. The format is read off the CLI's own recorder, version
0.63, and has been tried on hand-written files only: no session of a real
Gemini CLI was on the machine it was written on. Its sessions do not say
which folder they ran in; `.project_root` in the project's folder does,
and the archive keeps a copy of that file beside the sessions
(`archive_ref`), in `archive/_gemini/<project>/`. The file from before it
wrote lines (`.json`, one record) is read too.

Codex and Gemini CLI have no stop reason. Their phase is derived from the built model's tail
(`adapters::turn_state_from_session`), not from the rows.

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
