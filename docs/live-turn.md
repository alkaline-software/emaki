# A turn in progress

About `crates/emaki-app/src/workbench.rs` (the working row, Stop, the
question, dialog, shells and permission cards), the screen readers in
`crates/emaki-core/src/driver.rs`, and the turn state and stop handling in
`crates/emaki-core/src/build.rs`.

## Rules

- Whether a turn is running is the registry's word (`status`,
  `statusUpdatedAt`), not the transcript's alone: Escape during `/compact`
  leaves no row, and the transcript read as working for ever.
- Registry `idle` from before the rows the state was read from settles
  nothing: the registry has not caught up with a turn that just began.
- Hand a prompt back only when the agent wrote or ran nothing, and never
  over something already typed. Once the agent has started it has the
  message, and the stop only stops.
- A withdrawn prompt leaves `rounds` for `Session::withdrawn`. Left in,
  the message showed twice once it was sent again.
- A command that acts by itself (`/compact`) is never handed back.
- "[Request interrupted by user]" is Claude Code's marker, never a round
  of the person's.
- A stop's words are read out of the transcript, never written here, and
  every adapter says it through `build::push_interrupted`, once however
  many rows record it.
- Screen readers skip every escape sequence that is not a CSI. A quiet
  turn's mark is drawn behind `ESC ( B`; read as text it hid the working
  line.
- A screen colour goes through `driver::theme_pair` and the window takes
  its own side. The terminal's theme need not be the window's.
- A terminal dialog is answered one key at a time, each once the screen
  shows what the one before did (`DialogUntil`). Digit, words and Return
  in one write lost the words and answered with the first choice.
- No frame answers a terminal session's question: the inbox socket takes
  only `auth` and `user` frames (2.1.288). Keys do.
- A bare ↩ never answers a question, and "Allow all" skips questions.
- A background command with no end counts as running only while a process
  that registered before it began is behind the session
  (`Workbench::shells_running`, `Peer::started_at`): one whose Claude Code
  went away gets no end row.
- Nothing here stops a background command. That is the agent's or the
  terminal's to do.

## The working row

While a turn runs, Claude Code draws a line over its prompt: a turning
mark, a word that changes, and the turn's figures in a bracket. The row
under the conversation shows it as written: the word in the colour of the
terminal's mark, the bracket in the muted ink.

- `Workbench::read_working` reads the screen of the session showing once a
  second and whenever its status line runs, one read at a time off the
  main thread (`sys::terminal_styled`). WezTerm and Kaku give the colours
  (`cli get-text --escapes`); Terminal and iTerm2 give plain text, and the
  word takes the agent's colour.
- `driver::working_on_screen` finds the line among the last lines: one of
  the spinner's marks, a space, words ending in "…". A finished turn's
  line has no ellipsis and a tool call's begins with another mark.
- The bracket carries the turn's time, so the row's own clock is left out
  then. A screen without the line keeps the last word for
  `WORKING_KEPT_SECS`.
- `THEME_PAIRS` holds Claude Code's light and dark themes side by side
  (from the 2.1.290 binary); `workbench::shade` takes the window's side. A
  colour in neither theme is only kept readable.
- With no line to read (a driven session, an IDE's terminal, another
  agent) the row says "<agent> is working…" with what the transcript shows
  and the clock.

## Stopping a turn

Escape in the terminal usually leaves a "[Request interrupted" row, which
`turn_state` reads. For the rest, `Hub::settle_stopped` runs on every scan
and `TurnState::settle_idle` turns a working state into your turn, chip
"interrupted", when the registry has been idle since after the rows the
state was read from.

The working row has a Stop pill (`Workbench::interrupt`). A driver takes
an `interrupt` request. A terminal session gets Escape
(`sys::key_in_terminal`): WezTerm and Kaku by `cli send-text` and iTerm2
by `write text`, neither coming forward; Terminal and other hosts by a
System Events key press after being brought forward, the front going back
to Emaki after. The scan is then asked for twice.

Escape in the window does the same when it has nothing to close
(`Workbench::escape_stops`): the slash list, settings, the lightbox, the
search and the find bar come first. The textarea keeps its own Escape
action, so the composer's wrapper captures it and passes it on.

## Handing the prompt back

`Workbench::restore_prompt`, as Claude Code's terminal does on Escape: the
words in the composer with the caret after them, the attachments on the
chips.

- From the message as it left the window when that was the last thing
  sent (`last_sent`, pictures included), else from the transcript by its
  paths.
- On the Stop click, and when the index shows the session going from
  working to stopped (`was_working`), which is Escape in the terminal.
- The test is the withdrawal's own (no item in the round but notices),
  applied to `Session::withdrawn` or, while the stop's marker is not yet
  written, to the last round.
- `/compact` typed through the hidden terminal ran to its end, the
  registry said idle a moment before the boundary's rows were in the file,
  and "/compact" came back into the box. The scan reads the registry
  before the transcripts as well, which narrows that moment without
  closing it (Claude Code writes its rows late).

In `build`, `handle_user` moves a prompt stopped before the agent wrote or
ran anything into `Session::withdrawn`; it gets no line. A prompt the
agent had started on stays, and its round ends on
`NoticeVariant::Interrupted`:

- Claude Code: the marker with its brackets off ("Request interrupted by
  user", "…for tool use").
- Codex: the reason of its `turn_aborted` event, or where only the next
  user row says it, the first sentence inside `<turn_aborted>`. Both
  shapes are from Codex's source, not a real rollout. The line is also the
  state: `turn_state_from_session` reads a round ending on it as your
  turn, where a prompt with no reply read as working for ever.
- The terminal's own line ("Interrupted · What should Claude do instead?")
  is not shown: it is in no transcript and gone at the next prompt.

## A question from a driver

`AskUserQuestion` is a tool call, drawn as a card wherever it was asked
(`transcript::render_question_call`): each question with its header chip
and options, the chosen one ticked once the result's sidecar carries
`answers` (`ToolCall::answers`), typed words quoted, "not answered" when
declined. It never folds into a run of tool calls.

A driver gets the question as a `can_use_tool` request like any permission
(2.1.288). The reply is allow with the input completed by `answers`,
question text to the label chosen, as Claude Code's own dialog answers
(`Driver::answer_question`, `driver::question_decision`). The card where
permission cards sit (`Workbench::render_question`) takes a click on an
option, a button when there are several questions or choices (`picks`
holds the staging), or words typed in the composer (`send_message` routes
them to `answer_question_typed` while a question waits).

The status row says "Claude is waiting for your answer · below" for a
driver and "· in your terminal" where the screen cannot be read ("your
approval", "your go-ahead on the plan" for the other holds).

## A terminal's dialog

While a question waits in a terminal the transcript does not have it:
2.1.289 writes the `AskUserQuestion` row only once it is answered. The
registry says a dialog is up (`status: waiting`, `waitingFor` "input
needed" or "permission prompt"); the screen says which.

- `Workbench::read_dialog` reads the screen once a second and whenever the
  status line runs, for the session showing.
- `driver::dialog_on_screen` parses a question (under a rule: the tabs
  when there are several, the question, numbered choices with
  descriptions, "[ ]" and "[✔]" where several can be taken, the foot's key
  line), the review ("Review your answers", "1. Submit answers / 2.
  Cancel") and an approval (the command between dashed rules over "Do you
  want to proceed?").
- `render_dialog` draws it where the permission cards sit. The terminal's
  dialog is the one that moves, and the card is the screen read again: a
  click sends the choice's digit (`sys::text_in_terminal`, the pane never
  coming forward), Next sends Tab, Cancel sends Escape.
- The chips are the terminal's tabs, the review last. The one showing is
  ringed (`Dialog::current`), which the terminal says by a ground colour,
  so the screen is read with `--escapes`. A click on another goes there by
  arrow keys, one step at a time (`dialog_go`, `DialogUntil::Tab`). With
  no colours (iTerm2) the chips take no clicks.
- Words typed in the composer answer a question that offers "Type
  something" (`dialog_answer_typed`): digit, words, Return.
- Not done: typed words on a question that takes several, whose field
  works another way; Terminal and an IDE, where keys need the app in
  front. There the "in your terminal" line stands.
- Not shown while the person was sent to the terminal for `/model` or
  `/effort`, whose list is theirs to use there.

## Background commands

A command started with `Bash` `run_in_background` may outlive the turn.
The transcript has both ends (`build::shells_of`, into `Session::shells`):

- Start: a `Bash` result's sidecar carries `backgroundTaskId`, its words
  naming the output file.
- End: the first `<task-notification>` for that id, which Claude Code
  writes into the queue (`queue-operation` `enqueue`) when the command
  exits and again as the prompt of the turn that starts, or a stop the
  agent asks for (`KillShell`, `TaskStop`).

The row (`render_shells`) sits where the working line does, above it when
both are there: the count, the agent's line on what the command is for,
how long it has run. A click opens a card with each command and the last
`SHELL_TAIL_LINES` of its output, read from the output file on the clock
while the card is open (`file_tail`: colour sequences out, a progress line
that rewrites itself kept as it last read).

## Background subagents

A subagent launched without waiting for it (`Agent`, 2.1.292) is the same
kind of thing and is kept in the same list, as a `Shell` with `agent` set.

- Start: an `Agent` result whose sidecar says `isAsync: true` and names an
  `agentId`. A foreground agent's sidecar has an `agentId` and no
  `isAsync`; it is a tool card, not a task.
- End: the `<task-notification>` whose `task-id` is that `agentId`, as for
  a command. The agent's report also arrives as a peer row
  (`<agent-message>`), which ends nothing.
- `Shell::command` is the kind of agent (`subagent_type`, often empty) and
  there is no output file. The path Claude Code names as the output file
  is the agent's whole transcript and is never read here.

The row is a second `render_shells` (`agents: true`), over the commands'
row: "N agents running in the background", and for one agent its task and
how long it has run. The card lists each agent with one line on what it
was last seen doing (`build::agent_step`): the last tool call, or the
first line of the last thing it said, from the end of the agent's own
transcript (`build::agent_transcript`,
`<session>/subagents/agent-<id>.jsonl`), read on the clock while the card
is open (`read_agent_steps`). The same rule says whether one with no end
is running (`shells_running`).

## The permission card's keys

↩ on an empty composer allows the oldest card waiting on the session
showing, ⇧↩ denies it, and that card says so on its buttons. With words
typed, ↩ is a new line. The textarea inserts the newline before it reports
`PressEnter`, so `answer_pending_by_key` clears the composer after
answering. Stacked cards: the first also offers "Allow all".

## Probing

- `EMAKI_GO=dialog:<digit>` or `dialog:tab` presses that in the terminal's
  dialog; `answer:<words>` types an answer; `goto:<n>` goes to that tab.
- `EMAKI_GO=shells` opens the background commands' card, `EMAKI_GO=agents`
  the background subagents'. `emaki-core shells <id>` lists both, with
  each agent's last step.
- `emaki-core screen < text` says what a screen holds; `emaki-core shells
  <id>` lists a session's commands; `emaki-core drive` answers a question
  with its first option, so the wire can be checked from a terminal.
- `kaku cli spawn` hands the new pane the caller's environment, so a
  session started from inside a Claude Code session is its child and never
  registers. Start it from a script that unsets `CLAUDE*` first.
