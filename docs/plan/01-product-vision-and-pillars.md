# Product vision and pillars

**Purpose.** This document says why OpenBrick3D exists, who it is for, what each pillar must do, what we will not do in v1, and how we measure success. It also states the community principles that the company asked for: creativity first, long-term thinking, and a review culture that teaches rather than punishes.

## Vision

A brick-compatible design tool that is open, print-true and playful. Anyone can design a part that snaps onto the bricks they already own, print it with real clutch, and build with it on a PC, a phone or from a terminal. The company's merchandise business runs on the same tool the community uses.

## The three pillars

### Pillar 1: Part designer (merchandise engine)

- Parametric, declarative parts on the 8 mm grid: tiles, plates, bricks, nameplates, keychains, mosaics, baseplates, adapters.
- Artwork in: SVG (preferred), PNG/JPG (traced), text with bundled OFL fonts. Artwork out: emboss, deboss, or face-down inlay for two-colour prints.
- Connectivity is derived from features (a stud field is a male cylinder grid), so every designed part also works in the assembler and in LDCad via exported `!LDCAD SNAP_*` metas.
- Batch runs: a CSV of names or logos becomes nested print plates with serials and a manifest.
- Why first: it is where printing wins (parts nobody moulds) and where revenue is.

### Pillar 2: Assembler (play and planning)

- Snap bricks on an integer-LDU lattice with the 24 lattice rotations; SNOT building falls out of the lattice.
- 10k to 100k instances with LOD and chunked instancing; collision by occupancy masks; "is this attached" checks with explanations.
- Open text formats: native `.obrick` (JSON in a zip) and lossless LDraw `.ldr`/`.mpd` round-trip, so LeoCAD, Studio, LDCad and LPub3D users can exchange files.
- Play mode (M5): physics on connected components, hinges as joints, bricks come apart when the clutch threshold is exceeded.

### Pillar 3: Print pipeline (print-true output)

- Fit profiles per printer, material and nozzle: stud, tube, horizontal-hole and first-layer offsets, wall quantisation to line width.
- A calibration coupon generator and a first-run wizard that writes the profile from what fit.
- A validator that blocks, not just warns, on fit-critical failures: manifoldness, minimum walls and features, overhangs under the roof, features outside the calibrated window.
- Vanilla 3MF (Core 1.4.0, Materials 1.2.1, Production 1.1.2) with per-object colours as the canonical output; STL as legacy; STEP in v2.

## Personas and journeys

### Mara, merch designer at a small studio

1. Opens the desktop app, loads the client's SVG logo.
2. Picks "Logo tile 2x4", sets emboss 0.6 mm, sees hairlines under 0.8 mm removed and highlighted.
3. Validation passes for profile `fdm-0.4 / PETG / Prusa MK4S`.
4. Exports a batch of 50 with serials from a CSV; three 3MF plates and a manifest land in a folder.
5. Hands plates to the print farm. Every tile clutches.

### Dev, print-farm operator

1. Installs the CLI: `curl -fsSL https://openbrick3d.org/install.sh | sh`.
2. Runs `openbrick calibrate coupon --process fdm-0.4 --out coupon.3mf`, prints it, records the best-fitting cells with `openbrick calibrate record`.
3. Runs `openbrick run customers.obrun.json --out plates/` nightly from a script (the run document names the fit profile).
4. Re-runs are deterministic: same inputs, identical plates.

### Noor, hobbyist builder

1. Opens the PWA on a laptop, imports an `.mpd` from LDraw.
2. Adds a custom nameplate part from the community registry; it snaps because its connectors travel with it.
3. Saves `.obrick`, exports `.ldr` for a friend on LeoCAD, exports a 3MF of the whole model for a display print.

### Sam and Ada (parent and child), playing on a phone

1. Scan the QR code from a laptop running `openbrick serve --qr`; the PWA opens; "Add to Home Screen".
2. One finger drags bricks, two fingers orbit; snaps always align (the top complaint about mobile brick apps is non-aligning snaps).
3. Play mode knocks the tower over; pieces separate at the clutch threshold.

### Kai, open-source contributor

1. Clones the repo, runs `cargo xtask check` and `pnpm test`; everything passes locally without secrets.
2. Adds a `clip` feature to the part evaluator with a golden-mesh test and a connector test.
3. Signs off the commit (DCO), opens a pull request; review is specific, kind and teaches the convention they missed.

## Non-goals for v1

- Reproducing the standard brick catalogue for printing (we import LDraw geometry for the assembler; we do not optimise printing of catalogue bricks).
- Minifigure parts of any kind (trademark and copyright protected; see [12](12-legal-and-naming.md)).
- A scripting language of our own (we import OpenSCAD customizer files and later run TypeScript scripts that emit the part document in a sandbox).
- Real-time collaboration (benchmark in M3, decide for v1.x).
- STEP export in the client (v2 service).
- Marketplace, payments, hosting of user uploads by the company (decide separately; DSA obligations apply).

## Success metrics

| Metric | MVP (week 17) | 1.0 (week 56) |
|--------|---------------|---------------|
| Printed logo tiles that pass the pull test on the first calibrated print | 9 of 10 | 19 of 20 |
| Time from SVG to validated 3MF on desktop | under 5 min | under 2 min |
| Assembler instances at 60 fps on an Iris Xe laptop | n/a | 100k with LOD |
| Install channels live | CLI on 3 OSes plus unsigned desktop installers on 3 OSes | desktop stores, PWA, Android, iOS |
| Community parts in the extended registry | n/a | 200 |
| External contributors with merged PRs | 3 | 25 |
| Fit profiles contributed to the shared dataset | 5 printers | 100 printers |

## Community principles

- **Creativity is the product.** The designer must make it easy to try the odd idea: an off-grid keychain loop, a mosaic from a photo, a Technic adapter for another brand's bus.
- **Long-term solutions.** Prefer the choice that is still maintainable in 2029 over the one that demos faster this month; the ADR table in [03](03-tech-stack-decisions.md) records "revisit when" for each decision.
- **Explore the non-obvious.** The watch lists (native wgpu renderer, Godot play mode, SDF organic mode, CRDT collaboration) are real options with dates, not graveyards.
- **Teach, do not punish.** Reviews name the rule and link the doc; a failing check explains how to fix it; the first pull request from a newcomer gets a maintainer walkthrough. Mistakes are logged in [13](13-risks-and-open-questions.md) as assumptions to revisit, not as blame.
- **Be a good citizen of the LDraw ecosystem.** Credit the LDraw Parts Library in every client, propose the connectivity dataset on forums.ldraw.org, contribute generic parts upstream under CC BY 4.0.
