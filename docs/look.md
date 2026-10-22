# Look

About `crates/emaki-app/src/look.rs`, `fonts.rs`, `assets.rs`, `assets/`,
`themes/emaki.json`, and the avatar and name code in `workbench.rs`.

## Rules

- A button is `choice_pill` (`workbench.rs`): a pill with an icon before
  its word, as Select on the sessions page is. Its kind (`Press`) is how
  it is drawn: quiet for one of several and for the way out, the
  danger's for what cannot be taken back, filled with the accent for the
  answer a card is for. Every choice on a card that asks something is
  one, and so is any new button; the toolkit's `Button` is not used for
  a new one. A choice in a menu has an icon too (`MenuDo::icon`).
  On a strip or a foot line it is the smaller `branches::small_pill`. No button has
  a word and no icon, and none is the toolkit's `Button` with a label:
  the last of those, and the plain outlined pill they were drawn as
  before, were taken out. The toolkit's `Button` stays for a button that
  is an icon alone (a sheet's close).
- **A clip in gpui is a rectangle,** `overflow_hidden` too: a ground
  (a fill, a hover, a chosen row, a header strip, a rail, a picture)
  that reaches a rounded card's corner stands square past its curve.
  Round it there itself, by the card's radius less its border
  (`rounded_bl(px(17.))` in a card of 18). What scrolls cannot be
  rounded, since its edge moves: keep room between the scroller and the
  corner outside the scroller (a margin, or padding on the wrapper,
  never padding inside it, which scrolls away). Over a solid ground
  the corner can be painted over instead (`card_corners`); a sheet or a
  menu floats over a page, so it cannot.
- A hover's label names the key that does the same, in brackets after
  the words, as the Mac writes it: "Changes and History (⌘⇧G)". A
  button with a key and no label gets one for that.
- **Every key the window shows is drawn by `key_text`** (`workbench.rs`),
  now and for anything added. A shortcut is its keys on caps
  (`key_cap`): small plates of one height, none narrower than that,
  each key in its plate's middle, a sign (⌘ ⇧ ⌥ ⌃ 🌐 ↩ ⇥ ↑ ↓) as an icon
  (`icons/key-*.svg`, Tabler's, and the two arrows) and a letter as a
  letter, both of one size. Tried and dropped, twice: the signs as
  characters, each from whichever face has it, so ⌘ and ↩ stood at two
  heights; then the signs as icons on the words' line, which were level
  with each other and still neither the size nor the height of the
  letter beside them. On equal plates they are. Brackets that hold only
  shortcuts go ("Search (⌘K)" is the words and then the caps). Three
  ways in: `kbd_hint` for a shortcut beside what it does (the sidebar's
  rows, the commit's comment), `tip` for a hover's label (every
  `managed_tooltip` builds with it, key or no key), and `key_text`
  itself for a line of words, a button's hint or a notice. A place with
  caps of its own takes the sign alone (`key_glyph`: the slash list's
  foot). A new sign is a line in `key_sign` and `KEY_SIGNS` and an
  icon; a string with a sign in it is never handed to the window as it
  is. Off a Mac ⌘ and ⌥ are the words Ctrl and Alt on their caps. Not
  covered: the system's own menus, and what an agent's terminal prints.
- No call site names Tabler. A new icon is a file in `assets/icons/`
  under the toolkit's file name and a line in `OWN` (`assets.rs`).
- Every face the window carries is under a licence that lets an app
  carry it (the SIL Open Font License), with that licence beside its
  files in `assets/fonts`. Anthropic's faces are theirs: they are not
  carried, not loaded from the Claude app and not fetched.
- A face is a static file a weight. Handed a variable font's bytes,
  gpui holds one face of it (Regular) and draws every bold as regular.
- Call `look::apply`, never `sync_system_appearance`, the window's
  appearance observer included, or a pinned appearance flips with the
  system.
- Paint the accent into both theme configs before the toolkit gets them, so
  `Theme::change` keeps it on every switch with nothing to patch after.
- Write settings through `Config::edit`, which rewrites the file without
  the environment overrides `Config::load` applies.
- A new avatar gets a new file name, or the image cache answers with the
  old picture.
- Draw the avatar from the copy made for its size. Handed the whole
  picture, the GPU samples a few of its pixels for each one drawn and a
  large picture comes out jagged on a small disc.

## The grounds

Three things are kept apart by more than a shade, because a window of
creams a step from each other tired the eye and gave it nowhere to rest.

- The frame and the sidebar on it (`title_bar.background`): a warm
  graphite in the light appearance (the ink's own near black was tried
  and was too heavy), and in the dark one a grey
  lighter than the page, near the light appearance's graphite, so the
  page is a well in a lighter shell and the sidebar is the same thing in
  both.
- The page and what lies on it: in the light appearance near white with
  a trace of warmth, its greys (panels, tracks, bubbles, lines) the old
  creams with most of their colour taken out. In the dark appearance the
  page is the darkest ground there is.
- The accent, the one colour the window wears of its own, which is why
  it stands out (`docs/design.md`, The accents).

A track of buttons (a segmented control, the tabs' pill, the strip's two
tracks, the tray under the composer) is `tab_bar.segmented.background`
in the dark appearance, lighter than the page and than a card on it. It
was the panels' ground, which on the darker page was the page's own
shade, and a control had no edge.

The terminal keeps Kaku's cream in the light appearance
(`docs/channels.md`): it is a different thing from the page and may look
it.

## Icons

The app's own icon is drawn from two sizes of it (`assets.rs`): the 128
for the sidebar's strip, and the 512 for the About page, where it stands
larger. The 128 stretched to that size drew a hairline at the picture's
right and foot.

Every icon is Tabler's outline (tabler.io/icons), drawn with a stroke of
1.75 where Tabler's own is 2: at the sizes the window uses, 2 reads heavy.
They took the place of Phosphor's in v0.1.8, whose corners read as sharp.
The files panel's tree keeps Catppuccin's (`docs/panels.md`). The toolkit ships Lucide's and
asks for each by file name (`IconName::Close` is `icons/close.svg`), and
`assets.rs` answers for a path before the toolkit's own set does. So
`assets/icons/` holds a Tabler icon under each toolkit name (`close.svg`
is Tabler's `x`, `bot.svg` its `robot-face`, `tree-view.svg` its `subtask`), and the
icons the toolkit draws by itself change too. The files are from
the `@tabler/icons` package (`icons/outline/<name>.svg`; `star-fill` from
`icons/filled`), MIT, with `TABLER-LICENSE` beside them. Each is written
on one line without Tabler's empty bounding path. A new file is also
named in `OWN` in `assets.rs`: one that is not there draws nothing, and
nothing says so.

`file-plus.svg` is two of Tabler's put together: the sheet of its
`file-search`, which leaves the lower right corner free, and the plus of
its `folder-plus`, so New File and New Folder carry the plus in one
place. Tabler's own `file-plus` has it in the middle of the sheet.

Not Tabler: the flags in `icons/flags` and the handscroll's landscape in `icons/home`
(`docs/window.md`), drawn here, `bubble-tail.svg`, the tail of a prompt's bubble
(`docs/conversation.md`), `claude.svg`, Claude's own mark, the other agents' marks in
`icons/agents/` (`docs/agents.md`; Codex wears OpenAI's everywhere), `mark.svg`, and eleven toolkit icons
that were never replaced (the window controls, the right and bottom
panels, `inspector`, `resize-corner`, `star-off`), none of which the
window draws.

## Code's colours

One set of inks an appearance, `look::Inks` (`INKS_LIGHT`, `INKS_DARK`):
blue, violet, gold, rose, teal, green, red and a grey for comments.
Everything that colours code or a terminal takes them from there.

- The hues are Rosé Pine's (pine, iris, gold, rose, foam, love) with a
  sage green beside them, each taken darker or lighter until it reads
  on the window's own grounds: 4.5 to 1 or more against the page in
  either appearance, the grey a step quieter. They took the place of VS
  Code's Light+ and Dark+, whose crimson and pure blue were loud on the
  cream page.
- The `highlight` part of each theme in `themes/emaki.json` names an ink
  where it wants one ("@l-blue", "@d-rose"), and `look::install` puts
  the colour in before the file is parsed. It serves the editor in the
  file's pane and a conversation's code blocks alike: `md_view` takes
  `theme.highlight_theme`, not the toolkit's default, which was a
  second set of colours.
- Which token gets which ink: keywords, tags and a markdown title blue;
  control keywords, numbers, constants and attributes violet; strings
  and a markdown code span gold; functions rose; types and properties
  teal; comments and the marks of markdown grey. Variables, operators
  and brackets are the page's ink, so a line is not all colour.
- The terminals' sixteen named colours are the same inks
  (`term_panel`'s two schemes): red, green, yellow (gold), blue, magenta
  (violet), cyan (teal), normal and bright alike. Green and red are
  there for a terminal's "added" and "failed"; code does not use them.
  The terminals' ground, ink, cursor and tabs are still Kaku's.

## Inline code

The accent on a wash of itself. `md_view` sets
the letters to `theme.link` and `inline_code_chip` to a thinned
`theme.primary` with a border, so both follow the accent.

- The plate is the vendored toolkit's `Inline::paint_code_chips`. A text
  highlight's own background is a square box the full height of the line,
  so the plate is painted before the text, one rounded quad per line of
  each span.
- A text run has no padding, so the vendored markdown parser sets each span
  between two narrow no-break spaces (U+202F) at each end: the nearer is
  the plate's padding, the farther its margin. gpui's line wrapper counts
  that character as part of a word, so they stay with the span at a line's
  end; a breaking space left the next line starting with a gap.
- The view's Copy action takes the pairs out again. The copy buttons read
  the model and never see them.
- Inline code is the paragraph's size. A gpui text run carries a face
  but no size, so a smaller size would be a font of its own.

## The faces

Twelve, eleven of them built in (`fonts.rs`, `assets/fonts`), chosen in Settings,
Fonts, where each is a tile drawn in itself:

- **The chat font**, one of eight (`app.chat_font`): the serifs Source
  Serif, Newsreader, Literata and Lora, and the sans serifs Inter, IBM
  Plex Sans, Source Sans and Geist. The first serif to begin with. A
  file that says `serif` or `sans`, from when there were two, gets the
  first of each.
- **The monospace font**, one of four (`terminal.font`, where it was
  the terminal's alone): JetBrains Mono, the one to begin with, then
  Monaco on a Mac and IBM Plex Mono elsewhere, Maple Mono and Geist
  Mono. Monaco is Apple's: it is used from the system on a Mac, as
  Optima is, and never carried, which is why Windows and Linux have
  another in its place. IBM Plex Mono has no medium or semibold here:
  its files name those as families of their own. The second place is
  one choice whichever face a system has in it (`mono_face`), so a
  settings file carried from one to the other keeps its meaning. The chosen face is
  every mono in the window: code in a conversation,
  the file's editor, the terminal. `look::install` writes it into both
  theme configs as `mono_font_family`, so `Theme::change` keeps it, and
  `Workbench::set_mono_font` installs and draws again when it changes.
- Titles and the greeting are in the chat font (`display_family`), so a
  page's title and the conversation agree.
- The chat font is applied to the transcript container (`render_detail`),
  which the markdown view inherits, and to what the agent says on the
  cards under it: a plan, a question and its choices (`render_dialog`,
  `render_question`). Those are the conversation's words before the
  transcript has them. Code stays in the mono face; the cards' own
  words (their heads, badges, tabs and buttons), the chrome and the
  composer in the UI face, which is the system's.
- The files are Google Fonts' static instances (regular, semibold, bold
  and an italic; a medium too for a mono), Maple Mono its own release's.
- Code among a conversation's words is in a smaller cut of the
  monospace face (`fonts::inline_cut`, `INLINE_SCALE`,
  `transcript::chat_md_view`): the same face as a family of its own,
  "<family> Inline", made at launch from the face's bytes with more
  units to the em, which is what `size-adjust` does in a style sheet.
  A monospace face is large on its body, and at the words' size its
  code stood over them. gpui lays a paragraph out at one size (a
  `TextRun` has a face and no size), so code among words can be smaller
  only by being another face. One scale serves the three sizes, since
  the cut is drawn at the words' own. Monaco's cut is made from the
  system's file and not carried. Only the conversation uses it: a file
  shown, a code block, the editor and the terminal keep the face as it
  is.
- Anthropic Serif, Sans and Mono were the faces until v0.2.1, loaded
  from a Claude app on the machine and from `~/.emaki/fonts`. They are
  Anthropic's own and were taken out whole, the script that fetched the
  mono with them.
- The system's faces are used and never carried: Optima for the app's
  name in the sidebar and the person's in its footer (`WORDMARK_FACES`),
  and Monaco. Optima is the Mac's alone, so the window carries Belleza,
  a free face with the same flared strokes, for Windows and Linux.

## The look settings

Four settings in `config.json` under `app`, changed in the settings window
(the first two in Appearance, the others in Fonts with the monospace
face):

- `appearance` (`system`, `light`, `dark`), drawn by `look::apply`.
- `accent`, a name from `look::ACCENTS`, periwinkle to begin with. The
  palette stays in `themes/emaki.json`; an accent is a substitution over
  the keys that carry periwinkle there, and what is drawn on it
  (`primary.foreground`) with them. A name from before the seven
  (`terracotta`, `blue`, `graphite`) is periwinkle, `violet` orchid,
  `green` jade and `brass` amber (`look::accent`).
- `chat_font`, one of the eight chat faces (The faces).
- `chat_size` (`small`, `medium`, `large`), the window's one text size
  ("Font size"): the terminal's goes with it. `AppConfig::chat_px`
  turns it into the reply's pixel size, the prompt and thoughts a little
  under. Medium is the Claude desktop app's body size and line height, read
  from its stylesheet, which also sets bold to weight 600, matched through
  the vendored toolkit's `strong_font_weight`.

The window also sets `driver.default_mode` and `driver.default_model`.
The terminal's own settings are under `terminal`, in the window's Terminal
section (`docs/channels.md`, The terminal panel).
`driver.claude_path` names the `claude` binary when `PATH` does not.

## The picture and the name

`app.avatar` and `app.user_name`, the first two rows of Settings,
Appearance: Avatar and Name.

- `Workbench::pick_avatar` opens the file picker. `avatar_keep` writes
  `~/.emaki/avatar/<time>.png` with the `image` crate gpui already builds:
  turned the way its camera says, cut to the square at its middle, capped
  in size. Beside it goes a Lanczos copy for each size in `AVATAR_SIZES`
  (`<time>@28.png`, twice the points across). A picture kept before the
  copies existed gets them at launch.
- `set_avatar` deletes the picture it replaces and its copies.
- Remove is a pill of our own in the danger colour. The toolkit's outline
  button sets its own ink under the pointer and turned the red black.
- The name is a field (`name_input`, in the focus wrapper every own input
  has), saved at each change.
- Fallbacks: the first letter of the name on a disc of the accent
  (`Workbench::avatar`, also when the picture's file is gone), and the
  machine's account name (`sys::user_first_name`) when the name is empty.
- The name is the greeting's and the sidebar footer's, where it is set as
  the wordmark is (`fonts::wordmark_family`, the regular drawn twice half a
  pixel apart) in the ink, not the accent.
- In the footer the picture opens Settings on Appearance whatever section
  was last showing. The gear opens Settings where it was left, and the name
  is not a button.

## Probing

- `EMAKI_SETTINGS=appearance` opens the settings window on that section. A scratch
  `config.json` under `EMAKI_HOME` sets a picture and a name.
- A `mouseMoved` event posted to the pid moves gpui's hover, so a hover
  state (the Remove pill) is one event and an `EMAKI_SHOT` away.
