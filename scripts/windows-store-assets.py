#!/usr/bin/env python3
"""The Microsoft Store package's tile images, from the app icon: the rounded
icon with its margin, as Windows shows an app's icon, at the sizes the
package's manifest names (crates/emaki-app/windows/store/AppxManifest.xml).
The PNGs are committed, so this runs only when the icon changes:

    uv run --with pillow scripts/windows-store-assets.py
"""
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ICON = Image.open(ROOT / "crates/emaki-app/assets/icon/icon-1024.png").convert("RGBA")
OUT = ROOT / "crates/emaki-app/windows/store/Assets"


def square(n):
    return ICON.resize((n, n), Image.LANCZOS)


def wide(w, h):
    tile = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    icon = square(h)
    tile.paste(icon, ((w - h) // 2, 0), icon)
    return tile


for name, img in {
    "Square44x44Logo.png": square(44),
    "Square150x150Logo.png": square(150),
    "Wide310x150Logo.png": wide(310, 150),
    "StoreLogo.png": square(50),
}.items():
    img.save(OUT / name)
    print("wrote", name, img.size)
