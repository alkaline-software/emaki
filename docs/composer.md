# The composer and the row under it

About the composer parts of `crates/emaki-app/src/workbench.rs`,
`crates/emaki-app/src/a11y.rs`, `crates/emaki-core/src/files.rs`,
`crates/emaki-core/src/limits.rs` and `crates/emaki-core/src/statusline.rs`.

## Rules

- Keep both accessibility fixes (the focus wrapper in `render_composer`,
  the forwarder in `a11y.rs`). Without either, macOS reports the window as
  the focused element and a dictation app cannot find the text field.
- The composer is plain text. Do not bring back live markdown rendering
  without a real in-toolkit rich editor: the toolkit has no editable
  rich-text view, and a native webview draws above every gpui overlay.
- Never send a slash command down the inbox socket. Claude Code wraps every
  `user` frame there as a message from a peer, with or without our
  envelope, and the model refuses a peer's command. Type it in the
  session's terminal instead.
- ↩ or a click runs a command only when it acts alone
  (`driver::acts_alone`) and is the whole message. A skill run unasked
  cannot be taken back, so a name not on the list counts as a skill.
- The status line is the only write the app makes to Claude Code's
  settings. The script never exits non-zero and never writes to stderr, it
  lives at a stable path outside any checkout, and a copy run with
  `EMAKI_HOME` set leaves the settings alone (it would point Claude Code at
  its scratch tree).
- Never take a context window smaller than the tokens in it: one model id
  comes in two sizes.
- A thumbnail is never a crop.
- The row under the composer always takes `NOTICE_H`, so the composer does
  not jump as a notice comes and goes.
- Do nothing to "@path" in a message. Claude Code reads it out of a prompt
  itself, typed or pasted.
- `escape_at` is the one place that says how long an escape sequence is,
  for all four screen readers.
- Setting the textarea's value is not a change event, so bringing a draft
  back starts no hidden terminal. Keep it that way.

## Accessibility

- gpui-component's input frame tracks a focus handle of its own and only
  asks whether it contains the focus, so gpui never marks the editor as the
  focused accessibility node. The wrapper in `render_composer` tracks the
  editor's real handle with a `MultilineTextInput` role and the typed
  value.
- AppKit resolves the app's focused element through the key window, and
  gpui's window class never forwards `accessibilityFocusedUIElement` to its
  content view (the gap gpui-component patches for
  `accessibilityHitTest:`). `a11y.rs` adds that forwarder at open.

With both, the focused element is an `AXTextArea` carrying the text.

## Plain text and drafts

The composer is a growing `TextareaState`; markdown renders once a message
is sent. Tried and dropped: a preview block beside the text, a highlighted
editor mode, a block-by-block editor, a CodeMirror editor in a webview.

There is one textarea, and `Workbench::sync_draft` makes it stand for
whichever place is showing. On arriving somewhere else (`draft_key`: the
session, or the new-session page) the words and attachment chips are put
away under where they were typed (`drafts`) and what was left here comes
back with the caret at its end. It runs at every draw and before
`restore_prompt`, which asks whether the box is empty on a session's
behalf. The sessions page draws no composer and changes nothing. Drafts are in memory only. Tried and dropped: one box for the
window, whose words followed the person into the next session.

## Spelling, grammar and capitals

Three settings under Composer: auto-capitalization, spelling and grammar,
and the language (`app.auto_capitalize`, `check_writing`,
`writing_language`). `emaki-core/src/check.rs` is the part without a
window.

- English, US or UK, is Harper's (`harper-core`): spelling and grammar,
  in the process, nothing sent anywhere (`check::english`).
- A mark says "this is wrong", so a rule earns one only when what it
  marks is wrong however the sentence is read. Harper has some nine
  hundred rules and many are guesses or preferences, so three things are
  left out: its advice on style (by kind); the rules that guess a part
  of speech, find a word missing, or prefer one accepted punctuation
  (`GUESSES`, by name); and any correction that only joins or splits
  words (`only_joins`). "The effect triggers" was marked for "affect",
  and "file system" for "filesystem". A rule that turns out to guess
  goes in `GUESSES`; do not answer one false mark with a special case.
- Spanish, French and German get spelling only, from the system's checker
  (`sys::spelling`, the Mac's `NSSpellChecker`). Windows and Linux mark
  nothing in those three. The Mac's own grammar check was tried first and
  found none of a dozen plain mistakes in any of the four languages; real
  grammar in more languages means LanguageTool, which is a Java server or
  every prompt sent to someone else's.
- Only prose is checked. `check::not_prose` takes out code between
  backticks, fenced blocks, and any word that is a path, a command, an
  "@" file, a flag, a name out of code or a number; an issue touching one
  is dropped (`check::keep`), as is a word the person taught
  (`~/.emaki/words.txt`, "Learn Spelling").
- Every mark's menu ends in Ignore (`ignore_writing`). For a spelling it
  lasts until the app is quit, as the system's Ignore Spelling does,
  since Learn Spelling is the one that keeps a word. For grammar it is
  kept (`~/.emaki/ignored.txt`, `check::ignore_key`): the rule and the
  words, so the same words are still marked for another reason.
- A check runs once the typing has paused (`check_soon`), Harper's off the
  main thread. The word the caret ends is marked a little later than the
  rest. Until the answer comes, the marks move with the text
  (`check::carry`); marks are bytes of one text (`issues_for`) and are not
  drawn on another.
- The marks are the textarea's (`set_marks`, with the slash marks): a
  wavy underline, red for spelling and blue for grammar. A right click on
  one makes the input's menu what could stand there, in place of cut,
  copy and paste (`corrections`): corrections only, the grammar is not explained, and
  a grammar mark Harper has no correction for is not drawn; a correction goes in as one step for undo
  (the vendored `replace_bytes`).
- A correction is a change of the text like any other: the input says so
  (`InputEvent::Change`, silent or not) and `writing_changed` runs inside
  the click. So `check::typed` is handed changes no keystroke makes, with
  the caret anywhere, and must answer `Other` for them without slicing
  on trust. "mispelled" to "misspelled" (one letter in, the caret six
  bytes on) once sliced backwards and took the app down.
- A capital (`check::capital`) is given at the keystroke: the first
  letter of the message, of a line, or after ". ", "! " or "? ", and in
  English a lone "i". Not after an abbreviation or an ellipsis, not inside
  code. Only a single typed character counts (`check::typed`), so a
  paste, a draft coming back and an input method part way through a
  character (`composing`) are left as they are. A capital deleted and
  typed small again stays small.
- A sentence left small is marked (`check::capitals`, a grammar mark
  under the rule `SentenceCapital`, offering the capital): a word typed
  small again after a slip looked the same as one meant small, and
  nothing said so. Ignore is the choice to keep it small, and is kept
  as any grammar Ignore is. It uses the places `capital` knows and is
  added to every language's findings; Harper's own rule for this passes
  over a short sentence.
- `writing_sync` runs at every draw and notices a text set from outside a
  keystroke (setting the value is not a change event), so every such
  place is checked without knowing of it.

## Attachments

- Files first: a pasted image is written under
  `~/.emaki/uploads/<session>/` before anything else. Dropped or picked
  files stay where they are.
- They come by paste, drop and the "+" picker. `paste_attachments` is a
  `capture_action` for the input's own `Paste` on the composer wrapper: it
  takes images and file lists off the clipboard and lets text through.
- `fold_attachments` makes content blocks for a driver (images only) and
  `Attached file: <path>` lines for everything else, so the inbox channel,
  which the TUI reads as prose, still gets the path.
- A chip is a thumbnail for a picture, else a typed icon
  (`assets::file_icon_path`) with name and size.
- `image_dims` reads the pixel size from the PNG, JPEG, GIF or WebP header
  and `fit_thumb` gives the tile the picture's shape inside a maximum box
  (`ObjectFit::Contain`). The size is kept in `Attachment::size` for a
  chip, `Detail::sizes` for a session's file (`file_image_dims`), and in
  `Thumb` beside a pasted block's bytes. A header that says nothing gets a
  fixed box, letterboxed.
- In a transcript, `render_attachment` draws a pasted image from the kept
  file when the prompt names one, else reads the block back out of the
  JSONL (`transcript::image_block_bytes`, cached in `Detail::thumbs`,
  loaded off the main thread).
- A click on a tile or a chip goes through `preview_attachment`: a picture
  opens in the lightbox, any other file with the system's app for it.

## The limits row

On the left of the row under the composer:
`Context 37% (386k of 1M) · 5h 3% (4h26m) · 7d 6% (6d7h)`, coloured at the
thresholds of `~/.claude/statusline.sh`. No tooltip.

**Context.** The tokens are the transcript's (`Session::context_tokens`:
the last assistant row's input plus cache read plus cache creation). The
window is what the transcript cannot give: `claude-opus-5-5` is 200k or 1M
by the session's choice, and after a `/model` to Opus the row once read
119% where the terminal said 23%. `Limits::window_for` takes the session's
own window from `state/context/<session>.json` (written by the status-line
script, read by `limits::session_context`), else the size last seen for
the model (`learn_window`, from a driver's `result` frame's `modelUsage`),
else an assumption. `Session::models` ends on the model in use, so a
`/model` back to an earlier one counts; `<synthetic>` is not a model.
Context files a month old are pruned at launch.

**Five-hour and seven-day windows.** Per account, from two sources, the
newer winning: a driver's `rate_limit_event` frames and the terminal's
status line. Claude Code writes them to no file a terminal session leaves
behind; the `statusLine` command gets them on stdin and nothing else does.
So `scripts/statusline.sh` is built into the binary (`statusline::SCRIPT`):
it prints the terminal's line and, when `~/.emaki/state` exists, leaves
the windows in `state/rate_limits.json`, written whole and renamed into
place. `Limits::refresh_from_statusline` reads it on the clock
(`LIMITS_SECS`) and whenever the session showing is loaded.

`statusline::ensure` runs in `Hub::start` at every launch: it writes the
script to `~/.emaki/bin/statusline.sh` and sets `statusLine` to run it
unless it already does, touching no other key. There is no setting. The
previous value is kept in `state/statusline.json`. If the script fails,
the terminal's line goes blank and nothing is blocked. The tests run it.

A source that has no word on a window leaves the one held, and Claude
Code names no five-hour window between one running out and the request
that starts the next. So the row draws a window through `Window::at`:
past its reset it is 0% with no time, not the old window's number.

What was learned is kept in `state/limits.json` with its time; the row
says "limits as of …" once stale and `--` until a source has reported.
Tried and dropped: a hand-patched script outside the repository, whose
patch was lost and the row froze.

## The suggested prompt

What the empty composer says is drawn by the window over the textarea's
first line (`render_composer_hint`), and the textarea has no placeholder
of its own: the key beside a suggested prompt ("→ to accept") is
an icon, which a placeholder, being a string, cannot hold. A suggested
prompt is in italics, so it reads as offered and not yet written. Nothing
says that ⌘↩ sends.

⌘↩ sends. Settings, Composer, "Send with" makes a bare ↩ send as well
(`AppConfig::send_key`, `cmd-enter` or `enter`); ⇧↩ is a new line either
way, and the slash list takes ↩ before sending does.

After a turn Claude Code sets dim, untyped words in its input. The
composer shows them in that place. → or Tab in the empty box makes
them the text (`Workbench::accept_suggestion`, captured before the
textarea's own `MoveRight` and `IndentInline`); ⌘↩ on the empty box sends
them. `read_suggestion` reads the screen on the clock while the session
showing is idle in its terminal and the box is empty.
`driver::suggestion_on_screen` takes the line between the prompt's two
rules, after "❯", when all of it is written dim (`ESC [ 0;2 m`), which
typed words are not. A path
in the words is a link (`ESC ] 8 ;; url ESC \`), whose sequences once ran
into the rule below and hid it. Taking the words here leaves the
terminal's own suggestion in place.

## Slash commands

- A headless driver child runs `/compact` as the command it is (2.1.288
  marks it `supportsNonInteractive`: a `status: compacting` frame, the
  boundary, a `result` with no turns, so the turn ends as usual). In
  `send_message` a slash command goes to `run_in_terminal` on the `inbox`
  and `pty` channels and through the driver otherwise.
- The list over the composer (`render_slash_help`) shows the driver's own
  commands (`DriverView::commands`, from `initialize`) or the folder's
  catalogue, `BUILTIN_COMMANDS` until one is known. It follows the caret
  (`driver::slash_token_at`: a "/" at the start or after a space, and the
  name characters around the caret). Inside a sentence it offers skills
  only.
- `/compact`, `/model`, `/clear` act by themselves; `/code-review` is a
  skill, a prompt for the model, often named inside a sentence. The
  catalogue cannot tell them apart (`builtin` covers bundled skills too).
  The 2.1.288 binary can: each command declares `type` `local`,
  `local-jsx` or `prompt`, and `driver::LOCAL_COMMANDS` is the first two.
- `Workbench::slash_pick` (↩ or a click) follows the rule above; anything
  it does not run goes into the text at the caret with a space after
  (`slash_insert`). ⌘↩ on a message that is only a command sends it as
  that command, a skill with its arguments included.
- Every command named wears the accent: in the composer by ranges on the
  textarea (`Workbench::mark_slash`, on every change, through the vendored
  toolkit's `set_marks`), in a sent prompt as inline code
  (`driver::mark_commands`, which leaves code spans and fenced blocks
  alone). A token counts when `driver::slash_tokens` finds it as a word of
  its own (`/usr/bin` and `a/b` are not) and its name acts alone or is in
  the folder's catalogue (`Hub::knows_command`, cache only; the catalogue
  is read when a Claude session of that folder is opened).

## "@" files

"@" at the start of a word opens the slash list's card, with its keys, on
the folder's files and folders, as Claude Code's own prompt does.
`files.rs` is all of it:

- `list`: git's own (`ls-files`, tracked and untracked, ignored left out)
  or, outside a repository, a walk that skips hidden folders and build
  output. Folders are added behind a slash. Capped.
- `matches`: a name starting with the words, then a name holding them, a
  path holding them, letters in order. "dir/" lists what is in it; nothing
  typed shows the top.
- `at_token_at`, `at_tokens`: `a@b.com` is not a token, and a path with a
  space is `@"my file.txt"`, as Claude Code writes it.
- `mark_mentions`: an "@" whose path is on disk becomes inline code in a
  sent prompt (after `mark_commands`), trailing punctuation left out. In
  the composer it wears the same marks a command does.

`Workbench::files` keeps a list per folder, read off the main thread when
an "@" is first typed and again once stale (`want_files`, called from
`mark_slash`, the one place every change of the text goes through). ↩, ⇥
or a click inserts the path (`file_insert`): a file with a space after it,
a folder without, so the list goes on into it. The folder is the session's
or the new-session page's choice (`composer_cwd`).

On 2.1.291, a message pasted with "@notes.txt" at its very end has Claude
Code's own list open under it; Return still sends it and the file is read.
Unknown: a pasted message ending in half a name ("@note"), which the
terminal's list may complete instead of sending.

## Notices

`Workbench::notice` is the right side of the row under the composer and
the only place the app speaks: "opened in your terminal", "starting
claude…", "queued behind the running turn", refusals in the danger colour,
the update check's "Emaki x is available". With no notice it says which
channel a message would take or why nothing can send. A message that went
through gets no line; the transcript shows it. Every notice fades after
`NOTICE_SECS`, errors too. Only "Claude is working" sits above the card.
Tried and dropped: a status string in the sidebar footer, which the
scanner's index and archive counts rewrote after every turn.

After a hand-off to the terminal, `come_back` brings the window forward at
the next change to the transcript, armed only when the session was waiting
or idle at the hand-off (while the agent works, the next change is its
own).

## Probing

- `EMAKI_A11Y=1` prints gpui's view of the accessibility tree on every
  draw.
- A synthetic key does not reach a background window, so `EMAKI_KEYS`
  dispatches the list's actions through the focus:
  `EMAKI_KEYS="type:see @cr,down,tab,text"` types, presses the list's keys
  and prints what the box holds; `EMAKI_KEYS=tab` on an empty box accepts
  the suggestion.
- Nothing types into the box mid-run, so two sessions each holding a draft
  cannot be set up from a script.
- `EMAKI_KEYS="keys:i has teh apple,wait:,wait:,wait:,wait:,wait:,wait:,wait:,wait:,issues,marks"`
  types a keystroke a letter (so capitals are given), waits out the
  check, prints what is marked and opens the first mark's menu; `fix`
  takes its first correction and `back` is Backspace; `rightclick` is a
  right click on the composer's first word, made through the input's own
  handler (`rightclick:<x>` that many points along the line) and `all` selects everything. All the letters of
  one `keys:` handled in one update would arrive as one change, a paste.
- `emaki-core files <folder> [typed]` prints what "@" would offer.
- `emaki-core statusline restore` puts back the previous `statusLine`.
