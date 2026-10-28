# Device render harness

A web page that renders N instanced 2x4 bricks at one level of detail with the three.js `WebGPURenderer` and reports frame-time statistics as a CSV row (backlog item 3 in [14](../../docs/plan/14-lean-ai-agent-development.md#9-first-agent-ready-backlog-m0-and-m1), decision D10, [07 section 9](../../docs/plan/07-assembler-engine.md#9-rendering)). How to run it on a device, the URL parameters and the CSV columns are in [`bench/devices/README.md`](../devices/README.md).

```sh
pnpm install                       # once, from the repository root
pnpm --dir bench/harness dev       # local page; add --host for phones on the LAN
pnpm --dir bench/harness test      # unit tests (Node only, no browser); the root pnpm test runs them too
pnpm --dir bench/harness smoke     # build, serve and run headless Chromium
```

## Layout

| Path | What it is |
|------|------------|
| `index.html`, `src/main.js` | the page: renderer, scene, measurement loop, result panel |
| `src/csv.js` | CSV columns (single source of truth) and RFC 4180 quoting |
| `src/stats.js` | percentiles and the per-run summary |
| `src/params.js` | URL parameters, validation and suite plans |
| `src/layout.js` | lattice grid and chunking |
| `src/mesh.js` | reader for the kernel's OBMESH files |
| `src/frame.js` | kernel frame to three.js frame |
| `public/meshes/` | brick meshes written by the kernel (committed, about 75 KB) |
| `test/` | `node --test` unit tests |
| `smoke.mjs` | end-to-end smoke test in headless Chromium |

## Geometry comes from the kernel

The page never builds or edits geometry (see [CLAUDE.md](../../CLAUDE.md)). The meshes in `public/meshes/` are written by a Rust example from the nominal dimensions in `standards/dimensions-v1.json`:

```sh
cargo run --release -p openbrick-geom --example export_lods             # regenerate
cargo run --release -p openbrick-geom --example export_lods -- --check  # verify the committed files
```

`cargo test --workspace` (and so `cargo xtask check`) runs the same comparison in `crates/openbrick-geom/tests/harness_meshes.rs`: byte for byte on x86-64 Linux, where the Q01 spike verified bit-identical kernel output, and counts, sizes, volumes and bounds on other platforms.

| LOD | Content | Triangles |
|-----|---------|-----------|
| 0 | shell, roof, 64-segment studs and tubes (the brick of `examples/kernel_timing.rs`; same fingerprint) | 3,620 |
| 1 | shell, roof, 16-segment studs, no tubes | 548 |
| 2 | stud-less hull box | 12 |

`manifest.json` records the kernel id, the standard version, the pitch, heights, body size, and per LOD the triangle and vertex counts, volume, bounds and mesh fingerprint (FNV-1a over positions and indices, as `openbrick_geom::Mesh::fingerprint`). The page reads the pitch, undersize and brick height from the manifest, so no brick number is written in JavaScript. The unit tests recompute every fingerprint from the binary files.

Each `.bin` file is OBMESH version 1, little-endian: a 32-byte header (`OBMESH\0\0`, format version, vertex count, index count, units code 1 = millimetres, up-axis code 2 = +Z, reserved), then `f32` positions, then `u32` triangle indices. `crates/openbrick-geom/examples/export_lods.rs` documents the layout field by field.

## Frames

Kernel meshes are millimetres in a right-handed frame with +Z up and the body's minimum corner at the origin. three.js is right-handed with +Y up. All bricks live under one group rotated by -90 degrees about +X, which maps (x, y, z) to (x, z, -y); the determinant is +1, so triangle winding and back-face culling are unchanged. One three.js unit is one millimetre. `src/frame.js` holds the conversion and a test checks it against three's own matrix.

## Scene and measurement

- Bricks sit on the stud lattice: a brick's cell corner is at integer stud coordinates times the pitch, with one empty stud between neighbours, in a grid with about the same extent in both directions.
- Chunks are square tiles of floor(sqrt(`chunk`)) x floor(sqrt(`chunk`)) bricks (64 x 64 = 4096 by default), each one `InstancedMesh` with its own bounding sphere, so frustum culling drops whole chunks.
- Kernel meshes share vertices across sharp edges and carry no normals, so the material shades flat from screen-space derivatives.
- The camera orbits the grid centre once every 40 seconds, at a radius of 0.6 times the grid extent and a height of 0.3 times it, so the visible chunks change over a run as they would while orbiting a model.
- Frames are timed from `requestAnimationFrame` after the warm-up; see the devices README for the statistics.

## Smoke test

`smoke.mjs` builds the page, serves `dist/` on 127.0.0.1, opens it in headless Chromium through Playwright and checks every row each run produces, both `window.__harnessResult` and the CSV text on the page, with `rowProblems()` from `src/csv.js`: every column filled except `device`, `warnings` and (without a GPU timer) the `gpu_*` columns, numbers finite and in range, and the columns consistent with each other (for example `fps_p5` = 1000 / `frame_ms_p95`). It also checks fps above zero, the mesh fingerprint and the `renderer.info` counts against the manifest. Runs: 1000 LOD0 bricks on the WebGL2 backend, the same on the WebGPU backend, a short WebGPU run with the GPU timer (both WebGPU runs can be skipped with `--no-webgpu`), and a two-run suite that exercises the page reloads. It also checks that the download button hands out exactly the CSV on the page. Runs use a 640 x 360 viewport and no MSAA to keep software rendering fast. Chromium renders with SwiftShader there, so its numbers describe a CPU, not a GPU; they are printed for information only and never belong in `bench/devices/results.csv`.

Playwright is a pinned dev dependency (1.63.0). Its browsers are not downloaded by `pnpm install`; on a fresh machine run `pnpm --dir bench/harness exec playwright install chromium` once (CI adds `--with-deps`).
