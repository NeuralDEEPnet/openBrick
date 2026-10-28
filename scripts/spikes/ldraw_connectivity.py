#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Q04 spike: derive connectors for LDraw parts by scanning primitive references.

Question (docs/plan/13, Q04): how many common LDraw parts get usable connectors
(studs, tubes, underside pins, Technic pin holes, axle holes, pins, axles, bars,
clips, hinges) purely from the LDraw primitive files they reference?

Usage:
    python3 -I scripts/spikes/ldraw_connectivity.py <ldraw-dir> [options]

<ldraw-dir> is an extracted LDraw library (the folder that holds parts/ and p/,
or its parent). Python 3 standard library only. The script reads the library;
it never copies LDraw geometry into its outputs. Outputs:

    stdout                      headline numbers
    --csv PATH                  per-part results (default: q04_results.csv next
                                to this script; "-" to skip)
    --json PATH                 per-part connector lists (kind, gender, position
                                in LDU, direction, length) and counts per kind
    --explain ID [--heuristic]  print the derived connectors of one part and exit
    --library-stats             also derive every current part in parts/ (about
                                a minute) and print library-wide coverage and the
                                description probe (features named in the
                                description versus features derived)
    --bar-sample N              derive every current part with the bar-cylinder
                                heuristic, print a deterministic sample of N
                                parts with heuristic bars and the precision of
                                the hand-labelled sample, and exit
    --studf-holdout             score female stud synthesis on library parts
                                outside the part list whose description fixes
                                the count (plain W x L parts, Technic beams)
    --mapping                   print the primitive mapping table and exit
    --self-test                 check the pure logic (no library needed) and exit

Conventions: LDraw coordinates, LDU, right-handed, -Y is up. A type-1 line is
"1 colour x y z a b c d e f g h i file"; the child-to-parent transform is
p = [[a b c][d e f][g h i]] * v + (x y z).

Brick numbers (module, plate height, stud height and diameter, side wall, bar
diameter) come from standards/dimensions-v1.json (the ldraw_ldu column), never
from literals here. Primitive-local axes and extents (for example "peghole.dat
has its face at y=0 and goes into +y") were read from the LDraw primitives
themselves; the report docs/spikes/q04-ldraw-connectivity.md lists every mapping.

Female studs (studF, the receptacle a stud plugs into) are not a primitive. The
script synthesises them from the part's triangles (synth_studf: a stud-sized,
stud-deep empty cylinder under the part on the stud grid) and scores that
prototype separately and combined with the primitive scan.

Ground truth lives in this file: RULE derives expected counts from regular
descriptions ("Brick 2 x 4": 8 studs, 3 tubes) and HAND is the hand-checked
table for irregular parts, with a note per entry and UNVERIFIED where the
truth could not be established. STUDF holds the expected female stud counts.
"""

import argparse
import csv
import hashlib
import json
import math
import os
import re
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, os.pardir, os.pardir))
DEFAULT_PARTS = os.path.join(HERE, "q04_parts.txt")
DEFAULT_CSV = os.path.join(HERE, "q04_results.csv")
DEFAULT_STANDARD = os.path.join(REPO, "standards", "dimensions-v1.json")

KIND_ORDER = (
    "stud", "studF", "tube", "underpin", "pinhole", "axlehole", "pin", "axle", "bar",
    "clip", "hinge", "ball", "socket",
)
GENDER = {
    "stud": "M", "studF": "F", "tube": "F", "underpin": "F", "pinhole": "F",
    "axlehole": "F", "pin": "M", "axle": "M", "bar": "M", "clip": "F", "hinge": "N",
    "ball": "M", "socket": "F",
}
# How each kind is counted: "point" = one per instance, "module" = one per
# module of axial length (each module engages one hole or pin), "run" = one per
# continuous coaxial shaft, "cluster" = one per group of primitives within one
# module of each other (hinge halves).
COUNTING = {
    "stud": "point", "studF": "point", "tube": "point", "underpin": "point",
    "clip": "point", "ball": "point", "socket": "point", "pinhole": "module",
    "axlehole": "module", "pin": "module", "axle": "run", "bar": "run",
    "hinge": "cluster",
}
# Kinds whose emitted axis is not calibrated per primitive: the clip's bar axis
# and the hinge rotation axis are not the primitive's local -y (see the report).
UNCALIBRATED = ("clip", "hinge")
# Kinds with no meaningful lattice axis in this spike (point-like or uncalibrated).
AXIS_FREE = ("clip", "hinge", "ball", "socket")
HOLE_FAMILIES = ("technic_beam", "technic_brick")


# ---------------------------------------------------------------- standard

def load_standard(path):
    """Return the LDU values the script needs from the Dimensional Standard."""
    with open(path, encoding="utf-8") as fh:
        doc = json.load(fh)
    ldu = {f["id"]: f.get("ldraw_ldu") for f in doc["features"]}
    need = ("module", "plate_height", "brick_height", "bar_diameter", "stud_height",
            "stud_diameter", "side_wall")
    missing = [k for k in need if not isinstance(ldu.get(k), (int, float))]
    if missing:
        raise SystemExit(f"standard {path} lacks ldraw_ldu for {missing}")
    return {
        "module": float(ldu["module"]),
        "plate": float(ldu["plate_height"]),
        "brick": float(ldu["brick_height"]),
        "bar_radius": float(ldu["bar_diameter"]) / 2.0,
        "stud_height": float(ldu["stud_height"]),
        "stud_radius": float(ldu["stud_diameter"]) / 2.0,
        "side_wall": float(ldu["side_wall"]),
    }


# ---------------------------------------------------------------- vectors

def mmul(a, b):
    return tuple(
        sum(a[3 * r + k] * b[3 * k + c] for k in range(3))
        for r in range(3) for c in range(3)
    )


def mvec(m, v):
    return (
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    )


def add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def scale(a, s):
    return (a[0] * s, a[1] * s, a[2] * s)


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def norm(a):
    return math.sqrt(dot(a, a))


def unit(a):
    n = norm(a)
    return (0.0, 0.0, 0.0) if n < 1e-12 else scale(a, 1.0 / n)


def column(m, c):
    return (m[c], m[3 + c], m[6 + c])


IDENTITY = (1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
ORIGIN = (0.0, 0.0, 0.0)


# ---------------------------------------------------------------- library

class Library:
    """Case-insensitive index of parts/ and p/ (with s/, 48/ and 8/ below)."""

    def __init__(self, root):
        if not os.path.isdir(os.path.join(root, "parts")):
            nested = os.path.join(root, "ldraw")
            if os.path.isdir(os.path.join(nested, "parts")):
                root = nested
            else:
                raise SystemExit(f"{root}: no parts/ folder; pass the extracted LDraw library")
        self.root = root
        self.index = {}
        for sub_dir in ("parts", "p"):
            base = os.path.join(root, sub_dir)
            for dirpath, dirnames, files in os.walk(base):
                dirnames.sort()
                rel = os.path.relpath(dirpath, base)
                prefix = "" if rel == "." else rel.replace(os.sep, "/").lower() + "/"
                for fn in sorted(files):
                    if fn.lower().endswith(".dat"):
                        self.index[(sub_dir, prefix + fn.lower())] = os.path.join(dirpath, fn)
        self._files = {}
        self._faces = {}

    def locate(self, name):
        key = name.strip().replace("\\", "/").lower()
        for sub_dir in ("parts", "p"):  # LDraw search order
            path = self.index.get((sub_dir, key))
            if path:
                return sub_dir, key, path
        return None

    def load(self, path):
        cached = self._files.get(path)
        if cached is not None:
            return cached
        desc, org, entries = None, None, []
        invert_pending = False
        with open(path, encoding="utf-8", errors="replace") as fh:
            for raw in fh:
                tok = raw.split()
                if not tok:
                    continue
                if tok[0] == "0":
                    if desc is None:
                        desc = raw.strip()[1:].strip()
                    if len(tok) >= 3 and tok[1] == "BFC" and tok[2] == "INVERTNEXT":
                        invert_pending = True
                    elif len(tok) >= 2 and tok[1] == "!LDRAW_ORG":
                        org = " ".join(tok[2:])
                    continue
                if desc is None:
                    desc = ""
                if tok[0] == "1" and len(tok) >= 15:
                    try:
                        vals = [float(x) for x in tok[2:14]]
                    except ValueError:
                        invert_pending = False
                        continue
                    t = (vals[0], vals[1], vals[2])
                    m = tuple(vals[3:12])
                    entries.append((invert_pending, m, t, " ".join(tok[14:])))
                invert_pending = False
        result = (desc or "", org or "", entries)
        self._files[path] = result
        return result

    def faces(self, path):
        """Triangles (type 3, and type 4 split in two) of one file, as 9-tuples."""
        cached = self._faces.get(path)
        if cached is not None:
            return cached
        tris = []
        with open(path, encoding="utf-8", errors="replace") as fh:
            for raw in fh:
                tok = raw.split()
                if not tok or tok[0] not in ("3", "4"):
                    continue
                n = 3 if tok[0] == "3" else 4
                if len(tok) < 2 + 3 * n:
                    continue
                try:
                    v = [float(x) for x in tok[2:2 + 3 * n]]
                except ValueError:
                    continue
                tris.append(tuple(v[0:9]))
                if n == 4:
                    tris.append(tuple(v[0:3] + v[6:12]))
        self._faces[path] = tris
        return tris

    def latest_update(self):
        """Highest '!LDRAW_ORG ... UPDATE YYYY-RR' tag in parts/ and p/ headers."""
        best = ""
        pat = re.compile(r"UPDATE\s+(\d{4}-\d{2})")
        for path in self.index.values():
            with open(path, encoding="utf-8", errors="replace") as fh:
                for i, raw in enumerate(fh):
                    if i > 12:
                        break
                    if "!LDRAW_ORG" in raw:
                        hit = pat.search(raw)
                        if hit and hit.group(1) > best:
                            best = hit.group(1)
                        break
        return best


# ---------------------------------------------------------------- mapping
#
# Each mapped primitive stops the recursion and emits one raw connector.
#   form "point":   position = primitive origin, direction = transformed axis
#   form "segment": axial piece from origin + s * axis for s in extent
#   form "end":     one opening of a hole (or one end of a bar handle) that is
#                   paired with the opposite opening later; axis points outward
#                   (peghole) or towards the bar (phandle)
#   form "cluster": hinge pieces, grouped by distance
# Axes and extents are primitive-local and were read from the primitive files.

class Mod(float):
    """A length in modules (stud pitches); resolved with the standard's module LDU.

    LDraw names Technic primitives by module length ("Pin 1.0", "Beam Hole 0.5"),
    so these extents are written in modules, not in LDU literals."""


UP = (0.0, -1.0, 0.0)      # LDraw primitive "outward" for studs and tubes
DOWN = (0.0, 1.0, 0.0)
PLUS_Y = DOWN
PLUS_Z = (0.0, 0.0, 1.0)
PLUS_X = (1.0, 0.0, 0.0)

MAPPING_RULES = [
    # (regex on lower-case base name, kind, form, axis, extent, note)
    (r"stud(a|2|2a|6|6a|9|10|13|15|17|17a|26|el|h|hl|hr|p01|x|xa)?\.dat", "stud", "point", UP, None,
     "stud, open stud, truncated or patterned stud"),
    (r"stud2?-logo\d*\.dat", "stud", "point", UP, None, "stud with logo (alternate primitive)"),
    (r"stud4(a|h|o|od|oda|s|s2|f\d[nsw])?\.dat", "tube", "point", UP, None,
     "Stud Tube Open: 2-wide underside tube (and tube bores)"),
    (r"stud(12|16|16a|16od|18a|21a|22a|23|23d)\.dat", "tube", "point", UP, None,
     "split, slotted, square, snap and cross underside tubes"),
    (r"stud3a?\.dat", "underpin", "point", UP, None, "Stud Tube Solid: pin under 1-wide parts"),
    (r"peghole\d?\.dat", "pinhole", "end", UP, None, "Peg Hole End: one opening, paired later"),
    (r"(beamhole|connhole|connhol2)\.dat", "pinhole", "segment", PLUS_Y, (Mod(-0.5), Mod(0.5)),
     "full 1-module pin hole"),
    (r"beamhol2\.dat", "pinhole", "segment", PLUS_Y, (Mod(-0.25), Mod(0.25)),
     "full half-module pin hole"),
    # connhol3 is drawn from y=-10 to y=8: the closed side stops 2 LDU short.
    (r"connhol3\.dat", "pinhole", "segment", PLUS_Y, (Mod(-0.5), 8.0), "one-sided pin hole"),
    (r"(axlehole|axl2hole|axl3hole|axl4hole|axlehol4|axlehol5|axl2hol8|axl3hol8|axl5hol8)\.dat",
     "axlehole", "segment", PLUS_Y, (0.0, 1.0), "axle hole (unit length along +y, scaled)"),
    (r"(bush|bush0|bush0a)\.dat", "axlehole", "segment", PLUS_Z, (Mod(-0.5), Mod(0.5)),
     "Technic bush body: axle hole along z"),
    (r"(axle|axlehol8)\.dat", "axle", "segment", PLUS_Y, (0.0, 1.0),
     "axle shaft surface (unit length along +y, scaled)"),
    # axleend2 is drawn between y=0.5 and y=2.5 with its tip at the origin.
    (r"(axleend2|axleend2hole)\.dat", "axle", "segment", PLUS_Y, (0.0, 2.5), "bevelled axle end"),
    (r"axleend\.dat", "axle", "segment", PLUS_Y, (0.0, 0.5), "flat axle end"),
    (r"(connect|connect2|connect5|connect6|connect7|connect8|connect10)\.dat", "pin", "segment",
     UP, (0.0, Mod(1.0)), "Technic pin 1.0 segment"),
    (r"(connect3|connect4)\.dat", "pin", "segment", UP, (0.0, Mod(0.5)), "Technic pin 0.5 segment"),
    (r"confric(\d|1[0-2])?\.dat", "pin", "segment", UP, (0.0, Mod(1.0)),
     "Technic friction pin segment"),
    (r"clip\d+\.dat", "clip", "point", UP, None,
     "clip: position is the primitive origin; bar axis not calibrated"),
    (r"clh.*\.dat", "hinge", "cluster", UP, None,
     "click-lock hinge finger part; rotation axis not calibrated"),
    (r"(h1|h2|arm1|arm2|arm3)\.dat", "hinge", "cluster", UP, None,
     "hinge plate or arm fingers; rotation axis not calibrated"),
    (r"(phandle1|phandle2)\.dat", "bar", "end", PLUS_X, None, "bar handle attachment, paired"),
    (r"joint8ball\.dat", "ball", "point", UP, None, "Joint-8 ball"),
    (r"joint8socket\d\.dat", "socket", "point", UP, None, "Joint-8 socket"),
]
# confric3 is the only 2.0-long friction pin primitive.
EXTENT_OVERRIDE = {"confric3.dat": (0.0, Mod(2.0))}
# Bar handle attachments: the bar axis sits at local y=2; the attachment spans
# local x -3..3. phandle1 (one open side) starts the bar at x=3; phandle2 (two
# open sides) lets the bar pass through, so its anchor is the far face x=-3.
ANCHOR = {"phandle1.dat": (3.0, 2.0, 0.0), "phandle2.dat": (-3.0, 2.0, 0.0)}
# Open studs whose bore takes a bar ("Stud Open", inner cylinder radius 4 LDU in
# stud2, stud6 and stud17a). stud9 ("Stud Open with Small Holes") has a 2 LDU
# bore radius, half a bar, so it is not open in this sense.
OPEN_STUD = re.compile(r"^(stud2|stud2a|stud6|stud6a|stud17|stud17a|stud2-logo\d*)\.dat$")
_COMPILED = [(re.compile("^" + r[0] + "$"),) + r[1:] for r in MAPPING_RULES]

CYLINDER = re.compile(r"^(\d+)-(\d+)cyl[ico]\.dat$")


def lookup(name):
    """Return (kind, form, axis, extent, note, base) for a mapped primitive."""
    key = name.strip().replace("\\", "/").lower()
    folder, _, base = key.rpartition("/")
    if folder not in ("", "8"):
        return None
    for rx, kind, form, axis, extent, note in _COMPILED:
        if rx.match(base):
            return kind, form, axis, EXTENT_OVERRIDE.get(base, extent), note, base
    return None


def mapping_table():
    """Markdown table of the primitive-to-connector mapping (for the report)."""
    lines = ["| Primitive file (regex) | Kind | Gender | Form | Note |",
             "|---|---|---|---|---|"]
    for rx, kind, form, _axis, _extent, note in MAPPING_RULES:
        cell = rx.replace("|", "\\|")
        lines.append(f"| `{cell}` | {kind} | {GENDER[kind]} | {form} | {note} |")
    lines.append(f"| `{OPEN_STUD.pattern[1:-1].replace('|', chr(92) + '|')}` | stud, `open: true` "
                 "| M | point | open stud: the bore also takes a bar |")
    return "\n".join(lines)


# ---------------------------------------------------------------- derivation

class Deriver:
    def __init__(self, lib, dims, heuristics):
        self.lib = lib
        self.dims = dims
        self.heuristics = heuristics

    def raw(self, part_id):
        loc = self.lib.locate(part_id + ".dat")
        if loc is None:
            return None, [], [part_id + ".dat"]
        desc, _org, _ = self.lib.load(loc[2])
        raw, missing = [], []
        self._walk(loc[2], IDENTITY, ORIGIN, False, 0, raw, missing)
        return desc, raw, missing

    def _walk(self, path, m, t, parity, depth, raw, missing):
        if depth > 40:
            missing.append("recursion limit at " + os.path.basename(path))
            return
        _desc, _org, entries = self.lib.load(path)
        for inv, lm, lt, name in entries:
            cm = mmul(m, lm)
            ct = add(mvec(m, lt), t)
            # Facing parity: only BFC INVERTNEXT flips which side of a surface
            # is outside. A mirror matrix reverses winding, but BFC readers
            # compensate for a negative determinant, so it does not flip it.
            cpar = parity ^ inv
            spec = lookup(name)
            if spec is not None:
                raw.append(self._emit(spec, cm, ct))
                continue
            base = name.strip().replace("\\", "/").lower().rpartition("/")[2]
            cyl = CYLINDER.match(base)
            if cyl:
                if self.heuristics and not cpar:
                    bar = self._bar_candidate(cyl, cm, ct)
                    if bar is not None:
                        raw.append(bar)
                continue
            loc = self.lib.locate(name)
            if loc is None:
                missing.append(name)
                continue
            self._walk(loc[2], cm, ct, cpar, depth + 1, raw, missing)

    def _emit(self, spec, m, t):
        kind, form, axis, extent, _note, base = spec
        if extent is not None:
            extent = tuple(v * self.dims["module"] if isinstance(v, Mod) else v for v in extent)
        d = mvec(m, axis)
        pos = add(mvec(m, ANCHOR[base]), t) if base in ANCHOR else t
        item = {"kind": kind, "form": form, "src": base, "pos": pos, "dir": unit(d)}
        if kind == "stud" and OPEN_STUD.match(base):
            item["open"] = True
        if form == "segment":
            p0 = add(mvec(m, scale(axis, extent[0])), t)
            p1 = add(mvec(m, scale(axis, extent[1])), t)
            item["p0"], item["p1"] = p0, p1
            if norm(sub(p1, p0)) > 1e-9:
                item["dir"] = unit(sub(p1, p0))
        return item

    def _bar_candidate(self, cyl, m, t):
        """Heuristic (not name-based): an outward cylinder at bar radius."""
        num, den = int(cyl.group(1)), int(cyl.group(2))
        if den == 0 or num / den < 0.5:
            return None
        rx, rz = norm(column(m, 0)), norm(column(m, 2))
        r = self.dims["bar_radius"]
        tol = 0.3
        if abs(rx - r) > tol or abs(rz - r) > tol:
            return None
        p0 = t
        p1 = add(mvec(m, PLUS_Y), t)
        if norm(sub(p1, p0)) < 1e-6:
            return None
        return {"kind": "bar", "form": "segment", "src": "cylinder r=bar", "pos": t,
                "dir": unit(sub(p1, p0)), "p0": p0, "p1": p1, "heuristic": True}

    # ------------------------------------------------------ post-processing

    def connectors(self, raw):
        module = self.dims["module"]
        out = []
        by_kind = {}
        for item in raw:
            by_kind.setdefault(item["kind"], []).append(item)

        for kind, items in sorted(by_kind.items()):
            counting = COUNTING[kind]
            if counting == "point":
                for it in dedupe_points(items):
                    c = connector(kind, it["pos"], it["dir"], [it["src"]])
                    if it.get("open"):
                        c["open"] = True
                    out.append(c)
            elif counting == "cluster":
                for group in clusters(items, module):
                    pos = scale(add_all(g["pos"] for g in group), 1.0 / len(group))
                    out.append(connector(kind, pos, group[0]["dir"], sorted({g["src"] for g in group})))
            else:
                segments = [it for it in items if it["form"] == "segment"]
                ends = [it for it in items if it["form"] == "end"]
                if ends:
                    max_depth = 6 * module if kind == "bar" else 2 * module
                    segments += pair_ends(ends, max_depth, inward=(kind == "bar"),
                                          lone_length=module / 2.0)
                for run in merge_runs(segments):
                    c = connector(kind, run["center"], run["dir"], run["src"])
                    c["length"] = round(run["length"], 2)
                    if counting == "module":
                        c["modules"] = max(1, modules_of(run["length"], module))
                    if run.get("heuristic"):
                        c["heuristic"] = True
                    out.append(c)
        if self.heuristics:
            min_bar = self.dims["plate"]
            pins = [c for c in out if c["kind"] == "underpin"]
            out = [c for c in out if not (c.get("heuristic")
                                          and (c.get("length", 0) < min_bar
                                               or extends_underpin(c, pins)))]
        out.sort(key=lambda c: (KIND_ORDER.index(c["kind"]), c["pos"]))
        return out


def connector(kind, pos, d, src):
    return {
        "kind": kind,
        "gender": GENDER[kind],
        "pos": [round(x, 2) + 0.0 for x in pos],
        "dir": [round(x, 4) + 0.0 for x in d],
        "src": list(src),
    }


def extends_underpin(bar, pins, tol=1.0):
    """A bar-radius cylinder that continues an underside pin is part of the pin."""
    c, d, half = bar["pos"], bar["dir"], bar["length"] / 2.0
    ends = (add(c, scale(d, half)), add(c, scale(d, -half)))
    for p in pins:
        if abs(dot(d, p["dir"])) < 0.999:
            continue
        if any(norm(sub(e, p["pos"])) <= tol for e in ends):
            return True
    return False


def add_all(vectors):
    acc = ORIGIN
    for v in vectors:
        acc = add(acc, v)
    return acc


def dedupe_points(items, tol=0.5):
    """Same kind at the same place is one connector (for example a straight and
    a sloped tube primitive stacked at one tube position)."""
    kept = []
    for it in items:
        if any(norm(sub(it["pos"], k["pos"])) < tol for k in kept):
            continue
        kept.append(it)
    return kept


def clusters(items, radius):
    parent = list(range(len(items)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for i in range(len(items)):
        for j in range(i + 1, len(items)):
            if norm(sub(items[i]["pos"], items[j]["pos"])) <= radius:
                parent[find(i)] = find(j)
    groups = {}
    for i in range(len(items)):
        groups.setdefault(find(i), []).append(items[i])
    return list(groups.values())


def pair_ends(ends, max_depth, inward, lone_length):
    """Pair hole openings (or bar-handle attachments) that share an axis.

    peghole: axis points out of the hole, so the partner lies along -axis.
    phandle: axis points along the bar, so the partner lies along +axis.
    """
    cands = []
    for i, a in enumerate(ends):
        look = a["dir"] if inward else scale(a["dir"], -1.0)
        for j, b in enumerate(ends):
            if i == j or dot(a["dir"], b["dir"]) > -0.99:
                continue
            d = sub(b["pos"], a["pos"])
            along = dot(d, look)
            if along <= 0.05 or along > max_depth + 0.5:
                continue
            if norm(sub(d, scale(look, along))) > 0.5:
                continue
            cands.append((along, i, j))
    cands.sort()
    used, segs = set(), []
    for along, i, j in cands:
        if i in used or j in used:
            continue
        used.update((i, j))
        a, b = ends[i], ends[j]
        segs.append({"kind": a["kind"], "form": "segment", "src": a["src"], "p0": a["pos"],
                     "p1": b["pos"], "dir": unit(sub(b["pos"], a["pos"]))})
    for i, a in enumerate(ends):
        if i in used:
            continue
        look = a["dir"] if inward else scale(a["dir"], -1.0)
        segs.append({"kind": a["kind"], "form": "segment", "src": a["src"] + " (unpaired)",
                     "p0": a["pos"], "p1": add(a["pos"], scale(look, lone_length)), "dir": look})
    return segs


def merge_runs(segments, gap=1.0):
    """Union coaxial segments that overlap or touch; return runs."""
    n = len(segments)
    parent = list(range(n))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    def coaxial(a, b):
        if abs(dot(a["dir"], b["dir"])) < 0.999:
            return False
        for p in (b["p0"], b["p1"]):
            d = sub(p, a["p0"])
            if norm(sub(d, scale(a["dir"], dot(d, a["dir"])))) > 0.5:
                return False
        return True

    def interval(s, ref):
        x0 = dot(sub(s["p0"], ref["p0"]), ref["dir"])
        x1 = dot(sub(s["p1"], ref["p0"]), ref["dir"])
        return min(x0, x1), max(x0, x1)

    for i in range(n):
        for j in range(i + 1, n):
            a, b = segments[i], segments[j]
            if not coaxial(a, b):
                continue
            ia, ib = interval(a, a), interval(b, a)
            if max(ia[0], ib[0]) <= min(ia[1], ib[1]) + gap:
                parent[find(i)] = find(j)
    groups = {}
    for i in range(n):
        groups.setdefault(find(i), []).append(segments[i])
    runs = []
    for group in groups.values():
        ref = group[0]
        lo = min(interval(s, ref)[0] for s in group)
        hi = max(interval(s, ref)[1] for s in group)
        center = add(ref["p0"], scale(ref["dir"], (lo + hi) / 2.0))
        runs.append({
            "center": center,
            "dir": ref["dir"],
            "length": hi - lo,
            "src": sorted({s["src"] for s in group}),
            "heuristic": all(s.get("heuristic") for s in group),
        })
    return runs


def modules_of(length, module):
    """Whole modules in a length, rounding half up (a 10 LDU thin hole is 1)."""
    return math.floor(length / module + 0.5)


def counts(conns):
    c = {}
    for x in conns:
        k = x["kind"]
        c[k] = c.get(k, 0) + (x.get("modules", 1) if COUNTING[k] == "module" else 1)
    return c


# ---------------------------------------------------------------- geometry
#
# Used for two things the primitive scan cannot give: the part's bounding box
# (to check that derived connectors lie on the part) and the female stud
# prototype. Every file is descended into here, mapped primitives included.

def xform_tri(m, t, tri):
    out = []
    for i in (0, 3, 6):
        x, y, z = tri[i], tri[i + 1], tri[i + 2]
        out.append(m[0] * x + m[1] * y + m[2] * z + t[0])
        out.append(m[3] * x + m[4] * y + m[5] * z + t[1])
        out.append(m[6] * x + m[7] * y + m[8] * z + t[2])
    return tuple(out)


class Geometry:
    """Flattened triangles per file; sub-files are cached, top-level parts are not."""

    def __init__(self, lib):
        self.lib = lib
        self._flat = {}

    def triangles(self, part_id):
        loc = self.lib.locate(part_id + ".dat")
        return [] if loc is None else self._flatten(loc[2], 0, cache=False)

    def _flatten(self, path, depth, cache=True):
        cached = self._flat.get(path)
        if cached is not None:
            return cached
        out = list(self.lib.faces(path))
        if depth < 40:
            for _inv, lm, lt, name in self.lib.load(path)[2]:
                loc = self.lib.locate(name)
                if loc is None:
                    continue
                out.extend(xform_tri(lm, lt, tri) for tri in self._flatten(loc[2], depth + 1))
        if cache:
            self._flat[path] = out
        return out


def bbox_of(tris):
    if not tris:
        return None
    lo = [min(t[i] for t in tris for i in (k, k + 3, k + 6)) for k in range(3)]
    hi = [max(t[i] for t in tris for i in (k, k + 3, k + 6)) for k in range(3)]
    return tuple(lo), tuple(hi)


class Column:
    """Vertical-line queries against a triangle soup, bucketed on the xz plane."""

    def __init__(self, tris, cell):
        self.cell = cell
        self.buckets = {}
        for tri in tris:
            xs, ys, zs = tri[0::3], tri[1::3], tri[2::3]
            area = ((tri[3] - tri[0]) * (tri[8] - tri[2])
                    - (tri[6] - tri[0]) * (tri[5] - tri[2]))
            rec = (tri, area, min(xs), max(xs), min(zs), max(zs), max(ys))
            for bx in range(math.floor(rec[2] / cell), math.floor(rec[3] / cell) + 1):
                for bz in range(math.floor(rec[4] / cell), math.floor(rec[5] / cell) + 1):
                    self.buckets.setdefault((bx, bz), []).append(rec)

    def hits(self, x, z):
        """y of every non-vertical face crossed by the vertical line through (x, z)."""
        out = []
        key = (math.floor(x / self.cell), math.floor(z / self.cell))
        for tri, area, *_rest in self.buckets.get(key, ()):
            if abs(area) < 1e-9:
                continue
            x0, y0, z0, x1, y1, z1, x2, y2, z2 = tri
            l1 = ((x - x0) * (z2 - z0) - (x2 - x0) * (z - z0)) / area
            l2 = ((x1 - x0) * (z - z0) - (x - x0) * (z1 - z0)) / area
            l0 = 1.0 - l1 - l2
            if l0 >= -1e-9 and l1 >= -1e-9 and l2 >= -1e-9:
                out.append(l0 * y0 + l1 * y1 + l2 * y2)
        return out

    def lowest(self, cx, cz, half):
        """Largest y (lowest point, -Y is up) of any face touching the open square."""
        best = None
        x0, x1, z0, z1 = cx - half, cx + half, cz - half, cz + half
        for bx in range(math.floor(x0 / self.cell), math.floor(x1 / self.cell) + 1):
            for bz in range(math.floor(z0 / self.cell), math.floor(z1 / self.cell) + 1):
                for rec in self.buckets.get((bx, bz), ()):
                    if rec[3] <= x0 or rec[2] >= x1 or rec[5] <= z0 or rec[4] >= z1:
                        continue
                    if best is None or rec[6] > best:
                        best = rec[6]
        return best


def synth_studf(tris, dims):
    """Prototype: female studs under a part, from geometry alone.

    A cell on the stud grid is a female stud when a stud-sized cylinder can
    enter it from below to stud depth and something grips it there:
      - room: nine vertical lines (the centre and eight at stud radius minus
        1 LDU) cross no face within stud height above the cell's opening plane;
      - cover: at least one of them meets a face above that (the part is there);
      - grip: at least two of sixteen vertical lines 1.5 LDU outside the stud
        surface cross a face within stud height (a wall, tube or pin that
        touches the stud). Added after the first run, where the 1x4 arch got
        studs under its opening; 16 lines, because 8 missed the walls of the
        2x2 round parts. Both choices were made on this sample.
    The opening plane is the lowest point of the material in the cell's
    square. Both grid phases (on and between studs) are tried in x and z; the
    phase with most cells wins. Only downward-facing receptacles are searched.
    """
    module = dims["module"]
    half = module / 2.0
    depth = dims["stud_height"] - 0.5
    probe = dims["stud_radius"] - 1.0
    reach = dims["stud_radius"] + 1.5
    if not tris:
        return []
    col = Column(tris, half)
    (x_lo, _y_lo, z_lo), (x_hi, _y_hi, z_hi) = bbox_of(tris)
    rays = [(0.0, 0.0)] + [(probe * math.cos(k * math.pi / 4), probe * math.sin(k * math.pi / 4))
                           for k in range(8)]
    grips = [(reach * math.cos(k * math.pi / 8), reach * math.sin(k * math.pi / 8)) for k in range(16)]

    def touches(x, z, y_open):
        return any(y_open - depth < y <= y_open + 0.01 for y in col.hits(x, z))

    def lattice(phase, lo, hi):
        k = math.ceil((lo - 1.0 + half - phase) / module)
        out = []
        while phase + k * module + half <= hi + 1.0:
            out.append(phase + k * module)
            k += 1
        return out

    best = []
    for px in (0.0, half):
        for pz in (0.0, half):
            found = []
            for cx in lattice(px, x_lo, x_hi):
                for cz in lattice(pz, z_lo, z_hi):
                    y_open = col.lowest(cx, cz, half - 0.5)
                    if y_open is None:
                        continue
                    covered, blocked = False, False
                    for dx, dz in rays:
                        for y in col.hits(cx + dx, cz + dz):
                            if y > y_open + 0.01:
                                continue
                            if y > y_open - depth:
                                blocked = True
                                break
                            covered = True
                        if blocked:
                            break
                    if not covered or blocked:
                        continue
                    if sum(1 for dx, dz in grips if touches(cx + dx, cz + dz, y_open)) >= 2:
                        found.append((cx, y_open, cz))
            if len(found) > len(best):
                best = found
    return [connector("studF", p, DOWN, ["synthesised"]) for p in sorted(best)]


# ---------------------------------------------------------------- ground truth
#
# Regular families: expected counts come from the description line.
#   "Brick|Plate|Tile W x L [x H]": studs W*L (tiles 0); 1-wide: L-1 underside
#   pins; otherwise (W-1)*(L-1) underside tubes; "without Understud": none.
#   "Baseplate W x L": W*L studs, nothing underneath.
# Irregular parts: hand-checked table below. A kind set to None is not
# established; a part with UNVERIFIED is excluded from the verified set.
# Kinds not listed in an entry are expected to be absent (count 0).

RULE = re.compile(r"^(Brick|Plate|Tile|Baseplate)\s+(\d+)\s+x\s+(\d+)(?:\s+x\s+[\d.]+)?"
                  r"(?P<rest>|\s+with Groove|\s+without Groove|\s+without Understud"
                  r"|\s+with Square Corners)\s*$")

UNVERIFIED = "unverified"


def T(note="", dirs=None, axle_modules=None, **kinds):
    return {"kinds": kinds, "dirs": dirs or {}, "axle_modules": axle_modules, "note": note}


def U(note):
    return {"unverified": True, "note": note}


HAND = {
    # brick_modified
    "2357": U("corner brick: underside features not established independently"),
    "2877": T("1x2 brick body: 2 studs, 1 underside pin", stud=2, underpin=1),
    "30136": U("log brick: underside pin not established"),
    "3659": T("arch 1x4: 4 studs, open arch underneath, no pins", stud=4),
    # plate_modified
    "2420": U("corner plate: underside pins or tube not established"),
    "2450": U("cut-corner plate: stud and tube layout not established"),
    "3176": U("plate 3x2 with hole: layout not established"),
    "2817": U("plate 2x2 with holes: hole type and count not established"),
    # tile. 3069b ("Tile 1 x 2 with Groove") is scored by RULE like the other
    # 1xN tiles (1 underside pin); LDraw models none. Whether the physical part
    # has the pin is not settled here; the report gives the sensitivity.
    "2412b": U("grille tile 1x2: underside pin not established; LDraw models none"),
    "14719": U("corner tile: underside not established"),
    # slope
    "3040b": T("2 x 1 slope: 1 stud, 1 underside pin", stud=1, underpin=1),
    "3039": T("2 x 2 slope: 2 studs, 1 tube", stud=2, tube=1),
    "3037": T("2 x 4 slope: 4 studs, 3 tubes", stud=4, tube=3),
    "3665b": U("inverted slope: underside hollow pin geometry not established"),
    "3660b": U("inverted slope: underside not established"),
    "3298": U("3 x 2 slope: number of underside tubes not established"),
    "4286": U("3 x 1 slope: number of underside pins not established"),
    "4460b": U("75 degree slope: underside not established"),
    "3044b": U("double slope 2x1: underside pin not established"),
    "54200": T("1 x 1 cheese slope: no studs, plain 1x1 cavity", ),
    "3043": T("2 x 2 double slope: no studs, 1 tube", tube=1),
    "3747b": U("inverted 33 slope 3x2: stud and underside layout not established"),
    "15068": U("curved slope 2x2: underside not established"),
    "11477": U("curved slope 2x1: underside not established"),
    # technic_brick: holes = studs - 1, except 1x1 and the 1x2 with two holes
    "6541": T("1x1 with 1 hole", stud=1, pinhole=1),
    "3700": T("1x2 with 1 hole", stud=2, underpin=1, pinhole=1),
    "3701": T("1x4 with 3 holes", stud=4, underpin=3, pinhole=3),
    "3894": T("1x6 with 5 holes", stud=6, underpin=5, pinhole=5),
    "3702": T("1x8 with 7 holes", stud=8, underpin=7, pinhole=7),
    "2730": T("1x10 with 9 holes", stud=10, underpin=9, pinhole=9),
    "3703": T("1x16 with 15 holes", stud=16, underpin=15, pinhole=15),
    "32000": T("1x2 with 2 holes, no underside pin", stud=2, pinhole=2),
    "32064b": T("1x2 with 1 axle hole", stud=2, underpin=1, axlehole=1),
    "3709b": T("Technic plate 2x4: 8 studs, 3 tubes, 3 vertical pin holes", stud=8, tube=3, pinhole=3),
    "3738": T("Technic plate 2x8: 16 studs, 7 tubes, 7 vertical pin holes", stud=16, tube=7, pinhole=7),
    # technic_beam: holes = length; axle holes where the mould has them
    "18654": T("beam 1", pinhole=1),
    "43857": T("beam 2", pinhole=2),
    "32523": T("beam 3", pinhole=3),
    "32316": T("beam 5", pinhole=5),
    "32524": T("beam 7", pinhole=7),
    "40490": T("beam 9", pinhole=9),
    "32525": T("beam 11", pinhole=11),
    "32278": T("beam 15", pinhole=15),
    "32140": T("L 2x4: 4 pin holes, axle hole at the end of the short arm", pinhole=4, axlehole=1),
    "32526": T("L 3x5: 7 pin holes", pinhole=7),
    "32348": T("bent 4x4 (7 holes): axle holes at both ends", pinhole=5, axlehole=2),
    "6629": T("bent 4x6 (9 holes): axle holes at both ends", pinhole=7, axlehole=2),
    "32271": T("bent 3x7 (9 holes): axle holes at both ends", pinhole=7, axlehole=2),
    "60483": T("thick 1x2: one pin hole, one axle hole", pinhole=1, axlehole=1),
    "41677": T("thin 1x2: two axle holes", axlehole=2),
    "6632": T("thin 1x3: axle holes at the ends, pin hole in the middle", pinhole=1, axlehole=2),
    "32449": T("thin 1x4: axle holes at the ends, 2 pin holes", pinhole=2, axlehole=2),
    "32065": T("thin 1x7: 7 pin holes", pinhole=7),
    # technic_pin: pin counted per module of length
    "2780": T("2L friction pin", pin=2),
    "3673": T("2L pin", pin=2),
    "6558": T("3L friction pin", pin=3),
    "32054": U("3L pin with stop bush: whether the bush end has an axle hole is not established"),
    "43093": T("axle pin: 1 pin module, 1 axle module", pin=1, axle=1, axle_modules=1),
    "3749": T("axle pin: 1 pin module, 1 axle module", pin=1, axle=1, axle_modules=1),
    "4274": T("pin 1/2: 1 pin module and a hollow stud-sized end", pin=1, stud=1),
    "6628": T("towball pin", pin=1, ball=1),
    "87082": T("3L pin with perpendicular centre pin hole", pin=2, pinhole=1),
    "15100": T("pin with perpendicular pin hole", pin=1, pinhole=1),
    # technic_axle: one shaft, length in modules
    "3704": T("axle 2", axle=1, axle_modules=2),
    "4519": T("axle 3", axle=1, axle_modules=3),
    "3705": T("axle 4", axle=1, axle_modules=4),
    "32073": T("axle 5", axle=1, axle_modules=5),
    "3706": T("axle 6", axle=1, axle_modules=6),
    "3707": T("axle 8", axle=1, axle_modules=8),
    "3708": T("axle 12", axle=1, axle_modules=12),
    "6587": T("axle 3 with stud at one end", axle=1, stud=1),
    "87083": T("axle 4 with end stop", axle=1),
    "24316": T("axle 3 with end stop", axle=1),
    # technic_connector
    "3713": T("bush", axlehole=1),
    "32123a": T("half bush", axlehole=1),
    "6536": T("cross block 1x2: axle hole and pin hole", axlehole=1, pinhole=1),
    "59443": T("2L inline axle joiner: through axle hole", axlehole=2),
    "6538b": U("offset axle joiner: hole layout not established"),
    "32039": U("axle/bush connector: layout not established"),
    "32184": T("cross block 1x3 axle/pin/axle", axlehole=2, pinhole=1),
    "42003": T("cross block 1x3 axle/pin/pin", axlehole=1, pinhole=2),
    "32034": T("angle connector #2: axle hole per arm, centre pin hole", axlehole=2, pinhole=1),
    "32014": T("angle connector #6: axle hole per arm, centre pin hole", axlehole=2, pinhole=1),
    # technic_gear
    "3647": T("gear 8: centre axle hole", axlehole=1),
    "32270": T("gear 12 double bevel: centre axle hole", axlehole=1),
    "94925": T("gear 16 reinforced: centre axle hole", axlehole=1),
    # 3648b: first unverified because the holes around the centre were not
    # established. The description names the centre axle hole, so that much is
    # certain; the other holes stay unknown (None) and are not scored.
    "3648b": T("gear 24: the single centre axle hole named in the description; other holes "
               "not established", axlehole=1, pinhole=None),
    # bracket: horizontal plate studs up, vertical plate studs to the side, flat back
    "99781": T("1x2 - 1x2 down", stud=4, underpin=1, dirs={"stud": {"up": 2, "side": 2}}),
    "99780": T("1x2 - 1x2 up", stud=4, underpin=1, dirs={"stud": {"up": 2, "side": 2}}),
    "44728": T("1x2 - 2x2", stud=6, underpin=1, dirs={"stud": {"up": 2, "side": 4}}),
    "99207": T("1x2 - 2x2 up", stud=6, underpin=1, dirs={"stud": {"up": 2, "side": 4}}),
    # 3956: first written as 8 studs (4 up, 4 side). The LDraw model has a closed
    # top with a centre hole and no top studs; without a physical part the
    # disagreement cannot be settled here, so the part is not counted.
    "3956": U("2x2 - 2x2 up: top-face studs disputed (LDraw shows a holed top); not settled"),
    "2436a": T("1x2 - 1x4", stud=6, underpin=1, dirs={"stud": {"up": 2, "side": 4}}),
    "36840": T("1x1 - 1x1 up", stud=2, dirs={"stud": {"up": 1, "side": 1}}),
    "36841": T("1x1 - 1x1 down", stud=2, dirs={"stud": {"up": 1, "side": 1}}),
    "93274": T("1x2 - 2x4", stud=10, underpin=1, dirs={"stud": {"up": 2, "side": 8}}),
    # side_studs
    "87087": T("1x1, 1 side stud", stud=2, dirs={"stud": {"up": 1, "side": 1}}),
    "4070": T("1x1 headlight: recessed side stud", stud=2, dirs={"stud": {"up": 1, "side": 1}}),
    "30414": T("1x4, 4 side studs", stud=8, underpin=3, dirs={"stud": {"up": 4, "side": 4}}),
    "26604": T("1x1, 2 adjacent side studs", stud=3, dirs={"stud": {"up": 1, "side": 2}}),
    "47905": T("1x1, 2 opposite side studs", stud=3, dirs={"stud": {"up": 1, "side": 2}}),
    "4733": T("1x1, 4 side studs", stud=5, dirs={"stud": {"up": 1, "side": 4}}),
    "11211": T("1x2, 2 studs on one side", stud=4, underpin=1, dirs={"stud": {"up": 2, "side": 2}}),
    "32952": T("1x1x1.667, 2 side studs", stud=3, dirs={"stud": {"up": 1, "side": 2}}),
    # hinge
    "44301a": T("1x2 locking, single finger on end", stud=2, underpin=1, hinge=1),
    "44302a": T("1x2 locking, dual finger on end", stud=2, underpin=1, hinge=1),
    "60471": T("1x2 locking, dual finger on side", stud=2, underpin=1, hinge=1),
    "30383": T("1x2 locking, single finger on top in place of one stud", stud=1, underpin=1, hinge=1),
    "3937": T("hinge brick base: no studs; underside not established", hinge=1, underpin=None),
    "3938": T("hinge brick top: 2 studs; underside not established", stud=2, hinge=1, underpin=None),
    "2429": T("hinge plate 1x4 base: 2 studs and the pivot; underside not established",
              stud=2, hinge=1, underpin=None),
    "2430": T("hinge plate 1x4 top: 2 studs and the pivot; underside not established",
              stud=2, hinge=1, underpin=None),
    "4275b": T("1x2 with 3 fingers", stud=2, underpin=1, hinge=1),
    "4276b": T("1x2 with 2 fingers", stud=2, underpin=1, hinge=1),
    # bar_clip
    "4085c": T("1x1 with vertical clip", stud=1, clip=1),
    "60897": T("1x1 with vertical clip", stud=1, clip=1),
    "15712": T("tile 1x1 with clip", clip=1),
    "61252": T("1x1 with horizontal clip", stud=1, clip=1),
    "63868": T("1x2 with clip on end", stud=2, underpin=1, clip=1),
    "60470a": T("1x2 with 2 clips", stud=2, underpin=1, clip=2),
    "48336": T("1x2 with bar handle on side", stud=2, underpin=1, bar=1),
    "2540": T("1x2 with bar handle", stud=2, underpin=1, bar=1),
    "87994": T("bar 3L", bar=1),
    # 63965a: first written as bar=1; corrected to 2 because bars count one per
    # continuous shaft and the thick stop splits this bar into a 96 LDU and a
    # 16 LDU section (6 modules = 120 LDU = 96 + 8 (stop) + 16).
    "63965a": T("bar 6L with thick stop: two bar sections either side of the stop", bar=2),
    # 4081b: first unverified. The ring's bore (radius 4 LDU in the LDraw model,
    # a bar) holds a bar, which in D06 is a clipF receptacle; that much is
    # certain and no primitive gives it. Whether the two ring ends (drawn with
    # open-stud primitives) also count as studs is not settled, so stud is None.
    "4081b": T("clip light: ring bore holds a bar (clip); ring ends as studs not settled",
               stud=None, clip=1),
    "2921": T("brick 1x1 with handle", stud=1, bar=1),
    # round
    "6141": T("1x1 round plate; underside form not established", stud=1, tube=None),
    "85861": T("1x1 round plate open stud; underside form not established", stud=1, tube=None),
    "3062b": T("1x1 round brick; underside form not established", stud=1, tube=None),
    "4032a": T("2x2 round plate: 4 studs, centre tube with axle hole", stud=4, tube=1, axlehole=1),
    "3941": T("2x2 round brick: 4 studs, centre tube with axle hole", stud=4, tube=1, axlehole=1),
    "98138": T("1x1 round tile; underside form not established", tube=None),
    "14769": T("2x2 round tile: centre underside stud between 4 studs", tube=1),
    "4150": T("2x2 round tile: cross underside stud between 4 studs", tube=1),
    "4589": U("cone 1x1: whether the top hole is a usable axle hole is not established"),
    "3942b": T("cone 2x2x2: hollow stud with axle hole; underside not established",
               stud=1, axlehole=1, tube=None),
    "60474": U("round plate 4x4 with hole and snap stud: layout not established"),
    # wedge
    "41769a": U("wing 2x4: stud and tube layout not established"),
    "41770a": U("wing 2x4: stud and tube layout not established"),
    "43722a": U("wing 2x3: stud and underside layout not established"),
    "43723a": U("wing 2x3: stud and underside layout not established"),
    "51739": U("wing 2x4: stud and tube layout not established"),
    "41747": U("wedge 2x6: stud and tube layout not established"),
    # jumper
    "3794b": T("jumper 1x2", stud=1, underpin=1),
    "15573": T("jumper 1x2 without understud", stud=1),
    "87580": T("jumper 2x2", stud=1, tube=1),
    "34103": T("1x3 with 2 offset studs; underside not established", stud=2, underpin=None),
    "4590": U("1x2 with 2 offset hollow studs: layout not established"),
}

# Female studs (studF): downward receptacles that take one stud each. Rule
# parts: W*L (baseplates 0, their underside is flat). These families have no
# stud receptacle: 0. Otherwise the table below (the underside of the part's
# W x L plate or brick body, written from the description; brackets count the
# horizontal plate only). Parts missing here are not established and are not
# scored for studF: inverted and curved slopes, wedges and wings, cut-corner
# plates and plates with holes, the old hinge tops and swivel plates, 3956,
# 60474, 4590.
STUDF_ZERO_FAMILIES = ("technic_beam", "technic_pin", "technic_axle", "technic_connector",
                       "technic_gear")
STUDF = {
    # brick_modified (the 1x4 arch takes studs under its two legs only)
    "2357": 3, "2877": 2, "30136": 2, "3659": 2,
    # plate_modified
    "2420": 3,
    # tile
    "2412b": 2, "14719": 3,
    # slope
    # 15068 and 11477 (curved) were first written as 4 and 2. The LDraw underside
    # of 15068 is stepped (opening plane y=0 under one half, y=-8 under the
    # other), which contradicts the flat underside assumed, so both curved
    # slopes are now not established.
    "3040b": 2, "3039": 4, "3037": 8, "3298": 6, "4286": 3, "4460b": 2, "3044b": 2,
    "54200": 1, "3043": 4,
    # technic_brick
    "6541": 1, "3700": 2, "3701": 4, "3894": 6, "3702": 8, "2730": 10, "3703": 16,
    "32000": 2, "32064b": 2, "3709b": 8, "3738": 16,
    # bracket: the horizontal plate's underside
    "99781": 2, "99780": 2, "44728": 2, "99207": 2, "2436a": 2, "36840": 1, "36841": 1,
    "93274": 2,
    # side_studs
    "87087": 1, "4070": 1, "30414": 4, "26604": 1, "47905": 1, "4733": 1, "11211": 2,
    "32952": 1,
    # hinge
    "44301a": 2, "44302a": 2, "60471": 2, "30383": 2, "3937": 2, "4275b": 2, "4276b": 2,
    # bar_clip
    "4085c": 1, "60897": 1, "15712": 1, "61252": 1, "63868": 2, "60470a": 2, "48336": 2,
    "2540": 2, "87994": 0, "63965a": 0, "4081b": 1, "2921": 1,
    # round
    "6141": 1, "85861": 1, "3062b": 1, "4032a": 4, "3941": 4, "98138": 1, "14769": 4,
    "4150": 4, "4589": 1, "3942b": 4,
    # jumper
    "3794b": 2, "15573": 2, "87580": 4, "34103": 3,
}


def rule_truth(desc):
    hit = RULE.match(desc)
    if not hit:
        return None
    fam, w, n = hit.group(1), int(hit.group(2)), int(hit.group(3))
    rest = hit.group("rest").strip()
    kinds = {"stud": 0 if fam == "Tile" else w * n}
    under = fam != "Baseplate" and rest != "without Understud"
    if under and (w == 1 or n == 1):
        if max(w, n) > 1:
            kinds["underpin"] = max(w, n) - 1
    elif under:
        kinds["tube"] = (w - 1) * (n - 1)
    return {"kinds": kinds, "dirs": {}, "axle_modules": None, "grid": (w, n, fam),
            "note": f"rule from description ({fam} {w} x {n})"}


def studf_expected(pid, family, desc):
    """Expected female stud count, or None when not established."""
    rule = rule_truth(desc or "")
    if rule is not None:
        w, n, fam = rule["grid"]
        return 0 if fam == "Baseplate" else w * n
    if family in STUDF_ZERO_FAMILIES:
        return 0
    return STUDF.get(pid)


def grid(count, pitch):
    return [round((i - (count - 1) / 2.0) * pitch, 1) for i in range(count)]


def expected_grids(w, n, pitch):
    """Stud, tube and underpin xz sets for a W x N part, both orientations."""
    out = []
    for a, b in ((w, n), (n, w)):
        studs = {(x, z) for x in grid(b, pitch) for z in grid(a, pitch)}
        if a == 1 or b == 1:
            if b >= a:
                under = {(x, 0.0) for x in grid(b - 1, pitch)} if b > 1 else set()
            else:
                under = {(0.0, z) for z in grid(a - 1, pitch)} if a > 1 else set()
        else:
            under = {(x, z) for x in grid(b - 1, pitch) for z in grid(a - 1, pitch)}
        out.append((studs, under))
    return out


def dir_class(d):
    if d[1] < -0.99:
        return "up"
    if d[1] > 0.99:
        return "down"
    if abs(d[1]) < 0.01:
        return "side"
    return "other"


def on_lattice(d):
    """True when a direction is one of the six axis directions (Rot24 images of +y)."""
    return max(abs(d[0]), abs(d[1]), abs(d[2])) > 0.999


def connector_points(c):
    """Position, plus both ends for connectors with an axial length."""
    pts = [tuple(c["pos"])]
    if c.get("length"):
        half = c["length"] / 2.0
        pts.append(add(c["pos"], scale(c["dir"], half)))
        pts.append(add(c["pos"], scale(c["dir"], -half)))
    return pts


def geometry_problems(conns, bbox, family, module, offlattice=False):
    """Position and axis checks that do not depend on the expected counts."""
    problems = []
    if bbox is not None:
        lo, hi = bbox
        tol = 1.0
        for c in conns:
            if any(p[i] < lo[i] - tol or p[i] > hi[i] + tol
                   for p in connector_points(c) for i in range(3)):
                problems.append(f"{c['kind']} at {c['pos']} outside the part's bounding box")
                break
    if not offlattice:
        for c in conns:
            if c["kind"] not in AXIS_FREE and not on_lattice(c["dir"]):
                problems.append(f"{c['kind']} axis {c['dir']} not a lattice direction")
                break
    if family in HOLE_FAMILIES:
        holes = [c for c in conns if c["kind"] in ("pinhole", "axlehole")]
        if len(holes) >= 2:
            if any(abs(dot(holes[0]["dir"], h["dir"])) < 0.999 for h in holes):
                problems.append("hole axes not parallel")
            for h in holes:
                gap = min(norm(sub(h["pos"], o["pos"])) for o in holes if o is not h)
                if abs(gap - module) > 0.5:
                    problems.append(f"hole at {h['pos']}: nearest hole {gap:.1f} LDU away, "
                                    f"expected one module")
                    break
    return problems


def evaluate(truth, conns, dims, bbox=None, family=None, strict_axes=True):
    """Return (verdict, checks) for one part."""
    if truth is None or truth.get("unverified"):
        return UNVERIFIED, ["no ground truth"]
    derived = counts(conns)
    exp = truth["kinds"]
    unknown = {k for k, v in exp.items() if v is None}
    kinds = (set(exp) | set(derived)) - unknown
    matched = exp_total = der_total = 0
    problems = []
    for k in sorted(kinds, key=KIND_ORDER.index):
        e, d = exp.get(k, 0), derived.get(k, 0)
        matched += min(e, d)
        exp_total += e
        der_total += d
        if d < e:
            problems.append(f"missing {k} {d}/{e}")
        elif d > e:
            problems.append(f"extra {k} {d}/{e}")
    # extra checks (only meaningful when counts agree)
    module = dims["module"]
    if "grid" in truth and not problems:
        w, n, _fam = truth["grid"]
        studs = {(round(c["pos"][0], 1), round(c["pos"][2], 1)) for c in conns if c["kind"] == "stud"}
        unders = {(round(c["pos"][0], 1), round(c["pos"][2], 1))
                  for c in conns if c["kind"] in ("tube", "underpin")}
        fem = {(round(c["pos"][0], 1), round(c["pos"][2], 1)) for c in conns if c["kind"] == "studF"}
        ok = any((not studs or studs == s) and (not unders or unders == u) and (not fem or fem == s)
                 for s, u in expected_grids(w, n, module))
        if not ok:
            problems.append("positions off the expected grid")
        if any(dir_class(c["dir"]) != "up" for c in conns if c["kind"] == "stud"):
            problems.append("stud not pointing up")
        if any(dir_class(c["dir"]) != "down" for c in conns
               if c["kind"] in ("tube", "underpin", "studF")):
            problems.append("underside connector not pointing down")
    for k, want in truth.get("dirs", {}).items():
        got = {}
        for c in conns:
            if c["kind"] == k:
                cls = dir_class(c["dir"])
                got[cls] = got.get(cls, 0) + 1
        if got != want:
            problems.append(f"{k} directions {fmt_counts(got)} expected {fmt_counts(want)}")
    if truth.get("axle_modules") is not None:
        lengths = [c["length"] for c in conns if c["kind"] == "axle"]
        if len(lengths) == 1:
            got = modules_of(lengths[0], module)
            if got != truth["axle_modules"]:
                problems.append(f"axle length {lengths[0]} LDU, expected {truth['axle_modules']} modules")
    problems += geometry_problems(conns, bbox, family, module, truth.get("offlattice", False))
    if strict_axes:
        loose = sorted({c["kind"] for c in conns if c["kind"] in UNCALIBRATED})
        if loose:
            problems.append(f"{'/'.join(loose)} axis not calibrated (position is the primitive origin)")
    if not problems:
        if unknown:
            return UNVERIFIED, ["known kinds match; " + ", ".join(sorted(unknown)) + " not established"]
        return "full", []
    if exp_total == 0 and der_total == 0:
        return "partial", problems
    if exp_total == 0:
        return "wrong", problems
    recall = matched / exp_total
    precision = matched / der_total if der_total else 0.0
    if recall >= 0.5 and precision >= 0.5:
        return "partial", problems
    return "wrong", problems


def with_studf(truth, expected):
    """A copy of a truth with the female stud expectation added."""
    if truth is None or truth.get("unverified"):
        return truth
    out = dict(truth)
    out["kinds"] = dict(truth["kinds"], studF=expected)
    return out


def known_kinds(truth, studf):
    """Expected count per kind where the truth is established (for the per-kind table)."""
    out = {}
    if truth is not None and not truth.get("unverified"):
        out.update({k: v for k, v in truth["kinds"].items() if v is not None})
    if studf is not None:
        out["studF"] = studf
    return out


def studf_only(truth, expected):
    if expected is None:
        return None
    out = {"kinds": {"studF": expected}, "dirs": {}, "axle_modules": None}
    if truth is not None and "grid" in truth:
        out["grid"] = truth["grid"]
    return out


def fmt_counts(c):
    keys = sorted(c, key=lambda k: KIND_ORDER.index(k) if k in KIND_ORDER else 99)
    return ";".join(f"{k}={c[k]}" for k in keys) if c else "none"


def fmt_truth(truth):
    if truth is None or truth.get("unverified"):
        return "unverified"
    parts = []
    for k in sorted(truth["kinds"], key=KIND_ORDER.index):
        v = truth["kinds"][k]
        if v == 0 and k != "stud":
            continue
        parts.append(f"{k}={'?' if v is None else v}")
    return ";".join(parts) if parts else "none"


# ---------------------------------------------------------------- main

def read_parts(path):
    parts = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.split("#", 1)[0].strip()
            if line:
                pid, fam = line.split()[:2]
                parts.append((pid, fam))
    return parts


def summarise(rows, key):
    total = len(rows)
    verified = [r for r in rows if r[key] != UNVERIFIED]
    out = {"parts": total, "verified": len(verified)}
    for v in ("full", "partial", "wrong", UNVERIFIED):
        out[v] = sum(1 for r in rows if r[key] == v)
    nv = len(verified) or 1
    out["pct_verified"] = {
        "full": 100.0 * out["full"] / nv,
        "at_least_partial": 100.0 * (out["full"] + out["partial"]) / nv,
        "wrong": 100.0 * out["wrong"] / nv,
    }
    out["pct_all"] = {
        "full": 100.0 * out["full"] / total,
        "at_least_partial": 100.0 * (out["full"] + out["partial"]) / total,
        "wrong": 100.0 * out["wrong"] / total,
        "unverified": 100.0 * out[UNVERIFIED] / total,
    }
    return out


def print_summary(label, s):
    pv, pa = s["pct_verified"], s["pct_all"]
    print(f"[{label}] full {s['full']}, partial {s['partial']}, wrong {s['wrong']}, "
          f"unverified {s[UNVERIFIED]}")
    print(f"  over verified ({s['verified']}): fully correct {pv['full']:.1f}%, "
          f"at least partially correct {pv['at_least_partial']:.1f}%, wrong {pv['wrong']:.1f}%")
    print(f"  over all {s['parts']}: fully correct {pa['full']:.1f}%, "
          f"at least partially correct {pa['at_least_partial']:.1f}%, wrong {pa['wrong']:.1f}%, "
          f"unverified {pa['unverified']:.1f}%")


def per_kind(rows):
    """Expected, derived and matched connector counts per kind over verified parts."""
    acc = {}
    for r in rows:
        derived = r["_counts"]
        for k, e in r["_known"].items():
            d = derived.get(k, 0)
            a = acc.setdefault(k, [0, 0, 0, 0])
            a[0] += 1 if e else 0
            a[1] += e
            a[2] += d
            a[3] += min(e, d)
    print("per kind over parts with that kind established: kind parts-expecting expected derived matched")
    for k in KIND_ORDER:
        if k in acc:
            a = acc[k]
            print(f"  {k:9s} {a[0]:4d} {a[1]:6d} {a[2]:6d} {a[3]:6d}")


# Features named in a part's description and the kinds that satisfy them. A
# blind check of recall over the whole library: the description is written by
# LDraw authors, not by this spike. Noisy by construction (see the report).
DESC_FEATURES = [
    ("axle hole", r"axle ?holes?", ("axlehole",)),
    ("pin hole", r"(?:pin|peg) ?holes?", ("pinhole",)),
    ("clip", r"\bclips?\b", ("clip",)),
    ("bar or bar handle", r"\bbar\b(?! holder)|\bhandle\b", ("bar",)),
    ("hinge or fingers", r"\bhinge\b|\bfingers?\b", ("hinge",)),
    ("towball", r"towball", ("ball",)),
    ("Technic gear wheel (any hole)", r"^technic gear\s+\d+ tooth", ("axlehole", "pinhole")),
]


def current_parts(lib):
    for (sub_dir, key), path in sorted(lib.index.items()):
        if sub_dir != "parts" or "/" in key:
            continue
        desc, _org, _e = lib.load(path)
        if desc.startswith(("~", "=", "_")) or "(Obsolete)" in desc:
            continue
        yield key[:-4], desc


def library_stats(lib, deriver):
    """Coverage over every current part in parts/ (no ground truth)."""
    total = with_any = 0
    per_kind_parts, empty_words = {}, {}
    probe = {label: [0, 0, []] for label, _rx, _k in DESC_FEATURES}
    compiled = [(label, re.compile(rx, re.IGNORECASE), want) for label, rx, want in DESC_FEATURES]
    for pid, desc in current_parts(lib):
        total += 1
        _d, raw, _m = deriver.raw(pid)
        c = counts(deriver.connectors(raw))
        if c:
            with_any += 1
        else:
            word = (desc.split() or ["?"])[0]
            empty_words[word] = empty_words.get(word, 0) + 1
        for k in c:
            per_kind_parts[k] = per_kind_parts.get(k, 0) + 1
        for label, rx, want in compiled:
            hit = rx.search(desc)
            if not hit or re.search(r"\bwithout\b", desc[:hit.start()], re.IGNORECASE):
                continue
            p = probe[label]
            p[0] += 1
            if any(c.get(k) for k in want):
                p[1] += 1
            elif len(p[2]) < 8:
                p[2].append(pid)
    print(f"library-wide: {total} current part files, {with_any} "
          f"({100.0 * with_any / max(total, 1):.1f}%) get at least one connector from primitive names")
    print("  parts with at least one connector of kind: " +
          ", ".join(f"{k} {per_kind_parts[k]}" for k in KIND_ORDER if k in per_kind_parts))
    top = sorted(empty_words.items(), key=lambda kv: (-kv[1], kv[0]))[:15]
    print("  parts with no connector, by first word of the description: " +
          ", ".join(f"{w} {n}" for w, n in top))
    print("description probe (feature named in the description -> at least one such connector "
          "derived): feature named derived recall examples-missed")
    for label, _rx, _want in DESC_FEATURES:
        named, found, missed = probe[label]
        pct = 100.0 * found / named if named else 0.0
        print(f"  {label:30s} {named:5d} {found:5d} {pct:5.1f}%  {' '.join(missed)}")


# Bar-cylinder heuristic precision: a deterministic sample of library parts in
# which the heuristic finds a bar (the first 50 by SHA-256 of the part id, 2026-10
# library), labelled by hand. True = every heuristic bar in the part is a real
# bar (a bar-diameter shaft a clip or hand can grip); False = at least one is
# not; None = not decidable from the description and the printed bar positions.
# Labels were written before the precision was computed, from the description
# and positions only (no physical parts), so they are judgement, not truth.
BAR_SAMPLE_LABELS = {
    "90395": (None, "float: a strap handle may be a bar; three cylinders found"),
    "u9310c06": (False, "door hinge pins inside a complete assembly"),
    "92820p06": (False, "figure hip pins"),
    "11408": (False, "figure neck post"),
    "36198p03": (False, "figure hip pins"),
    "3899p05": (None, "cup handle shape not decidable"),
    "60800b": (None, "shutter hinge pins: bar-sized pins into frame holes"),
    "18394p03": (True, "bar named in the description"),
    "66967": (None, "rotor pins at the arm tips"),
    "98367p01": (True, "shield grip"),
    "817c03": (None, "trailer hitch"),
    "18031": (True, "sword grip"),
    "36083c": (True, "bar named in the description"),
    "2586p4h": (True, "shield grip"),
    "u9230c02": (False, "mechanism pins inside a complete assembly"),
    "97787": (True, "spatula handle"),
    "6572": (False, "towball neck"),
    "98567-f1": (None, "belt end pieces"),
    "47511p01": (False, "figure shoulder axles"),
    "75174p01c01": (False, "joint pins inside a complete assembly"),
    "35032e": (True, "bar named in the description"),
    "42114c02": (True, "hilt and bar-diameter blade"),
    "92456p0h": (False, "figure neck post"),
    "57539": (True, "flexible hose tips are bar-diameter"),
    "3957b": (True, "antenna whip"),
    "636": (None, "axle ends that sit in bearing holes"),
    "6053c01": (False, "rudder pivot pins inside a complete assembly"),
    "447c01": (False, "train wheel stub axle"),
    "20312": (True, "handle bars named in the description"),
    "75902p06": (True, "shield grip"),
    "245c01": (False, "train wheel stub axle"),
    "3899p07": (None, "cup handle shape not decidable"),
    "u9310c01": (False, "door hinge pins inside a complete assembly"),
    "4790c01": (None, "ship wheel: handles are bars, spokes inside the rim may not be"),
    "3846p4e": (None, "shield: grip found, second cylinder unclear"),
    "4219b": (False, "guide pins at the door ends; the named handle is not found"),
    "u9040c01": (False, "internal parts of a complete battery box"),
    "18675ps6": (True, "handle named in the description"),
    "3628": (True, "bar named in the description"),
    "90370p03": (True, "microphone handle"),
    "5330p02": (True, "hilt grip and bar named in the description"),
    "80623": (None, "flex-system hose sheath"),
    "11407p02": (False, "figure hip pins"),
    "6122c02": (False, "helmet pivot pins"),
    "90370p05": (True, "microphone handle"),
    "92241p08": (False, "figure neck post"),
    "26490": (False, "figure hip pins"),
    "2582pa0": (False, "panel hinge pins"),
    "30518": (None, "girder rail"),
    "60849": (True, "nozzle body held as a bar"),
}


def bar_sample(lib, dims, n):
    deriver = Deriver(lib, dims, heuristics=True)
    hits = []
    for pid, desc in current_parts(lib):
        _d, raw, _m = deriver.raw(pid)
        bars = [c for c in deriver.connectors(raw) if c["kind"] == "bar" and c.get("heuristic")]
        if bars:
            hits.append((hashlib.sha256(pid.encode()).hexdigest(), pid, desc, bars))
    hits.sort()
    print(f"parts with heuristic bars: {len(hits)}; sample: the first {n} by SHA-256 of the part id")
    tally = {True: 0, False: 0, None: 0}
    unlabelled = 0
    for _h, pid, desc, bars in hits[:n]:
        label = BAR_SAMPLE_LABELS.get(pid)
        if label is None:
            unlabelled += 1
            tag = "?"
        else:
            tally[label[0]] += 1
            tag = {True: "bar", False: "not-bar", None: "unsure"}[label[0]]
        where = "; ".join(f"{c['pos']} dir {c['dir']} len {c['length']}" for c in bars)
        print(f"  {pid:12s} {tag:8s} {desc[:60]:60s} {len(bars)} bar(s): {where}")
    yes, no, unsure = tally[True], tally[False], tally[None]
    decided = yes + no
    if decided:
        total = decided + unsure
        print(f"precision (parts whose heuristic bars are all real bars): {yes}/{decided} = "
              f"{100.0 * yes / decided:.1f}% of decided labels; {unsure} undecided, so between "
              f"{100.0 * yes / total:.1f}% and {100.0 * (yes + unsure) / total:.1f}%"
              + (f"; {unlabelled} sampled parts have no label" if unlabelled else ""))
    return 0


HOLDOUT_ZERO = re.compile(r"^Technic Beam\b")


def studf_holdout(lib, dims, sample_ids):
    """Female stud synthesis on library parts outside the sample whose count the
    description fixes: RULE parts (W*L, baseplates 0) and Technic beams (0)."""
    geometry = Geometry(lib)
    tally = {}
    fails = []
    for pid, desc in current_parts(lib):
        if pid in sample_ids:
            continue
        rule = rule_truth(desc)
        if rule is not None:
            group, truth = "rule (W x L)", studf_only(rule, studf_expected(pid, "", desc))
        elif HOLDOUT_ZERO.match(desc):
            group, truth = "Technic beam (0)", studf_only(None, 0)
        else:
            continue
        tris = geometry.triangles(pid)
        fem = synth_studf(tris, dims)
        verdict, problems = evaluate(truth, fem, dims, bbox_of(tris))
        for key in (group, "all"):
            t = tally.setdefault(key, {"n": 0, "full": 0, "partial": 0, "wrong": 0})
            t["n"] += 1
            t[verdict] += 1
        if verdict != "full":
            fails.append(f"{pid} ({desc}): {'; '.join(problems)}")
    print("studF hold-out: library parts outside the sample whose description fixes the count")
    for key in sorted(tally, key=lambda k: k == "all"):
        t = tally[key]
        print(f"  {key:18s} parts {t['n']:4d}  full {t['full']:4d} ({100.0 * t['full'] / t['n']:.1f}%)  "
              f"partial {t['partial']:3d}  wrong {t['wrong']:3d}")
    for line in fails:
        print("  " + line)
    return 0


def self_test():
    """Checks of the pure logic and of a tiny synthetic library; no LDraw needed."""
    dims = load_standard(DEFAULT_STANDARD)
    module = dims["module"]
    half = module / 2.0

    def kinds(desc):
        return rule_truth(desc)["kinds"]

    assert kinds("Brick  2 x  4") == {"stud": 8, "tube": 3}
    assert kinds("Plate  1 x  4") == {"stud": 4, "underpin": 3}
    assert kinds("Tile  2 x  2 with Groove") == {"stud": 0, "tube": 1}
    assert kinds("Tile  1 x  2 with Groove") == {"stud": 0, "underpin": 1}
    assert kinds("Brick  1 x  2 x  2 without Understud") == {"stud": 2}
    assert kinds("Baseplate 32 x 32") == {"stud": 1024}
    assert rule_truth("Brick  2 x  2 Corner") is None
    assert studf_expected("x", "brick", "Brick  2 x  4") == 8
    assert studf_expected("x", "baseplate", "Baseplate 16 x 16") == 0
    assert studf_expected("x", "technic_beam", "Technic Beam  3") == 0
    assert lookup("stud.dat")[0] == "stud" and lookup("8/stud.dat")[0] == "stud"
    assert lookup("stud22a.dat")[0] == "tube" and lookup("stud3a.dat")[0] == "underpin"
    assert lookup("stud20.dat") is None and lookup("48/4-4cyli.dat") is None
    assert lookup("axlehol8.dat")[0] == "axle" and lookup("axl2hol8.dat")[0] == "axlehole"
    assert OPEN_STUD.match("stud2a.dat") and not OPEN_STUD.match("stud9.dat")
    # Two peg hole openings half a module either side of the origin, facing out.
    ends = [{"kind": "pinhole", "src": "peghole.dat", "pos": (0.0, 0.0, z),
             "dir": (0.0, 0.0, z / half)} for z in (-half, half)]
    segs = pair_ends(ends, 2 * module, inward=False, lone_length=half)
    assert len(segs) == 1 and abs(norm(sub(segs[0]["p1"], segs[0]["p0"])) - module) < 1e-9
    a = {"src": "a", "p0": ORIGIN, "p1": (module, 0.0, 0.0), "dir": PLUS_X}
    b = {"src": "b", "p0": (module, 0.0, 0.0), "p1": (2 * module, 0.0, 0.0), "dir": PLUS_X}
    c = {"src": "c", "p0": (0.0, 0.0, module), "p1": (module, 0.0, module), "dir": PLUS_X}
    runs = sorted(merge_runs([a, b, c]), key=lambda r: r["length"])
    assert [r["length"] for r in runs] == [module, 2 * module]
    assert modules_of(module / 2, module) == 1 and modules_of(2 * module, module) == 2
    self_test_nested(dims)
    self_test_studf(dims)
    print("self-test: ok")
    return 0


def self_test_nested(dims):
    """Transform composition through three levels with non-commuting rotations.

    Level 1 (part -> s/q04t1): R1 = 90 deg about z, (x, y, z) -> (-y, x, z), t1 = (m, 0, 0).
    Level 2 (s/q04t1 -> s/q04t2): R2 = 90 deg about x, (x, y, z) -> (x, -z, y), t2 = (h, 0, 0).
    Level 3 (s/q04t2): stud.dat at t3 = (h, 0, 0); axlehole.dat at the origin, scaled m in y.
    m = module, h = m / 2. World = R1 (R2 p + t2) + t1.
      stud position  R1 R2 t3 + R1 t2 + t1 = (0, h, 0) + (0, h, 0) + (m, 0, 0) = (m, m, 0)
      stud direction R1 R2 (0, -1, 0) = R1 (0, 0, -1) = (0, 0, -1)
      axle hole      from R1 t2 + t1 = (m, h, 0) to that + R1 R2 (0, m, 0) = (m, h, m):
                     centre (m, h, h), axis +-z, length m
    Composing in the wrong order (R2 R1) puts the stud at (m, h, h) facing +x; not
    rotating child translations puts it at (2m, 0, 0).
    """
    m = dims["module"]
    h = m / 2.0
    files = {
        "parts/q04t.dat": f"0 Q04 self-test part\n1 16 {m} 0 0 0 -1 0 1 0 0 0 0 1 s/q04t1.dat\n",
        "parts/s/q04t1.dat": f"0 ~level 1\n1 16 {h} 0 0 1 0 0 0 0 -1 0 1 0 s\\q04t2.dat\n",
        "parts/s/q04t2.dat": (f"0 ~level 2\n1 16 {h} 0 0 1 0 0 0 1 0 0 0 1 stud.dat\n"
                              f"1 16 0 0 0 1 0 0 0 {m} 0 0 0 1 axlehole.dat\n"),
    }
    with tempfile.TemporaryDirectory() as root:
        for rel, text in files.items():
            path = os.path.join(root, *rel.split("/"))
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(text)
        deriver = Deriver(Library(root), dims, heuristics=False)
        desc, raw, missing = deriver.raw("q04t")
        conns = deriver.connectors(raw)
    assert desc == "Q04 self-test part" and not missing, (desc, missing)
    assert counts(conns) == {"stud": 1, "axlehole": 1}, counts(conns)
    stud = next(c for c in conns if c["kind"] == "stud")
    hole = next(c for c in conns if c["kind"] == "axlehole")

    def close(p, q):
        return norm(sub(tuple(p), tuple(q))) < 1e-6

    assert close(stud["pos"], (m, m, 0.0)), stud
    assert close(stud["dir"], (0.0, 0.0, -1.0)), stud
    assert close(hole["pos"], (m, h, h)), hole
    assert abs(abs(hole["dir"][2]) - 1.0) < 1e-6 and abs(hole["length"] - m) < 1e-6, hole


def self_test_studf(dims):
    """A 1x1 brick shell has one female stud; closing its bottom removes it."""
    m, h, wall, height = dims["module"], dims["module"] / 2.0, dims["side_wall"], dims["brick"]

    def quad(x0, x1, z0, z1, y):
        return [(x0, y, z0, x1, y, z0, x1, y, z1), (x0, y, z0, x1, y, z1, x0, y, z1)]

    def brick_shell(nx):
        """Top, underside of the top and wall rims of a 1 x nx brick (no walls'
        vertical faces: vertical lines never cross them)."""
        xa, xb = -h * nx, h * nx
        out = quad(xa, xb, -h, h, 0.0) + quad(xa, xb, -h, h, wall)
        out += quad(xa, xa + wall, -h, h, height) + quad(xb - wall, xb, -h, h, height)
        out += quad(xa, xb, -h, -h + wall, height) + quad(xa, xb, h - wall, h, height)
        return out

    shell = brick_shell(1)
    found = synth_studf(shell, dims)
    assert len(found) == 1 and found[0]["pos"] == [0.0, height, 0.0], found
    assert found[0]["dir"] == [0.0, 1.0, 0.0]
    closed = shell + quad(-h, h, -h, h, height)        # a solid bottom face
    assert synth_studf(closed, dims) == []
    # Shifted by half a module in x, the other grid phase finds the same cell.
    shifted = [tuple(v + (h if i % 3 == 0 else 0.0) for i, v in enumerate(t)) for t in shell]
    moved = synth_studf(shifted, dims)
    assert len(moved) == 1 and moved[0]["pos"] == [h, height, 0.0], moved
    # A 1 x 3 shell has three cells, on the grid phase of an odd length.
    three = synth_studf(brick_shell(3), dims)
    assert [c["pos"][0] for c in three] == [-m, 0.0, m], three
    # A 1 x 3 arch: 1x1 legs at the ends, a top over the middle and two feet
    # reaching the floor at the leg faces (as the arch curve does). The middle
    # cell has room and cover but nothing touches a stud there: no female stud.
    legs = [tuple(v + (s * m if i % 3 == 0 else 0.0) for i, v in enumerate(t))
            for s in (-1.0, 1.0) for t in brick_shell(1)]
    top = quad(-h, h, -h, h, 0.0) + quad(-h, h, -h, h, wall)
    feet = quad(-h, -h + 1.0, -h, h, height) + quad(h - 1.0, h, -h, h, height)
    arch = synth_studf(legs + top + feet, dims)
    assert [c["pos"][0] for c in arch] == [-m, m], arch


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("library", nargs="?", help="extracted LDraw library (folder with parts/ and p/)")
    ap.add_argument("--parts", default=DEFAULT_PARTS, help="part list (id family per line)")
    ap.add_argument("--standard", default=DEFAULT_STANDARD, help="dimensions-v1.json")
    ap.add_argument("--csv", default=DEFAULT_CSV, help="results CSV path, '-' to skip")
    ap.add_argument("--json", default=None, help="write per-part connector lists here")
    ap.add_argument("--explain", default=None, help="print derived connectors for one part id")
    ap.add_argument("--heuristic", action="store_true",
                    help="with --explain: also apply the bar-cylinder heuristic")
    ap.add_argument("--no-version-scan", action="store_true", help="skip the library update scan")
    ap.add_argument("--mapping", action="store_true",
                    help="print the primitive mapping table and exit (no library needed)")
    ap.add_argument("--self-test", action="store_true", help="check the pure logic and exit")
    ap.add_argument("--library-stats", action="store_true",
                    help="also derive connectors for every part file in parts/ and print coverage")
    ap.add_argument("--bar-sample", type=int, default=0, metavar="N",
                    help="print a sample of N parts with heuristic bars and the labelled precision")
    ap.add_argument("--studf-holdout", action="store_true",
                    help="score female stud synthesis on library parts outside the sample and exit")
    args = ap.parse_args(argv)
    if args.mapping:
        print(mapping_table())
        return 0
    if args.self_test:
        return self_test()
    if not args.library:
        ap.error("the LDraw library folder is required")

    dims = load_standard(args.standard)
    lib = Library(args.library)
    if args.bar_sample:
        return bar_sample(lib, dims, args.bar_sample)
    if args.studf_holdout:
        return studf_holdout(lib, dims, {pid for pid, _fam in read_parts(args.parts)})
    names_only = Deriver(lib, dims, heuristics=False)
    with_geom = Deriver(lib, dims, heuristics=True)
    geometry = Geometry(lib)

    if args.explain:
        deriver = with_geom if args.heuristic else names_only
        desc, raw, missing = deriver.raw(args.explain)
        conns = deriver.connectors(raw)
        tris = geometry.triangles(args.explain)
        print(json.dumps({"part": args.explain, "description": desc, "counts": counts(conns),
                          "connectors": conns, "studF_synthesised": synth_studf(tris, dims),
                          "bbox": bbox_of(tris), "unresolved": missing}, indent=1))
        return 0

    version = "" if args.no_version_scan else lib.latest_update()
    rows, dump = [], {}
    for pid, fam in read_parts(args.parts):
        desc, raw, missing = names_only.raw(pid)
        if desc is None:
            rows.append({"part_id": pid, "family": fam, "description": "MISSING",
                         "expected": "", "derived": "", "verdict": UNVERIFIED,
                         "truth": "missing", "derived_geom": "", "verdict_geom": UNVERIFIED,
                         "verdict_lenient": UNVERIFIED, "studf_expected": "",
                         "studf_derived": "", "verdict_studf": UNVERIFIED,
                         "verdict_combined": UNVERIFIED, "notes": "part not in library",
                         "_known": {}, "_counts": {}})
            continue
        conns = names_only.connectors(raw)
        _d, raw_g, _m = with_geom.raw(pid)
        conns_g = with_geom.connectors(raw_g)
        tris = geometry.triangles(pid)
        box = bbox_of(tris)
        fem = synth_studf(tris, dims)
        truth = HAND.get(pid)
        source = "hand" if truth is not None else ""
        if truth is None:
            truth = rule_truth(desc)
            source = "rule" if truth is not None else "none"
        if truth is not None and truth.get("unverified"):
            source = UNVERIFIED
        verdict, problems = evaluate(truth, conns, dims, box, fam)
        verdict_l, _p = evaluate(truth, conns, dims, box, fam, strict_axes=False)
        verdict_g, problems_g = evaluate(truth, conns_g, dims, box, fam)
        f_exp = studf_expected(pid, fam, desc)
        verdict_f, problems_f = evaluate(studf_only(truth, f_exp), fem, dims, box, fam)
        verdict_c, problems_c = evaluate(with_studf(truth, f_exp), conns + fem, dims, box, fam)
        note = "; ".join(problems) if problems else ""
        if truth is not None and truth.get("note") and source != "rule":
            note = (truth["note"] + (" | " + note if note else ""))
        if problems_f and verdict_f != UNVERIFIED:
            note += (" | " if note else "") + "studF: " + "; ".join(problems_f)
        if missing:
            note += (" | " if note else "") + "unresolved: " + ",".join(sorted(set(missing)))
        all_counts = counts(conns + fem)
        rows.append({
            "part_id": pid, "family": fam, "description": desc,
            "expected": fmt_truth(truth), "derived": fmt_counts(counts(conns)),
            "verdict": verdict, "truth": source,
            "studf_expected": "?" if f_exp is None else f_exp, "studf_derived": len(fem),
            "verdict_studf": verdict_f, "verdict_combined": verdict_c,
            "verdict_lenient": verdict_l,
            "derived_geom": fmt_counts(counts(conns_g)), "verdict_geom": verdict_g,
            "notes": note, "_known": known_kinds(truth, f_exp), "_counts": all_counts,
        })
        dump[pid] = {"family": fam, "description": desc, "counts": counts(conns),
                     "connectors": conns, "studF_synthesised": fem, "bbox": box,
                     "counts_with_bar_heuristic": counts(conns_g),
                     "verdict": verdict, "problems": problems,
                     "verdict_studf": verdict_f, "problems_studf": problems_f,
                     "verdict_combined": verdict_c, "problems_combined": problems_c,
                     "verdict_with_bar_heuristic": verdict_g, "problems_with_bar_heuristic": problems_g}

    if args.csv != "-":
        # No LDraw text or geometry in the CSV: ids, our labels and counts only.
        # The derived columns are derived from the LDraw Parts Library (CC BY 2.0
        # or CC BY 4.0); see q04_results.csv.license and the report.
        fields = ["part_id", "family", "expected", "derived", "verdict", "truth",
                  "studf_expected", "studf_derived", "verdict_studf", "verdict_combined",
                  "derived_geom", "verdict_geom", "notes"]
        with open(args.csv, "w", newline="", encoding="utf-8") as fh:
            w = csv.DictWriter(fh, fieldnames=fields, lineterminator="\n", extrasaction="ignore")
            w.writeheader()
            for r in rows:
                w.writerow(r)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"library_update": version, "parts": dump}, fh, indent=1, sort_keys=True)

    print(f"LDraw library: {lib.root} (latest UPDATE tag {version or 'not scanned'})")
    print(f"parts: {len(rows)}")
    print_summary("A: primitive names only (strict: clip and hinge axes count as missing)",
                  summarise(rows, "verdict"))
    print_summary("A': primitive names only, clip and hinge axes accepted (sensitivity)",
                  summarise(rows, "verdict_lenient"))
    print_summary("B: names + bar-cylinder heuristic (strict)", summarise(rows, "verdict_geom"))
    print_summary("C: female stud synthesis alone (studF only)", summarise(rows, "verdict_studf"))
    print_summary("D: names + female stud synthesis (strict), the decision measure",
                  summarise(rows, "verdict_combined"))
    if args.library_stats:
        library_stats(lib, names_only)
    per_kind(rows)
    cols = ("verdict", "verdict_combined")
    fams = {}
    for r in rows:
        empty = {"full": 0, "partial": 0, "wrong": 0, UNVERIFIED: 0}
        f = fams.setdefault(r["family"], {"n": 0, **{c: dict(empty) for c in cols}})
        f["n"] += 1
        for c in cols:
            f[c][r[c]] += 1
    print("per family: family n | A full partial wrong unverified | D full partial wrong unverified")
    for fam, f in fams.items():
        a, d = f["verdict"], f["verdict_combined"]
        print(f"  {fam:18s} {f['n']:3d} | {a['full']:4d} {a['partial']:7d} {a['wrong']:5d} "
              f"{a[UNVERIFIED]:10d} | {d['full']:4d} {d['partial']:7d} {d['wrong']:5d} {d[UNVERIFIED]:10d}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
