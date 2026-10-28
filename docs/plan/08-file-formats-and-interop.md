# File formats and interoperability

**Purpose.** The native formats (model, part, profiles, runs), their schemas and extensions, the LDraw import and export mapping, the 3MF, STL, glTF, STEP and OBJ exports, sanitising imported meshes, interoperability with the existing tools, and the compatibility and migration policy.

## Contents

1. Native formats
2. `model.json` schema sketch
3. LDraw import and export
4. 3MF
5. STL, OBJ, glTF
6. STEP (v2)
7. Importing user meshes
8. Interop matrix
9. Compatibility and migration policy

## 1. Native formats

| Extension | Content | Container |
|-----------|---------|-----------|
| `.obrick` | assembler document: `model.json`, `parts/*.obpart.json` or `parts/*.glb` for mesh-only custom parts, `thumbnail.png`, optional `journal.jsonl` | zip (deflate), text inside |
| `.obpart.json` | one `openbrick/part@1` document (see [06](06-part-designer-and-dsl.md)); referenced assets (SVG) inline as data URIs or beside it in an `.obpart` zip | JSON or zip |
| `.obprofile.json` | one `ProcessProfile` or `FitProfile` (see [05](05-3d-printing-pipeline.md)) | JSON |
| `.obrun.json` | one batch run | JSON |
| `.obpack` | a packed, versioned LDraw subset plus derived connectors and LODs (the catalog cache) | zip with index |

Every document starts with `"openbrick": "<schema semver>"` and a `kind` (optional on `openbrick/part@1` documents; when present it is `"part"`). JSON Schemas (draft 2020-12) are published in `schemas/`. In CI, `cargo xtask data` validates every data file and the schema fixtures against them, loads each with its Rust reader, and validates what the reader writes back.

## 2. `model.json` schema sketch

```json
{
  "openbrick": "1.0",
  "kind": "model",
  "id": "01J9...ULID",
  "name": "Lighthouse",
  "units": "ldu",
  "frame": "ldraw-y-down",
  "palette": { "4": { "name": "Red", "rgba": "#C91A09FF", "finish": "solid" } },
  "catalog": { "ldraw": "2026-10", "custom": ["sha256:..."] },
  "submodels": [
    { "id": "main", "instances": [
        { "ulid": "01J9...", "part": "3001", "pose": { "pos": [0, 0, 0], "rot": 0 }, "colour": 4, "step": 1 },
        { "ulid": "01J9...", "part": "sha256:...", "pose": { "free": { "t": [..], "r": [..], "s": [1,1,1] } }, "colour": 14, "step": 2 }
    ] }
  ],
  "steps": [ { "index": 1, "rotstep": null }, { "index": 2, "rotstep": [0, 45, 0] } ],
  "groups": [ { "name": "tower", "members": ["01J9..."] } ],
  "provenance": { "created": "2026-11-01T10:00:00Z", "generator": "openbrick3d 0.3.0" }
}
```

Instances are keyed by ULID; `part` is an LDraw id or a content hash of a custom part; lattice poses are integer LDU plus a rotation index 0 to 23; free poses are explicit. Colours are LDraw codes with a palette table for display.

## 3. LDraw import and export

| Aspect | Mapping |
|--------|---------|
| Units | 1 LDU = 0.4 mm; internal LDU, no conversion |
| Frame | LDraw right-handed, minus-Y up; identical internally; flip at glTF/3MF export |
| Type-1 lines | `1 <colour> x y z a b c d e f g h i <part>`: lattice pose becomes integer x y z and one of 24 exact matrices; free pose writes the float matrix |
| Steps | `0 STEP`, `0 ROTSTEP x y z [ABS|REL|ADD]` |
| Submodels | MPD `0 FILE` / `0 NOFILE`; custom parts embedded as `0 FILE name.dat` blocks with `!LDCAD SNAP_*` metas; textures and binary assets via `0 !DATA` base64 |
| Colours | `LDConfig.ldr` codes; unknown codes map to the nearest palette entry and are reported |
| Mirrored placements | negative-determinant matrices supported on import; the renderer uses pre-baked mirrored geometry; the assembler warns that mirrored placement is not physically buildable for asymmetric parts |
| Lossless guarantee | export then import of lattice poses reproduces the document bit for bit; a CI round-trip test enforces it on a corpus |
| Library | downloaded at first run from ldraw.org with checksum; headers and `!LICENSE` lines preserved; attribution screen lists "The LDraw Parts Library" with link; derivative caches carry the attribution |
| Parser | our own, from the 1.0.2 spec; no LeoCAD, LDView or LPub3D code (GPL) |
| Decals | `!TEXMAP` parsed and stored; rendered in v1.x |

## 4. 3MF

- Core 1.4.0 (3MF is ISO/IEC 25422:2025), Materials and Properties 1.2.1 (`basematerials` and `colorgroup`, per-triangle `p1 p2 p3`), Production 1.1.2 (text v1.2; UUIDs, optional `path` split). Units millimetres, Z-up.
- Part designer: one object per colour role. Assembler: one object per part under a component object by default; `--fuse` unions same-colour groups.
- Metadata namespace `openbrick:` with part id, version, content hash, fit profile id, generator and serial.
- Writer: `lib3mf-core` 0.4.0 behind a thin trait (fallback `threemf2` 0.4.1 or our own zip+XML); every CI fixture is validated with `lib3mf` 2.5.0 and opened by OrcaSlicer CLI.
- Reader: 3MF import of meshes for custom parts (core and materials only).
- Vendor project flavours (Bambu, Orca) are plugins, off by default, version-pinned.

## 5. STL, OBJ, glTF

- **STL** binary, millimetres, Z-up, no colour; one file per colour role on request.
- **OBJ** with MTL for colour roles; import supported.
- **glTF 2.0** (`.glb`) with `EXT_mesh_gpu_instancing`: one node per (part, colour) pair carrying instance transforms; a `_COLOR` custom attribute as a hint; LODs as separate meshes with `MSFT_lod` optional. Used for viewers, AR hand-off (USDZ conversion on Apple platforms via the OS) and sharing.

## 6. STEP (v2)

The same part document compiles to OCCT 8.0.1 out of process (build123d 0.13.0 in Python for CLI and CI; cadrum 0.9.1 for a Rust binary) producing AP214 STEP with true fillets. Never in mobile binaries; never in the default desktop install (optional component or server job). Watch truck-stepio for a permissive Rust writer.

## 7. Importing user meshes

Imported `.stl`, `.obj`, `.gltf/.glb`, `.3mf` (and `.ply`) become custom parts:

1. Parse with fuzzed readers; reject files over a size cap (default 50 MB) and triangle cap (2 M).
2. Normalise units (ask if the bounding box suggests inches or metres), re-orient to the LDraw frame, centre on the top stud plane if studs are detected.
3. Validate with Manifold: watertight, no self-intersections; offer repair (hole fill, flip normals) with a diff preview.
4. Detect connectors: cylinders of Ø4.8 x 1.7 on an 8 mm grid become studs; bores of Ø4.8 become sockets; the user confirms or edits in the connector tagger.
5. Bake LODs, occupancy mask and a collision proxy; content-hash and store in `parts/`.
6. Provenance block: source filename, licence chosen by the user, modified flag.

## 8. Interop matrix

| Tool | Read from it | Write to it | Notes |
|------|--------------|-------------|-------|
| LDraw library | yes (download) | contribute generic parts upstream | CC BY 2.0/4.0 attribution |
| LDCad | `.ldr/.mpd` with `SNAP_*` metas | `.dat` with metas | our parts snap there |
| LeoCAD | `.ldr/.mpd` | `.ldr/.mpd` | GPL app; files only |
| BrickLink Studio | `.ldr/.mpd` export from Studio; `.io` later via a clean-room reader | `.mpd` import into Studio | `.io` is proprietary; no `.conn` |
| LPub3D | n/a | `.mpd` with STEP/ROTSTEP | instructions |
| Blender | `.glb`, `.3mf`; ImportLDraw add-on reads `.ldr` | `.mpd`, `.glb` | GPL add-on, files only |
| Rebrickable | bulk CSV for names, colours, crosswalk | n/a | attribution; no scraping; no AI training |
| Slicers (Orca, Bambu, Prusa, Cura) | n/a | 3MF, STL | hints shown |
| OpenSCAD | `.scad` via CLI out of process | n/a | GPL kept external |

## 9. Compatibility and migration policy

- Schema versions are semver; minor versions are additive and readable by older minors; major versions ship an explicit migration function and a CLI `openbrick migrate`. Readers ignore fields from a newer minor; parameters and features keep them, other objects drop them on write until the Rust structs model them. Each minor's JSON Schema rejects unknown fields in documents of its own minor or older, so a typo fails in CI. In a newer-minor document it checks only the fields it knows (`$defs/atThisMinor` and `$defs/knownFields` in `schemas/`). A new minor gets a new schema `$id` (`.../schemas/1.1/...`).
- Every release can read every earlier schema; writes use the newest.
- Content hashes include the schema version, the kernel version and the normalised document so caches never collide across versions.
- Deprecations are announced one minor ahead and kept readable for two majors.
- The LDraw round-trip and 3MF conformance tests run on a stored corpus of real community files (with their licences) in CI.
