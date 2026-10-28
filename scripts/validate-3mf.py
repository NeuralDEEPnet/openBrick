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
                    3MF and STL it lists is checked against its expectations, or
    FILE.3mf        any OpenBrick3D export, checked with the generic rules only.

Generic rules for every 3MF, read in lib3mf strict mode:
    - lib3mf reads the package without errors and reports no warnings;
    - the unit is millimetre;
    - every object is a mesh of type "model" that lib3mf finds manifold and
      consistently oriented;
    - every object has a build item with no transform (the objects share one
      frame), and the build, every item and every object carry a Production
      extension UUID; all UUIDs are distinct;
    - the traceability metadata of docs/plan/05 section 3 is present and not
      blank: openbrick:partId, version, contentHash, fitProfile, process,
      generator, kernel, and every object names its colour role in
      openbrick:role;
    - every UUID equals the one this script derives, independently of the Rust
      code, from what lib3mf read: the metadata, and each object's name, role,
      colour, and vertex and triangle lists (the stored fingerprint), following
      the recipe documented on openbrick_io::threemf::package_uuids. So a file
      whose content changed after writing, or two different exports, cannot
      carry the same UUIDs unnoticed.
Manifest checks add, per 3MF: object names, roles and colours in order, vertex
and triangle counts, the stored fingerprint (FNV-1a over the exact f32 bits and
indices lib3mf read), volume (divergence theorem over lib3mf's vertices) and
bounding box, the build, object and item UUIDs, and the model metadata value by
value; per STL (read with lib3mf's STL reader): triangle count and volume, for
the merged file and each per-role file.

--self-test first writes broken copies of the first 3MF (a flipped triangle, a
malformed UUID, a missing build item, another unit, a missing and a blank
metadata entry, a moved vertex) into a temporary directory and requires each to
fail with the problem it was built to show, so a validator that passes
everything cannot go unnoticed.

Exit status: 0 when everything passes, 1 on any failure, 2 on bad usage.
Requires: pip install -r scripts/requirements-validate-3mf.txt (lib3mf 2.5.0).
"""

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


def object_count(model):
    it = model.GetObjects()
    n = 0
    while it.MoveNext():
        n += 1
    return n


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


def derived_uuids(model, meshes):
    """The UUIDs openbrick_io::threemf::package_uuids gives for what lib3mf read:
    (build, [object UUIDs in resource order], [item UUIDs in object order])."""
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
    package = uuid.uuid5(UUID_ROOT, seed)
    return (
        str(uuid.uuid5(package, "build")),
        [str(uuid.uuid5(package, f"object/{i}")) for i in range(len(meshes))],
        [str(uuid.uuid5(package, f"item/{i}")) for i in range(len(meshes))],
    )


def is_identity(t):
    identity = [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]]
    return all(t.Fields[r][c] == identity[r][c] for r in range(4) for c in range(3))


def check_3mf_generic(path):
    """Generic rules; returns (model, problems, summary)."""
    model, problems = read(path, "3mf")
    if model is None:
        return None, problems, ""
    if model.GetUnit() != lib3mf.ModelUnit.MilliMeter:
        problems.append(f"unit is {model.GetUnit()}, expected millimetre")
    uuids = []
    has, build_uuid = model.GetBuildUUID()
    if not has:
        problems.append("the build has no p:UUID")
    uuids.append(build_uuid)
    object_uuids = []
    item_uuids = {}
    meshes = mesh_objects(model)
    if not meshes:
        problems.append("no mesh objects")
    if object_count(model) != len(meshes):
        problems.append("the package holds objects that are not meshes")
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
    items = model.GetBuildItems()
    referenced = []
    while items.MoveNext():
        item = items.GetCurrent()
        rid = item.GetObjectResourceID()
        referenced.append(rid)
        if rid not in by_id:
            problems.append(f"a build item refers to resource {rid}, which is not a mesh object")
        if item.HasObjectTransform() and not is_identity(item.GetObjectTransform()):
            problems.append(f"the build item of resource {rid} has a transform")
        has, u = item.GetUUID()
        if not has:
            problems.append(f"the build item of resource {rid} has no p:UUID")
        uuids.append(u)
        item_uuids[rid] = u
    if sorted(referenced) != sorted(by_id):
        problems.append(
            f"build items refer to {sorted(referenced)}, objects are {sorted(by_id)}; "
            "every object needs exactly one build item"
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
    if meshes:
        build, objects, items = derived_uuids(model, meshes)
        read_items = [item_uuids.get(m.GetResourceID(), "") for m in meshes]
        names = [m.GetName() for m in meshes]
        for label, got, want in (
            [("the build", build_uuid, build)]
            + [(f"object {n!r}", g, w) for n, g, w in zip(names, object_uuids, objects)]
            + [(f"the build item of {n!r}", g, w) for n, g, w in zip(names, read_items, items)]
        ):
            if got and got.lower() != want:
                problems.append(
                    f"p:UUID of {label} is {got}, but the metadata and objects give {want}: "
                    "the UUIDs do not match the content (see package_uuids in "
                    "crates/openbrick-io/src/threemf.rs)"
                )
    vertices = sum(m.GetVertexCount() for m in meshes)
    triangles = sum(m.GetTriangleCount() for m in meshes)
    summary = f"{len(meshes)} objects, {vertices} vertices, {triangles} triangles"
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
        item_uuid = item_uuids.get(m.GetResourceID(), "")
        if item_uuid.lower() != e["itemUuid"]:
            problems.append(f"{label}: build item p:UUID {item_uuid!r}, expected {e['itemUuid']}")
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


def validate_manifest(path, report):
    with open(path, encoding="utf-8") as f:
        manifest = json.load(f)
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


def move_first_vertex(text):
    """Shift the first vertex by 0.5 mm along x: the topology stays manifold, so
    only the content-derived UUIDs can notice."""

    def shift(m):
        return f'{m.group(1)}{float(m.group(2)) + 0.5!r}"'

    return re.sub(r'(<vertex x=")([^"]+)"', shift, text, count=1)


# (label, mutation, a fragment of the problem the broken copy must report)
MUTATIONS = [
    ("flipped triangle", flip_first_triangle, "not manifold"),
    ("malformed object UUID", break_first_uuid, "UUID"),
    ("missing build item", drop_last_item, "exactly one build item"),
    ("unit inch", use_inches, "expected millimetre"),
    ("missing openbrick:contentHash", drop_content_hash, "openbrick:contentHash is missing"),
    ("blank openbrick:contentHash", blank_content_hash, "openbrick:contentHash is missing or empty"),
    ("moved vertex", move_first_vertex, "the UUIDs do not match the content"),
]


def self_test(path, report):
    """Every broken copy of a valid file must fail the generic rules, with the
    problem it was built to show."""
    with tempfile.TemporaryDirectory() as tmp:
        for label, edit, fragment in MUTATIONS:
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
    for target in targets:
        if target.endswith(".json"):
            threemfs += validate_manifest(target, report)
        elif target.endswith(".3mf"):
            _, problems, summary = check_3mf_generic(target)
            report.result(target, problems, summary)
            threemfs.append(target)
        else:
            print(f"unknown target {target!r}: pass a fixtures.json or a .3mf", file=sys.stderr)
            return 2
    if run_self_test:
        if not threemfs:
            print("--self-test needs at least one 3MF", file=sys.stderr)
            return 2
        self_test(threemfs[0], report)
    if report.failures:
        print(f"{report.failures} check(s) failed")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
