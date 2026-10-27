# Contributing to OpenBrick3D

Thank you for exploring with us. This project rewards creativity and long-term thinking; a wrong idea argued well is a contribution, and reviews here teach rather than punish.

## Before you start

- Read the [plan](docs/plan/README.md); decisions carry IDs (D01 to D24) in [docs/plan/03](docs/plan/03-tech-stack-decisions.md). If your change reverses one, add a new ADR row and mark the old one superseded.
- Numbers come from `standards/dimensions-v1.json`. Do not hard-code dimensions.
- Positions in the engine and in part layout are integer LDU. Never floating millimetres there.
- The Rust core has no three.js, DOM or Tauri types. The UI never computes geometry.

## Developer Certificate of Origin

Every commit must be signed off (`git commit -s`), which certifies the [DCO 1.1](https://developercertificate.org/). The GitHub DCO check enforces it. There is no CLA. Contributions are accepted under the licence of the tree they touch (see [LICENSE-POLICY.md](LICENSE-POLICY.md)); inbound equals outbound.

## Workflow

1. Open an issue or pick one labelled `good first issue`. For plan changes, title it `plan: <topic>`.
2. Branch from `main`; keep pull requests small.
3. Run `cargo xtask check` (fmt, clippy with `-D warnings`, tests, data validation). From M2 also `pnpm test`.
4. Geometry changes need golden-mesh tests; snapping changes need property tests; validator changes need a passing and a failing fixture; UI changes need a screenshot.
5. Use conventional commit titles: `feat(part): add clip feature`, `fix(print): clamp deboss depth on plates`, `docs(plan): ...`.
6. Add a line to `CHANGELOG.md` under Unreleased.

## Review culture

- Name the rule and link the document; suggest rather than demand style.
- Approve with nits rather than block on them; blockers are correctness, licence, safety and the conventions above.
- A newcomer's first pull request gets a maintainer walkthrough.
- Mistakes become assumptions logged in [docs/plan/13](docs/plan/13-risks-and-open-questions.md), not blame.

## Licence hygiene

No GPL, AGPL or LGPL code in client crates or packages; no LDCad shadow data; no files from CC BY-SA or CC BY-NC brick projects; cite the source of any geometry number you add. Use "brick-compatible"; the LEGO word mark appears only in the disclaimer.

## Fit profiles and parts

Fit profiles for your printer and new part documents are the easiest high-value contributions: submit them to `profiles/fit/` and `parts/` with a `credit` and `provenance` block. Include your pull-test median and control measurement for fit profiles (protocol in [docs/plan/05](docs/plan/05-3d-printing-pipeline.md)).
