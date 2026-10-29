#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Validate OpenBrick3D 3MF and STL exports with lib3mf, the reference 3MF library.

Backlog items 12 and 13 in docs/plan/14; decision D18 in docs/plan/03; docs/plan/05
section 7. lib3mf (BSD-2-Clause, 3MF Consortium) shares no code with the Rust
writer in crates/openbrick-io, so a file it accepts was not only checked by the
code that wrote it.

Usage:
    python3 -I scripts/validate-3mf.py [--self-test] TARGET...

Each TARGET is either
    fixtures.json   the manifest written by
                    `cargo run -p openbrick-io --example write_fixtures`; every
                    3MF and STL it lists is checked against its expectations,
    CLI.json        the JSON object of `openbrick build --json` or
                    `openbrick calibrate coupon --json`, or the coupon's
                    `--manifest`: every 3MF and STL in its "outputs" is checked
                    against what the CLI says it wrote (relative paths are
                    relative to the JSON file), or
    FILE.3mf        any OpenBrick3D export, checked with the generic rules only.

Generic rules for every 3MF, read in lib3mf strict mode:
    - lib3mf reads the package without errors and reports no warnings;
    - the unit is millimetre;
    - every object is either a mesh of type "model" that lib3mf finds manifold
      and consistently oriented, or a components object of type "model" (the
      one-object-with-parts layout of openbrick_io::ExportLayout) whose
      components reference mesh objects and carry no transform, so its parts
      keep the shared frame; there is at most one components object;
    - every mesh object is placed exactly once, by a build item of its own or
      as a component of the one placed components object; build items carry no
      transform (the objects share one frame); the build, every item, every
      object and every component carry a Production extension UUID; all UUIDs
      are distinct;
    - no two mesh objects overlap (the 3MF Core specification asks producers
      not to emit overlapping objects; touching is fine): sampled with a grid of
      vertical lines over the region where their boxes overlap, each line's
      inside intervals computed from its crossings with each mesh. A line
      that misses either mesh cannot show an overlap and is not counted; a
      line through an edge or a vertex, or with an odd number of crossings, is
      skipped, and a pair with more than half of its lines through both
      skipped fails as not checked. The sample can miss an overlap narrower than the grid spacing;
      the Rust tests prove non-overlap of the evaluator's objects with kernel
      intersections;
    - the traceability metadata of docs/plan/05 section 3 is present and not
      blank: openbrick:partId, version, contentHash, fitProfile, process,
      generator, kernel, and every mesh object names its colour role in
      openbrick:role;
    - every UUID equals the one this script derives, independently of the Rust
      code, from what lib3mf read: the metadata, each mesh object's name, role,
      colour, and vertex and triangle lists (the stored fingerprint), and the
      layout and components object name, following the recipe documented on
      openbrick_io::threemf::package_uuids_with_layout. So a file whose content
      changed after writing, or two different exports, cannot carry the same
      UUIDs unnoticed.
Manifest checks add, per 3MF: the layout, object names, roles and colours in
order, vertex and triangle counts, the stored fingerprint (FNV-1a over the
exact f32 bits and indices lib3mf read), volume (divergence theorem over
lib3mf's vertices) and bounding box, the build, object, item and component
UUIDs, the components object's name and each component's object and
transform, and the model metadata value by value; per STL (read with lib3mf's
STL reader): triangle count and volume, for the merged file and each per-role
file. A layout name other than "separateObjects" and "oneObjectWithParts"
fails. A CLI JSON gets the same 3MF checks against each output's "layout",
"assembly" (the components object: name, UUID, build item UUID, and per
component its object, UUID and transform), "objects" (with "itemUuid" null and
a "componentUuid" for a part of the components object), "buildUuid" and
"metadata": `openbrick build` writes several colour roles in the part-designer
layout (openbrick_io::ExportLayout::PART_DESIGNER) and one solid as the
separate layout. Manifests and CLI JSON written before the layout existed (no
"layout") are read as the separate layout, which is what they described. A
CLI JSON's STL must hold exactly the reported triangles (header and size), and
lib3mf must read a closed, oriented solid of the reported volume (its reader
drops triangles that become degenerate when it welds vertices, so it may count
fewer). A CLI JSON whose command failed ("ok": false) fails.

--self-test first writes broken copies of the first 3MF (a flipped triangle, a
malformed UUID, a missing build item, another unit, a missing and a blank
metadata entry, a moved vertex, no Production UUIDs at all) into a temporary
directory and requires each to fail with the problem it was built to show, so
a validator that passes everything cannot go unnoticed. When a target holds a
components object, the first such 3MF gets broken copies too (overlapping
parts, a component with a transform, a component without a UUID, a part placed
twice, a part left out, overlapping parts that cannot be sampled). lib3mf
itself refuses the malformed and the missing UUID; the self-test requires its
message. When a manifest or CLI JSON describes a components object, its
description is broken too, against the unchanged file (the separate layout
claimed, an assembly without its layout, an unknown layout, another
components object name, a part with a build item of its own, the parts in
another order), and each must fail; and the first description of a
separate-layout file must still pass with the keys the layout added removed,
as a description written before them.

Exit status: 0 when everything passes, 1 on any failure, 2 on bad usage.
Requires: pip install -r scripts/requirements-validate-3mf.txt (lib3mf 2.5.0).
"""

import copy
import json
import math
import os
import re
import struct
import sys
import tempfile
import uuid
import zipfile

try:
    import lib3mf
except ImportError:
    sys.exit(
        "lib3mf is missing: pip install --require-hashes -r scripts/requirements-validate-3mf.txt"
    )

OPENBRICK_NS = "https://openbrick3d.org/3mf/metadata/1.0"
REQUIRED_METADATA = (
    "partId",
    "version",
    "contentHash",
    "fitProfile",
    "process",
    "generator",
    "kernel",
)
# ExportMeta fields in declaration order, with the metadata entry that carries
# each (openbrick_io::threemf::package_uuids hashes them in this order).
META_FIELDS = (
    ("part_id", "openbrick:partId"),
    ("version", "openbrick:version"),
    ("content_hash", "openbrick:contentHash"),
    ("fit_profile_id", "openbrick:fitProfile"),
    ("process_id", "openbrick:process"),
    ("generator", "openbrick:generator"),
    ("kernel_id", "openbrick:kernel"),
    ("title", "Title"),
    ("serial", "openbrick:serial"),
)
UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
# Root of every OpenBrick3D UUID: uuid5(NAMESPACE_DNS, "openbrick3d.org").
UUID_ROOT = uuid.uuid5(uuid.NAMESPACE_DNS, "openbrick3d.org")
# Relative tolerance for volumes summed in a different order than the writer did.
VOLUME_RTOL = 1e-9
# The layout name of a package with a components object, as written into the
# UUID seed (openbrick_io::ExportLayout::OneObjectWithParts.name()).
GROUPED_LAYOUT = "oneObjectWithParts"
SEPARATE_LAYOUT = "separateObjects"
# Overlap sampling: vertical lines per axis over the region where two objects'
# boxes overlap, and the longest stretch of a line that may lie inside both
# (f32 coordinates of a part a few centimetres across round to about 1e-6 mm;
# 0.1 um leaves a margin and is far below any printed feature).
OVERLAP_LINES = 64
OVERLAP_TOLERANCE_MM = 1e-4
# A pair of objects whose boxes overlap must be sampled by at least this share
# of the lines that pass through both (a line that misses either one cannot
# show an overlap and is not counted); the rest ran through an edge or a
# vertex, or crossed a mesh an odd number of times (an open mesh), and say
# nothing. Below it the pair fails as not checked, so an overlap cannot hide
# behind lines that were all skipped, nor behind the empty parts of a sparse
# logo's box.
# (The nudged lines miss every edge of the exports in CI: none is skipped.)
OVERLAP_MIN_SAMPLED = 0.5
# A line closer than this (in barycentric terms) to a triangle edge is skipped:
# it would count a crossing twice or not at all.
EDGE_EPS = 1e-12
IDENTITY = [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]]

WRAPPER = lib3mf.get_wrapper()


def lib3mf_version():
    major, minor, micro = WRAPPER.GetLibraryVersion()
    return f"{major}.{minor}.{micro}"


def read(path, kind):
    """Read a file with lib3mf in strict mode; return (model, problems)."""
    model = WRAPPER.CreateModel()
    reader = model.QueryReader(kind)
    reader.SetStrictModeActive(True)
    try:
        reader.ReadFromFile(path)
    except lib3mf.ELib3MFException as e:
        return None, [f"lib3mf cannot read the file: {e}"]
    problems = [
        "lib3mf warning {}: {}".format(*reader.GetWarning(i))
        for i in range(reader.GetWarningCount())
    ]
    return model, problems


def mesh_objects(model):
    it = model.GetMeshObjects()
    out = []
    while it.MoveNext():
        out.append(it.GetCurrentMeshObject())
    return out


def components_objects(model):
    it = model.GetComponentsObjects()
    out = []
    while it.MoveNext():
        out.append(it.GetCurrentComponentsObject())
    return out


def object_count(model):
    it = model.GetObjects()
    n = 0
    while it.MoveNext():
        n += 1
    return n


def components_of(assembly):
    return [assembly.GetComponent(i) for i in range(assembly.GetComponentCount())]


def transform_rows(t):
    """A lib3mf transform as 4 rows of 3 (the 3MF matrix, translation last)."""
    return [[t.Fields[r][c] for c in range(3)] for r in range(4)]


def component_transform(component):
    """The component's transform as rows, the identity when it has none."""
    if not component.HasTransform():
        return [row[:] for row in IDENTITY]
    return transform_rows(component.GetTransform())


def mesh_lists(mesh):
    """Vertices (f32 values as Python floats) and triangles as lib3mf read them."""
    vertices = [tuple(p.Coordinates) for p in mesh.GetVertices()]
    triangles = [tuple(t.Indices) for t in mesh.GetTriangleIndices()]
    return vertices, triangles


def stored_fingerprint(mesh):
    """FNV-1a (64 bit) over every coordinate's little-endian f32 bits (-0 as 0),
    then every index as a little-endian u32: openbrick_io::threemf::
    stored_fingerprint, recomputed from what lib3mf read."""
    vertices, triangles = mesh_lists(mesh)
    data = b"".join(struct.pack("<3f", *(c + 0.0 for c in v)) for v in vertices)
    data += b"".join(struct.pack("<3I", *t) for t in triangles)
    h = 0xCBF29CE484222325
    for byte in data:
        h = ((h ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def geometry(mesh):
    """Volume (divergence theorem, in double precision) and bounds of a mesh."""
    vertices, triangles = mesh_lists(mesh)
    volume = 0.0
    for t in triangles:
        a, b, c = (vertices[i] for i in t)
        volume += (
            a[0] * (b[1] * c[2] - b[2] * c[1])
            - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
        ) / 6.0
    lo = [min(v[k] for v in vertices) for k in range(3)]
    hi = [max(v[k] for v in vertices) for k in range(3)]
    return volume, lo, hi


def colour_of(model, mesh):
    resource, prop, has = mesh.GetObjectLevelProperty()
    if not has:
        return None
    c = model.GetColorGroupByID(resource).GetColor(prop)
    return f"#{c.Red:02X}{c.Green:02X}{c.Blue:02X}{c.Alpha:02X}"


def metadata_value(group, namespace, name):
    try:
        return group.GetMetaDataByKey(namespace, name).GetValue()
    except lib3mf.ELib3MFException:
        return None


def model_metadata(model):
    """Model metadata as {key: value}; keys are "Title", "openbrick:partId" and so on."""
    group = model.GetMetaDataGroup()
    found = {}
    for i in range(group.GetMetaDataCount()):
        md = group.GetMetaData(i)
        ns, name = md.GetNameSpace(), md.GetName()
        key = name if ns == "" else ("openbrick:" + name if ns == OPENBRICK_NS else md.GetKey())
        found[key] = md.GetValue()
    return found


def derived_uuids(model, meshes, assembly=None):
    """The UUIDs openbrick_io::threemf::package_uuids_with_layout gives for what
    lib3mf read: (build, [object UUIDs in resource order], [item UUIDs in build
    order], assembly UUID or None, [component UUIDs in component order]).
    `assembly` is the components object, when the package has one."""
    metadata = model_metadata(model)

    def sized(value):
        return f"{len(value.encode('utf-8'))}:{value}"

    seed = "openbrick 3mf package\n"
    for field, key in META_FIELDS:
        value = metadata.get(key)
        if value is None and field == "serial":
            seed += "serial=-\n"
        else:
            seed += f"{field}={sized(value or '')}\n"
    seed += f"objects={len(meshes)}\n"
    for i, m in enumerate(meshes):
        role = metadata_value(m.GetMetaDataGroup(), OPENBRICK_NS, "role") or ""
        seed += f"object/{i}/name={sized(m.GetName())}\n"
        seed += f"object/{i}/role={sized(role)}\n"
        seed += f"object/{i}/color={colour_of(model, m) or '-'}\n"
        seed += (
            f"object/{i}/mesh={m.GetVertexCount()}:{m.GetTriangleCount()}:"
            f"{stored_fingerprint(m):016x}\n"
        )
    if assembly is not None:
        seed += f"layout={GROUPED_LAYOUT}\n"
        seed += f"assembly={sized(assembly.GetName())}\n"
    package = uuid.uuid5(UUID_ROOT, seed)
    n = len(meshes)
    return (
        str(uuid.uuid5(package, "build")),
        [str(uuid.uuid5(package, f"object/{i}")) for i in range(n)],
        [str(uuid.uuid5(package, f"item/{i}")) for i in range(1 if assembly else n)],
        str(uuid.uuid5(package, "assembly")) if assembly else None,
        [str(uuid.uuid5(package, f"component/{i}")) for i in range(n)] if assembly else [],
    )


def is_identity(t):
    return transform_rows(t) == IDENTITY


def triangle_list(mesh):
    """Triangles as vertex triples, from what lib3mf read."""
    vertices, triangles = mesh_lists(mesh)
    return [(vertices[a], vertices[b], vertices[c]) for a, b, c in triangles]


def box_of(tris):
    points = [p for t in tris for p in t]
    return (
        [min(p[k] for p in points) for k in range(3)],
        [max(p[k] for p in points) for k in range(3)],
    )


def inside_intervals(rows, x, y):
    """Where the vertical line through (x, y) is inside a closed mesh, as sorted
    (z0, z1) pairs, from its crossings with the triangles in `rows` (the mesh's
    triangles whose projection spans this line's row); None when the line runs
    through an edge or a vertex, where crossings would be miscounted."""
    zs = []
    for (a, b, c), (x0, x1) in rows:
        if x < x0 or x > x1:
            continue
        d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if d == 0:
            continue  # a vertical triangle: a generic vertical line misses it
        l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d
        l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d
        l3 = 1.0 - l1 - l2
        low = min(l1, l2, l3)
        if low < -EDGE_EPS:
            continue
        if low < EDGE_EPS:
            return None
        zs.append(l1 * a[2] + l2 * b[2] + l3 * c[2])
    if len(zs) % 2:
        return None
    zs.sort()
    return [(zs[i], zs[i + 1]) for i in range(0, len(zs), 2)]


def shared_length(a, b):
    """Total length two sorted interval lists have in common."""
    total, i, j = 0.0, 0, 0
    while i < len(a) and j < len(b):
        total += max(0.0, min(a[i][1], b[j][1]) - max(a[i][0], b[j][0]))
        if a[i][1] < b[j][1]:
            i += 1
        else:
            j += 1
    return total


def overlap(tris_a, tris_b):
    """Sampled overlap of two closed meshes: (longest shared stretch of a
    vertical line, its (x, y), estimated shared volume, lines used, lines
    skipped). None when their boxes share no volume, so they cannot overlap.
    A line that misses either mesh (no crossing at all) cannot show an overlap
    and counts as neither used nor skipped: only lines through both decide
    whether the pair was sampled well enough, so the empty parts of a sparse
    logo's box cannot outvote the lines that had to be skipped through it."""
    (alo, ahi), (blo, bhi) = box_of(tris_a), box_of(tris_b)
    lo = [max(alo[k], blo[k]) for k in range(3)]
    hi = [min(ahi[k], bhi[k]) for k in range(3)]
    if any(hi[k] - lo[k] <= 0 for k in range(3)):
        return None
    n = OVERLAP_LINES
    step = [(hi[k] - lo[k]) / n for k in range(2)]
    # Lines at cell centres, nudged by an irrational fraction of a cell so they
    # avoid the round coordinates that edges and vertices sit on.
    nudge = (math.sqrt(2) - 1.0) / 7.0
    xs = [lo[0] + (i + 0.5 + nudge) * step[0] for i in range(n)]
    ys = [lo[1] + (j + 0.5 - nudge) * step[1] for j in range(n)]

    def by_row(tris):
        rows = [[] for _ in range(n)]
        for t in tris:
            tx = (min(p[0] for p in t), max(p[0] for p in t))
            ty0, ty1 = min(p[1] for p in t), max(p[1] for p in t)
            for j in range(n):
                if ty0 <= ys[j] <= ty1:
                    rows[j].append((t, tx))
        return rows

    rows_a, rows_b = by_row(tris_a), by_row(tris_b)
    worst, where, total, used, skipped = 0.0, None, 0.0, 0, 0
    for j, y in enumerate(ys):
        for x in xs:
            ia = inside_intervals(rows_a[j], x, y)
            ib = inside_intervals(rows_b[j], x, y)
            if ia == [] or ib == []:
                continue  # the line misses a mesh: no overlap possible on it
            if ia is None or ib is None:
                skipped += 1
                continue
            used += 1
            shared = shared_length(ia, ib)
            total += shared * step[0] * step[1]
            if shared > worst:
                worst, where = shared, (x, y)
    return worst, where, total, used, skipped


def check_overlaps(meshes, parts):
    """Problems for every pair of mesh objects whose sampled overlap exceeds
    the tolerance; `parts` holds the resource ids of the components object's
    parts, so the message can say which kind of objects overlap."""
    problems = []
    tris = [triangle_list(m) for m in meshes]
    for i in range(len(meshes)):
        for j in range(i + 1, len(meshes)):
            result = overlap(tris[i], tris[j])
            if result is None:
                continue
            worst, where, total, used, skipped = result
            a, b = meshes[i], meshes[j]
            kind = (
                "parts"
                if a.GetResourceID() in parts and b.GetResourceID() in parts
                else "objects"
            )
            lines = used + skipped
            if used < OVERLAP_MIN_SAMPLED * lines:
                problems.append(
                    f"{kind} {a.GetName()!r} and {b.GetName()!r} were not checked for overlap: "
                    f"{skipped} of the {lines} vertical lines through both of them (over the "
                    "region where their boxes overlap) ran through an edge or a vertex or "
                    f"crossed a mesh an odd number of times, more than {1 - OVERLAP_MIN_SAMPLED:.0%}; "
                    "an overlap could hide there"
                )
            if worst > OVERLAP_TOLERANCE_MM:
                problems.append(
                    f"{kind} {a.GetName()!r} and {b.GetName()!r} overlap: a vertical line at "
                    f"x={where[0]:.4f}, y={where[1]:.4f} mm runs {worst:.4g} mm inside both "
                    f"(about {total:.4g} mm3 over {used} sampled lines, {skipped} skipped); "
                    "objects must touch, never overlap (docs/plan/05 section 7)"
                )
    return problems


def check_3mf_generic(path):
    """Generic rules; returns (model, problems, summary)."""
    model, problems = read(path, "3mf")
    if model is None:
        return None, problems, ""
    if model.GetUnit() != lib3mf.ModelUnit.MilliMeter:
        problems.append(f"unit is {model.GetUnit()}, expected millimetre")
    uuids = []
    # lib3mf 2.5.0 never lets the "has no p:UUID" checks below fire: it refuses
    # a package that declares the Production namespace and leaves out a UUID
    # ("A UUID for a build, build item or object is missing"), and invents
    # random UUIDs for one that does not declare it, which the derived-UUID
    # check at the end rejects (both are self-test mutations). The checks stay
    # as a guard should another lib3mf version read such files differently.
    has, build_uuid = model.GetBuildUUID()
    if not has:
        problems.append("the build has no p:UUID")
    uuids.append(build_uuid)
    object_uuids = []
    meshes = mesh_objects(model)
    assemblies = components_objects(model)
    if not meshes:
        problems.append("no mesh objects")
    if object_count(model) != len(meshes) + len(assemblies):
        problems.append("the package holds objects that are neither meshes nor components objects")
    if len(assemblies) > 1:
        problems.append(
            f"{len(assemblies)} components objects; an OpenBrick3D export has at most one"
        )
    by_id = {}
    for m in meshes:
        name = m.GetName()
        by_id[m.GetResourceID()] = m
        if m.GetType() != lib3mf.ObjectType.Model:
            problems.append(f"object {name!r} has type {m.GetType()}, expected model")
        if m.GetTriangleCount() == 0:
            problems.append(f"object {name!r} has no triangles")
        elif not m.IsManifoldAndOriented():
            problems.append(f"object {name!r} is not manifold and consistently oriented")
        has, u = m.GetUUID()
        if not has:
            problems.append(f"object {name!r} has no p:UUID")
        uuids.append(u)
        object_uuids.append(u)
        if not metadata_value(m.GetMetaDataGroup(), OPENBRICK_NS, "role"):
            problems.append(f"object {name!r} has no openbrick:role metadata")
    # The components object: its parts are mesh objects in the shared frame.
    assembly_ids = {a.GetResourceID(): a for a in assemblies}
    component_uuids = {}
    for a in assemblies:
        label = f"components object {a.GetName()!r}"
        if a.GetType() != lib3mf.ObjectType.Model:
            problems.append(f"{label} has type {a.GetType()}, expected model")
        has, u = a.GetUUID()
        if not has:
            problems.append(f"{label} has no p:UUID")
        uuids.append(u)
        if a.GetComponentCount() == 0:
            problems.append(f"{label} has no components")
        for k, c in enumerate(components_of(a)):
            rid = c.GetObjectResourceID()
            target = by_id.get(rid)
            part = f"component {k} of {label}"
            if target is None:
                problems.append(f"{part} refers to resource {rid}, which is not a mesh object")
            else:
                part += f" (object {target.GetName()!r})"
            if component_transform(c) != IDENTITY:
                problems.append(
                    f"{part} has a transform {component_transform(c)}: the parts must keep "
                    "the shared frame, so components carry no transform"
                )
            has, u = c.GetUUID()
            if not has:
                problems.append(f"{part} has no p:UUID")
            uuids.append(u)
            component_uuids.setdefault(a.GetResourceID(), []).append(u)
    items = model.GetBuildItems()
    placed = {rid: 0 for rid in by_id}
    item_uuids = {}
    while items.MoveNext():
        item = items.GetCurrent()
        rid = item.GetObjectResourceID()
        if rid in by_id:
            placed[rid] += 1
        elif rid in assembly_ids:
            for c in components_of(assembly_ids[rid]):
                if c.GetObjectResourceID() in placed:
                    placed[c.GetObjectResourceID()] += 1
        else:
            problems.append(
                f"a build item refers to resource {rid}, which is neither a mesh object "
                "nor a components object"
            )
        if item.HasObjectTransform() and not is_identity(item.GetObjectTransform()):
            problems.append(f"the build item of resource {rid} has a transform")
        has, u = item.GetUUID()
        if not has:
            problems.append(f"the build item of resource {rid} has no p:UUID")
        uuids.append(u)
        item_uuids.setdefault(rid, []).append(u)
    for rid, count in sorted(placed.items()):
        if count != 1:
            problems.append(
                f"object {by_id[rid].GetName()!r} (resource {rid}) is placed {count} times; "
                "every object needs exactly one build item, its own or that of the "
                "components object it is a part of"
            )
    for rid, a in assembly_ids.items():
        if len(item_uuids.get(rid, [])) != 1:
            problems.append(
                f"components object {a.GetName()!r} has {len(item_uuids.get(rid, []))} "
                "build items, expected exactly one build item"
            )
    for u in uuids:
        if u and not UUID_RE.match(u):
            problems.append(f"malformed UUID {u!r}")
    present = [u for u in uuids if u]
    if len({u.lower() for u in present}) != len(present):
        problems.append("UUIDs repeat within the package")
    group = model.GetMetaDataGroup()
    for name in REQUIRED_METADATA:
        if not (metadata_value(group, OPENBRICK_NS, name) or "").strip():
            problems.append(f"model metadata openbrick:{name} is missing or empty")
    assembly = assemblies[0] if len(assemblies) == 1 else None
    if meshes:
        build, objects, items, assembly_uuid, components = derived_uuids(model, meshes, assembly)
        names = [m.GetName() for m in meshes]
        checks = [("the build", build_uuid, build)] + [
            (f"object {n!r}", g, w) for n, g, w in zip(names, object_uuids, objects)
        ]
        if assembly is None:
            read_items = [
                (item_uuids.get(m.GetResourceID()) or [""])[0] for m in meshes
            ]
            checks += [
                (f"the build item of {n!r}", g, w) for n, g, w in zip(names, read_items, items)
            ]
        else:
            rid = assembly.GetResourceID()
            label = f"components object {assembly.GetName()!r}"
            checks += [
                (label, assembly.GetUUID()[1], assembly_uuid),
                (f"the build item of {label}", (item_uuids.get(rid) or [""])[0], items[0]),
            ]
            checks += [
                (f"component {k} of {label}", g, w)
                for k, (g, w) in enumerate(zip(component_uuids.get(rid, []), components))
            ]
        for label, got, want in checks:
            if got and got.lower() != want:
                problems.append(
                    f"p:UUID of {label} is {got}, but the metadata and objects give {want}: "
                    "the UUIDs do not match the content (see package_uuids_with_layout in "
                    "crates/openbrick-io/src/threemf.rs)"
                )
    parts = {c.GetObjectResourceID() for a in assemblies for c in components_of(a)}
    problems += check_overlaps(meshes, parts)
    vertices = sum(m.GetVertexCount() for m in meshes)
    triangles = sum(m.GetTriangleCount() for m in meshes)
    summary = f"{len(meshes)} objects, {vertices} vertices, {triangles} triangles"
    if assembly is not None:
        summary += (
            f", one components object {assembly.GetName()!r} with "
            f"{assembly.GetComponentCount()} parts"
        )
    return model, problems, summary


def close(a, b, rtol=VOLUME_RTOL):
    return math.isclose(a, b, rel_tol=rtol, abs_tol=1e-12)


def check_3mf_expected(model, fixture):
    problems = []
    meshes = mesh_objects(model)
    expected = fixture["objects"]
    if len(meshes) != len(expected):
        return [f"{len(meshes)} objects, expected {len(expected)}"]
    build_uuid = model.GetBuildUUID()[1]
    if build_uuid.lower() != fixture["buildUuid"]:
        problems.append(f"build p:UUID {build_uuid!r}, expected {fixture['buildUuid']}")
    item_uuids = {}
    items = model.GetBuildItems()
    while items.MoveNext():
        item = items.GetCurrent()
        item_uuids[item.GetObjectResourceID()] = item.GetUUID()[1]
    # The layout: manifests and CLI JSON written before it existed are the
    # separate layout.
    layout = fixture.get("layout", SEPARATE_LAYOUT)
    assemblies = components_objects(model)
    expected_assembly = fixture.get("assembly")
    if layout not in (SEPARATE_LAYOUT, GROUPED_LAYOUT):
        problems.append(
            f"unknown layout {layout!r}; expected {SEPARATE_LAYOUT!r} or {GROUPED_LAYOUT!r} "
            "(openbrick_io::ExportLayout::name)"
        )
    if (layout == GROUPED_LAYOUT) != bool(expected_assembly):
        problems.append(f"manifest layout {layout!r} and its assembly entry disagree")
    if expected_assembly is None:
        if assemblies:
            problems.append(f"{len(assemblies)} components objects, expected none ({layout})")
    elif len(assemblies) != 1:
        problems.append(f"{len(assemblies)} components objects, expected one ({layout})")
    else:
        a, e = assemblies[0], expected_assembly
        label = f"components object {e['name']!r}"
        if a.GetName() != e["name"]:
            problems.append(f"{label}: read name {a.GetName()!r}")
        if a.GetUUID()[1].lower() != e["uuid"]:
            problems.append(f"{label}: p:UUID {a.GetUUID()[1]!r}, expected {e['uuid']}")
        item_uuid = item_uuids.get(a.GetResourceID(), "")
        if item_uuid.lower() != e["itemUuid"]:
            problems.append(f"{label}: build item p:UUID {item_uuid!r}, expected {e['itemUuid']}")
        if sorted(item_uuids) != [a.GetResourceID()]:
            problems.append(f"{label}: the build places {sorted(item_uuids)}, expected only it")
        components = components_of(a)
        if len(components) != len(e["components"]):
            problems.append(
                f"{label}: {len(components)} components, expected {len(e['components'])}"
            )
        for k, (c, ec) in enumerate(zip(components, e["components"])):
            target = c.GetObjectResource()
            got = target.GetName() if target is not None else None
            if got != ec["object"]:
                problems.append(
                    f"{label}: component {k} is object {got!r}, expected {ec['object']!r}"
                )
            if c.GetUUID()[1].lower() != ec["uuid"]:
                problems.append(
                    f"{label}: component {k} p:UUID {c.GetUUID()[1]!r}, expected {ec['uuid']}"
                )
            # The parts keep their transforms: exactly the ones recorded.
            if component_transform(c) != ec["transform"]:
                problems.append(
                    f"{label}: component {k} transform {component_transform(c)}, "
                    f"expected {ec['transform']}"
                )
    for m, e in zip(meshes, expected):
        name = m.GetName()
        label = f"object {e['name']!r}"
        if name != e["name"]:
            problems.append(f"{label}: read name {name!r}")
        role = metadata_value(m.GetMetaDataGroup(), OPENBRICK_NS, "role")
        if role != e["role"]:
            problems.append(f"{label}: role {role!r}, expected {e['role']!r}")
        colour = colour_of(model, m)
        if colour != e["color"]:
            problems.append(f"{label}: colour {colour}, expected {e['color']}")
        if m.GetVertexCount() != e["vertices"]:
            problems.append(f"{label}: {m.GetVertexCount()} vertices, expected {e['vertices']}")
        if m.GetTriangleCount() != e["triangles"]:
            problems.append(f"{label}: {m.GetTriangleCount()} triangles, expected {e['triangles']}")
        uuid_read = m.GetUUID()[1]
        if uuid_read.lower() != e["uuid"]:
            problems.append(f"{label}: p:UUID {uuid_read!r}, expected {e['uuid']}")
        # A part of the components object has no build item of its own.
        item_uuid = item_uuids.get(m.GetResourceID())
        if e.get("itemUuid") is None:
            if item_uuid is not None:
                problems.append(f"{label}: has a build item of its own, expected none")
        elif (item_uuid or "").lower() != e["itemUuid"]:
            problems.append(f"{label}: build item p:UUID {item_uuid!r}, expected {e['itemUuid']}")
        if e.get("componentUuid") is not None:
            found = [
                c.GetUUID()[1].lower()
                for a in assemblies
                for c in components_of(a)
                if c.GetObjectResourceID() == m.GetResourceID()
            ]
            if found != [e["componentUuid"]]:
                problems.append(
                    f"{label}: component p:UUIDs {found}, expected [{e['componentUuid']}]"
                )
        # Exact f32 bits and indices: lib3mf's lists hash to the writer's value.
        fingerprint = f"{stored_fingerprint(m):016x}"
        if fingerprint != e["storedFingerprint"]:
            problems.append(
                f"{label}: stored fingerprint {fingerprint}, expected {e['storedFingerprint']}"
            )
        volume, lo, hi = geometry(m)
        if not close(volume, e["volumeMm3"]):
            problems.append(f"{label}: volume {volume} mm3, expected {e['volumeMm3']}")
        # The writer prints the shortest text that parses back to the same f32,
        # so the bounds must match exactly.
        if lo != e["boundsMm"]["min"] or hi != e["boundsMm"]["max"]:
            problems.append(f"{label}: bounds {lo} .. {hi}, expected {e['boundsMm']}")
    found = model_metadata(model)
    wanted = fixture["metadata"]
    for key in sorted(set(found) | set(wanted)):
        if found.get(key) != wanted.get(key):
            problems.append(
                f"model metadata {key}: {found.get(key)!r}, expected {wanted.get(key)!r}"
            )
    return problems


def check_stl(path, triangles, volume):
    model, problems = read(path, "stl")
    if model is None:
        return problems
    meshes = mesh_objects(model)
    got = sum(m.GetTriangleCount() for m in meshes)
    if got != triangles:
        problems.append(f"{got} triangles, expected {triangles}")
    # The fixtures are closed solids; lib3mf merges equal STL vertices, so a
    # flipped or missing triangle shows up here even when the volume cannot see it.
    for m in meshes:
        if not m.IsManifoldAndOriented():
            problems.append("lib3mf finds the mesh not manifold and consistently oriented")
    got_volume = sum(geometry(m)[0] for m in meshes)
    if not close(got_volume, volume):
        problems.append(f"volume {got_volume} mm3, expected {volume}")
    return problems


class Report:
    def __init__(self):
        self.failures = 0

    def result(self, label, problems, summary=""):
        if problems:
            self.failures += 1
            print(f"FAIL {label}")
            for p in problems:
                print(f"     - {p}")
        else:
            print(f"ok   {label}" + (f": {summary}" if summary else ""))


def validate_manifest(path, report, described=None):
    """A write_fixtures manifest or an openbrick CLI JSON object; returns the
    3MF paths it checked, and appends (3MF path, its description) to
    `described` for the self-test."""
    with open(path, encoding="utf-8") as f:
        manifest = json.load(f)
    if described is None:
        described = []
    if isinstance(manifest, dict) and "fixtures" in manifest:
        return validate_fixtures(path, manifest, report, described)
    if isinstance(manifest, dict) and "outputs" in manifest:
        return validate_cli_outputs(path, manifest, report, described)
    report.result(
        path,
        [
            "neither a write_fixtures manifest (no \"fixtures\") nor an openbrick "
            "CLI JSON object (no \"outputs\")"
        ],
    )
    return []


def validate_cli_outputs(path, manifest, report, described):
    """The 3MF and STL files an `openbrick ... --json` object (or a coupon
    --manifest) lists, each against the layout, components object, objects,
    UUIDs and metadata the CLI reports for it."""
    if manifest.get("ok") is False:
        report.result(
            path,
            [
                f"the command did not write its output (exit code {manifest.get('exitCode')}): "
                f"{manifest.get('error') or 'see its report'}"
            ],
        )
        return []
    base = os.path.dirname(os.path.abspath(path))
    outputs = [o for o in manifest["outputs"] if o.get("format") in ("3mf", "stl")]
    if not outputs:
        report.result(path, ["lists no 3MF or STL output"])
        return []
    files = []
    for o in outputs:
        file = os.path.join(base, o["path"])
        label = os.path.relpath(file)
        if o["format"] == "3mf":
            files.append(file)
            model, problems, summary = check_3mf_generic(file)
            if model is not None:
                problems += check_3mf_expected(model, o)
                if not problems:
                    described.append((file, o))
            layout = o.get("layout", SEPARATE_LAYOUT)
            report.result(
                label, problems, summary + f", {layout}, matches {os.path.basename(path)}"
            )
        else:
            problems, merged = check_cli_stl(file, o)
            report.result(
                label,
                problems,
                f"{o['triangles']} triangles ({merged} degenerate once lib3mf's STL reader welds "
                f"vertices), matches {os.path.basename(path)}",
            )
    return files


def check_cli_stl(path, entry):
    """A binary STL the CLI wrote: its header and size hold exactly the
    triangles the CLI reports, and lib3mf reads a closed, oriented solid of the
    reported volume. lib3mf's STL reader welds vertices that fall on the same
    point of its grid and drops the triangles that become degenerate. Kernel
    meshes carry a few zero-area triangles (coincident vertices that kept
    separate indices; the 3MF of the same mesh keeps them), so lib3mf may count
    fewer triangles than the file holds, never more. Returns (problems,
    triangles dropped)."""
    problems = []
    with open(path, "rb") as f:
        data = f.read()
    if len(data) < 84:
        return [f"{len(data)} bytes, shorter than a binary STL header"], 0
    (count,) = struct.unpack_from("<I", data, 80)
    if count != entry["triangles"]:
        problems.append(f"the header counts {count} triangles, the CLI wrote {entry['triangles']}")
    if len(data) != 84 + 50 * count:
        problems.append(f"{len(data)} bytes, expected {84 + 50 * count} for {count} triangles")
    model, read_problems = read(path, "stl")
    problems += read_problems
    if model is None:
        return problems, 0
    meshes = mesh_objects(model)
    got = sum(m.GetTriangleCount() for m in meshes)
    if got > count:
        problems.append(f"lib3mf read {got} triangles from a file with {count}")
    for m in meshes:
        if not m.IsManifoldAndOriented():
            problems.append("lib3mf finds the mesh not manifold and consistently oriented")
    volume = sum(geometry(m)[0] for m in meshes)
    if not close(volume, entry["volumeMm3"]):
        problems.append(f"volume {volume} mm3, expected {entry['volumeMm3']}")
    return problems, count - got


def validate_fixtures(path, manifest, report, described):
    if manifest.get("openbrickNamespace") != OPENBRICK_NS:
        report.result(path, [f"manifest namespace {manifest.get('openbrickNamespace')!r}"])
        return []
    base = os.path.dirname(os.path.abspath(path))
    files = []
    for fx in manifest["fixtures"]:
        threemf = os.path.join(base, fx["threemf"])
        files.append(threemf)
        model, problems, summary = check_3mf_generic(threemf)
        if model is not None:
            problems += check_3mf_expected(model, fx)
            if not problems:
                described.append((threemf, fx))
        report.result(fx["threemf"], problems, summary + ", matches the manifest")
        total_volume = sum(o["volumeMm3"] for o in fx["objects"])
        stl = os.path.join(base, fx["stl"])
        report.result(
            fx["stl"],
            check_stl(stl, fx["stlTriangles"], total_volume),
            f"{fx['stlTriangles']} triangles",
        )
        for role, entry in fx["stlPerRole"].items():
            volume = sum(o["volumeMm3"] for o in fx["objects"] if o["role"] == role)
            report.result(
                entry["file"],
                check_stl(os.path.join(base, entry["file"]), entry["triangles"], volume),
                f"{entry['triangles']} triangles",
            )
    return files


def rewrite(src, dst, edit_model):
    """Copy a 3MF, passing the model part through edit_model(text) -> text."""
    with zipfile.ZipFile(src) as zin, zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as zout:
        for info in zin.infolist():
            data = zin.read(info.filename)
            if info.filename == "3D/3dmodel.model":
                edited = edit_model(data.decode("utf-8"))
                if edited == data.decode("utf-8"):
                    raise RuntimeError("self-test mutation changed nothing")
                data = edited.encode("utf-8")
            zout.writestr(info.filename, data)


def flip_first_triangle(text):
    return re.sub(
        r'<triangle v1="(\d+)" v2="(\d+)" v3="(\d+)"/>',
        r'<triangle v1="\1" v2="\3" v3="\2"/>',
        text,
        count=1,
    )


def break_first_uuid(text):
    return re.sub(r'(<object [^>]*p:UUID=")[^"]+"', r'\1not-a-uuid"', text, count=1)


def drop_last_item(text):
    items = list(re.finditer(r"\s*<item [^>]*/>", text))
    last = items[-1]
    return text[: last.start()] + text[last.end() :]


def use_inches(text):
    return text.replace('unit="millimeter"', 'unit="inch"', 1)


def drop_content_hash(text):
    return re.sub(r'\s*<metadata name="openbrick:contentHash"[^<]*</metadata>', "", text, count=1)


def blank_content_hash(text):
    return re.sub(
        r'(<metadata name="openbrick:contentHash"[^>]*>)[^<]*(</metadata>)', r"\1 \2", text, count=1
    )


def drop_production_uuids(text):
    """Remove every p:UUID and the Production namespace: lib3mf then reads the
    package and makes up random UUIDs, so only the derived UUIDs notice."""
    text = re.sub(r' p:UUID="[^"]+"', "", text)
    return re.sub(r' xmlns:p="[^"]+"', "", text, count=1)


def move_first_vertex(text):
    """Shift the first vertex by 0.5 mm along x: the topology stays manifold, so
    only the content-derived UUIDs can notice."""

    def shift(m):
        return f'{m.group(1)}{float(m.group(2)) + 0.5!r}"'

    return re.sub(r'(<vertex x=")([^"]+)"', shift, text, count=1)


# (label, mutation, a fragment of the problem the broken copy must report)
MUTATIONS = [
    ("flipped triangle", flip_first_triangle, "not manifold"),
    # lib3mf's own message (lib3mf 2.5.0 refuses the file before this script's checks).
    ("malformed object UUID", break_first_uuid, "A UUID is ill formatted"),
    ("missing build item", drop_last_item, "exactly one build item"),
    ("unit inch", use_inches, "expected millimetre"),
    ("missing openbrick:contentHash", drop_content_hash, "openbrick:contentHash is missing"),
    ("blank openbrick:contentHash", blank_content_hash, "openbrick:contentHash is missing or empty"),
    ("moved vertex", move_first_vertex, "the UUIDs do not match the content"),
    ("no Production UUIDs", drop_production_uuids, "the UUIDs do not match the content"),
]


def mesh_object_blocks(text):
    return [
        b for b in re.finditer(r"<object [^>]*>.*?</object>", text, re.S) if "<mesh>" in b.group(0)
    ]


def sink_second_part(text):
    """Lower every vertex of the second mesh object by 0.5 mm, into the first:
    both stay manifold, so only the overlap check (and the UUIDs) can notice."""
    block = mesh_object_blocks(text)[1]

    def lower(m):
        return f'{m.group(1)}{float(m.group(2)) - 0.5!r}"'

    moved = re.sub(r'( z=")([^"]+)"', lower, block.group(0))
    return text[: block.start()] + moved + text[block.end() :]


def transform_last_component(text):
    last = list(re.finditer(r'<component objectid="\d+"', text))[-1]
    return text[: last.end()] + ' transform="1 0 0 0 1 0 0 0 1 0 0 0.5"' + text[last.end() :]


def drop_component_uuid(text):
    return re.sub(r'(<component [^>]*?) p:UUID="[^"]+"', r"\1", text, count=1)


def place_part_too(text):
    first = re.search(r'<component objectid="(\d+)"', text).group(1)
    extra = uuid.uuid5(UUID_ROOT, "validate-3mf self-test: part placed twice")
    return text.replace(
        "</build>", f'  <item objectid="{first}" p:UUID="{extra}"/>\n </build>', 1
    )


def open_sunk_second_part(text):
    """Sink the second mesh object into the first, then remove its top cap (the
    triangles whose three vertices lie at its highest z): every vertical line
    through it now crosses it once, so the overlap sample cannot use those lines
    and must say the pair was not checked."""
    text = sink_second_part(text)
    block = mesh_object_blocks(text)[1]
    body = block.group(0)
    zs = [float(z) for z in re.findall(r'<vertex [^>]*z="([^"]+)"', body)]
    top = max(zs)

    def keep(m):
        corners = (int(m.group(k)) for k in (1, 2, 3))
        return "" if all(zs[v] == top for v in corners) else m.group(0)

    opened = re.sub(r'\s*<triangle v1="(\d+)" v2="(\d+)" v3="(\d+)"[^>]*/>', keep, body)
    return text[: block.start()] + opened + text[block.end() :]


def drop_last_component(text):
    last = list(re.finditer(r"\s*<component [^>]*/>", text))[-1]
    return text[: last.start()] + text[last.end() :]


# Broken copies of a package with a components object.
COMPONENT_MUTATIONS = [
    ("overlapping parts", sink_second_part, "overlap"),
    ("component with a transform", transform_last_component, "has a transform"),
    # lib3mf's own message (lib3mf 2.5.0 refuses the file before this script's checks).
    ("component without a UUID", drop_component_uuid, "A UUID for a build, build item or object is missing"),
    ("part placed twice", place_part_too, "exactly one build item"),
    ("part left out", drop_last_component, "exactly one build item"),
    ("overlapping parts that cannot be sampled", open_sunk_second_part, "not checked for overlap"),
]


def has_components(path):
    with zipfile.ZipFile(path) as z:
        return "<components>" in z.read("3D/3dmodel.model").decode("utf-8")


def claim_separate_layout(entry):
    entry["layout"] = SEPARATE_LAYOUT
    entry["assembly"] = None


def drop_layout_keep_assembly(entry):
    entry["layout"] = SEPARATE_LAYOUT


def unknown_layout(entry):
    entry["layout"] = "sideBySide"


def rename_assembly(entry):
    entry["assembly"]["name"] += " (renamed)"


def give_part_an_item(entry):
    entry["objects"][0]["itemUuid"] = str(
        uuid.uuid5(UUID_ROOT, "validate-3mf self-test: a part's own build item")
    )


def reorder_components(entry):
    components = entry["assembly"]["components"]
    components[0], components[-1] = components[-1], components[0]


# Broken descriptions (manifest entries or CLI JSON outputs) of a valid file
# with a components object: (label, edit of a copy, a fragment of the problem).
DESCRIPTION_MUTATIONS = [
    ("the separate layout claimed for a components object", claim_separate_layout, "expected none"),
    ("an assembly without its layout", drop_layout_keep_assembly, "disagree"),
    ("an unknown layout", unknown_layout, "unknown layout"),
    ("another components object name", rename_assembly, "read name"),
    ("a part described with a build item of its own", give_part_an_item, "build item p:UUID None"),
    ("the parts in another order", reorder_components, "component 0 is object"),
]
# The keys the layout added to descriptions; one written before it has none.
LAYOUT_KEYS = ("layout", "assembly")
LAYOUT_OBJECT_KEYS = ("componentUuid",)


def without_layout_keys(entry):
    old = {k: v for k, v in entry.items() if k not in LAYOUT_KEYS}
    old["objects"] = [
        {k: v for k, v in o.items() if k not in LAYOUT_OBJECT_KEYS} for o in entry["objects"]
    ]
    return old


def self_test(paths, report, described=()):
    """Every broken copy of a valid file must fail the generic rules, with the
    problem it was built to show: the generic mutations on the first 3MF, and
    the component mutations on the first 3MF with a components object. Every
    broken description of the first described file with a components object
    must fail against that file, and the first description of a separate-layout
    file must still pass without the keys the layout added."""
    runs = [(paths[0], MUTATIONS)]
    grouped = next((p for p in paths if has_components(p)), None)
    if grouped is not None:
        runs.append((grouped, COMPONENT_MUTATIONS))
    for path, mutations in runs:
        self_test_one(path, mutations, report)
    grouped = next(((f, e) for f, e in described if e.get("assembly")), None)
    if grouped is not None:
        self_test_descriptions(*grouped, report)
    separate = next(((f, e) for f, e in described if not e.get("assembly")), None)
    if separate is not None:
        path, entry = separate
        model, problems = read(path, "3mf")
        if model is not None:
            problems += check_3mf_expected(model, without_layout_keys(entry))
        report.result(
            f"self-test: {os.path.basename(path)} described without the layout keys still passes",
            problems,
            "read as the separate layout",
        )


def self_test_descriptions(path, entry, report):
    model, problems = read(path, "3mf")
    if model is None:
        report.result(f"self-test: descriptions of {os.path.basename(path)}", problems)
        return
    for label, edit, fragment in DESCRIPTION_MUTATIONS:
        broken = copy.deepcopy(entry)
        edit(broken)
        if broken == entry:
            raise RuntimeError("self-test description mutation changed nothing")
        problems = check_3mf_expected(model, broken)
        hits = [p for p in problems if fragment in p]
        if hits:
            report.result(f"self-test: {label} is rejected", [], hits[0])
        else:
            report.result(
                f"self-test: {label} is rejected",
                [
                    f"no problem mentions {fragment!r}; the description check is too lenient",
                    *(f"(reported instead: {p})" for p in problems),
                ],
            )


def self_test_one(path, mutations, report):
    with tempfile.TemporaryDirectory() as tmp:
        for label, edit, fragment in mutations:
            dst = os.path.join(tmp, "mutated.3mf")
            rewrite(path, dst, edit)
            _, problems, _ = check_3mf_generic(dst)
            hits = [p for p in problems if fragment in p]
            if hits:
                report.result(f"self-test: {label} is rejected", [], hits[0])
            else:
                report.result(
                    f"self-test: {label} is rejected",
                    [
                        f"no problem mentions {fragment!r}; the validator is too lenient",
                        *(f"(reported instead: {p})" for p in problems),
                    ],
                )


def main(argv):
    args = argv[1:]
    run_self_test = "--self-test" in args
    targets = [a for a in args if a != "--self-test"]
    if not targets or any(a.startswith("-") for a in targets):
        usage = next(p for p in __doc__.split("\n\n") if p.startswith("Usage:"))
        print(usage, file=sys.stderr)
        return 2
    print(f"lib3mf {lib3mf_version()}, strict mode")
    report = Report()
    threemfs = []
    described = []
    for target in targets:
        if target.endswith(".json"):
            threemfs += validate_manifest(target, report, described)
        elif target.endswith(".3mf"):
            _, problems, summary = check_3mf_generic(target)
            report.result(target, problems, summary)
            threemfs.append(target)
        else:
            print(
                f"unknown target {target!r}: pass a fixtures.json, an openbrick CLI .json or a .3mf",
                file=sys.stderr,
            )
            return 2
    if run_self_test:
        if not threemfs:
            print("--self-test needs at least one 3MF", file=sys.stderr)
            return 2
        self_test(threemfs, report, described)
    if report.failures:
        print(f"{report.failures} check(s) failed")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
