# Changelog

All notable changes to OpenBrick3D. Format follows Keep a Changelog; versions follow semver.

## Unreleased

### Fixed
- DCO workflow skips merge commits, as the DCO app does; it failed on the merge commit in the M0 scaffold pull request.

### Added
- `openbrick-geom`: `Kernel` implementation on `manifold-rust` 0.19.1 (pure Rust), conformance corpus built from the Dimensional Standard, a timing example and a wasm32 probe; CI checks that the wasm32 build produces the same mesh fingerprint as native. Spike report in `docs/spikes/q01-kernel-conformance.md`.
- Planning record under `docs/plan/` (14 documents plus research handoff files).
- Repository scaffold for milestone M0: Cargo workspace with `openbrick-units` (LDU, Rot24, Dimensional Standard loader), typed skeletons for geom, part, print, engine, catalog, io, the `openbrick` CLI and `openbrick-wasm`, an `xtask` runner.
- `standards/dimensions-v1.json` (CC0) and process profiles `fdm-0.4`, `fdm-0.2`, `resin-msla`.
- Example part documents: logo tile and nameplate keychain.
- Governance, licence policy, trademark policy, contributing guide, code of conduct, security policy, packaging guide, CI workflow and issue templates.
- Plan document 14: lean development with AI agents (operating model, task loop, budgets, first agent-ready backlog) and an `agent-task` issue template.
