# Mode, model and effort

About `crates/emaki-core/src/options.rs` and `driver.rs` (the lists, their
words and colours), the pill code in `crates/emaki-app/src/workbench.rs`,
and `render_notice` in `crates/emaki-app/src/transcript.rs`.

## Rules

- Nothing in the window names a mode, a model or a level. They are the
  agent's own lists (`Options`), so a release that adds a model shows it
  with no change here, and another agent brings its own.
- Never count ⇧Tab presses to aim at a mode. The order depends on session
  flags nothing outside can read (`isBypassPermissionsModeAvailable`,
  `isAutoModeAvailable`, a gate), and a mode after plan can be bypass.
  Counting twice ended in the wrong mode on a live session. One press for
  one press, and the pill shows where it landed.
- No wait here is a length of time. `terminal_ready` and `watch_terminal`
  wait on states; a slow machine takes as long as it takes.
- Never type into a terminal whose registry record says `waiting`: a
  login, a trust screen or a dialog would take the keys.
- A pick is over by the registry's word, not by the status line or the
  transcript moving: after a fresh open the status line's first run can
  differ from a stale file and the resume writes rows, so the window came
  back before anything was chosen.
- A status change alone does not end a pick: on 2.1.289 the record goes
  `busy`, `waiting` 7 ms later, then `idle` at the choice or at Escape.
- A terminal session takes no setting over its inbox: it reads everything
  as prose, and a `control_request` frame is dropped without a reply
  (2.1.284). The terminal's own picker or key is the only way in.
- Every colour is a light and dark pair and the window's appearance picks
  the side, whatever theme the person's terminal is on.
- Mid-turn in a terminal of the person's, a mode, a pick or a command is
  refused with a line saying to wait (`theirs_busy`): no second Claude
  Code is started on a running turn.

## The lists

`options.rs` holds what an agent offers (`Options`: `modes`, `models` each
with its `efforts`, and the key for the agent's default model). An adapter
fills it (`Adapter::catalogue`, which also carries the slash commands); an
agent the app cannot drive offers nothing.

Claude Code (`driver::options_from`, checked against 2.1.289):

- Models: the `models` of a headless child's `initialize` reply, each with
  `value`, `displayName`, `description`, `resolvedModel` and
  `supportedEffortLevels` (Haiku takes none, the 4.6 models no `xhigh`).
- Modes: the choices `claude --help` lists for `--permission-mode`, the
  only place they are listed. `manual` there is `default` on the wire.
- Effort levels at large: `--effort`'s.
- The wire gives no words, so `driver::mode_words` and `effort_words`
  carry Claude Code's titles and a line on each known name. A name never
  met is made readable (`options::humanize`) and offered all the same.

`Hub::options_for(agent, cwd)` answers per folder once that folder has
been asked (a session there opened, or a driver started), else with the
last answer from anywhere, at launch `cache/options.json`. It never starts
a read. `emaki-core options [folder]` asks and prints.

## Names and colours

A pill says the agent's name for the choice (`mode_name`, `model_name`),
with "mode" after a one-word mode and "effort" after a level;
`driver::model_label` reads an id not on the list. On pills and in the
conversation's lines (`PillText`, `render_notice`) the value is semibold.

- A mode's colour is the agent's own when it has one (`Choice::color`):
  Claude Code's footer colours, read out of the 2.1.289 binary into
  `driver::mode_color`. Otherwise one by its place on the list
  (`workbench::ramp`, `RAMP`).
- An effort's colour is the `/effort` slider's (`driver::effort_color`).
  Max, a moving rainbow there, is the seven colours through the letters,
  still (`Choice::spectrum`, `workbench::tinted`; `SPECTRUM_LIGHT` in a
  light window).
- The model is uncoloured.

Tried and dropped: the mode's colour read off the terminal and used in
both appearances; effort levels coloured by their place alone.

## The three pills

`composer_pill`: an icon and the value, no tooltip, caret or list. The
toolkit draws a custom button colour at a fifth of its strength, so the
resting grey is the muted ink thinned. The pointer over a pill asks for
the arrow for the whole window (`arrow_over`, `set_window_cursor_style`),
which wins over any element's cursor: it flickered to the text cursor
there, and what asked for that was not found.

`Workbench::pill_clicked`:

- No session yet: the pills show what a new session starts in, and a click
  opens Settings on New sessions, which has the two remaining lists
  (`picker`).
- Mode: one ⇧Tab (`cycle_mode`). The mode has no command and no picker;
  Claude Code's key bindings offer only `chat:cycleMode`.
- Model, effort: `via_terminal(TerminalAction::Pick)` to
  `pick_in_terminal`, which types a bare `/model` or `/effort` so Claude
  Code's own picker opens, drawn on the hidden terminal's card over the
  composer (`show_terminal`). Enter saves the choice as the default for
  new sessions and `s` keeps it to the session; that is Claude Code's and
  the person's to decide. `/model` on a warm cache asks "Switch model?"
  first.

`cycle_mode` is also ⇧Tab in the composer (the wrapper captures the
textarea's `OutdentInline`). With a terminal behind the session it is
`step_mode`: `sys::key_in_terminal` with `TerminalKey::ShiftTab` once,
then two looks in case the status line's run is slow. With no process it
is `via_terminal(StepMode)`. A driven session, or the new-session page,
steps down the agent's list through `set_mode`.

`pick_in_terminal(Pill::Mode)` only says "Use ⇧Tab to change the mode";
`set_mode` reaches it when a terminal is behind the session.

Tried and dropped: lists under the pills; the mode pill going to the
terminal and coming back once the status line went quiet, which landed
sometimes and not others.

## via_terminal

The one way in for what only the terminal takes (`TerminalAction`: a pick,
a typed command, ⇧Tab, going there).

- In a terminal that has registered: done at once (`do_in_terminal`).
- With none: an idle driver is stopped, a driver mid-reply is refused
  (`terminal_check`, one writer per transcript), and a terminal is
  started. That is the hidden one (`Hub::start_terminal`) for Claude Code
  unless `driver.hidden_terminal` is false; else the person's, as the
  top-right button opens it. The action waits in `pending_terminal` and
  the row under the composer says so.
- Asked again during the wait, the newer action replaces the older and
  goes on waiting. Done at once, it ran a second time when the wait ended.

`terminal_ready` looks on the clock and whenever the status line runs, for
two things: the registry record says `idle`, and the prompt is on the
screen, which is the mode's footer under it (`terminal_up`; where the
screen cannot be read the registry's word stands). The wait otherwise ends
only when the person leaves the session.

`watch_terminal` ends a pick or a typed command: the record was seen
`waiting` since the typing and no longer is, or says `idle` as of a time
after it (a picker opened and closed between two looks). A record with no
status falls back to the status line naming another model or effort. A
command in the hidden terminal that opens nothing and starts no turn
(`/cost`) changes no status and is not waited on.

With the person's terminal (`driver.hidden_terminal` false): a pick
brings it forward and the window comes back when it is over (`come_back`).
For ⇧Tab, WezTerm, Kaku and iTerm2 take the key for the pane without
coming forward; any other host comes forward and the window takes the
front back at once.

## What the pills show

- Model and effort: the status line's word (`terminal_ctx`, from
  `state/context/<session>.json`), else the driver's or the transcript's.
- Mode: read off the terminal's screen (`sys::terminal_text`;
  `driver::mode_on_screen` looks in the footer under the prompt for each
  listed mode by name) when the session is opened and every time the
  status line runs. Kept in `mode_seen` until a turn starts, whose prompt
  row carries the mode. One read at a time (`mode_reading`).
- The signal is the status line: the hub watches `state/context/` and
  sends `HubEvent::Context` for the rewritten file, which a ⇧Tab causes
  within about a third of a second.
- An IDE's terminal cannot be read, so after a change there the pill says
  "Mode" until the next turn, not the transcript's old mode.

## A driver

- `Hub::set_driver_mode` and `set_driver_model` answer with what Claude
  Code holds: a refused switch (bypass without the flag) puts the pill
  back and says why.
- `set_permission_mode` accepts `auto` and answers `{"mode": "auto"}` but
  sends no `system/status` frame for it, unlike the other modes (2.1.284),
  so the reply is what the driver trusts.
- No control request sets the effort (`set_effort` is "Unsupported").
  `/effort <level>` as a user turn runs as a local command and is recorded
  as a `system/local_command` row with `commandRun: {command: "effort",
  args}`, which `build` reads into `Session::effort`.
  `Driver::set_effort` sends that turn.

## The line in the conversation

`NoticeVariant::Mode`, `Model` and `Effort`: the pill's icon, the word
muted, the value in the foreground, no plate. `NoticeVariant::said` is the
line as find, search and the markdown read it.

- `/model` and `/effort` answer with a sentence. `build::setting_said`
  reads what was set out of it and `RoundBuilder::add_output` puts the
  line in place of the command's chip. "Kept model as …" leaves no line.
  Anything else those commands say is shown as it is.
- Claude Code writes nothing at ⇧Tab (2.1.289 only sets a field). It names
  the mode on the next prompt row and in a `permission-mode` row whenever
  it writes rows. `RoundBuilder::saw_mode` says a change where the
  transcript first has it, for a prompt the foot of the round before. The
  first mode a session names is where it began, not a change.
- Until then the window says it: `Workbench::unwritten_mode` is the mode
  read off the terminal, or the driver's, when it differs from
  `Session::mode`.
- One rule for all three: AAABBB reads A, B and ABAB reads A, B, A, B.
  `RoundBuilder::add_notice` replaces a notice of the same kind when it is
  the last thing said.
- `mode_touched` keeps every unwritten change of mode with its time.
  `render_round` sets each before the first item written after it, puts
  back the lines the transcript folded (`Round::superseded`) and folds
  again over the whole, so effort, mode, effort is three lines.
- A mode stepped away and back to the transcript's keeps its line: the
  person changed it.
- Once the next turn writes the mode, the transcript's order is the only
  one: the mode is recorded with the prompt, after the efforts, which
  fold again.

Other notices: all in the window's face, not the reply's serif; a
command's output is quiet text behind a rule; compaction and errors keep a
tinted plate.

## Probing

- `EMAKI_GO=pill:mode`, `pill:model`, `pill:effort` click a pill.
- `open` hands a newly launched terminal app the opener's environment, so
  a probe copy with `CLAUDE_CONFIG_DIR` at a scratch tree starts a
  terminal whose Claude Code is not logged in. Have the terminal app
  running first.
