# OpenBrick3D conventions for AI-assisted work

These rules apply to every contributor, human or AI-assisted. The full reasoning is in docs/plan/.

- Positions are integer LDU (1 LDU = 0.4 mm) in the engine and in part layout; never floating millimetres there. Rotations are one of the 24 lattice rotations (`Rot24`) unless a part is explicitly off-lattice.
- Dimensions come from `standards/dimensions-v1.json` through `openbrick-units`; do not hard-code 8.0, 4.8, 9.6 or any other brick number in code.
- The Rust core (`crates/`) has no three.js, DOM or Tauri types. UI packages never compute geometry or connectivity; they call the core.
- Every validator message carries a rule id, the measured value, the threshold and a fix hint. Blockers disable export; warnings never do.
- Geometry changes need golden-mesh tests; snapping changes need property tests; validator changes need one passing and one failing fixture.
- Fit offsets live in fit profiles, never in part documents. Connectivity is derived from features, never hand-authored separately.
- Use "brick-compatible". The LEGO word mark appears only in the disclaimer and in docs/plan/12. Never generate minifigure geometry; never emboss text on stud tops.
- No GPL, AGPL or LGPL code or data in client crates or packages. LDCad shadow data is off limits. Cite the source of every geometry number.
- Commits: conventional titles, signed off (`git commit -s`). Keep pull requests small; add a changelog line.
- Run `cargo xtask check` before pushing (fmt, clippy -D warnings, tests, data validation). Also run `pnpm test` when you touch `bench/` (it runs the device harness tests), and always from M2.
- Commit with the environment's configured git identity; do not override `user.name` or `user.email` (overrides make commits show as unverified).
- Attribution: end every commit message and pull request description with the line `Done by Neuraldeep.net design agents` (after the sign-off in commits).
- Agent pull requests merge automatically once every CI check passes and there is no merge conflict (see GOVERNANCE.md). List any licence, legal, money or Dimensional Standard decision in the PR description so a human can revert it.
- Be kind in reviews and in generated messages; explain the rule, link the document, do not scold.
