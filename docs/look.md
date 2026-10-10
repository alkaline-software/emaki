# Look

About `crates/emaki-app/src/look.rs`, `fonts.rs`, `assets.rs`, `assets/`,
`themes/emaki.json`, and the avatar and name code in `workbench.rs`.

## Rules

- No call site names Tabler. A new icon is a file in `assets/icons/`
  under the toolkit's file name and a line in `OWN` (`assets.rs`).
- Anthropic's fonts are not ours to ship. They are loaded from the machine,
  each with a fallback.
- On macOS register the Claude app's variable fonts with CoreText, not
  `text_system().add_fonts`: handed the bytes, gpui holds one face per file
  (Regular) and draws every bold as regular.
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

## Icons

Every icon is Tabler's outline (tabler.io/icons), drawn with a stroke of
1.75 where Tabler's own is 2: at the sizes the window uses, 2 reads heavy.
They took the place of Phosphor's in v0.1.8, whose corners read as sharp.
The files tree keeps Catppuccin's (`docs/panels.md`). The toolkit ships Lucide's and
asks for each by file name (`IconName::Close` is `icons/close.svg`), and
`assets.rs` answers for a path before the toolkit's own set does. So
`assets/icons/` holds a Tabler icon under each toolkit name (`close.svg`
is Tabler's `x`, `bot.svg` its `robot-face`, `tree-view.svg` its `subtask`), and the
icons the toolkit draws by itself change too. The files are from
the `@tabler/icons` package (`icons/outline/<name>.svg`; `star-fill` from
`icons/filled`), MIT, with `TABLER-LICENSE` beside them. Each is written
on one line without Tabler's empty bounding path.

Not Tabler: `claude.svg`, Claude's own mark, `mark.svg`, and eleven toolkit icons
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

The accent on a wash of itself, as the Claude app draws it. `md_view` sets
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
- The Claude app sets inline code at 0.9em. A gpui text run carries a face
  but no size, so the smaller size is a font of its own, `Inline Anthropic
  Mono` (`fonts::inline_code_family`). Without it inline code is the size
  of the paragraph.
- Still different from the Claude app: the plate has a small margin where
  theirs has none, and a span that wraps gets a plate per line with no
  padding at the break. Their rule is `code:not(pre code)`: `.9em`,
  `padding: .0625em .25em`, `border: .5px solid` at about 15%,
  `border-radius: .4em`, danger red on a 5% wash of the text colour.

## The code font

Anthropic Mono when the machine has it. The Claude app downloads it when it
runs, and its bundle holds the serif and the sans and no mono, so it cannot
be loaded the way those two are.

- `fonts::install` registers whatever is in `~/.emaki/fonts`.
  `fonts::code_family` takes the first family that starts with "Anthropic
  Mono" (the file calls itself "Anthropic Mono Web"), else on macOS
  `.AppleSystemUIFontMonospaced`, else the toolkit's own.
- `look::install` writes it into both theme configs as `mono_font_family`,
  so it is every mono in the window but the terminal panel's, which is
  JetBrains Mono (`docs/channels.md`, The terminal panel).
- `scripts/anthropic-mono.py` fills `~/.emaki/fonts`: given the roman and
  italic woff2, it unpacks each to TrueType and writes a second copy under
  the family `Inline Anthropic Mono` with the em enlarged by 1/0.9.
- The woff2 URLs are in the Claude app's live stylesheet, fetched from
  `assets-proxy.anthropic.com` (`@font-face{font-family:anthropic-mono}`).
  They carry a content hash and change, so read them again each time. The
  stylesheet inside the app bundle shows only the fallbacks and led to SF
  Mono, which was wrong.

## The conversation's font

Anthropic Serif or Anthropic Sans, loaded by `fonts.rs` at start from a
Claude app on this machine (`Resources/fonts` on macOS, the Squirrel
install under `LOCALAPPDATA` on Windows).

- macOS: `CTFontManagerRegisterFontsForURL` for the process. CoreText lists
  a variable font's named instances as faces of their own, and gpui's
  family lookup falls through to the system source when it was not handed
  the family, so bold finds a bold.
- Elsewhere the bytes go in as they are, and bold stays regular until that
  platform's text system learns variable fonts.
- Without a Claude app the serif falls back to Georgia and the sans to the
  window's face, and the settings panel says so.
- The face is applied to the transcript container (`render_detail`),
  which the markdown view inherits, and to what the agent says on the
  cards under it: a plan, a question and its choices (`render_dialog`,
  `render_question`). Those are the conversation's words before the
  transcript has them, and in the window's face a plan changed face the
  moment it was answered. Code stays in the mono face; the cards' own
  words (their heads, badges, tabs and buttons), the chrome and the
  composer in the UI face.

## The look settings

Four settings in `config.json` under `app`, changed in the settings panel:

- `appearance` (`system`, `light`, `dark`), drawn by `look::apply`.
- `accent`, a name from `look::ACCENTS`. The palette stays in
  `themes/emaki.json`; an accent is a substitution over the keys that carry
  the terracotta there.
- `chat_font` (`serif`, the default, or `sans`).
- `chat_size` (`small`, `medium`, `large`), which `AppConfig::chat_px`
  turns into the reply's pixel size, the prompt and thoughts a little
  under. Medium is the Claude desktop app's body size and line height, read
  from its stylesheet, which also sets bold to weight 600, matched through
  the vendored toolkit's `strong_font_weight`.

The panel also sets `driver.default_mode` and `driver.default_model`.
The terminal's own settings are under `terminal`, in the panel's Terminal
section (`docs/channels.md`, The terminal panel).
`driver.claude_path` names the `claude` binary when `PATH` does not.

## The picture and the name

`app.avatar` and `app.user_name`, the first two rows of Settings,
Appearance.

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

- `EMAKI_SETTINGS=appearance` opens the panel on that section. A scratch
  `config.json` under `EMAKI_HOME` sets a picture and a name.
- A `mouseMoved` event posted to the pid moves gpui's hover, so a hover
  state (the Remove pill) is one event and an `EMAKI_SHOT` away.
