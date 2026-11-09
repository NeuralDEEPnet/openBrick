# Scale benchmark: models of 1,000 to 100,000 bricks

Two reproducible scripts that answer "how does Build mode behave as the model grows" without a device
([docs/plan/07](../../docs/plan/07-assembler-engine.md) sections 4, 9, 14 and 15). They complement the
device render harness in `bench/harness` (the same budgets, measured on real laptops and phones by
people; see `bench/devices/README.md`).

| Script | Measures | Needs |
|--------|----------|-------|
| `core.mjs` | The Rust core as WebAssembly in node: `.obrick` size, JSON parse, core open, save (what an autosave pays), stats, bill of materials, the table, snap / pick / add / remove (with undo) as p50 and p95, the cost of one edit as the page pays it, wasm memory. Each case runs in its own process so memory is the model's own. | `cargo xtask wasm`, node 22 |
| `render.mjs` | Build mode in headless Chromium: time to show the model, draw calls and triangles per frame, frame time, the page's snap and place+undo round trips. | `cargo xtask wasm`, `pnpm --dir apps/web build`, Playwright's Chromium |
| `layouts.mjs` | The deterministic models both use: `towers` (16 layers of 2x4 bricks over 16x16 baseplates), `plates` (the same with plates) and `city` (a seeded random skyline of bricks and plates in twelve colours). | |

```sh
cargo xtask wasm
node bench/scale/core.mjs                                  # 1k 5k 10k 25k 100k, three layouts (about 1 minute)
node bench/scale/core.mjs --sizes 10000 --layouts city --samples 100 --json core.json --write /tmp/models
pnpm --dir apps/web build
node bench/scale/render.mjs --sizes 1000,10000 --frames 3  # headless Chromium, SOFTWARE rendering
node bench/scale/render.mjs --sizes 100000 --frames 0      # counts only, no frame timing
```

## Read the render numbers with this in mind

Headless Chromium on a machine without a GPU draws with SwiftShader, a CPU implementation of WebGL.
**Draw calls and triangles per frame are real, device-independent counts. Frame times from this script are
software numbers: they say how fast one CPU can rasterise this many triangles, not how a laptop or phone GPU
does** (a laptop iGPU is several hundred times faster; see the budgets in docs/plan/07 section 9). The page's
own frame counter runs at the compositor's pace even when software rendering is seconds behind, so `render.mjs`
measures the time until the GPU has drawn the frames (`readPixels`), not `requestAnimationFrame` deltas. Real
device numbers come from the harness in `bench/harness` and from the page's own `?bench=1` mode
([apps/web/README.md](../../apps/web/README.md)).

## Baseline, before the scaling work (2026-11-09, commit 55fadb8)

Core in WebAssembly under node 22 (4 CPUs, no other load), `city` layout, p50 / p95 in ms:

| bricks | `.obrick` | core open | save | stats | one edit's state reply | snap | pick | add | remove (mid) + undo | wasm memory |
|---|---|---|---|---|---|---|---|---|---|---|
| 1,000 | 0.09 MB | 19 ms | 1.9 ms | 4.5 ms | 3.5 ms | 0.13 / 0.69 | 0.04 / 0.11 | 0.02 | 0.03 + 0.02 | 26 MiB |
| 5,000 | 0.45 MB | 71 ms | 6.5 ms | 24 ms | 25 ms | 0.31 / 0.79 | 0.19 / 0.53 | 0.03 | 0.08 + 0.06 | 26 MiB |
| 10,000 | 0.89 MB | 149 ms | 14 ms | 45 ms | 42 ms | 0.50 / 1.8 | 0.37 / 1.8 | 0.03 | 0.12 + 0.11 | 37 MiB |
| 25,000 | 2.3 MB | 472 ms | 29 ms | 137 ms | 127 ms | 1.0 / 3.0 | 0.93 / 1.2 | 0.03 | 0.32 + 0.30 | 104 MiB |
| 100,000 | 9.1 MB | 8,784 ms | 107 ms | 628 ms | 553 ms | 3.7 / 5.2 | 3.4 / 3.7 | 0.04 | 1.8 + 1.7 | 413 MiB |

("One edit's state reply" is what the worker computed and sent after every edit: all instances, the statistics,
the bill of materials and the whole model as text for the page's autosave copy.)

Build mode in headless Chromium, SwiftShader (**software**, 1280 x 800), `city` layout:

| bricks | page open | draw calls | triangles / frame | frame time (software) | snap + place/undo round trips as the page sees them |
|---|---|---|---|---|---|
| 1,000 | 0.3 s | 3 | 1.1 M | 1.7 s | 1.9 ms, 9.7 ms |
| 5,000 | 0.4 s | 3 | 5.4 M | 6.4 s | 1.9 ms, 31 ms |
| 10,000 | 0.8 s | 3 | 10.9 M | 13.3 s | 1.8 ms, 90 ms |
| 25,000 | 1.1 s | 3 | 27.0 M | not timed | 3.8 ms, 237 ms |
| 100,000 | 3.7 s | 3 | 107.8 M | not timed | 131 ms, - |

What the baseline shows:

* The **core's per-edit algorithms were not the problem at 10k**; the problems were the O(n) pick (every part's
  box tested per pointer move: 3.4 ms at 100,000 parts, and a snap does one), the connection statistics (counted
  again from the whole model for every edit: 0.6 s at 100,000), the **state reply built after every edit** (all
  instances, statistics, bill of materials and the whole model as text: 0.55 s at 100,000, so one placement
  costs half a second), an open that takes 8.8 s and 413 MiB at 100,000 parts (hash tables with one heap
  allocation per cell and per connector position), and removing from the middle of the table (the whole index
  renumbered: 1.8 ms).
* **Rendering** was three draw calls however large the model: no chunks to cull and no level of detail, so the
  triangles drawn are the triangles of every part (10.9 M at 10,000 bricks, 108 M at 100,000), and in software
  a frame takes seconds. The plan's budget is 10 to 20 M triangles per frame on an integrated GPU.

## After the scaling work (2026-11-09)

The tables for the core and Build mode, before and after, are in [docs/plan/07 section 14](../../docs/plan/07-assembler-engine.md#measured-performance-2026-11-09); `node bench/scale/core.mjs` and `node bench/scale/render.mjs` reproduce them. The native engine timings are `cargo run --release -p openbrick-engine --example scale -- 100000`. On a real device use `?bench=1` ([bench/devices/README.md](../devices/README.md)).
