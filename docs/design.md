# Design

The standard every part of Emaki a person sees is held to: the window,
its pages, sheets, cards and menus, the settings window, the icon. The bar
is the work that wins Awwwards, the Webbys and the FWA: minimal, calm,
intuitive, finished in every state. Kotoba, the studio's dictation app,
is held to the same (`../kotoba/docs/design.md`), and so is emaki.lol
(`../emaki.lol/docs/design.md`); the three read as one family. What the
grounds, faces and icons are is in `docs/look.md`; how the window's parts
work, and the motion each uses, is in `docs/window.md`.

## Rules

- **Calm.** One accent, the person's choice of seven (The accents), for
  what is on, in use, chosen, or the one thing a page is for. Nothing
  else competes with it: a page with many of the same action draws them
  quiet. The agents' own colours (Claude's terracotta, Codex's green)
  mark the agent and nothing else, and are never an accent.
- **Everything fits.** Every section of the settings shows all it has
  without scrolling. The page still scrolls, for a window made smaller;
  a new row that would push a section past the window as it opens means
  the section is reworked, not left to scroll.
- **Nothing jumps.** What appears, goes, opens, closes, grows, moves,
  fills or turns does so over a moment, with the pattern its neighbours
  use (`docs/window.md`, Motion). A section's name fades with its page.
- **Words are few.** A row's name says what it is. A line under it is
  there only where the name does not say it all, in a few plain words,
  and never repeats what the control shows. Say what to do once, to
  someone who can use a computer.
- **One way to draw each thing.** A setting is a row on a card (`line`,
  `group`); on or off is a switch; a choice among a few is `segmented`; a
  list of many drops down (`settings_pick_h`); a button is a pill with an
  icon (`choice_pill`, `branches::small_pill`); a face is a tile drawn in
  itself; a list of many on or off is ticks. A new control is a new
  pattern for every page, or it is not made.
- **Things line up.** Names start on one line, controls end on another,
  and the widest controls on a page start and end together. Every field,
  button and choice in a row is one height.
- **Both appearances, every state.** Light and dark are designed
  together, never one inverted. A change is looked at in both, and in
  every state it has: empty, full, refused, waiting, off, mid-motion, a
  narrow window.
- **Readable.** Words read on their ground at 4.5 to 1 or better, a
  control's edge at 3 to 1. A key is drawn as a key (`key_text`).

## The accents

Seven, in `look::ACCENTS`, chosen in Settings, Appearance. Periwinkle is
the first and the one a new install wears.

| Accent | Light: base, hover, active | Dark: base, hover, active |
|---|---|---|
| Periwinkle | `#5061B3` `#4A5BAC` `#4050A0` | `#94A7ED` `#A0B3FB` `#879AE0` |
| Orchid | `#845293` `#7E4C8D` `#724181` | `#C498D2` `#D1A4DF` `#B78BC4` |
| Rose | `#9C4A66` `#954360` `#893856` | `#DA93A9` `#E79FB5` `#CC879C` |
| Amber | `#A86003` `#A05B00` `#905203` | `#DD9B5F` `#EAA76B` `#CF8E52` |
| Moss | `#5A723A` `#556C34` `#4A6129` | `#9CB67C` `#A7C288` `#8FA86F` |
| Jade | `#327860` `#2B725A` `#1E6750` | `#77BCA1` `#83C8AD` `#6AAE95` |
| Teal | `#277580` `#1F6F79` `#0E646E` | `#6FB9C4` `#7BC5D0` `#61ACB7` |

- **One family.** Periwinkle is emaki.lol's indigo, OKLCH lightness 0.52
  and chroma 0.13 in the light appearance, 0.74 and 0.105 in the dark.
  The other six are other hues at that lightness, so every accent weighs
  the same on the page. Hover is 0.02 darker in the light appearance and
  0.04 lighter in the dark, active 0.06 and 0.04 darker. A link is the
  hover shade in the light appearance and the base in the dark.
- **Soft.** At one chroma the greens and the cyan are loud, so their
  chroma is eased, by most for teal and jade. An orange at 0.52 is
  brown, so amber's light shade is 0.56, as light as keeps white on it
  at 4.5 to 1. Brass, a gold at 0.52, took that place first and read as
  olive.
- **Even steps, two gaps.** The hues go round the wheel 40 to 66 degrees
  apart (272, 318, 0, 62, 128, 168, 208) and leave out two places on
  purpose: the red-orange around hue 40, Anthropic's (terracotta
  `#D97757`, and Crail `#C15F3C` beside it), and the blues between teal
  and periwinkle, which periwinkle stands for. Amber, at 62, is an
  orange of its own: yellower and further from red than theirs.
  Terracotta was the accent until v0.2.8; graphite was dropped as too
  pale to mark anything.
- **On an accent.** White on the light shade, the ink (`#1F1E1D`) on the
  dark one: 4.8 to 5.9 to 1 and 6.9 to 7.5 to 1 across the seven.
- **On the frame.** The sidebar stands on graphite beside a light page,
  where the light shade is 2 to 1. It wears the dark shade in both
  appearances (`look::on_frame`), as emaki.lol's navbar does.
- A new accent is a hue in a gap that keeps the steps even, at the same
  lightness, its contrast checked on and off it in both appearances. The
  icon's rolls are periwinkle's dark shade whatever accent is worn.

## The settings window

Kotoba's settings window: a window of its own, 820 by 600 as it opens,
on the popover's ground, its title bar clear with the traffic lights
over the rail. It is never narrower than it opens, since its rows are
laid out for that width (Kotoba's goes to 700, where Emaki's names
broke mid-word); it goes down to 460 tall, and the page scrolls.
`crates/emaki-app/src/workbench/settings.rs`.

- **The rail** at the left, 190 wide, on the muted shade at half in the
  light appearance and the sidebar's in the dark. Its rows start under
  the traffic lights, one a section, 30 tall, an icon and a name, the
  chosen one on a plate. Updates wears a dot in the accent while a newer
  Emaki waits.
- **The page** has a margin of 36 at either side. The section's name
  stands in the window's top (52 tall), level with the traffic lights,
  15 points semibold, as the Mac's own settings have it. There is no
  close button: the window closes as any does. The name and the page
  fade in with each section.
- **Groups**, 18 apart. A group is its name, 12 points semibold in the
  quiet ink, 6 over its card, in line with the rows' names (14 in from
  the card's edge). What belongs to the whole group (whose new sessions
  are shown) stands at the name's right, in line with the controls.
- **A card** is rounded 12, with a hairline edge in the border's colour
  and a ground a shade off the page: the muted colour at 40% in the light
  appearance, the sidebar's in the dark (`card_ground`). Its rows are
  parted by the same hairline. A card that holds tiles or ticks has a
  padding of 14 in place of rows.
- **A row** is at least 46 tall, 14 in at either side, 8 above and below:
  its name in 13 points, its line under it in 11.5 on 15 in the quiet
  ink, and its control at the right.
- **Controls.** Every field, button and choice in a row is 28 tall, its
  text 12.5 points. The widest are 336 (`CONTROL_W`). A switch is 36 by
  20, its knob 16, the track the switch colour when off and the accent
  when on, both moving over a moment. A control on a card takes the
  muted shade, never the sidebar's (the dark card is the sidebar's
  shade). `Press::Quiet` is the default, `Press::Primary` for the one
  thing a page waits on (Update to x), `Press::Danger` for what cannot be
  taken back (Remove).

## The icon

`scripts/icon/icon.html`: a handscroll seen from the front on the
Alkaline Software cream, a sheet with a conversation between two rolls of
paper. Navy lines, an amber line and the violet spark, and the rolls in
periwinkle's dark shade (`#94A7ED`), which keeps the terracotta's weight
and stands clear of the navy at 16 pixels (6 to 1). Chosen from five
treatments of periwinkle; one with a light band and a shade on each roll
looked the same as the flat fill below 32 pixels. `scripts/make-icon.sh`
renders every size.

## Checking

Shoot every state a change touches, in both appearances, and look at it
whole and close up (`docs/probing.md`; `EMAKI_SETTINGS_SHOTS` shoots
every section of the settings in both in one launch). Then go round
again: what is out of line, louder than it needs to be, or not
consistent with its neighbours is fixed and shot again, until nothing is
left to change.
