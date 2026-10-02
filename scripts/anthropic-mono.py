#!/usr/bin/env python3
"""Put Anthropic Mono where Emaki looks for it, on this machine only.

The Claude app sets code in a web font its stylesheet calls `anthropic-mono`
and fetches from Anthropic's servers when it runs; the app bundle holds no
file for it. The font is Anthropic's, so Emaki does not ship it. This
unpacks the two woff2 files (roman, italic) into `~/.emaki/fonts`, which
`fonts::install` registers at launch, and writes a second copy of each for
inline code: the Claude app draws inline code at 0.9 of the body size, and
a gpui text run carries a face but no size, so the copy is the same font
with a larger em, which draws every glyph at 0.9 of what was asked for. It
goes under its own family name, `Inline Anthropic Mono`.

The woff2 URLs carry a content hash and change. They are in the app's live
stylesheet, the `c6a992d55-*.css` under
`https://assets-proxy.anthropic.com/claude-ai/v2/assets/v1/`, in the two
`@font-face{font-family:anthropic-mono;...}` rules.

    uv run --with fonttools --with brotli scripts/anthropic-mono.py <roman> <italic>

Each argument is a URL or a file.
"""

import os
import sys
import urllib.request
from io import BytesIO
from pathlib import Path

from fontTools.ttLib import TTFont

INLINE_SCALE = 0.9
INLINE_FAMILY = "Inline Anthropic Mono"


def read(src: str) -> bytes:
    if src.startswith(("http://", "https://")):
        with urllib.request.urlopen(src) as r:
            return r.read()
    return Path(src).read_bytes()


def rename(font: TTFont, family: str) -> None:
    """Give the font a family of its own, so it never stands in for the original."""
    name = font["name"]
    old_family = name.getDebugName(16) or name.getDebugName(1)
    old_ps = name.getDebugName(6).split("-")[0]
    ps = family.replace(" ", "")
    for rec in name.names:
        text = rec.toUnicode()
        if rec.nameID in (1, 4, 16, 3):
            rec.string = text.replace(old_family, family)
        elif rec.nameID in (6, 25):
            rec.string = text.replace(old_ps, ps)


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    home = Path(os.environ.get("EMAKI_HOME") or Path.home() / ".emaki")
    out = home / "fonts"
    out.mkdir(parents=True, exist_ok=True)
    for src, style in zip(sys.argv[1:], ("Roman", "Italic")):
        data = read(src)

        font = TTFont(BytesIO(data))
        font.flavor = None
        font.save(out / f"AnthropicMono-{style}.ttf")

        inline = TTFont(BytesIO(data))
        inline.flavor = None
        # Outlines and metrics stay as they are; a larger em makes them all
        # that much smaller at any size.
        inline["head"].unitsPerEm = round(inline["head"].unitsPerEm / INLINE_SCALE)
        rename(inline, INLINE_FAMILY)
        inline.save(out / f"InlineAnthropicMono-{style}.ttf")

        print(f"{style}: {font['name'].getDebugName(16) or font['name'].getDebugName(1)} and {INLINE_FAMILY} written to {out}")


if __name__ == "__main__":
    main()
