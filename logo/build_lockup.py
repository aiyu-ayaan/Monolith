"""Builds the Monolith wordmark and lockups as outlined SVG paths.

Font: Inter Bold (SIL OFL 1.1, allows logo use). Run:
  python3 logo/build_lockup.py /path/to/Inter-Bold.otf
"""
import sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen

RED, INK, PAPER = "#E11D2E", "#111827", "#FFFFFF"
# Keystone M on a 256 grid, translated so its left edge sits at x=0.
MARK = "M8 24H48L78 136L108 24H148A8 8 0 0 1 156 32V224A8 8 0 0 1 148 232H8A8 8 0 0 1 0 224V32A8 8 0 0 1 8 24Z"
MARK_W, MARK_TOP, MARK_BOT = 156, 24, 232

font = TTFont(sys.argv[1])
gs, cmap, hmtx = font.getGlyphSet(), font.getBestCmap(), font["hmtx"]
TEXT, TRACK = "monolith", -0.02  # tracking as a share of the em
upm = font["head"].unitsPerEm

def word(scale, x0, baseline):
    pen = SVGPathPen(gs)
    x = 0.0
    for ch in TEXT:
        g = cmap[ord(ch)]
        gs[g].draw(TransformPen(pen, (scale, 0, 0, -scale, x0 + x * scale, baseline)))
        x += hmtx[g][0] + TRACK * upm
    return pen.getCommands(), (x - TRACK * upm) * scale

# Scale so the ascender of "l" is 62% of the mark height; centre it on the mark.
bp = BoundsPen(gs); gs[cmap[ord("l")]].draw(bp)
asc = bp.bounds[3]
scale = 0.62 * (MARK_BOT - MARK_TOP) / asc
baseline = (MARK_TOP + MARK_BOT) / 2 + asc * scale / 2
gap = round(MARK_W * 0.36)

def svg(w, h, body, title="Monolith"):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0f} {h}"><title>{title}</title>{body}</svg>\n'

d, w = word(scale, MARK_W + gap, baseline)
W = MARK_W + gap + w + 2
for name, mark, text in [("light", RED, INK), ("dark", RED, PAPER), ("black", "#000", "#000"), ("white", PAPER, PAPER)]:
    open(f"monolith-lockup-{name}.svg", "w").write(
        svg(W, 256, f'<path fill="{mark}" d="{MARK}"/><path fill="{text}" d="{d}"/>'))

# Wordmark alone, cropped to the type.
d, w = word(scale, 0, baseline - MARK_TOP + 8)
open("monolith-wordmark.svg", "w").write(svg(w + 2, round(256 - 2 * MARK_TOP + 16), f'<path fill="{INK}" d="{d}"/>'))
