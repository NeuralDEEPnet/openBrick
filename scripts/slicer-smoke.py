#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Helpers of the OrcaSlicer smoke test (scripts/slicer-smoke.sh).

Backlog item 18 in docs/plan/14; docs/plan/05 section 7; Q18 in
docs/spikes/q18-slicer-multimaterial-import.md. OrcaSlicer is AGPL-3.0: it runs
as a separate program only (LICENSE-POLICY.md), and nothing here comes from its
source. What this script knows about OrcaSlicer's files was read from its
public source for behaviour only (tag v2.4.2: src/OrcaSlicer.cpp,
src/libslic3r/Config.cpp, src/libslic3r/Format/bbs_3mf.cpp) and is described
in the comments.

Python 3 standard library only. Usage:

    python3 -I scripts/slicer-smoke.py flatten --profiles DIR --vendor BBL \\
        --machine NAME --process NAME --filament NAME [--colours C1,C2] --out DIR
    python3 -I scripts/slicer-smoke.py report --work DIR [--dry-run] \\
        [--orca VERSION] [--appimage NAME] [--sha256 HEX]
    python3 -I scripts/slicer-smoke.py self-test

flatten
    OrcaSlicer's command line reads `--load-settings` and `--load-filaments`
    files key by key and does not follow their `inherits` chain (Config.cpp,
    ConfigBase::load_from_json), and a system profile such as
    "Bambu Lab A1 0.4 nozzle" holds only what differs from its parents. So the
    chain is resolved here, from the vendor index (`profiles/<vendor>.json`,
    which names every profile and its file) of the extracted AppImage: parents
    first, each child's keys replacing its parent's. The result keeps the
    leaf's name, type and `"from": "system"` (the CLI accepts only system or
    user profiles) and drops `inherits`. Writes machine.json, process.json and
    one filament-N.json per colour in --colours (filament_colour set to it).

report
    Reads WORK/inputs/<name>.3mf with the openbrick CLI's --json description
    <name>.json, and for each the slicer's output in WORK/sliced/<name>/:
    exit-code, result.json (OrcaSlicer's CLI writes it on Linux), the exported
    project <name>.sliced.3mf and orca.log. From the project it reads
    Metadata/model_settings.config (objects, their parts and each part's
    `extruder`, which falls back to its object's) and Metadata/slice_info.config
    (the filaments the sliced plate used). Writes WORK/summary.md and
    WORK/results.json. Exits 1 when a file did not slice (not in --dry-run),
    0 otherwise.

self-test
    Checks flatten on a small synthetic profile tree and report on synthetic
    slicer outputs, so the parsing is tested where OrcaSlicer cannot run.
"""

import argparse
import json
import os
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile

INPUT_ORDER = [
    "logo-tile-emboss",
    "logo-tile-inlay-parts",
    "logo-tile-inlay-parts-no-colour",
    "brand-tile",
    "coupon-fdm-0.4-cross",
]

DESCRIPTIONS = {
    "logo-tile-emboss": "2x4 logo tile, raised logo (body and accent parts, with colours)",
    "logo-tile-inlay-parts": "2x4 logo tile, multi-material inlay (body and accent parts, with colours, face down)",
    "logo-tile-inlay-parts-no-colour": "the same inlay without a colour group (`--no-colour`)",
    "brand-tile": "Neuraldeep.net demo tile, multi-material inlay (body and accent parts, with colours)",
    "coupon-fdm-0.4-cross": "calibration coupon fdm-0.4 (one object, one colour): the baseline",
}


# ---------------------------------------------------------------------------
# flatten


def load_index(profiles_dir):
    """{(vendor, type, name): path} from every vendor index in profiles_dir."""
    index = {}
    lists = (("machine_list", "machine"), ("process_list", "process"), ("filament_list", "filament"))
    for entry in sorted(os.listdir(profiles_dir)):
        if not entry.endswith(".json"):
            continue
        vendor = entry[: -len(".json")]
        try:
            with open(os.path.join(profiles_dir, entry), encoding="utf-8") as f:
                data = json.load(f)
        except (OSError, ValueError):
            continue
        if not isinstance(data, dict):
            continue
        for key, kind in lists:
            for item in data.get(key, []) or []:
                if isinstance(item, dict) and "name" in item and "sub_path" in item:
                    path = os.path.join(profiles_dir, vendor, item["sub_path"])
                    index.setdefault((vendor, kind, item["name"]), path)
    return index


def find_profile(index, vendor, kind, name):
    """The file of profile `name`: in `vendor` first, then in any vendor."""
    path = index.get((vendor, kind, name))
    if path:
        return path
    for (v, k, n), p in sorted(index.items()):
        if k == kind and n == name:
            return p
    raise SystemExit(f"slicer-smoke: no {kind} profile named {name!r} in the vendor indexes")


def resolve(index, vendor, kind, name, chain=()):
    """The profile with its whole inherits chain merged, parents first."""
    if name in chain:
        raise SystemExit(f"slicer-smoke: inherits loop {' -> '.join(chain + (name,))}")
    path = find_profile(index, vendor, kind, name)
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    parent = data.get("inherits") or ""
    merged = resolve(index, vendor, kind, parent, chain + (name,)) if parent else {}
    merged.update(data)
    return merged


def flatten(args):
    index = load_index(args.profiles)
    if not index:
        raise SystemExit(f"slicer-smoke: no vendor index (*.json) in {args.profiles}")
    os.makedirs(args.out, exist_ok=True)
    written = {}
    for kind, name, file in (("machine", args.machine, "machine.json"), ("process", args.process, "process.json")):
        written[file] = flat_profile(index, args.vendor, kind, name)
    colours = [c.strip() for c in args.colours.split(",") if c.strip()]
    for i, colour in enumerate(colours or ["#E6E6E6"], start=1):
        profile = flat_profile(index, args.vendor, "filament", args.filament)
        profile["filament_colour"] = [colour]
        written[f"filament-{i}.json"] = profile
    for file, profile in written.items():
        with open(os.path.join(args.out, file), "w", encoding="utf-8") as f:
            json.dump(profile, f, indent=2, sort_keys=True)
            f.write("\n")
    print(f"slicer-smoke: wrote {', '.join(sorted(written))} to {args.out}")
    return 0


def flat_profile(index, vendor, kind, name):
    profile = resolve(index, vendor, kind, name)
    profile.pop("inherits", None)
    profile["name"] = name
    profile["type"] = kind
    profile["from"] = "system"
    profile["instantiation"] = "true"
    return profile


# ---------------------------------------------------------------------------
# report


def read_text(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return None


def read_json(path):
    text = read_text(path)
    if text is None:
        return None
    try:
        return json.loads(text)
    except ValueError:
        return None


def metadata(element):
    """{key: value} of an element's <metadata key=… value=…/> children."""
    return {m.get("key"): m.get("value") for m in element.findall("metadata") if m.get("key")}


def parse_model_settings(xml_text):
    """Objects of Metadata/model_settings.config: name, extruder and parts.

    Each <object id> carries <metadata key="name"/"extruder"> and one <part
    id subtype> per volume with its own metadata; a part without `extruder`
    prints with its object's (bbs_3mf.cpp, _add_model_config_file_to_archive).
    """
    root = ET.fromstring(xml_text)
    objects = []
    for obj in root.findall("object"):
        meta = metadata(obj)
        obj_extruder = meta.get("extruder")
        parts = []
        for part in obj.findall("part"):
            pmeta = metadata(part)
            extruder = pmeta.get("extruder") or obj_extruder or "1"
            parts.append(
                {
                    "name": pmeta.get("name", ""),
                    "subtype": part.get("subtype", ""),
                    "extruder": int(extruder) if str(extruder).isdigit() else extruder,
                    "extruderFrom": "part" if pmeta.get("extruder") else ("object" if obj_extruder else "default"),
                }
            )
        objects.append({"name": meta.get("name", ""), "extruder": obj_extruder, "parts": parts})
    return objects


def parse_slice_info(xml_text):
    """Plates of Metadata/slice_info.config: the filaments each used, warnings."""
    root = ET.fromstring(xml_text)
    plates = []
    for plate in root.findall("plate"):
        filaments = []
        for fil in plate.findall("filament"):
            filaments.append(
                {
                    "id": fil.get("id"),
                    "type": fil.get("type"),
                    "colour": fil.get("color"),
                    "usedG": fil.get("used_g"),
                    "usedM": fil.get("used_m"),
                }
            )
        warnings = [w.get("msg", "") for w in plate.findall("warning")]
        objects = [o.get("name", "") for o in plate.findall("object")]
        plates.append({"index": metadata(plate).get("index"), "filaments": filaments, "objects": objects, "warnings": warnings})
    return plates


def read_project(path):
    """What OrcaSlicer wrote into its exported project 3MF."""
    out = {"objects": None, "plates": None, "gcodeToolChanges": None, "problems": []}
    try:
        with zipfile.ZipFile(path) as z:
            names = set(z.namelist())
            if "Metadata/model_settings.config" in names:
                out["objects"] = parse_model_settings(z.read("Metadata/model_settings.config").decode("utf-8", "replace"))
            else:
                out["problems"].append("no Metadata/model_settings.config in the project")
            if "Metadata/slice_info.config" in names:
                out["plates"] = parse_slice_info(z.read("Metadata/slice_info.config").decode("utf-8", "replace"))
            gcodes = sorted(n for n in names if n.startswith("Metadata/plate_") and n.endswith(".gcode"))
            if gcodes:
                tools = set()
                for g in gcodes:
                    for line in z.read(g).decode("utf-8", "replace").splitlines():
                        word = line.split(";", 1)[0].strip()
                        if len(word) >= 2 and word[0] == "T" and word[1:].isdigit():
                            tools.add(int(word[1:]))
                out["gcodeToolChanges"] = sorted(tools)
    except (OSError, zipfile.BadZipFile, ET.ParseError) as e:
        out["problems"].append(f"cannot read the project: {e}")
    return out


def expected_parts(description):
    """Layout and parts the openbrick CLI says it wrote (its --json)."""
    if not description:
        return None, []
    outputs = description.get("outputs") or []
    if not outputs:
        return None, []
    first = outputs[0]
    parts = [{"name": o.get("name"), "role": o.get("role"), "colour": o.get("color")} for o in first.get("objects", [])]
    return first.get("layout"), parts


def interpret(layout, expected, sliced, project):
    """One sentence per finding, for the Q18 column."""
    notes = []
    if not sliced:
        return ["Did not slice, so this run says nothing about Q18 for this file; see its log."]
    objects = project.get("objects") or []
    n_parts = sum(len(o["parts"]) for o in objects)
    if not objects:
        return ["Sliced, but the project records no objects: check the log."]
    if n_parts == 0:
        return ["Sliced, but the project lists no parts for its objects: check the log."]
    plural = lambda n, word: f"{n} {word}" + ("" if n == 1 else "s")  # noqa: E731
    if len(expected) > 1 and layout == "oneObjectWithParts":
        if len(objects) == 1 and n_parts == len(expected):
            notes.append(f"One object with {n_parts} parts, as Bambu Studio loads it: the part-designer layout holds in OrcaSlicer.")
        elif len(objects) == len(expected):
            notes.append("Split into one object per part, as PrusaSlicer does: the part-designer layout does not hold here.")
        else:
            notes.append(f"Unexpected: {plural(len(objects), 'object')} with {plural(n_parts, 'part')} for {len(expected)} colour roles.")
        extruders = [p["extruder"] for o in objects for p in o["parts"]]
        has_colours = any(e.get("colour") for e in expected)
        if len(set(map(str, extruders))) == 1:
            if has_colours:
                notes.append(
                    f"Every part prints with filament {extruders[0]}: the colour group did not reach the filaments, so the user picks a filament per part (Q18 recommendation 2)."
                )
            else:
                notes.append(f"No colour group in the file; every part prints with filament {extruders[0]} until the user picks one per part.")
        else:
            notes.append("The parts got different filaments: " + ", ".join(f"{p['name'] or '?'} {p['extruder']}" for o in objects for p in o["parts"]) + ".")
    else:
        notes.append(f"{plural(len(objects), 'object')} with {plural(n_parts, 'part')}: the single-colour baseline.")
    used = [f for plate in project.get("plates") or [] for f in plate["filaments"]]
    if used:
        notes.append("Sliced with filament(s) " + ", ".join(f"{f['id']} ({f['usedG']} g)" for f in used) + ".")
    return notes


def report(args):
    work = args.work
    inputs = os.path.join(work, "inputs")
    names = [n for n in INPUT_ORDER if os.path.exists(os.path.join(inputs, n + ".3mf"))]
    extra = sorted(f[:-4] for f in os.listdir(inputs) if f.endswith(".3mf") and f[:-4] not in INPUT_ORDER) if os.path.isdir(inputs) else []
    names += extra
    results = []
    failed = 0
    for name in names:
        description = read_json(os.path.join(inputs, name + ".json"))
        layout, expected = expected_parts(description)
        out_dir = os.path.join(work, "sliced", name)
        entry = {
            "file": name + ".3mf",
            "description": DESCRIPTIONS.get(name, ""),
            "layout": layout,
            "expectedParts": expected,
        }
        if args.dry_run:
            entry.update({"status": "not run (dry run)", "sliced": None, "notes": ["Dry run: the input exists; OrcaSlicer was not downloaded or run."]})
            results.append(entry)
            continue
        code_text = read_text(os.path.join(out_dir, "exit-code"))
        if code_text is None:
            # Not sliced yet (a partial report during the run) or never (the
            # run stopped before this file): counted as a failure.
            failed += 1
            entry.update({"status": "not run", "sliced": None, "notes": ["Not sliced: the run had not reached this file, or stopped before it (see the job log)."]})
            results.append(entry)
            continue
        exit_code = int(code_text.strip()) if code_text and code_text.strip().lstrip("-").isdigit() else None
        result = read_json(os.path.join(out_dir, "result.json")) or {}
        project_path = os.path.join(out_dir, name + ".sliced.3mf")
        project = read_project(project_path) if os.path.exists(project_path) else {"objects": None, "plates": None, "gcodeToolChanges": None, "problems": ["no exported project"]}
        return_code = result.get("return_code")
        sliced = exit_code == 0 and return_code in (0, None) and os.path.exists(project_path)
        if not sliced:
            failed += 1
        entry.update(
            {
                "status": "sliced" if sliced else "failed",
                "sliced": sliced,
                "exitCode": exit_code,
                "returnCode": return_code,
                "errorString": result.get("error_string"),
                "objects": project["objects"],
                "plates": project["plates"],
                "gcodeTools": project["gcodeToolChanges"],
                "problems": project["problems"],
                "notes": interpret(layout, expected, sliced, project),
            }
        )
        results.append(entry)

    meta = {"orcaSlicer": args.orca, "appImage": args.appimage, "sha256": args.sha256, "dryRun": args.dry_run}
    with open(os.path.join(work, "results.json"), "w", encoding="utf-8") as f:
        json.dump({"run": meta, "results": results}, f, indent=2)
        f.write("\n")
    md = render_markdown(meta, results, read_text(os.path.join(work, "profiles", "settings.txt")))
    with open(os.path.join(work, "summary.md"), "w", encoding="utf-8") as f:
        f.write(md)
    sys.stdout.write(md)
    if not names:
        print("slicer-smoke: no input files in " + inputs, file=sys.stderr)
        return 1
    return 1 if failed and not args.dry_run else 0


def cell(text):
    return str(text).replace("|", "\\|").replace("\n", " ")


def parts_cell(entry):
    objects = entry.get("objects")
    if not objects:
        return "–"
    rows = []
    for o in objects:
        parts = ", ".join(f"{p['name'] or '?'} → filament {p['extruder']}" for p in o["parts"]) or "no parts"
        rows.append(f"**{o['name'] or '?'}**: {parts}")
    return "<br>".join(rows)


def render_markdown(meta, results, settings):
    lines = []
    title = "OrcaSlicer slice smoke test" + (" (dry run)" if meta["dryRun"] else "")
    lines.append(f"## {title}")
    lines.append("")
    if meta["dryRun"]:
        lines.append("Dry run: the inputs were built and checked; OrcaSlicer was not downloaded or run.")
    else:
        lines.append(
            f"OrcaSlicer {meta['orcaSlicer'] or '?'} (`{meta['appImage'] or '?'}`, SHA-256 `{meta['sha256'] or '?'}`), run from its extracted AppImage as a separate program under Xvfb. "
            "Backlog item 18; what each result means for Q18 is in the last column ([Q18 report](https://github.com/NeuralDEEPnet/openBrick/blob/main/docs/spikes/q18-slicer-multimaterial-import.md))."
        )
    if settings:
        lines.append("")
        lines.append(settings.strip())
    lines.append("")
    lines.append("| File | What it is | Sliced | Objects and parts (filament) | What it means for Q18 |")
    lines.append("|------|------------|--------|------------------------------|-----------------------|")
    for r in results:
        sliced = {True: "yes", False: "**no**", None: "not run"}[r.get("sliced")]
        if r.get("sliced") is False:
            detail = r.get("errorString") or ""
            sliced += f" (exit {r.get('exitCode')}, return code {r.get('returnCode')}{': ' + detail if detail else ''})"
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{cell(r['file'])}`",
                    cell(r.get("description", "")),
                    cell(sliced),
                    parts_cell(r),
                    cell(" ".join(r.get("notes", []))),
                ]
            )
            + " |"
        )
    lines.append("")
    expected = [r for r in results if len(r.get("expectedParts") or []) > 1]
    if expected:
        lines.append("Parts the openbrick CLI wrote (its `--json`), in file order:")
        lines.append("")
        for r in expected:
            parts = ", ".join(f"{p['name']} ({p['role']}{', ' + p['colour'] if p.get('colour') else ', no colour'})" for p in r["expectedParts"])
            lines.append(f"- `{r['file']}` ({r.get('layout')}): {parts}")
        lines.append("")
    pair = {r["file"]: r for r in results}
    a, b = pair.get("logo-tile-inlay-parts.3mf"), pair.get("logo-tile-inlay-parts-no-colour.3mf")
    has_parts = lambda r: r and r.get("sliced") and any(o["parts"] for o in r.get("objects") or [])  # noqa: E731
    if has_parts(a) and has_parts(b):
        same = [[p["extruder"] for o in a["objects"] for p in o["parts"]], len(a["objects"])] == [
            [p["extruder"] for o in b["objects"] for p in o["parts"]],
            len(b["objects"]),
        ]
        lines.append(
            "Colour group or not: "
            + (
                "the inlay with colours and the one without load the same way (same objects, same filaments), so OrcaSlicer's command line ignores the colours."
                if same
                else "the inlay with colours and the one without load differently, so OrcaSlicer's command line reads the colours: compare their rows."
            )
        )
        lines.append("")
    problems = [(r["file"], p) for r in results for p in r.get("problems") or []]
    for f, p in problems:
        lines.append(f"- `{f}`: {p}")
    if problems:
        lines.append("")
    lines.append("The inputs, OrcaSlicer's projects (`*.sliced.3mf`, with the G-code), its logs and `results.json` are in the `slicer-smoke` artifact.")
    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# self-test


def write_json(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f)


def write_project(path, model_settings, slice_info, gcode):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("3D/3dmodel.model", "<model/>")
        if model_settings is not None:
            z.writestr("Metadata/model_settings.config", model_settings)
        if slice_info is not None:
            z.writestr("Metadata/slice_info.config", slice_info)
        if gcode is not None:
            z.writestr("Metadata/plate_1.gcode", gcode)


def cli_description(objects, layout):
    return {"outputs": [{"layout": layout, "objects": objects}]}


def self_test():
    failures = []

    def check(cond, what):
        if not cond:
            failures.append(what)

    with tempfile.TemporaryDirectory() as tmp:
        # flatten: a three-level machine chain, a process chain, a filament
        # whose parent lives in another vendor.
        prof = os.path.join(tmp, "profiles")
        write_json(
            os.path.join(prof, "BBL.json"),
            {
                "machine_list": [
                    {"name": "base", "sub_path": "machine/base.json"},
                    {"name": "mid", "sub_path": "machine/mid.json"},
                    {"name": "A1", "sub_path": "machine/A1.json"},
                ],
                "process_list": [{"name": "P", "sub_path": "process/P.json"}],
                "filament_list": [{"name": "PLA @A1", "sub_path": "filament/PLA.json"}],
            },
        )
        write_json(os.path.join(prof, "Lib.json"), {"filament_list": [{"name": "PLA base", "sub_path": "filament/base.json"}], "process_list": [{"name": "Pbase", "sub_path": "process/base.json"}]})
        write_json(os.path.join(prof, "BBL/machine/base.json"), {"name": "base", "type": "machine", "from": "system", "instantiation": "false", "printer_technology": "FFF", "a": "base", "b": "base"})
        write_json(os.path.join(prof, "BBL/machine/mid.json"), {"name": "mid", "inherits": "base", "from": "system", "b": "mid", "c": "mid"})
        write_json(os.path.join(prof, "BBL/machine/A1.json"), {"name": "A1", "inherits": "mid", "from": "system", "instantiation": "true", "c": "A1"})
        write_json(os.path.join(prof, "BBL/process/P.json"), {"name": "P", "inherits": "Pbase", "from": "system", "layer_height": "0.2"})
        write_json(os.path.join(prof, "Lib/process/base.json"), {"name": "Pbase", "from": "system", "wall_loops": "2"})
        write_json(os.path.join(prof, "BBL/filament/PLA.json"), {"name": "PLA @A1", "inherits": "PLA base", "from": "system", "filament_id": "GFA00"})
        write_json(os.path.join(prof, "Lib/filament/base.json"), {"name": "PLA base", "from": "system", "filament_type": ["PLA"], "filament_colour": ["#000000"]})
        out = os.path.join(tmp, "flat")
        ns = argparse.Namespace(profiles=prof, vendor="BBL", machine="A1", process="P", filament="PLA @A1", colours="#E6E6E6,#1E5AA8", out=out)
        flatten(ns)
        with open(os.path.join(out, "machine.json")) as f:
            m = json.load(f)
        check(m.get("a") == "base" and m.get("b") == "mid" and m.get("c") == "A1", f"machine chain merged parents first: {m}")
        check("inherits" not in m and m["name"] == "A1" and m["from"] == "system" and m["type"] == "machine", f"machine identity: {m}")
        check(m.get("printer_technology") == "FFF", "the root's printer_technology survives")
        with open(os.path.join(out, "process.json")) as f:
            p = json.load(f)
        check(p.get("wall_loops") == "2" and p.get("layer_height") == "0.2" and p["type"] == "process", f"process from another vendor's parent: {p}")
        with open(os.path.join(out, "filament-2.json")) as f:
            fl = json.load(f)
        check(fl.get("filament_colour") == ["#1E5AA8"] and fl.get("filament_type") == ["PLA"] and fl.get("filament_id") == "GFA00", f"filament 2: {fl}")
        check(os.path.exists(os.path.join(out, "filament-1.json")) and not os.path.exists(os.path.join(out, "filament-3.json")), "one filament file per colour")
        loop = os.path.join(tmp, "loop")
        write_json(os.path.join(loop, "V.json"), {"machine_list": [{"name": "x", "sub_path": "x.json"}, {"name": "y", "sub_path": "y.json"}]})
        write_json(os.path.join(loop, "V/x.json"), {"name": "x", "inherits": "y"})
        write_json(os.path.join(loop, "V/y.json"), {"name": "y", "inherits": "x"})
        try:
            resolve(load_index(loop), "V", "machine", "x")
            check(False, "an inherits loop is refused")
        except SystemExit as e:
            check("loop" in str(e), f"loop message: {e}")

        # report: one file per outcome.
        work = os.path.join(tmp, "work")
        two = [{"name": "t body", "role": "body", "color": "#E6E6E6FF"}, {"name": "t accent", "role": "accent", "color": "#1E5AA8FF"}]
        two_plain = [{"name": "t body", "role": "body", "color": None}, {"name": "t accent", "role": "accent", "color": None}]
        one = [{"name": "coupon", "role": "body", "color": "#E6E6E6FF"}]
        settings_one_object = (
            '<?xml version="1.0"?><config><object id="2"><metadata key="name" value="Logo tile"/><metadata key="extruder" value="1"/>'
            '<part id="1" subtype="normal_part"><metadata key="name" value="t body"/></part>'
            '<part id="2" subtype="normal_part"><metadata key="name" value="t accent"/></part></object><plate/></config>'
        )
        settings_mapped = settings_one_object.replace('value="t accent"/>', 'value="t accent"/><metadata key="extruder" value="2"/>')
        settings_split = (
            '<config><object id="1"><metadata key="name" value="t body"/><metadata key="extruder" value="1"/><part id="1" subtype="normal_part"/></object>'
            '<object id="2"><metadata key="name" value="t accent"/><metadata key="extruder" value="1"/><part id="2" subtype="normal_part"/></object></config>'
        )
        info = '<config><plate><metadata key="index" value="1"/><object identify_id="9" name="Logo tile" skipped="false"/><filament id="1" type="PLA" color="#E6E6E6" used_m="0.5" used_g="1.5"/></plate></config>'
        cases = {
            "logo-tile-emboss": (two, "oneObjectWithParts", 0, {"return_code": 0}, settings_one_object, info, "G28\nT0 ; tool\nG1 X1\n"),
            "logo-tile-inlay-parts": (two, "oneObjectWithParts", 0, {"return_code": 0}, settings_mapped, info, "T0\nT1\n"),
            "logo-tile-inlay-parts-no-colour": (two_plain, "oneObjectWithParts", 0, {"return_code": 0}, settings_split, info, None),
            "brand-tile": (two, "oneObjectWithParts", 156, {"return_code": -100, "error_string": "Slicing error"}, None, None, None),
            "coupon-fdm-0.4-cross": (one, "separateObjects", 0, None, '<config><object id="1"><metadata key="name" value="coupon"/><metadata key="extruder" value="2"/><part id="1" subtype="normal_part"/></object></config>', info, None),
        }
        for name, (objs, layout, code, result, ms, si, gcode) in cases.items():
            write_json(os.path.join(work, "inputs", name + ".json"), cli_description(objs, layout))
            with open(os.path.join(work, "inputs", name + ".3mf"), "wb") as f:
                f.write(b"PK\x03\x04")
            d = os.path.join(work, "sliced", name)
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, "exit-code"), "w") as f:
                f.write(f"{code}\n")
            if result is not None:
                write_json(os.path.join(d, "result.json"), result)
            if ms is not None:
                write_project(os.path.join(d, name + ".sliced.3mf"), ms, si, gcode)
        ns = argparse.Namespace(work=work, dry_run=False, orca="2.4.2", appimage="Orca.AppImage", sha256="00")
        devnull = open(os.devnull, "w")
        saved, sys.stdout = sys.stdout, devnull
        try:
            code = report(ns)
        finally:
            sys.stdout = saved
            devnull.close()
        check(code == 1, f"a file that did not slice fails the run (exit {code})")
        with open(os.path.join(work, "results.json")) as f:
            res = {r["file"]: r for r in json.load(f)["results"]}
        check([r for r in res] == [n + ".3mf" for n in INPUT_ORDER], f"results in input order: {list(res)}")
        emboss = res["logo-tile-emboss.3mf"]
        check(emboss["sliced"] and len(emboss["objects"]) == 1 and [p["extruder"] for p in emboss["objects"][0]["parts"]] == [1, 1], f"emboss: {emboss}")
        check(emboss["objects"][0]["parts"][0]["extruderFrom"] == "object", "a part without extruder takes its object's")
        check(emboss["gcodeTools"] == [0], f"tools from the G-code: {emboss['gcodeTools']}")
        check(any("layout holds" in n for n in emboss["notes"]) and any("did not reach the filaments" in n for n in emboss["notes"]), f"emboss notes: {emboss['notes']}")
        inlay = res["logo-tile-inlay-parts.3mf"]
        check([p["extruder"] for p in inlay["objects"][0]["parts"]] == [1, 2], f"inlay extruders: {inlay['objects']}")
        check(any("different filaments" in n for n in inlay["notes"]), f"inlay notes: {inlay['notes']}")
        plain = res["logo-tile-inlay-parts-no-colour.3mf"]
        check(any("Split into one object per part" in n for n in plain["notes"]) and any("No colour group" in n for n in plain["notes"]), f"split notes: {plain['notes']}")
        check(plain["plates"][0]["filaments"][0]["usedG"] == "1.5", f"slice info: {plain['plates']}")
        brand = res["brand-tile.3mf"]
        check(brand["sliced"] is False and brand["returnCode"] == -100 and brand["exitCode"] == 156, f"brand failure: {brand}")
        coupon = res["coupon-fdm-0.4-cross.3mf"]
        check(coupon["sliced"] and any("baseline" in n for n in coupon["notes"]), f"coupon: {coupon}")
        check([p["extruder"] for p in coupon["objects"][0]["parts"]] == [2], f"a part inherits its object's extruder: {coupon['objects']}")
        check(coupon["returnCode"] is None, "no result.json: no return code, judged by the exit code and the project")
        with open(os.path.join(work, "summary.md")) as f:
            md = f.read()
        check("| `brand-tile.3mf` |" in md and "**no** (exit 156, return code -100: Slicing error)" in md, "the summary names the failure")
        check("load differently" in md, "the colour pair comparison is in the summary")
        check("**Logo tile**: t body → filament 1, t accent → filament 2" in md, "parts and filaments in the summary")

        # A run stopped before the last file (the partial report after each
        # slice, or a step timeout): that file is "not run" and still fails.
        os.remove(os.path.join(work, "sliced", "coupon-fdm-0.4-cross", "exit-code"))
        write_json(os.path.join(work, "sliced", "brand-tile", "result.json"), {"return_code": 0})
        with open(os.path.join(work, "sliced", "brand-tile", "exit-code"), "w") as f:
            f.write("0\n")
        write_project(os.path.join(work, "sliced", "brand-tile", "brand-tile.sliced.3mf"), settings_one_object, info, None)
        saved, sys.stdout = sys.stdout, open(os.devnull, "w")
        try:
            code = report(ns)
        finally:
            sys.stdout.close()
            sys.stdout = saved
        check(code == 1, f"a file that was not sliced fails the run (exit {code})")
        with open(os.path.join(work, "results.json")) as f:
            res = {r["file"]: r for r in json.load(f)["results"]}
        coupon = res["coupon-fdm-0.4-cross.3mf"]
        check(coupon["sliced"] is None and coupon["status"] == "not run", f"an unsliced file: {coupon}")
        check(res["brand-tile.3mf"]["sliced"] is True, "the other files keep their results")
        with open(os.path.join(work, "summary.md")) as f:
            check("| `coupon-fdm-0.4-cross.3mf` | " in f.read(), "the unsliced file is in the summary")

        # report --dry-run: inputs only.
        ns.dry_run = True
        saved, sys.stdout = sys.stdout, open(os.devnull, "w")
        try:
            code = report(ns)
        finally:
            sys.stdout.close()
            sys.stdout = saved
        check(code == 0, "a dry run passes")
        with open(os.path.join(work, "summary.md")) as f:
            check("(dry run)" in f.read(), "the dry-run summary says so")

    if failures:
        for f in failures:
            print("self-test FAILED: " + f, file=sys.stderr)
        return 1
    print("slicer-smoke self-test: ok")
    return 0


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    sub = parser.add_subparsers(dest="command", required=True)
    f = sub.add_parser("flatten")
    f.add_argument("--profiles", required=True)
    f.add_argument("--vendor", required=True)
    f.add_argument("--machine", required=True)
    f.add_argument("--process", required=True)
    f.add_argument("--filament", required=True)
    f.add_argument("--colours", default="#E6E6E6,#1E5AA8")
    f.add_argument("--out", required=True)
    r = sub.add_parser("report")
    r.add_argument("--work", required=True)
    r.add_argument("--dry-run", action="store_true")
    r.add_argument("--orca", default="")
    r.add_argument("--appimage", default="")
    r.add_argument("--sha256", default="")
    sub.add_parser("self-test")
    args = parser.parse_args(argv)
    if args.command == "flatten":
        return flatten(args)
    if args.command == "report":
        return report(args)
    return self_test()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
