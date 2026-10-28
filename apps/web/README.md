# Web app: the playground

The M1 browser playground ([02 sections 1 to 3](../../docs/plan/02-architecture.md#3-the-wasm-boundary-and-threading), [06 sections 5, 11 and 13](../../docs/plan/06-part-designer-and-dsl.md#11-designer-ui-on-desktop-and-mobile), decisions D10 and D14). Pick a tile size, drop an SVG, see the brick-compatible logo tile in 3D with its validation report, and download a printable 3MF or STL. A second mode builds the calibration coupon ([05 section 4](../../docs/plan/05-3d-printing-pipeline.md#4-calibration-coupon-and-wizard)) and downloads it. Everything runs in the browser: the Rust core is compiled to WebAssembly with wasm-bindgen and runs in a Web Worker, and the page shows the meshes it returns with the three.js `WebGPURenderer`, which falls back to WebGL2 where WebGPU is missing.

It is published at <https://neuraldeepnet.github.io/openBrick/> once GitHub Pages is switched on for the repository (Settings > Pages > Source: GitHub Actions); the `pages` workflow builds it with the base path `/openBrick/`.

JavaScript computes no geometry and no validation ([CLAUDE.md](../../CLAUDE.md)). The tile comes from `openbrick_part::evaluate` with `parts/ndn.logo-tile.obpart.json`, the report from `openbrick_part::validate`, the files from `openbrick_io`, and the coupon from `openbrick_part::build_coupon`, all through `crates/openbrick-wasm`. The page builds its controls from the part document's `params`, reads files into bytes, and displays what comes back.

## What the page does

- **Logo tile** (the default mode). Controls come from the part document: the size (two stud steppers, 1 to 16 per side, the document's limits capped at the core's footprint limit), the height (plate or brick), the relief (emboss, deboss or inlay), the relief height (a slider with the document's minimum, maximum and step, hidden for an inlay by its `visibleWhen`) and the margin. A hidden parameter is not sent, so it takes the document's default. The artwork is the sample mark (`parts/assets/mark.svg`, CC0) until you drop an SVG on the drop zone or the 3D view, or choose one (the drop zone is a button: click, Enter or Space open the file picker). Files over the core's limit (4 MiB, `max_asset_bytes()`) are refused in the page with a message before anything is sent; the core refuses hostile SVGs with its own messages.
- **Printer.** The process profile (fdm-0.4, fdm-0.2, resin-msla) and the fit profile: the example (Prusa MK4S, PETG; illustrative values, not measured), "Load my fit profile…" (the JSON file `openbrick calibrate record` writes), or "None (uncalibrated)". With none, FIT.MISSING blocks the downloads, and the panel links to the coupon mode and the [print test guide](https://github.com/NeuralDEEPnet/openBrick/blob/main/docs/guides/print-test.md). Body and logo colours are pickers; they recolour the view and go into the 3MF.
- **Live 3D.** Every change re-evaluates after 150 ms. The body and the accent object show in their colours, and the artwork that does not print as drawn is drawn in red on the face, always with a legend entry that says how much (percent of the artwork area), so colour is never the only cue. The result lists the triangle count, the size in millimetres and the mass printed solid in PLA. A slow evaluation shows its seconds in the status line; any change cancels it.
- **Validation.** Every finding with an icon and the severity in words, the rule id, the message, the measured value and threshold, and the fix hint; blockers first. The evaluator's own notes follow (for an inlay, where the filament change goes). Download 3MF and Download STL stay off while any blocker stands, with the reason next to them, and the core refuses such an export too. Files are named `{id}-{w}x{h}-{relief}.3mf`, for example `ndn.logo-tile-2x4-emboss.3mf`. The 3MF has one object per colour role; the STL is the kernel's fused union, one solid for a single colour. An inlay is exported face down, as it prints.
- **Calibration coupon.** Process, pocket layout (cross, cross and diagonal, full) and bed size; the coupon in 3D, its stats, the cell table (the label on each cell of the first copy and what to record for it), Download coupon 3MF and STL, and how to use it with the `openbrick calibrate record` line.

URL parameters: `?mode=coupon` opens the coupon; `?backend=webgl2` forces the WebGL2 backend (`webgpu` and `auto`, the default, let three.js choose).

## Run it

You need the Rust toolchain with the `wasm32-unknown-unknown` target, Node 22, pnpm 10, and the pinned wasm-bindgen CLI. The CLI version must equal the `wasm-bindgen` crate in `Cargo.lock`; `cargo xtask wasm` checks this and prints the install command if it does not match.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version =0.2.129 --locked   # once; it compiles for a few minutes
pnpm install                                                 # once, from the repository root
pnpm --dir apps/web start                                    # build the core, then serve the app
```

`start` runs `cargo xtask wasm` and then Vite's dev server. Open the printed `Local:` URL (normally http://localhost:5173/). After that:

```sh
pnpm --dir apps/web dev        # serve again without rebuilding the core
pnpm --dir apps/web wasm       # rebuild the core after a Rust change (Vite reloads the page)
pnpm --dir apps/web build      # production build into dist/
pnpm --dir apps/web preview    # serve dist/
pnpm --dir apps/web test       # unit tests (node --test); the root pnpm test runs them too
pnpm --dir apps/web smoke      # build everything and check it in headless Chromium
OPENBRICK_BASE=/openBrick/ pnpm --dir apps/web build   # the GitHub Pages build
```

`OPENBRICK_BASE` is the path the app is served under, `/` by default. The page, the worker, the WebAssembly module and every asset resolve under it.

`cargo xtask wasm` builds `crates/openbrick-wasm` in release for `wasm32-unknown-unknown`. It then runs `wasm-bindgen --target web` into `src/wasm/`, which is generated and gitignored, and prints the module size. It fails if the gzipped module exceeds the 2.5 MB budget in [10 section 5](../../docs/plan/10-repo-layout-and-dev-tooling.md#5-ci-matrix). With the evaluator, usvg, the validator, the coupon and the 3MF and STL writers linked, the module is 2,997,817 bytes raw and 985,044 bytes gzipped (flate2 at its best compression), 39 percent of the budget.

The footer carries the non-affiliation disclaimer ([12 section 4](../../docs/plan/12-legal-and-naming.md#4-disclaimer-text)) from `crates/openbrick-units/disclaimer.txt`, the one file the CLI's `openbrick about` and the core's `disclaimer()` also read; `vite.config.js` writes it into the HTML.

### On a phone on the same network

```sh
pnpm --dir apps/web start --host      # or, with the core already built: pnpm --dir apps/web dev --host
```

Vite prints a `Network:` URL such as `http://192.168.1.20:5173/`. Open it on the phone, which must be on the same Wi-Fi. If the page does not load, allow port 5173 through the computer's firewall. The 3D view is on top and the controls scroll below it. One finger orbits, two fingers pinch to zoom and pan, and the + and - buttons are 40 px touch targets. The orbit goes all the way round: drag up to look at the underside (the tubes). While the camera is below the ground, the grid and the contact shadow hide.

Browsers expose WebGPU only in secure contexts, and `http://192.168.1.20:5173/` is not one, so a phone on plain HTTP renders with WebGL2. To try WebGPU on a phone, use `adb reverse tcp:5173 tcp:5173` and http://localhost:5173/ on Android. Or serve HTTPS with a locally trusted certificate made with [mkcert](https://github.com/FiloSottile/mkcert), kept outside the repository:

```sh
WEB_TLS_CERT="$HOME"/.config/openbrick-harness/192.168.1.20.pem \
WEB_TLS_KEY="$HOME"/.config/openbrick-harness/192.168.1.20-key.pem \
pnpm --dir apps/web dev --host
```

[`bench/devices/README.md`](../../bench/devices/README.md#run-it-on-a-phone-on-the-same-network) describes how to make the certificate and trust it on the phone.

## How it fits together

```text
main thread                                   Web Worker (src/worker.js)
-----------                                   --------------------------
compile openbrick_wasm_bg.wasm once
CoreClient (src/client.js) --- init(module) ---> instantiate the core
                           --- bundled ---------> profiles, part document, sample, disclaimer
                           --- evaluatePart ----> evaluate_part(doc, params, process, fit, assets)
                           --- exportPart ------> export_part(format, ...)  -> 3MF or STL bytes
                           --- buildCoupon -----> build_coupon(process, layout)
                           --- exportCoupon ----> export_coupon(format, ...)
viewer (src/viewer.js)   <--- positions, indices per colour role and the red overlay
                              (moved out of Rust, copied once out of wasm memory,
                              then transferred), report, measurements, file bytes
```

| Path | What it is |
|------|------------|
| `src/main.js`, `index.html` | the page: modes, controls, panels, downloads; light and dark themes from CSS custom properties and `prefers-color-scheme`; desktop sidebar and phone stacking |
| `src/params.js` | the part document's `params` to controls: kinds, limits, labels, groups, `visibleWhen`, the overrides sent, download names, the asset size message |
| `src/client.js` | the page's side of the worker: request ids, latest-wins builds, exports that are never superseded, cancelling by replacing the worker, trap recovery |
| `src/worker.js` | the worker: loads the core and answers requests with the handler from `src/protocol.js` |
| `src/protocol.js` | the messages, their validation and the worker's handler, unit-tested against a fake core |
| `src/viewer.js` | three.js scene: renderer, orbit controls, lattice grid, studio lights, contact shadow, one material per colour role |
| `src/placement.js` | display transforms only: lattice placement, centring and camera framing |
| `src/ui.js` | URL parameters, status, stats and validation panel text, the coupon table, the record command |
| `src/wasm/` | generated by `cargo xtask wasm` (gitignored) |
| `test/` | `node --test` unit tests; `core.test.js` loads the real core when `src/wasm/` exists and is skipped otherwise |
| `smoke.mjs` | end-to-end check in headless Chromium |

### Messages

Every request carries an `id`, and the worker answers each one with `{ id, ok: true, result }` or `{ id, ok: false, error }`. The error reply also has `fatal: true` when the core trapped (a WebAssembly `RuntimeError`, which is how a Rust panic ends on wasm32). `src/protocol.js` lists every message and its fields: `init`, `info`, `bundled`, `brick`, `checkPart`, `evaluatePart`, `exportPart`, `resolveParams`, `buildCoupon`, `couponLayout` and `exportCoupon`. Mesh arrays and file bytes in results are transferred, not copied. Asset bytes in requests are copied by `postMessage`, because the page keeps them to send them again to a replacement worker.

A running WebAssembly call cannot be interrupted. Builds (`brick`, `evaluatePart`, `buildCoupon`) are therefore "latest wins": a new one replaces any build still waiting. If the build in progress has run for 250 ms, the client terminates the worker and starts a new one from the already compiled module. In Node on the shared build machine the default 2x4 tile takes about 110 ms in WebAssembly, a 16x16 tile 2.2 s and the default coupon 0.55 s, so changing a 16x16 back to 2x4 shows the small tile at once. Exports (`exportPart`, `exportCoupon`) are never superseded: while one is owed, the client does not restart the worker, and it cancels a superseded build only after the export has its reply.

After a trap, the instance's memory may be inconsistent. The worker therefore refuses every later request, and the client replaces it at once with a fresh worker from the same compiled module. The fresh worker also receives every request the old one still owed, exports included, with the original ids. The request that trapped fails with the error, and the page shows it.

### Frames

The core returns millimetres in the kernel frame: right-handed, +Z up, with the body's minimum corner at the origin. three.js is right-handed with +Y up. The part lives in a group rotated by -90 degrees about +X, which maps (x, y, z) to (x, z, -y). The determinant is +1, so triangle winding and back-face culling are unchanged, and one three.js unit is one millimetre. This conversion is `packages/viewport/src/frame.js`, the same module the render harness uses. Inside that group, a tile is moved so that its lattice cell corner lies on a grid line (`src/placement.js`), using the pitch and undersize the core reports from the Dimensional Standard; the coupon is centred. The grid has one cell per stud.

## Smoke test

`smoke.mjs` builds the core and the app (skip both with `--no-build`). It gets the native values from `cargo run --release -p openbrick-wasm --example native_eval`, which calls the same Rust functions the worker calls. Then it serves `dist/` and opens the page in headless Chromium:

- WebGL2: the first tile (sample logo, emboss, example fit) has the native content hash, triangle count and per-object fingerprints and is drawn; the legend gives the removed percentage; the FEATURE.MIN warning shows its rule id, values and fix; an uploaded SVG and then "Use sample logo" give the native tile back; a file over the limit is refused before it is sent; deboss and inlay equal native (the inlay 3MF is downloaded); "None" disables both downloads with FIT.MISSING and the links shown; with the example fit the 3MF and STL download; a 16x16 tile shows its seconds and going back to 2x4 cancels it by replacing the worker; dragging up shows the underside; the coupon mode builds the native coupon, lists its cells and downloads its 3MF.
- WebGPU (skip with `--no-webgpu`): the first tile again.
- Phone (390 x 844, touch, dark theme): no horizontal scrolling, every control inside the width, a different background from the light theme, a tap on +, and a one-finger drag that orbits.
- GitHub Pages path (skip with `--no-base-check`): the app built with `OPENBRICK_BASE=/openBrick/` into a temporary directory and served only under `/openBrick/` loads everything from there and gives the native tile.

The downloads go to `apps/web/smoke-downloads/` (gitignored; `--downloads <dir>` for another place), and CI then reads every 3MF with lib3mf in strict mode (`scripts/validate-3mf.py`). `--screenshots <dir>` saves what the runs saw; `apps/web/smoke-screenshots/`, the folder CI uses, is gitignored. Chromium renders with SwiftShader there, so the timings say nothing about a device.

Playwright is a pinned dev dependency (1.63.0). `pnpm install` does not download its browsers; on a fresh machine run `pnpm --dir apps/web exec playwright install chromium` once (CI adds `--with-deps`).
