#!/usr/bin/env python3
"""The picture on the handscroll over the greeting on the new-session
page, as line drawings a layer each: scripts/home-scroll.py <assets/icons/home>.

The scroll itself (its two rolls, their axles, the sheet's two edges, the
sun) is drawn by the window, which moves it (`workbench::home_scroll`).
What is here is what is painted on the sheet: a landscape longer than the
sheet shows, in four layers by how far off each is, so the window can
wind it past at four speeds. Lines only, in the manner of hairline's
figures (github.com/lucasmarkes/hairline): nothing is filled, there are
no words, and what is far is dimmer, which the window does with each
layer's colour. Every layer is the same strip, STRIP wide and SHEET
high, drawn from its left.
"""
import sys

SW = 1.5
STRIP, SHEET = 640, 74


def line(*pts):
    return "M" + " L".join(f"{x} {y}" for x, y in pts)


def smooth(pts, n=10):
    """A curve through the points (Catmull-Rom), as a path."""
    out = []
    for i in range(len(pts) - 1):
        p0, p1, p2, p3 = pts[max(i - 1, 0)], pts[i], pts[i + 1], pts[min(i + 2, len(pts) - 1)]
        for j in range(n):
            t = j / n
            f = lambda a, b, c, e: 0.5 * ((2 * b) + (-a + c) * t + (2 * a - 5 * b + 4 * c - e) * t * t + (-a + 3 * b - 3 * c + e) * t ** 3)
            out.append((round(f(p0[0], p1[0], p2[0], p3[0]), 2), round(f(p0[1], p1[1], p2[1], p3[1]), 2)))
    out.append(pts[-1])
    return line(*out)


def pine(x, y, h=11):
    """A pine on a ridge: its trunk and two boughs."""
    return [line((x, y), (x, y - h)), line((x - 4, y - h + 7), (x, y - h + 2), (x + 4, y - h + 7)), line((x - 3, y - h + 3), (x, y - h - 1), (x + 3, y - h + 3))]


# The sky, furthest: bands of mist, as a picture scroll has them, and birds.
sky = []
for x, y, w in ((54, 12, 34), (70, 17, 22), (196, 9, 26), (300, 14, 40), (322, 19, 20), (452, 10, 30), (560, 15, 36), (578, 20, 18)):
    sky.append(line((x, y), (x + w, y)))
for x, y in ((150, 16), (160, 12), (410, 20), (421, 15), (512, 12)):
    sky.append(line((x, y), (x + 3, y + 2), (x + 6, y)))

# The far range, and a stroke of snow down its three peaks.
far = [smooth([(0, 44), (40, 36), (70, 40), (110, 24), (150, 38), (185, 30), (230, 42), (270, 34), (300, 40), (350, 20), (395, 36), (430, 30), (470, 40), (520, 26), (560, 38), (600, 32), (640, 42)])]
for x, y in ((110, 24), (350, 20), (520, 26)):
    far.append(line((x + 1, y + 5), (x + 5, y + 10)))
    far.append(line((x - 3, y + 6), (x - 6, y + 11)))

# The near hills: pines on their ridges, a bridge over a gap, a pavilion.
mid = [smooth([(0, 54), (50, 46), (90, 52), (140, 40), (175, 50), (215, 44), (260, 54)]), smooth([(282, 54), (310, 48), (360, 54), (410, 42), (450, 52), (500, 46), (545, 54), (600, 48), (640, 54)])]
for x, y, h in ((132, 42, 9), (140, 40, 12), (149, 43, 9), (404, 43, 9), (412, 42, 12), (500, 46, 10), (56, 46, 9)):
    mid += pine(x, y, h)
mid.append("M258 54 Q270 45 284 54")
mid.append(line((264, 50), (264, 55)))
mid.append(line((278, 50), (278, 55)))
# The pavilion: two posts, a roof with its eaves turned up, a smaller one over it.
mid += [line((316, 48), (316, 41)), line((330, 48), (330, 41)), "M311 42 Q323 35 335 42", line((319, 37), (319, 33)), line((327, 37), (327, 33)), "M315 34 Q323 28 331 34", line((323, 29), (323, 26))]

# The water, nearest: its lines, a boat under sail, reeds at the bank.
water = []
for y, runs in ((60, ((10, 30), (56, 18), (120, 44), (196, 14), (240, 36), (372, 26), (430, 40), (500, 16), (548, 50))), (65, ((30, 22), (88, 40), (160, 16), (216, 30), (300, 12), (356, 44), (452, 20), (520, 34), (590, 26))), (70, ((6, 18), (70, 12), (130, 30), (250, 20), (330, 34), (410, 14), (470, 40), (566, 22)))):
    for x, w in runs:
        water.append(line((x, y), (x + w, y)))
water += ["M434 60 Q446 67 458 60", line((446, 61), (446, 49)), line((446, 50), (455, 59), (446, 59))]
for x in (14, 20, 26, 604, 611, 618):
    water.append(line((x, 58), (x + 2, 50)))


def svg(paths):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{STRIP}" height="{SHEET}" viewBox="0 0 {STRIP} {SHEET}" fill="none" stroke="currentColor" '
            f'stroke-width="{SW}" stroke-linecap="round" stroke-linejoin="round">' + "".join(f'<path d="{p}"/>' for p in paths) + "</svg>\n")


out = sys.argv[1]
for name, paths in (("sky", sky), ("far", far), ("mid", mid), ("water", water)):
    open(f"{out}/scroll-{name}.svg", "w").write(svg(paths))
