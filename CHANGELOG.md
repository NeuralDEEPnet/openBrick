# Changelog

All notable changes to OpenBrick3D. Format follows Keep a Changelog; versions follow semver.

## Unreleased

### Changed
- Agent pull requests merge automatically once CI is green (GOVERNANCE.md, plan 14); commits and pull requests are attributed to the Neuraldeep.net design agents (CLAUDE.md).

### Fixed
- Phase B review fixes. `openbrick_print::record` takes one printed pocket (pocketTubeOD and pocketRib together, from the coupon's pocket set, saved as `calibration.pocketSet`; fit-profile schema 1.1), and the example fit profile records the printed pocket 6.65/0.30. FIT.PROCESS also compares the fit offsets a part was built with, so a re-recorded profile with the same id is caught. FEATURE.MIN reports artwork whose stroke could not be measured, blocks on it when fit-critical, and treats exactly `maxRemovedAreaPct` as a pass, as the evaluator's warning now does. Deboss and inlay report face material thinner than minFeature between recesses and at the edge (`Artwork.thinFaceAreaMm2`). An inlay's face on the bed gets the first-layer chamfer, and its warning says it is the single-extruder filament-swap variant (the multi-material variant is next). MESH.VOLUME no longer double-counts tubes and ribs in a thickened roof. The coupon's print estimate is always finite, its hash ignores bed sizes that do not change its layout, and its labels are fit-critical. `EVALUATOR_REVISION` is 2, so every part's content hash changes.
- `deny.toml` used a value current cargo-deny rejects, so the licence policy could not be checked; CI now runs `cargo deny --all-features check`, and a `geom-manifold-csg` job runs the C++ kernel cross-check.
- `canonical_fingerprint` uses `as_chunks`, which the clippy lint added in Rust 1.99 requires.
- DCO workflow skips merge commits, as the DCO app does; it failed on the merge commit in the M0 scaffold pull request.

### Added
- `openbrick` CLI (backlog items 11 and 14):
  - `build` validates a part document and writes a 3MF or a single-colour STL.
    - The 3MF is one object whose parts are the colour roles (the part-designer layout), with traceability metadata.
    - `--fuse` writes one fused 3MF object.
    - `--no-colour` (or `--no-color`) leaves out the display colours, so Bambu Studio's import colour dialog has nothing to read (Q18). Colours stay the default.
  - `validate` prints the report.
  - `calibrate coupon` writes the calibration coupon, with a table and a JSON map of its cells. The table says whether the copies lie one behind the other or side by side.
  - `calibrate record` writes a fit profile: offset = chosen cell minus nominal, plus one pocket from the printed pocket set.
  - Parameters come from the command line (`--param size=2x4 --param logo=./my-logo.svg`).
  - Assets:
    - Asset files are regular files of at most 4 MiB.
    - The evaluator gets each under its file name, so the content hash does not depend on how the path was typed.
    - A part document's assets must stay in its folder, symbolic links included.
  - `--fit` takes a file or `example`.
  - `--json` prints one object on stdout: the report per `schemas/report.schema.json`, the measurements, the content hash, timings, and for each output its layout, components object, objects, UUIDs and metadata.
  - Exit codes: 0 ok, 1 blockers (nothing written), 2 usage or input error (also arguments that are not UTF-8), 3 internal error.
  - Inlays are written face down.
  - Colour hints:
    - Two-colour FDM builds name the layer for a single-extruder filament change. A layer whose middle lies on an inlay's boundary already prints the body colour.
    - A `parts` inlay needs one filament per part and gets no change layer.
    - Resin builds show resin volumes instead of filament advice.
  - Release timings: the logo tile builds in about 0.06 to 0.1 s and the default coupon in about 0.7 s.
  - New: the first print-test guide, `docs/guides/print-test.md`, for the Prusa MINI+, the Prusa XL and the Bambu Lab A1 with AMS lite.
    - Its slicer steps for the part-designer layout come from the Q18 reading and are marked for confirmation by a print.
    - It includes a colourless build for the Bambu colour dialog.
    - Also new: a print-test report issue form that asks how the 3MF loaded.
- `apps/web`: the M1 browser playground.
  - Design a brick-compatible logo tile from your SVG or the sample mark, and see it in live 3D. You choose:
    - the size and the height;
    - the relief: emboss, deboss or inlay;
    - the inlay style: filament swap, or the logo as its own part for an AMS or tool changer (shown only for an inlay);
    - the relief height and the margin.
  - Artwork that will not print is drawn in red with its percentage.
  - The validation report gives the rule id, the measured value and threshold with the unit from the core (`openbrick_print::finding_unit`), and a fix hint, blockers first.
  - The 3MF and STL downloads stay off while a blocker or an invalid field stands.
  - Downloads:
    - The 3MF is one object whose parts are the colour roles.
    - Files are named `{id}-{w}x{h}-{relief}[-{inlayStyle}]`, so the two inlay styles never share a name.
    - An inlay downloads face down through `openbrick_geom::turn_face_down`.
  - You can also build and download the calibration coupon, with its cell table and the `openbrick calibrate record` line.
  - `openbrick-wasm` gains `evaluate_part`, `export_part`, `build_coupon`, `export_coupon`, `coupon_layout_json`, `resolve_params_json`, the bundled profiles, part and sample, and `export_solids` (Rust only: the solids `export_part` writes).
  - The module is 3.01 MB raw and 990 KB gzipped (budget 2.5 MB).
  - The smoke test:
    - checks every case, the parts inlay included, against `native_eval`;
    - requires the downloaded files to be byte-identical to the native exports, and every part 3MF to have one build item and one components object;
    - validates the 3MF files with lib3mf.
  - The app builds under `OPENBRICK_BASE` for GitHub Pages (`/openBrick/`).
  - The non-affiliation disclaimer has one source, `crates/openbrick-units/disclaimer.txt`, read by the CLI, the core and the page footer.
- Multi-material output (Q18, `docs/spikes/q18-slicer-multimaterial-import.md`):
  - `openbrick_io::write_3mf_with_layout` adds `ExportLayout::OneObjectWithParts` (`ExportLayout::PART_DESIGNER`): one mesh object per colour role, then one components object (no pid, no transforms) and one build item.
    - Bambu Studio loads it as one object with its parts in place.
    - PrusaSlicer offers to merge the parts into one multi-part object.
    - Production UUIDs are on every component.
    - `write_3mf` is unchanged.
  - `openbrick build` and the playground's 3MF download both write it, and both turn an inlay face down with the one shared transform, `openbrick_geom::turn_face_down`. `crates/openbrick/tests/export_parity.rs` checks that they write the same meshes, with and without colours.
  - `relief.svg` gains `inlayStyle`: `swap`, the default, or `parts`.
    - In `parts`, the logo fill is its own accent part, and the body is recessed by exactly that region, so together they are the plain tile.
    - The parts-inlay warning says the fused single-colour export shows no artwork.
    - An unused `parts` style from a hidden parameter is not warned about.
  - A colour-role warning names each role and the face that goes on the bed.
  - `EVALUATOR_REVISION` is now 3, and the logo tile (1.3.0) gains the `inlayStyle` parameter.
  - `ManifoldRust::solid_from_mesh` is new.
  - `scripts/validate-3mf.py` now:
    - checks components (each part placed once, identity transforms, UUIDs);
    - samples every package for overlapping objects, counting only lines through both objects, and fails a pair whose lines through both could mostly not be sampled;
    - compares each file with its description (fixture manifest or CLI JSON, layout and components object included). Descriptions written before the layout read as the separate layout;
    - in its self-test, breaks files (8 generic and 6 component mutations) and descriptions (6 mutations), and checks that an old-style description still passes.
- `openbrick-part`: `relief.svg` (backlog item 9), so the example logo tile builds with its new CC0 artwork `parts/assets/mark.svg` (24 ms in release). SVG input is read with usvg 0.49 (no fonts, no image decoding) as untrusted input: images are never resolved, entities and markers are refused, and fills only are built (strokes, text, images, clip paths, masks, filters and invisible shapes are ignored with a warning). Limits on size, XML nodes, attributes, CSS work, `<use>` expansion and chains of nesting and references (64 levels, cycles refused, so hostile files cannot overflow a 1 MiB wasm stack), and on the sweep work of every polygon operation, estimated by the new `openbrick_geom::region::sweep_cost` before Clipper2 runs. The artwork is fitted into the flat top face box, flattened at 0.05 mm, resolved under evenodd and nonzero fill rules and opened at `minFeature` with round joins (`Kernel::offset2d_round`); `Built.artwork` carries the removed regions and `Measurements` gains `removed_area_pct`, `min_stroke_mm` and `relief_height_mm`. Emboss is its own colour-role object; deboss and inlay lower the cavity ceiling by the depth, clamped so the cavity keeps `minCavityDepthMm`; inlay splits off the face slab minus the logo, printed face down. `Built.fused` is the kernel union for single-colour STL. New dependencies: `usvg` 0.49.0 and `simplecss` 0.2.2 (both Apache-2.0 OR MIT).
- `openbrick-print`: validator `validate(&Measurements, &ProcessProfile, Option<&FitProfile>) -> Report` with 12 rules from docs/plan/05 section 5 (MESH.MANIFOLD, MESH.VOLUME, WALL.MIN, TUBE.WALL, CAVITY.DEPTH, WALL.QUANTISED, ROOF.MIN, FEATURE.MIN, RELIEF.HEIGHT, FIT.PROCESS, FIT.MISSING, CLUTCH.RISK), each finding with rule id, value, threshold and fix hint, blockers first, and 41 data fixtures with at least one passing and one failing case per rule. FIT.PROCESS also blocks a part validated with a different fit profile from the one it was built with; FEATURE.MIN blocks on fit-critical artwork and warns otherwise. `openbrick_part::validate(&Built, ...)` has the plan's signature. The coupon sweep is data (`standards/coupon-v1.json`, `schemas/coupon.schema.json`, `coupon_v1()`), shared by the coupon generator, `record` and CLUTCH.RISK. `record` turns chosen coupon cells into a `FitProfile` (offset = chosen minus nominal) and refuses off-grid values. `schemas/report.schema.json` is the contract for `--json` reports. Process profiles are now schema 1.1 with optional `recommendedTubeWallMm` (0.8 on fdm-0.4) and `maxRemovedAreaPct` (1.0).
- `openbrick-part`: calibration coupon generator `build_coupon` (backlog item 11): stud (OD 4.60 to 4.95), tube bore (ID 4.80 to 5.05) and 2x2 pocket rows (tube OD 6.40 to 6.65 crossed with rib depth 0.20 to 0.50), twice, each cell with an embossed label in an original pixel font. Every cell opens upward, so the coupon prints flat without supports, and each cell is built as a part is built with the offset the wizard records, so recorded offsets reproduce the chosen cell on every process. The default pocket set is the nominal row and column of the cross (13 pockets per copy): 118.4 x 160.8 x 3.2 mm on fdm-0.4, which fits a Prusa MINI+ bed, about 32 g of PLA. `crossAndDiagonal` and `full` are options for larger beds. The `coupon` example writes 3MF, STL and a JSON cell manifest; a `part-exports` CI job validates the coupons and the logo tile exports with lib3mf.
- `apps/web`: the M1 browser pipeline. `openbrick-wasm` is now a wasm-bindgen crate (`kernel_id`, `standard_json`, `check_part_json`, `max_studs`, `build_nominal_brick`). Its meshes' arrays are moved out of Rust and copied once, out of WebAssembly memory. The new `cargo xtask wasm` builds it with the pinned wasm-bindgen CLI 0.2.129: 1.53 MB raw, 485 KB gzipped, checked against the 2.5 MB gzip budget. The core runs in a Web Worker. A three.js viewer shows nominal bricks from 1x1 to 16x16 with their triangle count, volume and fingerprint, and you can orbit underneath to see the tubes. It uses WebGPU with WebGL2 fallback and has light and dark themes. A long build shows its elapsed time and is cancelled when you change the size. After a WebAssembly trap, the page replaces the worker. The nominal brick construction now lives in `openbrick_geom::nominal`, with golden tests, and the kernel-to-three.js frame conversion in `packages/viewport`. A `web` CI job runs the unit tests and a headless smoke test, which checks the browser's 2x4 and 2x2 against the native kernel.
- `openbrick-io`: deterministic 3MF writer (`write_3mf`: one Core 1.4.0 object per colour role, a Materials 1.2.1 colour group, required `openbrick:*` traceability metadata, Production UUIDs derived from the metadata and every object's name, role, colour and stored mesh fingerprint) and binary STL writers (`write_stl_binary`, `write_stl_binary_per_role`). Both return bytes, so they run natively and in wasm32, and identical input gives byte-identical files (backlog items 12 and 13, decision D18). The `write_fixtures` example writes a kernel-built 2x4 brick with an accent cube and a three-role fixture. `scripts/validate-3mf.py` reads them with lib3mf 2.5.0 in strict mode, re-derives every UUID from what lib3mf read, compares the files with the example's manifest, and self-tests that seven kinds of broken copy fail; the new `io-3mf` CI job runs it.
- `openbrick-part`: part evaluator for `body.tile`, `body.plate`, `body.brick`, `stud.field` and `tube.field` (backlog items 6 to 8). `evaluate` turns a part document, parameter values, a process profile and an optional fit profile into one mesh per colour role, `studM` and `tubeF` connectors in integer LDU, validator `Measurements` (walls above and at the first-layer chamfer), warnings and a SHA-256 content hash. Other ops return `Unsupported`, or are skipped for previews. Parameters are checked against type, min, max, step and options, and `if` and `visibleWhen` use a small expression language. Bodies are limited to 64 studs per side and 64 bricks tall. Fit offsets change studs, bores, tube outer diameters (grown so the wall keeps `minTubeWallMm`), pins and ribs; ribs an offset cancels are left out. Golden tests for five parts check the exact volume, bounding box, connectors and mesh fingerprint. `openbrick-print` gains `Measurements`, the process `tessellation` field and offset helpers. Release build: a 2x4 brick in about 30 ms, a 6x16 plate in about 0.7 s. New dependency: `sha2` 0.10.
- `bench/harness`: device render harness (backlog item 3, decision D10). It renders N instanced 2x4 bricks at three kernel-built LODs with the three.js `WebGPURenderer` (WebGPU or WebGL2) and reports frame-time statistics as one CSV row, with copy, download, an optional GPU timer and a suite mode. Every column has a rule, and the smoke test and the `results.csv` guard check each row against it; `fps_*` columns are derived from the frame-time percentiles. Meshes come from the new `export_lods` example of `openbrick-geom`, and `cargo test` checks the committed copies. CSV columns and phone instructions are in `bench/devices/README.md`; the root `pnpm test` now runs the harness unit tests. A `harness` CI job runs the tests and the smoke run.
- Q04 spike: `scripts/spikes/ldraw_connectivity.py` derives connectors for LDraw parts by scanning primitive references, synthesises female studs from geometry, and checks positions and axes. On 200 common parts with the 2026-10 library, 84.8 % of the 164 with ground truth are fully correct and 95.7 % at least partially correct (strict scoring); the lower bound over all 200 is 69.5 %. Report in `docs/spikes/q04-ldraw-connectivity.md`.
- `openbrick-geom`: second `Kernel` backend on `manifold-csg` 0.4.1 (C++ Manifold v3.5.3 through FFI) behind the opt-in feature `manifold-csg`. The conformance corpus now runs on every enabled backend, plus a cross-backend comparison. Also new: `Mesh::canonical_fingerprint`, which does not depend on construction order, and backend selection in `kernel_timing`. Spike report in `docs/spikes/q01b-manifold-csg-comparison.md`.
- JSON Schemas (draft 2020-12) for the standard, process and fit profiles and parts in `schemas/`. `cargo xtask data` validates every data file and valid fixture against them, loads each with its Rust reader and re-validates what the reader writes back, and prints each failure with its JSON pointer. Unknown fields are errors in 1.0 documents and allowed in later 1.x minors. Process profiles now carry `credit`, and `couponVersion` accepts `1.0` (backlog item 5).
- `openbrick-geom`: `Kernel` implementation on `manifold-rust` 0.19.1 (pure Rust), conformance corpus built from the Dimensional Standard, a timing example and a wasm32 probe; CI checks that the wasm32 build produces the same mesh fingerprint as native. Spike report in `docs/spikes/q01-kernel-conformance.md`.
- Planning record under `docs/plan/` (14 documents plus research handoff files).
- Repository scaffold for milestone M0: Cargo workspace with `openbrick-units` (LDU, Rot24, Dimensional Standard loader), typed skeletons for geom, part, print, engine, catalog, io, the `openbrick` CLI and `openbrick-wasm`, an `xtask` runner.
- `standards/dimensions-v1.json` (CC0) and process profiles `fdm-0.4`, `fdm-0.2`, `resin-msla`.
- Example part documents: logo tile and nameplate keychain.
- Governance, licence policy, trademark policy, contributing guide, code of conduct, security policy, packaging guide, CI workflow and issue templates.
- Plan document 14: lean development with AI agents (operating model, task loop, budgets, first agent-ready backlog) and an `agent-task` issue template.
