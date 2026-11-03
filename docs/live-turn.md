# A turn in progress

About `crates/emaki-app/src/workbench.rs` and `workbench/tray.rs` (the tray over the composer, the toggles under it, the working row, Stop, the question, dialog, shells and permission cards), the screen readers in `crates/emaki-core/src/driver.rs`, and the turn state and stop handling in `crates/emaki-core/src/build.rs`.

## Rules

- Whether a turn is running is the registry's word (`status`, `statusUpdatedAt`), not the transcript's alone: Escape during `/compact` leaves no row, and the transcript read as working for ever.
- Registry `idle` from before the rows the state was read from settles nothing: the registry has not caught up with a turn that just began.
- Rows older than the process behind a session are not a turn of that process. A Claude Code killed part way through a turn leaves a tail that reads as working, and the hidden terminal started on the session at the next launch is behind it some seconds before its record says idle: the row said "Claude is working…" with a clock of hours. `settle_stopped` settles on the process's start too (the record's `startedAt`, or the terminal's own before there is a record). A record that says `busy` is left alone: its turn's rows may not be written yet.
- Hand a prompt back only when the agent wrote or ran nothing, and never over something already typed. Once the agent has started it has the message, and the stop only stops.
- A withdrawn prompt leaves `rounds` for `Session::withdrawn`. Left in, the message showed twice once it was sent again.
- A command that acts by itself (`/compact`) is never handed back.
- Escape pressed at once leaves no marker (2.1.293): the prompt's row stays with nothing under it, and the next prompt is written beside it, under the same parent. `build::taken_back` drops such a row; until the next prompt is written nothing in the transcript tells it from a prompt just sent, so the window takes the round out itself when it hands the words back (`handed_back`, applied in `set_detail`, and kept while the conversation showing is set again with the round already out). A prompt the agent began on has rows of the agent's under it and stays, as in the terminal.
- Rows under a prompt are not all the agent's. A prompt sent with a picture has an `isMeta` row under it saying where the picture came from, and `attachment` rows hang under that. Counting those as the agent having begun kept a prompt taken back with a picture in the conversation, beside the one sent after it. They go with the prompt.
- "[Request interrupted by user]" is Claude Code's marker, never a round of the person's.
- A stop's words are read out of the transcript, never written here, and every adapter says it through `build::push_interrupted`, once however many rows record it.
- Screen readers skip every escape sequence that is not a CSI. A quiet turn's mark is drawn behind `ESC ( B`; read as text it hid the working line.
- A screen color goes through `driver::theme_pair` and the window takes its own side. The terminal's theme need not be the window's.
- A terminal dialog is answered one key at a time, each once the screen shows what the one before did (`DialogUntil`). Digit, words and Return in one write lost the words and answered with the first choice.
- No frame answers a terminal session's question: the inbox socket takes only `auth` and `user` frames (2.1.288). Keys do.
- A bare ↩ never answers a question, and "Allow all" skips questions.
- A session with a name has a rule under the dialog carrying that name, the last line on the screen. `dialog_on_screen` leaves it out: taken for the dialog's own last rule, only "Chat about this" was read, no question was seen, and the raw terminal card came up in place of the question card.
- A click on "Type something" on the terminal card goes there and does not press Return (`pty::hits`): Return on the empty field ended the question unanswered.
- A background command with no end counts as running only while a process that registered before it began is behind the session (`Workbench::shells_running`, `Peer::started_at`): one whose Claude Code went away gets no end row.
- Nothing here stops a background command. That is the agent's or the terminal's to do.
- No card is held over the composer. A card there took the conversation's room for as long as it was up: with a long question in a low window a few lines of the conversation were left, and nothing the reader did gave them more. What answers a card stands in the tray, a line and not a card (The tray).
- A scroller drawn inside the conversation's list is named and kept by the list's own drawing (`list_draw`, `in_list`), never by the window's frames. The list is a view the window keeps: a frame from the cache draws none of its scrollers, and kept by the window's frames a card's scroller was forgotten at the second such frame and made anew at its top. That is what took the reader back up a card a second or two after they had scrolled it, and only while something else (a countdown on the card, a dot, the turning mark) was drawing frames.
- A scroller inside a card's body is made before the body's own (`card_scroll`): `route_scroll` takes the first one under the pointer.
- What the lines over the composer say of a card is read from what the card is drawn from (`terminal_screen`, `dialog_seen`, `permissions`), never from the card as last drawn. The cards are part of the list's last item, which the list does not draw while the reader is up the conversation: read from the drawing, the tray said "Claude is working…" over a terminal that waited, and kept a waiting line after the wait was over. Whether the cards were drawn is the list's last fresh drawing's (`cards_drawn`, reset in `list_draw`): reset at every frame, a frame from the cache took them for gone and moved the keyboard off the terminal card.

## The cards

Whatever comes up for the person to read or answer while a session runs is a card in one column (`Workbench::render_cards`): the published pages', the background commands' and agents' cards, the hidden terminal's screen, a terminal's dialog, a driver's questions and permissions. A new card goes there.

- The column is drawn under the last round, inside the list's last item (`render_list_item`), so it scrolls with the conversation: going up leaves the cards behind and the conversation has its whole room. The list is told of no item more, so nothing is spliced (`docs/conversation.md`). A conversation with no round yet has no item to stand them in, and there they stand over the composer.
- What a card holds is drawn through `card_scroll`: no taller than `card_most`, a share of the window's height, and past that it scrolls inside the card, its head and its buttons staying. It scrolls sideways too, for what cannot wrap: `wide` is that width (the terminal's rows). Words wrap and need none. A text's least width in gpui is its whole line, so a body cannot be sized by what it holds: one that was would never wrap.
- The body is `Inner::Chained`, as a tool call's is: a stroke begun at its edge is the conversation's, so the wheel over a card still leaves it behind.
- A dialog's tabs stand in the decision line (`dialog_tabs`), a strip of their own that scrolls sideways and is brought to the tab showing when that changes. A sideways scroller in a card takes a sideways stroke only (`restrict_scroll_to_axis`): without it gpui turns the wheel's up and down into across, and the strip moved together with the conversation. While the strip has more tabs than show, an arrow stands at each end, lit where more lies that way, and the tabs fade into the card's ground at that end; a click on the arrow goes there. An arrow with no more its way keeps its room, so the tabs do not shift. All of it is in motion: the strip runs to where it is going (`tabs_glide_step`), also when it is brought to the tab showing, and the arrows and fades come and go by `presence`.
- The column has no gap: each card carries the room under it, which moves with it as it opens and closes (`render_terminal`, `card_fold`).
- The reader is left where they are when the agent asks. While a card is up neither the working line nor the waiting line is shown: the transcript still reads as working while a question waits, and the line said "Claude is working…" over a card the agent was held on. With the card left behind, the decision line carries a "Show" pill that goes to the end (`cards_show`), run from where the reader is (`end_glide_step`); the terminal card, which has no decision line, gets a line of its own for that. What the person asked for themselves (a pill's picker, a typed command, the background commands' row) goes there at once.
- The terminal card holds the keyboard, and a card far above the view is not drawn. The keyboard then goes to the composer (`cards_drawn`).

## The tray

One card over the composer that holds what waits on the person, a line each (`workbench/tray.rs`, `render_tray`). It is there only while something does, and most of the time nothing does. Top to bottom:

1. A message that cannot get to the prompt (`held_line`).
2. The terminal's card left behind, with the way back to it.
3. What the agent waits for the person to decide (`decision_line`).

The agent at work, or waiting, is not in it. That line is the turn's own and stands by itself under the tray, over the composer (`foot_line`), its mark and its Stop in the columns of the rows above it. All the rest a session has going on is behind a toggle (The toggles): a tray that also held the pages, the background work and the notes stood there for as long as a session had a page, over the agent's own line, and looked like one more thing to deal with.

- **One shape.** Every row in it is `tray_row`: its mark in one column, what it is, a line that gives way first, how long, and the buttons last. So what can be pressed stands at one right edge, level with Stop and the composer's send button, whatever the row is.
- **Even air.** The card stands in from the composer's edges by what a button's air is (`TRAY_IN`, `ROWS_IN`), so a button has the same air above, below and at the card's edge while still ending at that one right edge. As wide as the composer, it had four points above a button and fourteen beside it. The card is rounded as far as a line is tall, so a button sits in it as a pill in a pill.
- **A button of a row is `tray_pill`,** which takes the press for itself. Stop is one too (`Press::Halt`), so it matches what stands over it: gpui panics on a second `hover` on one element, so a look of its own is a `Press`, not a `hover` over the pill.
- **Motion.** A line comes and goes by its height (`render_tray`). The first in and the last out take the card with them instead, opening up from the composer (`fold_with`), so the two never run at once. The card and each card at the conversation's end bring their own air.
- **A new thing that waits on the person is a line here,** in the place its urgency gives it. Anything else is a toggle.

## The toggles

What a session has going on that does not wait on the person: its published pages, background agents, monitors and commands, and Claude Code's note. Each is a small button at the right of the row under the composer (`render_toggles`, `foot_toggle`), an icon and a count past one, shut until pressed. That row is always there, so a toggle coming takes no room from the conversation.

- **A press opens its card at the conversation's end,** another shuts it, and the toggle wears the accent while it is on. It is the same card from any toggle, in the column every card stands in (The cards); there is no line in between.
- **Pointing says what it is,** in a count and a word ("1 artifact", "2 background commands", "1 notice") and no more. What each one is, is on its card.
- **What is new shows without opening.** A dot breathes on what is running, and a dot in the note's own color, with the bell in that color, stands on a note not yet read. Opening reads it.
- **Off is the default and stays so.** A list that empties is shut, or the next command to start would open its card by itself; a note's card shuts with its note.
- **The last toggle stands under the middle of the send button** (`TOGGLES_END`), the bell last when there is one. A toggle comes and goes sideways on its `presence`, those beside it moving over; its width is known from its count, so the motion has no pause at its end. On and off is a `presence` too, the accent coming over a moment.
- **Room.** The toggles keep theirs. The limits beside them lose whole figures from the end (`render_limits`): a figure and the bar before it are one piece, and a piece that does not fit wraps to a line the row's height hides. A fade was tried and drew a pale block, the composer's shadow being on that ground.

### The decision line

What the agent asks the person to decide stands here, in a row, in view wherever the conversation is scrolled to and however long what is asked: the card at the conversation's end says what is asked and scrolls, and the choices are not on it. On the card they scrolled away from under the pointer, and the composer cut them off. The tray's edge wears the accent while it is up.

- An approval, a review and a plan's go-ahead have their choices here as buttons, in the terminal's order and sending the same digits (`dialog_bar`). A choice's words are as long as the agent makes them: a button is cut at `bar_pill`'s width, with the whole of them under the pointer, and buttons that do not fit go under what is asked.
- A question keeps its choices on its card, since they carry descriptions. Here are its tabs, Next, Choose and Cancel: what moves through it.
- A driver's permission, plan and question are the same (`permission_bar`): Allow and Deny with their keys, for the oldest waiting, and Allow all when several wait. What only the agent's terminal can take (`PermissionRequest::in_terminal`) has Show the terminal and Skip instead, and neither ↩ nor Allow all answers it (`docs/channels.md`, Codex).

## Claude Code's notes

Claude Code says things beside its prompt that are in no transcript: a limit coming up ("You've used 90% of your Fable limit · resets Nov 3 at 12am (Australia/Melbourne)", straight over the prompt's rule and set to the right, in a color). `driver::notes_on_screen` reads them (2.1.316), `read_notes` once a second for the session showing, from a hidden terminal of ours only. A line is one by where it stands: touching the rule and reaching the rule's right end. The conversation's own words are not. What says the session's effort, model or mode stands there too ("● high · /effort") and is left out (`says_a_setting`): the composer's pills show it. So is a tip under the working line ("⎿  Tip: …"): it teaches the terminal, and came and went in seconds. Claude Code shows a note for moments, so one stays `NOTE_KEPT_SECS` after it goes, behind the bell (The toggles), and goes at the × on its card (`render_notes_card`).

## The working row

While a turn runs, Claude Code draws a line over its prompt: a turning mark, a word that changes, and the turn's figures in a bracket. The row under the conversation shows it as written: the word in the color of the terminal's mark, the bracket in the muted ink.

- `Workbench::read_working` reads the screen of the session showing once a second and whenever its status line runs, one read at a time off the main thread (`sys::terminal_styled`): the hidden terminal's, with its colors. A session in a terminal of the person's has no screen to read here.
- `driver::working_on_screen` finds the line among the last lines: one of the spinner's marks, a space, words ending in "…". A finished turn's line has no ellipsis and a tool call's begins with another mark.
- The bracket carries the turn's time, so the row's own clock is left out then. A screen without the line keeps the last word for `WORKING_KEPT_SECS`.
- `THEME_PAIRS` holds Claude Code's light and dark themes side by side (from the 2.1.290 binary); `workbench::shade` takes the window's side. A color in neither theme is only kept readable.
- With no line to read (a driven session, an IDE's terminal, another agent) the row says "<agent> is working…" with what the transcript shows and the clock.

## Stopping a turn

Escape in the terminal usually leaves a "[Request interrupted" row, which `turn_state` reads. For the rest, `Hub::settle_stopped` runs on every scan and `TurnState::settle_idle` turns a working state into your turn, chip "interrupted", when the registry has been idle since after the rows the state was read from.

The working row has a Stop pill (`Workbench::interrupt`). A driver takes an `interrupt` request. A terminal session gets Escape (`sys::key_in_terminal`), written to the hidden terminal; in a terminal of the person's the row says to press it there. The scan is then asked for twice.

Escape in the window does the same when it has nothing to close (`Workbench::escape_stops`): the slash list, settings, the lightbox, the search and the find bar come first. The textarea keeps its own Escape action, so the composer's wrapper captures it and passes it on.

## Handing the prompt back

`Workbench::restore_prompt`, as Claude Code's terminal does on Escape: the words in the composer with the caret after them, the attachments on the chips.

- From the message as it left the window when that was the last thing sent (`last_sent`, pictures included), else from the transcript by its paths.
- On the Stop click, and when the index shows the session going from working to stopped (`was_working`), which is Escape in the terminal.
- The test is the withdrawal's own (no item in the round but notices), applied to `Session::withdrawn` or, while the stop's marker is not yet written, to the last round.
- `/compact` typed through the hidden terminal ran to its end, the registry said idle a moment before the boundary's rows were in the file, and "/compact" came back into the box. The scan reads the registry before the transcripts as well, which narrows that moment without closing it (Claude Code writes its rows late).

In `build`, `handle_user` moves a prompt stopped before the agent wrote or ran anything into `Session::withdrawn`; it gets no line. A prompt the agent had started on stays, and its round ends on `NoticeVariant::Interrupted`:

- Claude Code: the marker with its brackets off ("Request interrupted by user", "…for tool use").
- Codex: the reason of its `turn_aborted` event, or where only the next user row says it, the first sentence inside `<turn_aborted>`. Both shapes are from Codex's source, not a real rollout. The line is also the state: `turn_state_from_session` reads a round ending on it as your turn, where a prompt with no reply read as working for ever.
- The terminal's own line ("Interrupted · What should Claude do instead?") is not shown: it is in no transcript and gone at the next prompt.

## A question from a driver

`AskUserQuestion` is a tool call, drawn as a card wherever it was asked (`transcript::render_question_call`): each question with its header chip and options, the chosen one ticked once the result's sidecar carries `answers` (`ToolCall::answers`), typed words quoted, "not answered" when declined. It never folds into a run of tool calls.

`ExitPlanMode` is drawn the same way once its row is written (`transcript::render_plan_call`): the plan whole, as markdown, with "approved" or "not approved", and never folded into a run. As a plain tool card the plan was shut inside it.

A driver gets the question as a `can_use_tool` request like any permission (2.1.288). The reply is allow with the input completed by `answers`, question text to the label chosen, as Claude Code's own dialog answers (`Driver::answer_question`, `driver::question_decision`). The card where permission cards sit (`Workbench::render_question`) takes a click on an option, a button when there are several questions or choices (`picks` holds the staging), or words typed in the composer (`send_message` routes them to `answer_question_typed` while a question waits).

The status row says "Claude is waiting for your answer · below" for a driver and "· in your terminal" where the screen cannot be read ("your approval", "your go-ahead on the plan" for the other holds).

## A terminal's dialog

While a question waits in a terminal the transcript does not have it: 2.1.289 writes the `AskUserQuestion` row only once it is answered. The registry says a dialog is up (`status: waiting`, `waitingFor` "input needed" or "permission prompt"); the screen says which.

- `Workbench::read_dialog` reads the screen once a second and whenever the status line runs, for the session showing.
- `driver::dialog_on_screen` parses a question (under a rule: the tabs when there are several, the question, numbered choices with descriptions, "[ ]" and "[✔]" where several can be taken, the foot's key line), the review ("Review your answers", "1. Submit answers / 2. Cancel") and an approval (the command between dashed rules over "Do you want to proceed?"). Any question of that shape is read, whoever asks it: the one a cold resume asks (2.1.312, "Resume this conversation?", two numbered choices, "Enter to confirm · Esc to resume" at the foot) comes up as the question card. The foot is any line naming what Esc does, not only "Esc to cancel": read as a choice's description, it stood under the last choice.
- A question whose choices carry previews is laid out in two columns (2.1.294): the choices at the left, a box at the right holding the preview of the one the pointer is on, a line about notes under it, and no descriptions. `dialog_on_screen` cuts each line at the box's column (`Dialog::preview`); read whole, the box's rows were the choices' labels. There a digit only moves the pointer and Return takes the choice, so the card rings the pointed choice, shows its preview in the mono face, sends the digit for a click on another choice, and Return for a click on the pointed one or on Choose.
- A question's lines on the screen are one question on the card, joined. Only an approval and the review have something before what is asked (the command, the answers), which goes in the box over it.
- `render_dialog` draws it where the permission cards sit. The terminal's dialog is the one that moves, and the card is the screen read again: a click sends the choice's digit (`sys::text_in_terminal`, no terminal coming forward), Next sends Tab, Cancel sends Escape.
- The chips are the terminal's tabs, the review last. The one showing is ringed (`Dialog::current`), which the terminal says by a ground color, so the screen is read with its colors. A click on another goes there by arrow keys, one step at a time (`dialog_go`, `DialogUntil::Tab`).
- Words typed in the composer answer a question that offers "Type something" (`dialog_answer_typed`): digit, words, Return.
- Not done: typed words on a question that takes several, whose field works another way; Terminal and an IDE, where keys need the app in front. There the "in your terminal" line stands.
- Not shown while a command typed in the terminal is watched (`/model`, `/effort`, with a value or bare): what it opens is the terminal's, on its card.
- A plan's dialog ("Claude has written up a plan…", 2.1.296) carries the plan, as markdown in a box that scrolls, over the question. The terminal prints the plan and then asks, most of it off the screen by then, and the transcript does not have it: the `ExitPlanMode` row is written once it is answered, and so is the row of a `Write` the agent asked for in the same message, which is how the plan's file is usually written. The dialog's last line names the file ("ctrl+g to edit in VS Code · ~/.claude/plans/<name>.md", `driver::plan_file_on_dialog`), and `plan_for_dialog` reads it at each read of the dialog. Where the screen did not say, the last plan the transcript shows being written is read, and with neither the card shows the lines the screen has. Tried and dropped: the transcript alone, which had the file only when another call came between the `Write` and the question.
- The card comes and goes in motion (`DialogShown`; `docs/window.md`, Motion).
- The registry says `waiting` before the question is on the screen, by as long as Claude Code takes to look something up first. Until then the screen is its prompt, standing still: neither card comes up for a screen with the prompt on it and no picker, and it is read again. A screen still being written (`Pty::quiet_for` under `SCREEN_SETTLED`) is read again too. Read once, the terminal card came up on the prompt and then drew the question itself, a second before the dialog card.
- Escape is the card's Cancel (`Workbench::escape`), as its button says.

## Background commands

A command started with `Bash` `run_in_background` may outlive the turn. The transcript has both ends (`build::shells_of`, into `Session::shells`):

- Start: a `Bash` result's sidecar carries `backgroundTaskId`, its words naming the output file.
- End: the first `<task-notification>` for that id, which Claude Code writes into the queue (`queue-operation` `enqueue`) when the command exits and again as the prompt of the turn that starts, or a stop the agent asks for (`KillShell`, `TaskStop`).

Its toggle (The toggles) has the count. A press opens a card at the conversation's end (`render_shells_card`) with each command and the last `SHELL_TAIL_LINES` of its output, read from the output file on the clock while the card is open (`file_tail`: color sequences out, a progress line that rewrites itself kept as it last read).

The command and the output are both code and are drawn alike (`shell_box`): a labeled box, every line wrapped and none cut short, a copy button at its top right under the pointer. Past `SHELL_BOX_H` each scrolls, with the bar every scroller has (`docs/window.md`): the command from its start, the output from its foot, where it stays as more is printed unless the reader has scrolled up.

## Background subagents

A subagent launched without waiting for it (`Agent`, 2.1.292) is the same kind of thing and is kept in the same list, as a `Shell` with `agent` set.

- Start: an `Agent` result whose sidecar says `isAsync: true` and names an `agentId`. A foreground agent's sidecar has an `agentId` and no `isAsync`; it is a tool card, not a task.
- End: the `<task-notification>` whose `task-id` is that `agentId`, as for a command. The agent's report also arrives as a peer row (`<agent-message>`), which ends nothing.
- Resumed: a `SendMessage` whose result says `resumedAgentId` starts the agent again, until its next notification, which comes each time it stops. One that ran before is its row again from the resume; one that first ran in the foreground gets a row from its own `Agent` call. Taken as ended at its first notification, an agent sent more work showed no row while it did it.
- `Shell::command` is the kind of agent (`subagent_type`, often empty) and there is no output file. The path Claude Code names as the output file is the agent's whole transcript and is never read here.

It has a toggle of its own (`TaskKind::Agents`). The card lists each agent with one line on what it was last seen doing (`build::agent_step`): the last tool call, or the first line of the last thing it said, from the end of the agent's own transcript (`build::agent_transcript`, `<session>/subagents/agent-<id>.jsonl`), read on the clock while the card is open (`read_agent_steps`). The same rule says whether one with no end is running (`shells_running`).

## Monitors

A `Monitor` (2.1.316) watches with a script of its own and reports each line it prints. It is a thing that runs in the background like a command, and is kept in the same list as a `Shell` with `monitor` set.

- Start: a `Monitor` result whose sidecar names `taskId` and `timeoutMs`, and `persistent` for one that runs for the session.
- A report is a `<task-notification>` with no `<status>`, its words in `<event>`, kept in `Shell::events`. It ends nothing: taken as an end, a monitor was gone at its first report.
- End: the notification that says a status (`completed`, "stream ended"), or its time running out, which leaves no row (`shells_running`, `Shell::timeout_ms`).

It has a toggle of its own (`TaskKind::Monitors`), and its card has the script and what it has reported. The output file is named only by the notification that ends it, and the reports are its contents.

## Artifacts

A page the agent published with Claude's Artifact tool is the session's (`build::artifacts_of`, into `Session::artifacts`): an `Artifact` result whose sidecar names the page (`url`, `artifact_id`, `title`, `icon`, `path`), from a call that publishes (no `action`, or `publish`) and does not upload an asset. A read or a list names a page too and is not one. A page published again is the same one, its title and time the newest.

Its toggle is there for as long as the session has any. It is not the background work's kind of thing, which comes and goes with what runs; it stays. A press opens a card at the conversation's end (`render_artifacts_card`) with each page, newest first: its title, where it is, when it was last published, and Open. A click anywhere on a page's row opens it too.

## The permission card's keys

↩ on an empty composer allows the oldest card waiting on the session showing, ⇧↩ denies it, and the decision line says so on its buttons. With words typed, ↩ is a new line. The textarea inserts the newline before it reports `PressEnter`, so `answer_pending_by_key` clears the composer after answering. Stacked cards: the first also offers "Allow all".

## Probing

- `EMAKI_GO=dialogdemo:approval` holds an approval whose command is longer than a card shows. `held:<name>` says a message waits on that panel (`held:asked`, `held:other`, `held:off`), `note:<words>` puts up a note, `_` for a space, `notes` is a press of the bell, `undelivered:<words>` shows a message that did not go, `heldfix` is a click on Back to the input, `heldshow` prints what holds a message and how its bubble stands. `monitors` opens the monitors' card.
- `EMAKI_GO=dialogdemo:preview` holds a sample question with previews on the card. `dialogdemo:long` holds one with six tabs and more choices than a card shows; `termdemo:tall` draws the terminal card on a screen taller than a card. `cards:<across>:<down>` moves every card's body as a wheel would and prints where each is and how far it can go. `scroll:` from the conversation's end moves nothing until it is more than the view's height. `tabs:left` and `tabs:right` are clicks on the strip's arrows, `cardsshow` one on "Show".
- `EMAKI_GO=dialogdemo:plan` holds a plan's dialog, with the plan the session showing last wrote. For the real one: `emaki-core pty <cwd> --keys "/plan" --keys "\r" --keys "<a prompt>" --keys "\r"` prints the screen and what the reader makes of it (a real model call).
- `EMAKI_GO=dialog:<digit>` or `dialog:tab` presses that in the terminal's dialog; `answer:<words>` types an answer; `goto:<n>` goes to that tab.
- `EMAKI_GO=shells` opens the background commands' card, `EMAKI_GO=agents` the background subagents', `EMAKI_GO=artifacts` the published pages'. `emaki-core shells <id>` lists both, with each agent's last step.
- `emaki-core screen < text` says what a screen holds; `emaki-core shells <id>` lists a session's commands; `emaki-core drive` answers a question with its first option, so the wire can be checked from a terminal.
- `kaku cli spawn` hands the new pane the caller's environment, so a session started from inside a Claude Code session is its child and never registers. Start it from a script that unsets `CLAUDE*` first.
