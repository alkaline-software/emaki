#!/usr/bin/env python3
"""The handscroll over the greeting on the new-session page, as three
line drawings a colour each: scripts/home-scroll.py <assets/icons/home>.

Seen from the front, as the app's icon is: a sheet held between two
rolls on their axles, the left one thick with what is still wound on it,
and on the sheet a picture. Lines only, in the manner of hairline's
figures (github.com/lucasmarkes/hairline): a solid is its outline and
one dim line inside it, nothing is filled, there are no words, and one
mark is bright. With no fill nothing hides what is behind it, so an
axle is drawn only where it shows past its roll.
"""
import sys

SW = 1.5
TOP, FOOT = 10.0, 86.0       # a roll's two ends
KNOB = 8.0                   # how far an axle shows past them
EDGE = 6.0                   # the sheet's top and foot, inside a roll's ends


def rrect(x, y, w, h, r):
    return (f"M{x + r} {y} H{x + w - r} A{r} {r} 0 0 1 {x + w} {y + r} V{y + h - r} A{r} {r} 0 0 1 {x + w - r} {y + h} "
            f"H{x + r} A{r} {r} 0 0 1 {x} {y + h - r} V{y + r} A{r} {r} 0 0 1 {x + r} {y} Z")


def knob(cx, y, up, half=3.0, r=2.0):
    """An axle's end past its roll: three sides, open where the roll is."""
    tip = y - KNOB if up else y + KNOB
    turn = tip + r if up else tip - r
    sweep = 1 if up else 0
    return f"M{cx - half} {y} V{turn} A{r} {r} 0 0 {sweep} {cx - half + r} {tip} H{cx + half - r} A{r} {r} 0 0 {sweep} {cx + half} {turn} V{y}"


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


# the two rolls: where each begins, how wide it is, its corners
LEFT, RIGHT = (4.0, 22.0, 8.0), (224.0, 12.0, 5.0)
sil, dim, hi = [], [], []
for x, w, r in (LEFT, RIGHT):
    sil.append(rrect(x, TOP, w, FOOT - TOP, r))
    sil.append(knob(x + w / 2, TOP, True))
    sil.append(knob(x + w / 2, FOOT, False))
# the sheet: its top and its foot, from roll to roll
x0, x1 = LEFT[0] + LEFT[1], RIGHT[0]
sil.append(line((x0, TOP + EDGE), (x1, TOP + EDGE)))
sil.append(line((x0, FOOT - EDGE), (x1, FOOT - EDGE)))
# the paper wound on the left roll, seen as its last turn
dim.append(line((LEFT[0] + LEFT[1] - 6, TOP + EDGE + 3), (LEFT[0] + LEFT[1] - 6, FOOT - EDGE - 3)))
# the picture: a ridge, water under it, and a sun
dim.append(smooth([(52, 62), (72, 44), (86, 54), (106, 34), (128, 58), (144, 50), (164, 62)]))
dim.append(line((62, 70), (104, 70)))
dim.append(line((114, 70), (140, 70)))
hi.append("M185 38 m-7 0 a7 7 0 1 0 14 0 a7 7 0 1 0 -14 0")

W, H = 240, 96


def svg(paths):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" fill="none" stroke="currentColor" '
            f'stroke-width="{SW}" stroke-linecap="round" stroke-linejoin="round">' + "".join(f'<path d="{p}"/>' for p in paths) + "</svg>\n")


out = sys.argv[1]
for name, paths in (("sil", sil), ("dim", dim), ("hi", hi)):
    open(f"{out}/scroll-{name}.svg", "w").write(svg(paths))
