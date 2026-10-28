# Part designer and the `openbrick/part@1` document

**Purpose.** The part definition format, its feature vocabulary and typed parameters, how artwork and text become printable relief, how connectivity is derived so designed parts snap in the assembler and in LDCad, sandboxing of scripted parts, versioning, the desktop and mobile designer UI, batch merchandise runs, and the validation feedback loop. Dimensions come from [04](04-brick-geometry-spec.md); print rules from [05](05-3d-printing-pipeline.md).

## Contents

1. Three layers: document, scripts, evaluator
2. Document structure
3. Parameters
4. Feature vocabulary
5. Artwork: SVG, raster, text
6. Connectivity derivation and LDraw export
7. Worked example: 2x4 logo tile
8. Worked example: nameplate keychain
9. Scripted parts and sandboxing
10. Versioning and hashing
11. Designer UI on desktop and mobile
12. Batch merchandise runs
13. Validation feedback loop
14. OpenSCAD customizer import

## 1. Three layers

1. **Document** (`openbrick/part@1`, JSON): the canonical, shareable artefact. Declarative, hashable, sandbox-safe.
2. **Scripts** (M5): a TypeScript API `@openbrick/part` whose only job is to emit a document; runs in an Extism (QuickJS-in-WASM) sandbox with no network or filesystem, fuel and memory caps, output cached by content hash. Official generators may be Rust via the Extism Rust PDK. Scripts never touch the kernel.
3. **Evaluator** (Rust, `openbrick-part`, also compiled to WASM): turns features into Manifold geometry and a connector list, applying the `FitProfile` as 2D offsets before extrusion or revolve.

We decided not to build an OpenSCAD-like language (D04). The brick vocabulary is small and closed; merchandise needs reproducibility more than freedom; and OpenSCAD files are supported by an importer.

## 2. Document structure

```json
{
  "openbrick": "1.0",
  "id": "ndn.logo-tile",
  "version": "1.2.0",
  "name": "Logo tile",
  "license": "CC0-1.0",
  "credit": { "author": "Neural Deep Network", "url": "https://openbrick3d.org" },
  "provenance": { "source": "original", "modified": false, "history": [] },
  "params": { },
  "features": [ ],
  "snap": "derived",
  "print": { },
  "export": { }
}
```

- `meta` fields: `id` (reverse-DNS-ish, lower case), semver `version`, SPDX `license`, mandatory `credit`, `provenance` (so LDraw attribution travels with derived parts; see [12](12-legal-and-naming.md)).
- `params`: typed, UI-bearing parameters (section 3).
- `features`: an ordered list of brick-aware operations (section 4). Each has an `id`, an `op`, an `on` anchor (a face of a previous feature), an optional `if` expression (the feature is skipped when false), an optional `colorRole`, and op-specific fields. Values may be literals with unit suffixes (`"0.6mm"`, `"2stud"`, `"1plate"`), references (`"$size"`), or small expressions (`"$length - 1"`).
- `snap`: `"derived"` (default) or an explicit connector list for exotic parts.
- `print`: which face is up per mode, allowed process profiles, fit-critical feature ids, colour roles, and `rules` (extra checks with an `onFail` action such as `shrink-or-bolder`).
- `export`: formats and batch naming.

## 3. Parameters

Modelled on Onshape FeatureScript, not on OpenSCAD comment annotations.

| Field | Meaning |
|-------|---------|
| `type` | `int`, `length`, `angle`, `bool`, `string`, `enum`, `grid2`, `svg`, `image`, `font`, `color`, `list` |
| `unit` | `stud`, `plate`, `ldu`, `mm`, `deg` |
| `default`, `min`, `max`, `step` | bounds enforced by the UI and the evaluator |
| `options` | enum values with labels |
| `group` | panel tab |
| `visibleWhen` | expression over other params, e.g. `"relief != 'inlay'"` |
| `help` | one-line hint |
| `manipulator` | in-viewport drag handle: `x`, `y`, `z`, `corner`, grid-snapped |
| `onChange` | declarative editing logic, e.g. clamp `margin` when `size` changes |

Presets: a `presets` array of named param sets, importable from OpenSCAD `parameterSets` JSON.

All layout positions are integer LDU internally; `stud` and `plate` units convert exactly (20 and 8 LDU); `mm` values are used only for artwork, relief heights and off-grid decorations. There is no general constraint solver: snap to grid, optional half-stud and half-plate offsets, and alignment to another feature cover the brick domain.

## 4. Feature vocabulary

| op | Produces | Key fields | Connector contribution |
|----|----------|------------|------------------------|
| `body.tile` / `body.plate` / `body.brick` | prismatic body with 0.1 mm per side undersize, bottom chamfer, optional top fillet tier | `size` (grid2), `height` (`plate`, `brick`, or `n plate`) | none |
| `body.baseplate` | thin plate, no tubes | `size`, `thickness` | none |
| `stud.field` | studs on a face: solid, hollow, open | `on`, `grid` (`auto` or explicit), `studType`, `logo` (never a trademark) | male R studs |
| `tube.field` | underside tubes, 1-wide pins, ribs | `on`, `grid`, `fitCritical` | female R sockets |
| `technic.hole` | through hole with counterbores at 5.8 mm axis | `on`, `positions`, `count` | female R 20 |
| `axle.hole` / `axle` | cross hole / cross shaft | `length` | female / male A |
| `pin` | Technic pin shaft with collar | `length`, `friction` | male R |
| `bar` / `clip` | Ø3.18 bar / clip for a bar | `length`; `open` direction | male R 4 / CLP |
| `hinge.finger` | click-hinge fingers | `count`, `side` | FGR |
| `ball` / `socket` | 5.9 or 10.2 mm | `size` | GEN |
| `relief.svg` | emboss, deboss or inlay of an SVG on a face | `src`, `mode`, `height`, `fit`, `minFeature`, `colorRole` | none |
| `relief.text` | shaped text on a face | `text`, `font`, `mode`, `height`, `fit`, `minStroke`, `colorRole` | none |
| `relief.image` | traced raster (vtracer) | `src`, `threshold`, `posterize` | none |
| `mosaic` | n x m 1x1 tiles coloured from an image, or a single plate with coloured studs | `size`, `palette`, `dither` | studs/tubes as per base |
| `loop.keyring` | ring fused to a side with chamfer underneath | `attachTo`, `side`, `holeDiameter`, `wall`, `thickness`, `fillet`, `offGrid: true` | none |
| `cut.svg` / `cut.shape` | subtract an outline through the body | `src`, `depth` | none |
| `round` | rounding tier: `chamfer`, `smooth`, `minkowski`, `sdf` | `radius`, `tier` | none |
| `adapter.*` | bridges to other systems (e.g. 8 mm stud field on one face, Technic holes on another) | per adapter | as per faces |

Every feature knows its LDU footprint, which faces may touch the bed, which fields are fit-critical, and the SNAP sections it emits.

## 5. Artwork: SVG, raster, text

**SVG:** parse with usvg (0.49), flatten curves at 0.05 mm tolerance, honour the file's fill rule, build a Manifold `CrossSection` (union-on-construct removes self-intersections), apply a **morphological opening** (inset by minFeature/2 then outset by the same) to drop hairlines the process cannot print, report the removed-area percentage and show removed regions in red in the preview, fit into the face box with margin, extrude by relief height, union (emboss) or subtract (deboss). Strokes are converted to outlines in v1.1 (lyon or Clipper2 open-path offset); v1 supports fills.

In M1 the evaluator reads the asset through usvg 0.49 without default features (no fonts, no image decoding, no SVGZ), as untrusted input. Before usvg, roxmltree or the CSS parser does work that could grow with a hostile file, it checks size (4 MiB), XML nodes, attributes per element, `<style>` rules and selector-matching work, `<use>` expansion, and the longest chain of nesting and references. That chain covers `<use>`, `href` and `url(#id)` from attributes and CSS, is limited to 64 levels so usvg's recursion fits a 1 MiB WebAssembly stack, and is refused when it forms a cycle. Entity declarations and `<marker>` are refused, and `<image>` is never resolved. Strokes, text, images, clip paths, masks, filters and invisible shapes are ignored with a warning, and so is paint order (more than one fill paint gets a warning). Before any polygon operation, `openbrick_geom::region::sweep_cost` and `offset_sweep_cost` estimate Clipper2's sweep work, and artwork that would block the evaluator is refused with a request to simplify it. The opening's round joins stay within the 0.05 mm flattening tolerance. There is one `relief.svg` per part, on the top face, never together with studs on that face. Fill rules and 2D booleans go through `openbrick_geom::region` (Clipper2). `Built.artwork` carries the removed regions and the face on the bed. `Built.fused` is the single-colour union for STL and fused 3MF.

**Raster:** PNG or JPG go through threshold or posterize, then vtracer (MIT, WASM) to SVG paths, then the SVG pipeline.

**Text:** shape with harfrust/rustybuzz (Rust) or harfbuzzjs (web), outline with ttf-parser or opentype.js, flatten with kurbo at a tolerance derived from nozzle width, NonZero fill, extrude. Bundled fonts are complete, unmodified OFL files with their licence text in an in-app Fonts screen (subsetting would trigger Reserved Font Name rules): Inter, Noto Sans, Atkinson Hyperlegible, Liberation Sans as the default (OpenSCAD users know it); each weight carries `printSafe` metadata (minimum stroke at 10 mm cap height) so the designer auto-picks a weight that meets `minFeature`. Users may add local fonts; the document stores name and hash only, never the font. Generated 3D text carries no font-licence obligation (OFL FAQ).

**Process minimums (FDM 0.4):** stroke at least 0.8 mm (two lines), relief 0.4 to 1.0 mm (1.2 to 1.5 mm for standalone legibility), bold sans-serif, text on top or bottom faces, never on walls.

**Three relief modes:** emboss (face-up print), deboss (face-up; depth clamped so the cavity keeps 1.8 mm, which means 0.4 mm on plate-height parts), inlay (face-down print; for a single-extruder swap the `accent` object is the face slab minus the logo so the logo shows the body colour; for multi-material units the logo fill is its own object; see [05](05-3d-printing-pipeline.md#6-orientation-supports-hollow-roofs)).

**Trademark guard (M5, after the Q16 spike):** uploaded artwork is checked against a blocklist (LEGO marks, minifigure silhouette) and rejected with an explanation ([12](12-legal-and-naming.md)); until the spike proves a false-positive rate under 1 percent the guard is a warning.

## 6. Connectivity derivation and LDraw export

- Each connector-bearing feature emits typed connectors: `{ type: studM | studF | tubeF | pinM | pinF | axleM | axleF | barM | clipF | hingeFinger | ball | socket, pos: [LDU;3], dir: axis, sections: [R|A|S, radius, length], group }`. In `openbrick_part::Connector` the type is the field `kind`, the group is `feature`, `dir` is the unit vector out of the part (studs (0, -1, 0), underside sockets (0, 1, 0)), and each connector carries `fitCritical`; `stud.field` emits `studM` with `R 6 4` and `tube.field` emits one `tubeF` per stud position with `R 6 8`.
- Features marked `offGrid: true` (keychain loops, decorations) emit none, so the assembler treats them as decoration.
- On LDraw export the part is written as an unofficial `.dat` (triangles and quads in LDU, BFC CCW, `0 !LICENSE` line, Unofficial header) with inline `0 !LDCAD SNAP_*` metas generated from the connectors, for example a 2x4 tile underside: `0 !LDCAD SNAP_CYL [gender=F] [caps=one] [secs=R 6 8] [grid=C 2 C 4 20 20] [pos=0 0 0] [ori=1 0 0 0 1 0 0 0 1]` (illustrative). The part snaps in LDCad immediately and renders in three.js LDrawLoader.
- We do not target Studio's binary `.conn` (no community parser as of Feb 2026) and never ship LDCad's shadow library.

## 7. Worked example: 2x4 logo tile

```json
{
  "openbrick": "1.0",
  "id": "ndn.logo-tile", "version": "1.2.0", "name": "Logo tile", "license": "CC0-1.0",
  "credit": { "author": "Neural Deep Network" },
  "params": {
    "size":   { "type": "grid2", "unit": "stud", "default": [2,4], "min": [1,1], "max": [16,32], "group": "Size", "manipulator": "corner" },
    "height": { "type": "enum", "default": "plate", "options": [ {"value":"plate","label":"Plate (1/3)"}, {"value":"brick","label":"Brick"} ], "group": "Size" },
    "logo":   { "type": "svg", "default": "assets/mark.svg", "group": "Artwork", "help": "Single-colour SVG; lines under 0.8 mm are removed" },
    "relief": { "type": "enum", "default": "emboss", "options": ["emboss","deboss","inlay"], "group": "Artwork" },
    "reliefHeight": { "type": "length", "unit": "mm", "default": 0.6, "min": 0.2, "max": 1.5, "step": 0.1, "visibleWhen": "relief != 'inlay'" },
    "margin": { "type": "length", "unit": "mm", "default": 1.0, "min": 0.4, "max": 4 }
  },
  "features": [
    { "id": "body",   "op": "body.tile",  "size": "$size", "height": "$height", "round": { "tier": "chamfer", "radius": "0.3mm" } },
    { "id": "bottom", "op": "tube.field", "on": "body.bottom", "grid": "auto", "fitCritical": true },
    { "id": "art",    "op": "relief.svg", "on": "body.top", "src": "$logo", "mode": "$relief", "height": "$reliefHeight",
      "fit": { "margin": "$margin", "align": "center", "keepAspect": true }, "minFeature": "profile.minFeature", "colorRole": "accent" }
  ],
  "snap": "derived",
  "print": { "up": { "emboss": "top", "deboss": "top", "inlay": "bottom" }, "profiles": ["fdm-0.4", "fdm-0.2", "resin-msla"], "fitCritical": ["bottom"] },
  "export": { "formats": ["3mf", "stl", "ldr"], "batch": { "filename": "{id}-{size[0]}x{size[1]}-{serial:04}", "serial": { "mode": "deboss", "on": "body.bottom", "height": "0.4mm" } } }
}
```

Evaluation at 2x4 with `fdm-0.4`: body 15.8 x 31.8 x 3.2 mm (0.1 mm undersize per side), three tubes OD 6.514 / ID 4.8 at nominal (Ø3.18 pins appear only on 1-wide parts), then the fit profile's offsets applied as 2D offsets (bore, tube OD and rib depth from the pocket row); SVG opened at 0.8 mm and extruded 0.6 mm; in deboss mode the roof thickens under the artwork (the whole cavity ceiling moves down by the depth) and the evaluator clamps depth to 0.4 mm so 1.8 mm of cavity remains; derived snap: eight female sockets on the bottom; no male connectors (tile).

## 8. Worked example: nameplate keychain

```json
{
  "openbrick": "1.0",
  "id": "ndn.nameplate-keychain", "version": "1.0.0", "name": "Nameplate keychain", "license": "CC0-1.0",
  "credit": { "author": "Neural Deep Network" },
  "params": {
    "text":   { "type": "string", "default": "OPENBRICK3D", "maxLength": 14, "group": "Text" },
    "font":   { "type": "font", "default": "Inter:Bold", "bundledOnly": true, "group": "Text" },
    "length": { "type": "int", "unit": "stud", "default": 8, "min": 4, "max": 16, "group": "Size", "manipulator": "x" },
    "width":  { "type": "int", "unit": "stud", "default": 2, "options": [1, 2], "group": "Size" },
    "studsOnTop": { "type": "bool", "default": false, "group": "Size" },
    "loop":   { "type": "enum", "default": "left", "options": ["none","left","right"], "group": "Keychain" },
    "ringHole": { "type": "length", "unit": "mm", "default": 5.0, "min": 3, "max": 8, "step": 0.5, "visibleWhen": "loop != 'none'" }
  },
  "features": [
    { "id": "body",   "op": "body.plate", "size": ["$width", "$length"] },
    { "id": "studs",  "op": "stud.field", "on": "body.top", "if": "$studsOnTop", "studType": "solid" },
    { "id": "bottom", "op": "tube.field", "on": "body.bottom", "fitCritical": true },
    { "id": "label",  "op": "relief.text", "on": "body.top", "if": "!$studsOnTop", "text": "$text", "font": "$font", "mode": "emboss", "height": "0.6mm",
      "fit": { "box": "face", "margin": "1.2mm", "shrinkToFit": true, "minCapHeight": "4mm" }, "minStroke": "profile.minFeature", "colorRole": "accent" },
    { "id": "ring",   "op": "loop.keyring", "if": "$loop != 'none'", "attachTo": "body", "side": "$loop", "holeDiameter": "$ringHole", "wall": "2.4mm", "thickness": "plate", "fillet": "1mm", "offGrid": true }
  ],
  "snap": "derived",
  "print": { "up": "top", "profiles": ["fdm-0.4", "fdm-0.2"], "fitCritical": ["bottom"],
             "rules": [ { "feature": "label", "check": "strokeWidth >= profile.minFeature", "onFail": "shrink-or-bolder" } ] },
  "export": { "formats": ["3mf", "stl", "ldr"], "batch": { "rowsFrom": "csv:names.csv", "map": { "text": "name" }, "filename": "{id}-{text|slug}-{serial:03}" } }
}
```

Derived snap: `tube.field` gives a female grid `C 2 C 8 20 20`; `stud.field` (if enabled) gives male `secs=R 6 4` on top; the loop is `offGrid` and contributes nothing. Evaluation: 15.8 x 63.8 x 3.2 mm plate; text shaped with HarfBuzz, opened at 0.8 mm, shrunk until it fits with cap height at least 4 mm; ring = 2.4 mm wall around a 5 mm hole fused to the short side with a 1 mm 2D round.

## 9. Scripted parts and sandboxing (M5)

- Runtime: Extism 1.41 with the JS PDK (QuickJS) in the browser, Node and the Rust host; the same plugin binary runs everywhere.
- Capabilities: none. No network, no filesystem, no clock, no randomness (deterministic output); fuel and memory limits (default 1 s, 64 MB); output must be a valid `openbrick/part@1` document; the host validates and content-hashes it; cached by (script hash, params hash).
- If the community needs per-vertex control, the second step is a Rust or AssemblyScript host ABI, decided after M5 feedback.

## 10. Versioning and hashing

- Document pragma `"openbrick": "1.0"` (whole-document, like KCL's settings pragma); part `version` semver; `contentHash` of the normalised document; `generatorVersion` and kernel version written into every export.
- The feature IR is append-only within a major version; explicit migration functions per schema bump.
- A `lock` block records font hashes, process profile id and kernel version for reproducibility.
- Export filenames: `{partId}-v{version}-{paramsHash8}.3mf`.

## 11. Designer UI on desktop and mobile

| Capability | Desktop | Phone / tablet |
|------------|---------|----------------|
| Typed param panel with groups, presets | full | full, bottom sheet |
| Viewport with manipulators | drag handles on grid | tap-select, slider handles; no free drag of handles |
| Artwork import | file dialog, drag-drop, paste SVG | share sheet, photo library (raster traced on device) |
| Live preview | continuous | debounced; coarse tessellation until idle |
| Validation panel | inline, blockers highlighted on the mesh | same |
| Rounding tier 3 (Minkowski) | background job | not offered |
| Export | 3MF, STL, LDraw, batch | 3MF and STL via share sheet; batch via desktop or CLI |
| Calibration wizard | full | full (tap the coupon cells) |

## 12. Batch merchandise runs

See [05](05-3d-printing-pipeline.md#8-batch-print-plates): run document, CSV rows, nesting, serials, manifest. The designer exposes "Export batch" which writes a run document the CLI can repeat.

## 13. Validation feedback loop

Validation runs after every evaluation (debounced). Blockers show in the panel and on the mesh (hatched red faces for thin features, dotted orange for overhangs, always paired with the list entry so colour is never the only cue). Each message: rule id, measured value, threshold, fix hint, and where possible a one-click fix ("make bolder", "raise relief to 0.4 mm", "remove hairlines"). Export is disabled until blockers clear. Fit-critical blockers link to the calibration wizard.

## 14. OpenSCAD customizer import

An importer maps `// [min:step:max]`, `// [a, b, c]`, `/* [Tab] */`, `[Hidden]`, MakerWorld's `// font` annotations and `parameterSets` JSON onto typed params, so LEGO.scad-style files open with a working panel. Geometry is produced by running the OpenSCAD CLI out of process (GPL remains outside our binary) and imported as a custom mesh part with hand-tagged connectors. The importer is a convenience, not a kernel.
