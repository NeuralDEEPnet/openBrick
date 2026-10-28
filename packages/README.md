# TypeScript packages (from M2)

`core-wasm` (built artefact, types, worker wrapper), `viewport` (three.js scene, chunks, LOD, picking, edges), `ui` (panels, wizards), `state` (document store mirroring the command log), `part-api` (M5, `@openbrick/part`). pnpm workspace; see docs/plan/10.

`viewport/` exists already, with one module: the kernel-to-three.js frame conversion that `bench/harness` and `apps/web` share.
