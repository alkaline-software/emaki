# Adding an agent

What it takes to bring a coding agent into Emaki, in the order to do it,
with what was learned doing it for Codex. Each step names the doc that
has the detail; this file is the list and the order.

An agent comes in by stages, and each stage is a release-worthy place to
stop: its sessions **read**, then **driven** from the window, then its
own **terminal** on the panel. Codex has all three.

## Rules

- The rules in `AGENTS.md` hold for every agent, not for Claude Code
  alone: nothing writes to its transcript, nothing enters its context,
  no hooks, one writer a transcript, nothing typed in a terminal of the
  person's.
- Copy first. The agent's sessions are in the archive before anything
  of its is parsed.
- The window draws an agent's tool calls as the calls it already draws.
  A new agent adds an adapter and a driver, not cards of its own.
- An agent is driven by a wire its maker gives for that (Claude Code's
  stream-json, Codex's app server), never by reading its screen where a
  wire exists.
- Its terminal on the panel is a second view of the one process that
  has the session, never a second process on the same file.
- Every `match` on `AgentId` names each agent. A `_` arm, or an `if`
  that tests for one agent and gives the rest to the other, sends the
  new agent down a path written for another. Codex's sessions were
  handed to `claude --resume` this way.
- No model is called from a test. A driver is checked by hand in a
  probe copy.

## Before any code

Answer these from the agent's documentation and a session run by hand.
The answers decide how far the agent can come in, and whether it should
at all.

1. **Can most people sign in to it?** Gemini CLI had an adapter for one
   commit and lost it when Google closed the program to personal
   accounts. A card for an agent people cannot use promises more than
   the app does (`docs/agents.md`).
2. **Where are its sessions, and what files make one?** One file, or a
   file and things beside it. Claude Code's is four; missing one makes
   "fully reproducible" untrue.
3. **Does it delete or rewrite them?** After how long, and does a
   compaction keep the rows before it. This says how much the archive
   matters for it.
4. **What does a row say, and what does it say twice?** A Codex rollout
   has the same prompt as a `response_item` and as an item event. Find
   which is the source before writing a parser.
5. **Where is what the transcript lacks?** A session's name, its
   folder, its mode. Codex keeps names in `session_index.jsonl`. Open
   nothing in the agent's home beyond what is needed, and never a
   credential.
6. **How does it say a turn began and ended?** Explicit rows, or
   something to infer. The working row and "one writer" both rest on
   this.
7. **Is there a wire to drive it?** What starts a session, resumes one,
   sends a turn, stops one; how it asks leave and asks a question; how
   a mode, a model and an effort are set and listed.
8. **Who names a session's id?** Claude Code takes ours. Codex names its
   own, so the window begins under an id of its own and adopts the real
   one (`HubEvent::Adopted`).
9. **Can its terminal join a process that is already running the
   session?** Codex's can (`--remote`). If not, the terminal is the
   process, as Claude Code's hidden terminal is, and there is no
   headless child beside it.
10. **Can it tell whose turn is running?** Claude Code has a registry.
    Codex has none, so a rollout that reads as mid-turn with no child
    of ours is refused.
11. **Where does it say how full the context is and how much of the
    account's allowance is spent?** Claude Code writes neither to its
    transcript and hands both to a status-line script. Codex writes
    both on every request, so nothing is installed for it. Find the
    lengths of its usage windows too: they go by plan (five hours and a
    week on one, thirty days alone on another), so they are read, not
    assumed.
12. **What is its maker's colour?** Its mark and its page wear it.

## Stage one: its sessions read

Read `docs/core.md` (Adapters) and `docs/transcript-format.md` first.

- `model.rs`: a variant of `AgentId`, in `ALL`, with its id, name,
  archive folder and speaker.
- `paths.rs`: its home, moved by the variable the agent itself reads
  (`CODEX_HOME`), so a test can point it at a temp tree.
- `adapters/<agent>.rs`: an `Adapter`. `data_roots` feeds the watcher
  and the sweep; `owns` must answer for an archived copy too, since the
  archive keeps the agent's layout; `list` and `peek` are cheap, a stat
  and the head of a file.
- `adapters/mod.rs`: in `all()` and `for_agent`.
- `archive.rs`: what a session's files are for it. The main file is
  mirrored for any agent; files beside it are Claude Code's alone until
  the new agent's are named.
- The parser maps onto the model as it is: a command is a `Bash` call,
  a patch is `Write` or `Edit` with hunks in `structuredPatch`'s shape,
  a question is `AskUserQuestion`, a plan is `ExitPlanMode`. Then the
  window, the markdown and the search need nothing new.
- Set `Session::mode`, the model, the tokens, `turn_open`, and the
  title. A mode's change mid-session is a line in the round
  (`docs/modes.md`).
- For the row under the composer (`docs/composer.md`):
  `Session::context_tokens`, and where the transcript has them
  `context_window` and `usage_windows`, each window with its own
  length. An agent whose transcript lacks them needs a source of its
  own, as Claude Code's status line is, and a place of its own in
  `Limits`: one agent's windows never overwrite another's.
- `terminal.rs`: its resume command.
- `agents.rs`: its entry, with `reads` set, and install and sign-in
  commands word for word from its maker (`docs/agents.md`). A mark in
  `assets/icons/agents/`, named in `MARKS`, and in `workbench.rs`
  (`agent_icon_path` and the three beside it).
- Its colour in `agent_color`: one of the accents in `look::ACCENTS`,
  which has a shade for each appearance, never the window's accent
  (`docs/agents.md`, Marks). The agents page takes the same colour for
  everything of the agent's.
- `tests/core.rs`: `isolated()` sets the agent's home variable. Tests
  build rows by hand and parse them: the doubled rows, a stopped turn,
  a failed turn, a session from an older release of the agent.
- Check on real sessions: `emaki-core list`, `render`, `search`, then
  the conversation in a probe copy.

## Stage two: driven from the window

Read `docs/channels.md`, `docs/modes.md` and `docs/live-turn.md` first.

- A driver that is a `driver::Drive`, and an arm in `driver::start`.
  The hub and the window hold `Arc<dyn Drive>` and nothing else, so
  Stop, the queue, the cards and the pills' setters come with it.
- What the agent asks becomes the tool call the cards draw
  (`PermissionRequest`): leave for a command, leave for a patch with
  its diff, a question with its options. A request settled elsewhere
  takes its card away (`Event::PermissionSettled`).
- A turn anyone begins is said to the window (`Event::Turn`), not only
  one the driver sent.
- `Adapter::catalogue`: its modes, models, effort levels and slash
  commands, asked of the agent with no model call and no session.
  Models are named as the agent's own list names them
  (`driver::model_label`). The list is the agent's answer for the
  account signed in, less what it marks hidden; no model's name is
  written into the code.
- Its modes as one key in `Session::mode` and on the pill
  (`docs/modes.md`). A mode that lets everything through is picked from
  a list, never stepped to.
- The channel in `reply_via_for`, and when sending is refused.
- A constant for the release it was checked against
  (`TESTED_CODEX_VERSION`), and a note when the one found is newer.
- `emaki-core drive` takes the agent, so a turn can be run with no
  window.
- Whatever is Unix-only (a socket, a signal) has a stated fallback
  (`docs/platform.md`).

## Stage three: its terminal on the panel

Read `docs/channels.md` (The terminal panel) first.

- `Drive::attach` gives the command that joins the agent's own terminal
  to the driver's process. The hub runs it on a pty
  (`Hub::attach_terminal`) and the panel draws it (`agent_screen`).
- The terminal lives as long as the driver: looked at with no driver,
  one is started; a driver whose terminal shows is not reaped for being
  idle; when the driver goes, the terminal goes.
- The window types nothing into it. An agent's terminal can open on a
  question of its own (an update, a folder to trust), and a Return
  typed blind answers it. Typing blind into Codex's ran an upgrade.
- The panel's words for the agent: starting, closed with a way to
  start, and why not on a system where it cannot be.

## The window, at each stage

Search `crates/emaki-app` for `AgentId::` and read every place. The ones
that were wrong for Codex:

- Anything that starts a `claude` process: the warm-up, the rename, the
  hidden terminal. Guard by `== AgentId::ClaudeCode`.
- The mode the window says before the transcript does
  (`unwritten_mode`): an agent that writes a setting when it is made
  needs none.
- The home page's pills and what a new session is sent (`codex_next`).
- The row under the composer (`render_limits`), which was drawn for
  Claude Code alone and named its two windows by hand.
- A mark's colour. Where a mark is grey on purpose it stays grey for
  the new agent too: an idle session's row, the terminal button until
  pointed at. A folder follows the accent, whichever agent is live in
  it.
- The composer's least width is measured, since another agent's pills
  are longer (`docs/panels.md`).
- The new-session agent picker, the sessions page's badge, the agents
  page's projects.

## Checking

On an install of the agent with no config of the person's, since a
config hides faults and makes them. Wiping one is the person's to ask
for. Every turn below is a real call on their account: keep them short.

In a probe copy (`docs/probing.md`), with `EMAKI_GO` steps:

- A new session, and the id adopted.
- A session resumed, and a patch shown as a diff card.
- Leave asked: allowed by click, denied by click, answered in the
  agent's terminal with the card leaving the window.
- A question: drawn, an option clicked, an answer typed.
- Plan mode, the plan's card, and the go-ahead.
- Mode, model and effort changed on an idle session, then read back
  from the transcript.
- A message queued during a turn. Stop during a turn, and Stop with a
  card up.
- The agent's own compaction.
- Its least careful mode.
- A picture attached.
- The row under the composer against the agent's own status line or
  `/status`: the context's share, each window's share and time left.
  A window that has just reset reads 0%, which is right.
- The model list against the agent's own picker.
- Its mark live and idle in the sidebar, on a tab, in the conversation
  and on the terminal panel, and its card and page, under an accent
  that is not its colour.
- A turn typed in its terminal showing in the conversation, and a turn
  sent from the composer showing in its terminal.
- The terminal side on a session not yet started.
- A quit mid-turn and mid-card, then a relaunch.
- A Claude Code session afterwards, since `Drive`, the hub and the
  panel are shared.
- `cargo test -p emaki-core`, and the build for Windows and Linux.

## Writing it down

- `docs/core.md`: a section under Adapters on its transcript.
- `docs/channels.md`: a section on its wire and its terminal.
- `docs/modes.md`: its modes.
- `docs/composer.md`: where its context and usage windows come from.
- `docs/agents.md`: the count in Two lists.
- `AGENTS.md`: the layout's lines, and "What this is" where it names
  the agents.
- `README.md` and `CHANGELOG.md`: what the person installing it gets.
- What is not done for it, in its section of `docs/channels.md`.
