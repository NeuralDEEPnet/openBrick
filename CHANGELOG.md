# Changelog

All notable changes to OpenBrick3D. Format follows Keep a Changelog; versions follow semver.

## Unreleased

### Fixed
- DCO workflow skips merge commits, as the DCO app does; it failed on the merge commit in the M0 scaffold pull request.

### Added
- `openbrick-geom`: second `Kernel` backend on `manifold-csg` 0.4.1 (C++ Manifold v3.5.3 through FFI) behind the opt-in feature `manifold-csg`. The conformance corpus now runs on every enabled backend, plus a cross-backend comparison. Also new: `Mesh::canonical_fingerprint`, which does not depend on construction order, and backend selection in `kernel_timing`. Spike report in `docs/spikes/q01b-manifold-csg-comparison.md`.
- JSON Schemas (draft 2020-12) for the standard, process and fit profiles and parts in `schemas/`. `cargo xtask data` validates every data file and valid fixture against them, loads each with its Rust reader and re-validates what the reader writes back, and prints each failure with its JSON pointer. Unknown fields are errors in 1.0 documents and allowed in later 1.x minors. Process profiles now carry `credit`, and `couponVersion` accepts `1.0` (backlog item 5).
- `openbrick-geom`: `Kernel` implementation on `manifold-rust` 0.19.1 (pure Rust), conformance corpus built from the Dimensional Standard, a timing example and a wasm32 probe; CI checks that the wasm32 build produces the same mesh fingerprint as native. Spike report in `docs/spikes/q01-kernel-conformance.md`.
- Planning record under `docs/plan/` (14 documents plus research handoff files).
- Repository scaffold for milestone M0: Cargo workspace with `openbrick-units` (LDU, Rot24, Dimensional Standard loader), typed skeletons for geom, part, print, engine, catalog, io, the `openbrick` CLI and `openbrick-wasm`, an `xtask` runner.
- `standards/dimensions-v1.json` (CC0) and process profiles `fdm-0.4`, `fdm-0.2`, `resin-msla`.
- Example part documents: logo tile and nameplate keychain.
- Governance, licence policy, trademark policy, contributing guide, code of conduct, security policy, packaging guide, CI workflow and issue templates.
- Plan document 14: lean development with AI agents (operating model, task loop, budgets, first agent-ready backlog) and an `agent-task` issue template.
