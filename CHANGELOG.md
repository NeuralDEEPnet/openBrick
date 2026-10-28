# Changelog

All notable changes to OpenBrick3D. Format follows Keep a Changelog; versions follow semver.

## Unreleased

### Changed
- Agent pull requests merge automatically once CI is green (GOVERNANCE.md, plan 14); commits and pull requests are attributed to the Neuraldeep.net design agents (CLAUDE.md).

### Fixed
- `deny.toml` used a value current cargo-deny rejects, so the licence policy could not be checked; CI now runs `cargo deny --all-features check`, and a `geom-manifold-csg` job runs the C++ kernel cross-check.
- `canonical_fingerprint` uses `as_chunks`, which the clippy lint added in Rust 1.99 requires.
- DCO workflow skips merge commits, as the DCO app does; it failed on the merge commit in the M0 scaffold pull request.

### Added
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
