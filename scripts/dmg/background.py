#!/usr/bin/env python3
"""The disk image's Finder background: "Drag Emaki to Applications" over the
app's cream, with a terracotta arrow between where the app icon and the
Applications link sit. 1320x840 pixels at 144 dpi, which Finder shows as
the 660x420 window release-mac.sh opens. Needs Pillow and LXGW WenKai
Medium (the face Pingfan's other apps' disk images use); the PNG beside
this script is committed, so the script runs only when the picture changes:

    python3 scripts/dmg/background.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H = 1320, 840
CREAM = (250, 249, 245)        # the app's background, #FAF9F5
INK = (31, 30, 29)             # the app's foreground, #1F1E1D
TERRACOTTA = (217, 119, 87)    # the app's accent, #D97757
FONT = Path.home() / "Library/Fonts/LXGWWenKai-Medium.ttf"
OUT = Path(__file__).with_name("background.png")

img = Image.new("RGB", (W, H), CREAM)
draw = ImageDraw.Draw(img)

text = "Drag Emaki to Applications"
for size in range(96, 40, -1):
    font = ImageFont.truetype(str(FONT), size)
    box = draw.textbbox((0, 0), text, font=font)
    if box[2] - box[0] <= W * 0.85:
        break
tw, th = box[2] - box[0], box[3] - box[1]
draw.text(((W - tw) // 2 - box[0], int(H * 0.28) - th // 2 - box[1]), text, font=font, fill=INK)

# The arrow runs between the icon positions the layout script sets
# (165 and 495 logical points, doubled), clear of both 128pt icons.
y, x1, x2, head = int(H * 0.63), 480, 840, 28
draw.line([(x1, y), (x2 - head, y)], fill=TERRACOTTA, width=5)
draw.polygon([(x2, y), (x2 - head, y - head // 2), (x2 - head, y + head // 2)], fill=TERRACOTTA)

img.save(OUT, "PNG", dpi=(144, 144))
print(f"wrote {OUT} {W}x{H}")
