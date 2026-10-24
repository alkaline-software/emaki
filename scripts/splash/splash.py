#!/usr/bin/env python3
"""The Windows installer's splash: what Velopack's Setup.exe shows while it
puts Emaki in place, then the app opens. The app's icon and its name in the
wordmark face Windows gets (Belleza), on the app's cream, in a frame with a
hairline edge. Velopack draws its progress bar over the bottom 12 pixels, in
the accent (release.yml passes the color), so that band is the bar's track.
Velopack takes the picture's pixels as points and scales them for the
screen. Needs Pillow; the PNG beside this script is committed, so the script
runs only when the picture changes:

    uv run --with pillow scripts/splash/splash.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H = 520, 320
BAR = 12                         # Velopack's progress bar, at the foot
CREAM = (250, 249, 245)          # the app's background, #FAF9F5
TRACK = (238, 236, 229)          # the bar's track, a step under the cream
EDGE = (226, 223, 214)           # the frame's hairline
INK = (31, 30, 29)               # the app's foreground, #1F1E1D
ROOT = Path(__file__).resolve().parents[2]
ICON = ROOT / "crates/emaki-app/assets/icon/icon-512.png"
FONT = ROOT / "crates/emaki-app/assets/fonts/Belleza-Regular.ttf"
OUT = Path(__file__).with_name("splash.png")

# Drawn at four times the size and brought down, for smooth edges.
S = 4
img = Image.new("RGB", (W * S, H * S), CREAM)
draw = ImageDraw.Draw(img)
draw.rectangle([0, (H - BAR) * S, W * S, H * S], fill=TRACK)

icon = Image.open(ICON).convert("RGBA").resize((132 * S, 132 * S), Image.LANCZOS)
top = 62
img.paste(icon, ((W * S - icon.width) // 2, top * S), icon)

font = ImageFont.truetype(str(FONT), 40 * S)
text = "Emaki"
box = draw.textbbox((0, 0), text, font=font)
tw = box[2] - box[0]
draw.text(((W * S - tw) // 2 - box[0], (top + 132 + 22) * S - box[1]), text, font=font, fill=INK)

img = img.resize((W, H), Image.LANCZOS)
ImageDraw.Draw(img).rectangle([0, 0, W - 1, H - 1], outline=EDGE)
img.save(OUT, "PNG")
print(f"wrote {OUT} {W}x{H}")
