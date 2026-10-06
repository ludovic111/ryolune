#!/usr/bin/env python3
"""Writes ryolune's mark and app icon (lsuite design system v2, one ink, one dithered part).

The mark is "the ring and the dot" cut square: an octagonal ring (the corners cut off like the
letters of Chakra Petch, the interface face) around a solid square dot, its lower-left side
dissolving into ordered dither, like the shadow side of the moon (lune).

    desktop/icons/mark.svg          ink on paper
    desktop/icons/ryolune.svg       the app icon (then run scripts/make-icon.sh)
    desktop/assets/icons/mark.svg   the window's copy, in currentColor

Run from anywhere: python3 scripts/gen-mark.py
"""
import math
import os

C = 32.0                      # centre of the 64-unit grid
OUTER, RING = 22.0, 8.0       # half-size of the outer square, ring thickness
CUT = 11.0                    # how much each outer corner is cut
DOT = 5.0                     # half-size of the square dot
SPLIT = -0.12                 # where the dither starts, on the axis lower-left (-1) to upper-right (+1)


def octagon(half, cut):
    a, b = C - half, C + half
    return [(a + cut, a), (b - cut, a), (b, a + cut), (b, b - cut),
            (b - cut, b), (a + cut, b), (a, b - cut), (a, a + cut)]


INNER_CUT = CUT - RING * math.tan(math.radians(22.5))
OUT = octagon(OUTER, CUT)
IN = octagon(OUTER - RING, INNER_CUT)


def axis(p):
    """Position along the diagonal from the lower-left (-1) to the upper-right (+1)."""
    x, y = p
    return ((x - C) - (y - C)) / (2 * OUTER)


def clip(poly, keep):
    """Sutherland-Hodgman against the half-plane axis(p) >= SPLIT (keep) or <= SPLIT."""
    def inside(p):
        return axis(p) >= SPLIT if keep else axis(p) <= SPLIT

    def cross(p, q):
        t = (SPLIT - axis(p)) / (axis(q) - axis(p))
        return (p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t)

    out = []
    for i, p in enumerate(poly):
        q = poly[(i + 1) % len(poly)]
        if inside(p):
            out.append(p)
            if not inside(q):
                out.append(cross(p, q))
        elif inside(q):
            out.append(cross(p, q))
    return out


def inside_poly(p, poly):
    x, y = p
    c = False
    for i in range(len(poly)):
        (x1, y1), (x2, y2) = poly[i], poly[(i + 1) % len(poly)]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            c = not c
    return c


BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]


def dots(cell=2.2, size=1.75):
    """The dissolving side of the ring: square dots on a grid, fewer as they go."""
    out = []
    j, y = 0, C - OUTER
    while y < C + OUTER:
        i, x = 0, C - OUTER
        while x < C + OUTER:
            p = (x + cell / 2, y + cell / 2)
            if inside_poly(p, OUT) and not inside_poly(p, IN) and axis(p) < SPLIT:
                depth = min(1.0, (SPLIT - axis(p)) / (SPLIT + 0.78))
                level = 0.95 - 0.9 * depth ** 0.9
                if level > (BAYER[j % 4][i % 4] + 0.5) / 16:
                    out.append(f'<rect x="{x:.2f}" y="{y:.2f}" width="{size}" height="{size}"/>')
            x += cell
            i += 1
        y += cell
        j += 1
    return out


def path(poly):
    return "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in poly) + " Z"


def mark(fill="currentColor"):
    ring = path(clip(OUT, True)) + " " + path(clip(IN, True))
    dot = f'<rect x="{C - DOT}" y="{C - DOT}" width="{2 * DOT}" height="{2 * DOT}"/>'
    return (f'<g fill="{fill}"><path fill-rule="evenodd" d="{ring}"/>{dot}'
            + "".join(dots()) + "</g>")


# The macOS icon grid's continuous-corner tile (824 px on 1024), as lsuite's template draws it.
TILE = ("M383.41 100 L640.59 100 C722.19 100 763 100 799.79 112.14 L806.92 113.89 C854.88 131.34 "
        "892.66 169.12 910.11 217.08 C924 261 924 301.81 924 383.41 L924 640.59 C924 722.19 924 763 "
        "911.86 799.79 L910.11 806.92 C892.66 854.88 854.88 892.66 806.92 910.11 C763 924 722.19 924 "
        "640.59 924 L383.41 924 C301.81 924 261 924 224.21 911.86 L217.08 910.11 C169.12 892.66 "
        "131.34 854.88 113.89 806.92 C100 763 100 722.19 100 640.59 L100 383.41 C100 301.81 100 261 "
        "112.14 224.21 L113.89 217.08 C131.34 169.12 169.12 131.34 217.08 113.89 C261 100 301.81 100 "
        "383.41 100 Z")


def icon():
    # Dithered light in the top-left corner of the tile, as on the app's page.
    corner, cell = [], 16
    for j in range(26):
        for i in range(26):
            d = math.hypot(i / 26, j / 26) / 1.2
            level = max(0.0, 1 - d * 1.5) ** 1.4
            if level > (BAYER[j % 4][i % 4] + 0.5) / 16:
                corner.append(f'<rect x="{100 + i * cell}" y="{100 + j * cell}" '
                              f'width="{cell - 5}" height="{cell - 5}"/>')
    scale = 478 / (2 * OUTER)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <!-- ryolune app icon (lsuite v2): the macOS icon grid (824 px continuous-corner tile on 1024)
       in near black, a corner of dithered light like the app's page, and the mark
       (desktop/icons/mark.svg) in white at about 58 % of the tile. scripts/gen-mark.py writes
       this file; scripts/make-icon.sh renders every size from it. -->
  <defs>
    <path id="tile" d="{TILE}"/>
    <clipPath id="tile-clip"><use href="#tile"/></clipPath>
  </defs>
  <use href="#tile" fill="#0b0b0b"/>
  <g clip-path="url(#tile-clip)" fill="#fff" fill-opacity="0.16">{"".join(corner)}</g>
  <use href="#tile" fill="none" stroke="#fff" stroke-opacity="0.16" stroke-width="3"/>
  <g transform="translate(512 512) scale({scale:.4f}) translate({-C} {-C})">{mark("#fff")}</g>
</svg>'''


def write(path_, text):
    with open(path_, "w") as f:
        f.write(text + "\n")


if __name__ == "__main__":
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    write("desktop/assets/icons/mark.svg",
          '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">'
          + mark() + "</svg>")
    write("desktop/icons/mark.svg",
          '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">\n'
          "  <!-- ryolune's mark: the ring and the dot, cut square, the ring's shadow side\n"
          "       dissolving into ordered dither. One colour: ink on paper or paper on ink.\n"
          "       scripts/gen-mark.py writes it. -->\n  " + mark("#0a0a0a") + "\n</svg>")
    write("desktop/icons/ryolune.svg", icon())
