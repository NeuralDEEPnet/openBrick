# @openbrick3d/viewport

Seed of the three.js viewport package in [docs/plan/02](../../docs/plan/02-architecture.md#2-crates-and-packages). So far it holds one module, `src/frame.js`. It converts from the geometry kernel's frame (millimetres, right-handed, +Z up) to three.js (right-handed, +Y up). The conversion is a -90 degree rotation about +X that maps (x, y, z) to (x, z, -y) and keeps triangle winding. `bench/harness` and `apps/web` both import it, so there is only one copy to get right.

```sh
pnpm --dir packages/viewport test   # node --test; the root pnpm test runs it too
```
