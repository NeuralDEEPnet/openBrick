# OpenBrick3D master plan

**Status:** proposed, awaiting founder review. **Date:** 2026-10-27. **Owner:** Neural Deep Network LTD.

This folder is the planning record for OpenBrick3D (repository `openBrick`): an open-source, brick-compatible part designer, assembler and print pipeline for desktop, mobile and the command line. Every document here was produced from nine web-verified research passes run on 2026-10-27 (geometry standard, 3D printing, geometry kernels, app frameworks, ecosystem, legal, distribution, part-designer UX, assembler engine). Versions and prices quoted are as of that date.

## Documents, in reading order

| # | Document | One line |
|---|----------|----------|
| 00 | [Executive summary](00-executive-summary.md) | The decision on one page: what, for whom, the stack, the MVP, key numbers, top risks. |
| 01 | [Product vision and pillars](01-product-vision-and-pillars.md) | The three pillars, personas, journeys, non-goals, success metrics, community principles. |
| 02 | [Architecture](02-architecture.md) | Layers, module boundaries, the WASM boundary, two end-to-end data flows, plugin model, budgets. |
| 03 | [Tech stack decisions](03-tech-stack-decisions.md) | ADR table (D01 to D24), alternatives considered, the four candidate directions and why one won. |
| 04 | [Brick geometry spec](04-brick-geometry-spec.md) | Normative dimensions in mm and LDU, feature catalogue, compatibility classes, print targets. |
| 05 | [3D printing pipeline](05-3d-printing-pipeline.md) | Fit profiles, calibration coupon, validator rules, 3MF output, slicer guidance, clutch test protocol. |
| 06 | [Part designer and DSL](06-part-designer-and-dsl.md) | The `openbrick/part@1` document, features, parameters, SVG and text pipelines, batch merch runs. |
| 07 | [Assembler engine](07-assembler-engine.md) | Integer-LDU lattice, instance tables, snapping, collision, connection graph, rendering, play mode. |
| 08 | [File formats and interop](08-file-formats-and-interop.md) | `.obrick`, part and profile documents, LDraw round-trip, 3MF, STL, glTF, STEP, import sanitising. |
| 09 | [Platforms and distribution](09-platforms-and-distribution.md) | Support matrix, install matrix with commands, signing costs, honest mobile strategy, releases. |
| 10 | [Repo layout and dev tooling](10-repo-layout-and-dev-tooling.md) | Monorepo tree, crates and packages, tests, CI matrix, contribution workflow, AI-assisted conventions. |
| 11 | [Roadmap](11-roadmap.md) | Milestones M0 to M6 with weeks, demo criteria, the MVP definition, what is deferred. |
| 12 | [Legal and naming](12-legal-and-naming.md) | Trademarks and patents, disclaimer text, naming decision, licences, DCO, toy-safety checklist. |
| 13 | [Risks and open questions](13-risks-and-open-questions.md) | Risk register, open questions with the spike that resolves each, assumptions log. |

## How to read this plan

1. Read 00 first. If you only have five minutes, that is the page.
2. 01 says why; 02 and 03 say how; 04 to 08 are the engineering specs; 09 to 11 are delivery; 12 and 13 are the guard rails.
3. Numbers are canonical in 04 (geometry) and 05 (printing). Other documents reuse them and must not redefine them.
4. "We decided" marks a decision. "We assume" marks an assumption that is logged in 13. "OPEN:" marks a question with a spike attached in 13.
5. Decisions carry IDs (D01 to D24) in 03 so issues and pull requests can reference them.

## How to propose a change

Open an issue titled `plan: <topic>` or a pull request that edits the relevant document and, if the change reverses a decision, adds a row to the ADR table in 03 with a new ID and marks the old one superseded. Research sources are cited inline; please cite yours. Be kind in review: this project rewards exploration, and a wrong idea argued well is a contribution.

Back to the [project README](../../README.md).
