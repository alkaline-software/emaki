#!/usr/bin/env python3
"""The Emaki icon, drawn: a handscroll (絵巻) on the terracotta plate.
Writes an SVG for the margin master and one at full bleed."""
import sys
S = 1024

def scroll(ox, oy, k):
    """The scroll group, scaled by k and offset so it sits in the plate."""
    paper_x0, paper_x1 = 196, 828
    paper_y0, paper_y1 = 322, 702
    r_w = 96
    r_y0, r_y1 = 266, 758
    ink = "#2A2927"
    accent = "#C4573A"
    g = [f'<g transform="translate({ox} {oy}) scale({k})">']
    # the paper's shadow on the plate, blurred
    g.append(f'<rect x="{paper_x0}" y="{paper_y0+22}" width="{paper_x1-paper_x0}" height="{paper_y1-paper_y0}" rx="12" fill="#5A2412" opacity="0.35" filter="url(#soft)"/>')
    # paper, lit from the top
    g.append(f'<rect x="{paper_x0}" y="{paper_y0}" width="{paper_x1-paper_x0}" height="{paper_y1-paper_y0}" rx="12" fill="url(#paper)"/>')
    # the sheet darkens where it goes under each roller
    g.append(f'<rect x="{paper_x0}" y="{paper_y0}" width="90" height="{paper_y1-paper_y0}" fill="url(#foldL)"/>')
    g.append(f'<rect x="{paper_x1-90}" y="{paper_y0}" width="90" height="{paper_y1-paper_y0}" fill="url(#foldR)"/>')
    # rollers: cylinders, lit along their length
    for x in (paper_x0 - r_w // 2 - 2, paper_x1 - r_w // 2 + 2):
        g.append(f'<rect x="{x}" y="{r_y0+20}" width="{r_w}" height="{r_y1-r_y0}" rx="{r_w//2}" fill="#5A2412" opacity="0.35" filter="url(#soft)"/>')
        g.append(f'<rect x="{x}" y="{r_y0}" width="{r_w}" height="{r_y1-r_y0}" rx="{r_w//2}" fill="url(#roller)"/>')
        g.append(f'<rect x="{x+14}" y="{r_y0-22}" width="{r_w-28}" height="46" rx="14" fill="{ink}"/>')
        g.append(f'<rect x="{x+14}" y="{r_y1-24}" width="{r_w-28}" height="46" rx="14" fill="{ink}"/>')
    # the mark on the sheet: a chevron in the accent, three lines of ink
    cx, cy = 330, 512
    g.append(f'<path d="M{cx-30} {cy-60} L{cx+34} {cy} L{cx-30} {cy+60}" fill="none" stroke="{accent}" stroke-width="36" stroke-linecap="round" stroke-linejoin="round"/>')
    for (lx, ly, lw) in [(428, 438, 300), (428, 512, 222), (428, 586, 346)]:
        g.append(f'<rect x="{lx}" y="{ly-14}" width="{lw}" height="28" rx="14" fill="{ink}" opacity="0.85"/>')
    g.append('</g>')
    return "\n".join(g)

def plate(x, y, w, radius):
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{w}" rx="{radius}" fill="url(#plate)"/>'
            f'<rect x="{x}" y="{y}" width="{w}" height="{w}" rx="{radius}" fill="url(#sheen)"/>')

def svg(full_bleed: bool):
    defs = '''<defs>
  <linearGradient id="plate" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#E88A62"/>
    <stop offset="1" stop-color="#C85E3C"/>
  </linearGradient>
  <linearGradient id="paper" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#FBF5E8"/>
    <stop offset="1" stop-color="#F0E6D0"/>
  </linearGradient>
  <linearGradient id="foldL" x1="0" y1="0" x2="1" y2="0">
    <stop offset="0" stop-color="#B99F74" stop-opacity="0.55"/>
    <stop offset="1" stop-color="#B99F74" stop-opacity="0"/>
  </linearGradient>
  <linearGradient id="foldR" x1="1" y1="0" x2="0" y2="0">
    <stop offset="0" stop-color="#B99F74" stop-opacity="0.55"/>
    <stop offset="1" stop-color="#B99F74" stop-opacity="0"/>
  </linearGradient>
  <linearGradient id="roller" x1="0" y1="0" x2="1" y2="0">
    <stop offset="0" stop-color="#D6C4A0"/>
    <stop offset="0.35" stop-color="#FBF5E8"/>
    <stop offset="0.7" stop-color="#EEE2C8"/>
    <stop offset="1" stop-color="#C9B48C"/>
  </linearGradient>
  <filter id="soft" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="14"/></filter>
  <radialGradient id="sheen" cx="0.3" cy="0.08" r="0.9">
    <stop offset="0" stop-color="#fff" stop-opacity="0.22"/>
    <stop offset="0.6" stop-color="#fff" stop-opacity="0"/>
  </radialGradient>
</defs>'''
    if full_bleed:
        body = plate(0, 0, S, 0) + scroll(0, 0, 1.0)
    else:
        P = 824; o = (S - P) // 2; k = P / S
        body = plate(o, o, P, 184) + scroll(o, o, k)
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{S}" height="{S}" viewBox="0 0 {S} {S}">{defs}{body}</svg>'

out = sys.argv[1]
open(f"{out}/icon-margin.svg", "w").write(svg(False))
open(f"{out}/icon-bleed.svg", "w").write(svg(True))
print("wrote", out)
