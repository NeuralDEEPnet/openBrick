#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Generate the NEURALDEEP.NET wordmark drawings in parts/assets/brand/ from
the outlines of the Inter typeface.

parts/assets/brand/NOTICE.md says what these files are and who owns the
wordmark. This is a development tool: nothing ships it, and the core never
reads a font. It turns the letters into single-colour, fill-only SVG paths in
millimetres (no <text>, CSS, strokes or images), one drawing per target tile,
so the core's hostile-SVG limits accept them and the validator measures them
like any other artwork (rule FEATURE.MIN, docs/plan/06).

Font: Inter 4.0 by Rasmus Andersson and the Inter Project Authors, SIL Open
Font License 1.1 (https://rsms.me/inter/, https://github.com/rsms/inter); the
Debian package fonts-inter 4.0+ds-1 installs the static OTF files under
/usr/share/fonts/opentype/inter/. The OFL covers the font, not artwork made
with it ("The requirement for fonts to remain under this license does not
apply to any document created using the fonts", OFL preamble; fonts/README.md).
The SHA-256 of every font file used is pinned below, so the drawings are
reproduced from exactly the outlines they were made from; --font-dir points
at another folder holding the same files.

Layout: each line is set in capitals with the font's advance widths plus a
small uniform letter-spacing (no kerning); a pair of letters that would come
closer than LETTER_GAP_MM anywhere is moved apart until it does not, so the
printer can resolve the gap. Lines are centred and spaced by a fraction of
the cap height. The outlines are then closed and opened at a radius just
over half the 0.8 mm minimum feature of process fdm-0.4
(profiles/process/fdm-0.4.json): every corner is rounded to that radius, and
a layout whose strokes or gaps are too narrow is refused instead of drawn
(see round_corners). The block is scaled to the face frame of its tile: the tile's
footprint (module and undersize from standards/dimensions-v1.json) inside the
top chamfer and the margin of parts/ndn.brand-tile.obpart.json, the frame the
relief fits the drawing into, so the drawing prints at 1:1 on that tile. On a
tile longer than wide the lines run along the length, read from the tile's
long side (rotated a quarter turn).

Requires fontTools 4.60.1 (MIT) and pyclipper 1.3.0.post6 (MIT), in a
virtual environment: pip install fonttools==4.60.1 pyclipper==1.3.0.post6. Run with that environment's Python, from anywhere:
    python -I scripts/brand-text.py            write the drawings
    python -I scripts/brand-text.py --check    exit 1 when a file differs
"""

import argparse
import hashlib
import json
import sys
from pathlib import Path

import pyclipper
from fontTools.pens.basePen import BasePen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "parts/assets/brand"
FONT_DIR = Path("/usr/share/fonts/opentype/inter")
FONTTOOLS_VERSION = "4.60.1"
PYCLIPPER_VERSION = "1.3.0.post6"

# Clipper works in integers: units per mm.
SCALE = 100_000
# Curves are flattened into chords no longer than this, in mm.
CHORD_MM = 0.12
# Round joins deviate from the true arc by at most this, in mm.
ARC_TOLERANCE_MM = 0.01
# The most a corner may round off, in squared radii: a 30-degree notch, such
# as where the diagonal of N meets a stem, rounds off about 2.4 r^2 (r^2
# (cot(a/2) - (pi - a)/2)); a stroke or slot narrower than 2 r loses more.
MAX_CORNER_R2 = 3.0
VERBOSE = False
# The narrowest gap between two letters, in mm: more than twice the corner
# radius, so closing the drawing never joins them.
LETTER_GAP_MM = 0.95
# The height of a profile band for the letter gaps, in font units (2048 per em).
BAND_UNITS = 4


# The font files the drawings are made from, with their SHA-256 (fonts-inter
# 4.0+ds-1, Debian; the same OTF files as Inter 4.0's release archive).
FONTS = {
    "InterDisplay-ExtraBold.otf": "7f233cb770bedfb0e4d1d5bada87dade7505513b020ca4b1ae2566fa358263b4",
}

# One drawing per tile: the file, the tile [width, length] in studs, the lines,
# the font, the letter-spacing in em, and the line gap in cap heights.
DRAWINGS = [
    {
        "file": "neuraldeep-wordmark-2x6.svg",
        "tile": [2, 6],
        "lines": ["NEURAL", "DEEP.NET"],
        "font": "InterDisplay-ExtraBold.otf",
        "tracking": 0.02,
        "gap": 0.3,
        "round": 0.425,
    },
    {
        "file": "neuraldeep-wordmark-2x8.svg",
        "tile": [2, 8],
        "lines": ["NEURALDEEP", ".NET"],
        "font": "InterDisplay-ExtraBold.otf",
        "tracking": 0.02,
        "gap": 0.3,
        "round": 0.425,
    },
    {
        "file": "neuraldeep-wordmark-2x10.svg",
        "tile": [2, 10],
        "lines": ["NEURALDEEP.NET"],
        "font": "InterDisplay-ExtraBold.otf",
        "tracking": 0.02,
        "gap": 0.3,
        "round": 0.425,
    },
    {
        "file": "neuraldeep-wordmark-4x4.svg",
        "tile": [4, 4],
        "lines": ["NEURAL", "DEEP", ".NET"],
        "font": "InterDisplay-ExtraBold.otf",
        "tracking": 0.02,
        "gap": 0.3,
        "round": 0.425,
    },
]


def dimension(dims, ident):
    """A nominal dimension in mm from standards/dimensions-v1.json."""
    for f in dims["features"]:
        if f["id"] == ident:
            return float(f["nominal_mm"])
    raise SystemExit(f"standards/dimensions-v1.json has no {ident}")


def face_frame(tile):
    """The frame, in mm, that the brand tile's relief fits a drawing into on
    `tile`: the footprint inside the top chamfer and the default margin."""
    dims = json.loads((REPO / "standards/dimensions-v1.json").read_text())
    doc = json.loads((REPO / "parts/ndn.brand-tile.obpart.json").read_text())
    pitch, gap = dimension(dims, "module"), dimension(dims, "undersize_per_side")
    chamfer = float(doc["features"][0]["round"]["radius"].removesuffix("mm"))
    margin = float(doc["params"]["margin"]["default"])
    inset = chamfer + margin
    return [round(n * pitch - 2 * gap - 2 * inset, 6) for n in tile], margin


def num(v):
    """A coordinate with at most three decimals and no trailing zeros."""
    s = f"{v:.3f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


class FlatPen(BasePen):
    """Contours as lists of points in mm, curves flattened into chords."""

    def __init__(self):
        super().__init__(None)
        self.contours = []

    def _moveTo(self, p):
        self.contours.append([p])

    def _lineTo(self, p):
        self.contours[-1].append(p)

    def _curveToOne(self, a, b, c):
        p0 = self.contours[-1][-1]
        length = sum(((q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2) ** 0.5 for p, q in [(p0, a), (a, b), (b, c)])
        n = max(2, int(length / CHORD_MM) + 1)
        for i in range(1, n + 1):
            t = i / n
            u = 1 - t
            self.contours[-1].append(
                tuple(u**3 * p0[j] + 3 * u * u * t * a[j] + 3 * u * t * t * b[j] + t**3 * c[j] for j in range(2))
            )

    def _closePath(self):
        pass

    _endPath = _closePath


def offset(paths, mm):
    po = pyclipper.PyclipperOffset(arc_tolerance=ARC_TOLERANCE_MM * SCALE)
    po.AddPaths(paths, pyclipper.JT_ROUND, pyclipper.ET_CLOSEDPOLYGON)
    return po.Execute(round(mm * SCALE))


def difference(a, b):
    """The pieces of a minus b, each as (area in mm2, a point of it in mm)."""
    pc = pyclipper.Pyclipper()
    pc.AddPaths(a, pyclipper.PT_SUBJECT, True)
    if b:
        pc.AddPaths(b, pyclipper.PT_CLIP, True)
    tree = pc.Execute2(pyclipper.CT_DIFFERENCE, pyclipper.PFT_NONZERO, pyclipper.PFT_NONZERO)
    pieces = []
    stack = list(tree.Childs)
    while stack:
        node = stack.pop()
        if not node.IsHole:
            area = pyclipper.Area(node.Contour) + sum(pyclipper.Area(h.Contour) for h in node.Childs)
            x, y = node.Contour[0]
            pieces.append((abs(area) / SCALE**2, (x / SCALE, y / SCALE)))
        for child in node.Childs:
            stack.extend(child.Childs)
    return pieces


def topology(paths):
    """(pieces, holes) of a union."""
    pieces = sum(1 for p in paths if pyclipper.Orientation(p))
    return pieces, len(paths) - pieces


def round_corners(contours, radius, name):
    """The union of the contours (non-zero winding), closed and then opened
    at `radius` mm: every corner, inside and outside, rounded to that radius,
    so the relief's own opening at minFeature (a radius of half of it) finds
    nothing to remove from the letters, nor, for deboss and inlay, from the
    face material between them (crates/openbrick-part/src/eval/relief.rs,
    `min_stroke` and `thin_face`).

    It refuses a layout whose strokes or gaps are narrower than twice the
    radius, where this would change the letters rather than their corners:
    two letters merged or a counter filled (the pieces and holes change), or
    a piece filled in or removed bigger than a corner rounds off
    (MAX_CORNER_R2 times radius squared)."""
    paths = [[(round(x * SCALE), round(y * SCALE)) for x, y in c] for c in contours if len(c) > 2]
    pc = pyclipper.Pyclipper()
    pc.AddPaths(paths, pyclipper.PT_SUBJECT, True)
    union = pc.Execute(pyclipper.CT_UNION, pyclipper.PFT_NONZERO, pyclipper.PFT_NONZERO)
    closed = offset(offset(union, radius), -radius)
    opened = offset(offset(closed, -radius), radius)
    if topology(closed) != topology(union) or topology(opened) != topology(union):
        raise SystemExit(
            f"{name}: letters merge or a counter closes at {num(2 * radius)} mm "
            f"(pieces and holes {topology(union)}, closed {topology(closed)}, opened {topology(opened)}): "
            "use larger letters or more letter-spacing"
        )
    limit = MAX_CORNER_R2 * radius * radius
    for what, pieces in (("gap", difference(closed, union)), ("stroke", difference(closed, opened))):
        if VERBOSE:
            print(f"  {name}: largest {what} piece {max((a for a, _ in pieces), default=0):.3f} mm2, limit {limit:.3f}")
        big = [(round(a, 3), (round(p[0], 2), round(p[1], 2))) for a, p in pieces if a > limit]
        if big:
            raise SystemExit(
                f"{name}: a {what} is narrower than {num(2 * radius)} mm (pieces in mm2 at mm: {big}): "
                "use larger letters, a bolder weight or more letter-spacing"
            )
    return [[(x / SCALE, y / SCALE) for x, y in p] for p in opened]


def path_data(polygons):
    """SVG path data: one closed subpath per polygon, absolute, three decimals,
    in a fixed order so the output is reproducible."""
    out = []
    for poly in sorted(polygons, key=lambda p: (min(q[1] for q in p), min(q[0] for q in p))):
        pts = []
        for x, y in poly:
            q = (num(x), num(y))
            if not pts or pts[-1] != q:
                pts.append(q)
        if len(pts) > 1 and pts[0] == pts[-1]:
            pts.pop()
        if len(pts) < 3:
            continue
        out.append("M" + "L".join(f"{x} {y}" for x, y in pts) + "Z")
    return "".join(out)


def check_font(path):
    want = FONTS[path.name]
    got = hashlib.sha256(path.read_bytes()).hexdigest()
    if got != want:
        raise SystemExit(f"{path}: SHA-256 {got}, the drawings are made from {want}")


def profiles(glyphs, name):
    """A glyph's left and right profiles: for each band of BAND_UNITS font
    units of height, the smallest and the largest x of its outline there
    (curves flattened), as {band: x}. None for a glyph with no outline."""
    pen = FlatPen()
    glyphs[name].draw(pen)
    left, right = {}, {}
    for c in pen.contours:
        for p, q in zip(c, c[1:] + c[:1]):
            n = max(1, int(abs(q[1] - p[1]) / BAND_UNITS) * 2 + 1)
            for i in range(n + 1):
                x = p[0] + (q[0] - p[0]) * i / n
                band = round((p[1] + (q[1] - p[1]) * i / n) / BAND_UNITS)
                left[band] = min(left.get(band, x), x)
                right[band] = max(right.get(band, x), x)
    return (left, right) if left else None


def shift_for_gap(prev, cur, gap):
    """The smallest shift of `cur`'s origin from `prev`'s that keeps every
    point of the two outlines at least `gap` font units apart (to within a
    band): for bands dy apart, the right profile of `prev` and the left
    profile of `cur` need sqrt(gap^2 - dy^2) between them."""
    need = -float("inf")
    reach = int(gap / BAND_UNITS) + 1
    _, right = prev
    left, _ = cur
    for bi, xr in right.items():
        for dj in range(-reach, reach + 1):
            xl = left.get(bi + dj)
            if xl is None:
                continue
            dy = dj * BAND_UNITS
            if abs(dy) >= gap:
                continue
            need = max(need, xr - xl + (gap * gap - dy * dy) ** 0.5)
    return need


def line_layout(font, text, tracking, min_gap=0.0):
    """The glyphs of a line as (glyph name, x offset) in font units, and the
    line's ink extent [x0, x1]. Letters are set at their advance widths plus
    `tracking` em; a pair closer than `min_gap` font units anywhere is moved
    apart until it is not (letter-spacing for the printer, not kerning)."""
    cmap = font.getBestCmap()
    hmtx = font["hmtx"]
    upem = font["head"].unitsPerEm
    glyphs = font.getGlyphSet()
    x, placed, ink = 0.0, [], [None, None]
    prev = None
    for ch in text:
        name = cmap[ord(ch)]
        prof = profiles(glyphs, name)
        if prev is not None and prof is not None and min_gap > 0:
            px, pprof = prev
            x = max(x, px + shift_for_gap(pprof, prof, min_gap))
        placed.append((name, x))
        bp = BoundsPen(glyphs)
        glyphs[name].draw(bp)
        if bp.bounds:
            x0, _, x1, _ = bp.bounds
            ink[0] = x + x0 if ink[0] is None else min(ink[0], x + x0)
            ink[1] = x + x1 if ink[1] is None else max(ink[1], x + x1)
        if prof is not None:
            prev = (x, prof)
        x += hmtx[name][0] + tracking * upem
    return placed, ink


def drawing(spec, font_dir):
    path = font_dir / spec["font"]
    check_font(path)
    font = TTFont(path)
    glyphs = font.getGlyphSet()
    cap = font["OS/2"].sCapHeight
    frame, margin = face_frame(spec["tile"])
    rotate = spec["tile"][0] < spec["tile"][1]
    # The frame in the text's own direction: along the lines, then across.
    along, across = (frame[1], frame[0]) if rotate else (frame[0], frame[1])
    n = len(lines_text := spec["lines"])
    height = cap * (n + (n - 1) * spec["gap"])
    # The scale sets the letter gap in font units, and the gaps set the
    # scale: a few rounds settle it.
    k = None
    for _ in range(8):
        min_gap = 0.0 if k is None else LETTER_GAP_MM / k
        lines = [line_layout(font, t, spec["tracking"], min_gap) for t in lines_text]
        width = max(ink[1] - ink[0] for _, ink in lines)
        k_next = min(along / width, across / height)
        if k is not None and abs(k_next - k) < 1e-9:
            break
        k = k_next
    lines = [line_layout(font, t, spec["tracking"], LETTER_GAP_MM / k) for t in lines_text]
    width = max(ink[1] - ink[0] for _, ink in lines)
    k = min(along / width, across / height)
    pen = FlatPen()
    for i, (placed, ink) in enumerate(lines):
        # Text coordinates in mm, y down: the line centred along, its
        # baseline at the bottom of its cap-height band.
        x0 = (along - (ink[1] - ink[0]) * k) / 2 - ink[0] * k
        top = (across - height * k) / 2
        base = top + (i * (1 + spec["gap"]) + 1) * cap * k
        for name, dx in placed:
            # Font units (y up) to text mm (y down).
            t = (k, 0, 0, -k, x0 + dx * k, base)
            if rotate:
                # A quarter turn: text x runs up the drawing (along its
                # length, read from the long side), text y runs right.
                a, b, c, d, e, f = t
                # (x, y) -> (y, along - x): compose with the text transform.
                t = (b, -a, d, -c, f, along - e)
            glyphs[name].draw(TransformPen(pen, t))
    w, h = frame
    d = path_data(round_corners(pen.contours, spec["round"], spec["file"]))
    tile = f"{spec['tile'][0]}x{spec['tile'][1]}"
    cap_mm = cap * k
    turned = (
        "\n  The lines run along the tile's length, read from its long side."
        if rotate
        else ""
    )
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!--
  Copyright Neural Deep Network Ltd. All rights reserved.
  The NEURALDEEP.NET wordmark: {" / ".join(spec["lines"])}.
  Target tile: {tile}. The {num(w)} x {num(h)} mm frame is the {tile} tile face inside
  a {num(margin)} mm margin, printed 1:1; capitals {num(cap_mm)} mm high.{turned}
  Letterforms: Inter ({path.stem}, version 4.0), Copyright 2016 The Inter
  Project Authors (https://github.com/rsms/inter), SIL Open Font License 1.1,
  set in capitals with {num(spec["tracking"])} em letter-spacing, no kerning,
  convex corners rounded to {num(spec["round"])} mm so the 0.8 mm opening keeps them.
  Generated by scripts/brand-text.py (fontTools {FONTTOOLS_VERSION}); do not edit by hand.
  Single colour, fills only (fill #000, fill-rule evenodd), units are mm.
  Not covered by the licences of this repository: see NOTICE.md in this
  folder (parts/assets/brand/NOTICE.md).
-->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {num(w)} {num(h)}" width="{num(w)}mm" height="{num(h)}mm">
<path fill="#000" fill-rule="evenodd" d="{d}"/>
</svg>
""", cap_mm


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--font-dir", type=Path, default=FONT_DIR)
    ap.add_argument("--check", action="store_true", help="exit 1 when a drawing differs from its file")
    args = ap.parse_args()
    differ = []
    for spec in DRAWINGS:
        text, cap_mm = drawing(spec, args.font_dir)
        out = OUT / spec["file"]
        if args.check:
            if not out.exists() or out.read_text() != text:
                differ.append(spec["file"])
        else:
            out.write_text(text)
            print(f"wrote {out.relative_to(REPO)} ({len(text)} bytes, capitals {cap_mm:.2f} mm)")
    if differ:
        print("out of date: " + ", ".join(differ), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
