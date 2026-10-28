# Web app

The browser pipeline that the playground builds on ([02 sections 1 to 3](../../docs/plan/02-architecture.md#3-the-wasm-boundary-and-threading), decisions D10 and D14). The Rust core is compiled to WebAssembly with wasm-bindgen and runs in a Web Worker. The page shows the meshes it returns with the three.js `WebGPURenderer`, which falls back to WebGL2 where WebGPU is missing. For now the page has one control row, the width and length of a nominal brick-compatible brick (1 to 16 studs). It re-requests the brick from the worker whenever you change either value. The status line shows the triangle count, volume and mesh fingerprint the core reported.

JavaScript computes no geometry ([CLAUDE.md](../../CLAUDE.md)). The brick comes from `openbrick_geom::nominal`, the same construction as the render harness's LOD0, so the 2x4 here has the native kernel's fingerprint (`0xfa969a6c0bf657e7` with `manifold-rust 0.19.1`).

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
```

`cargo xtask wasm` builds `crates/openbrick-wasm` in release for `wasm32-unknown-unknown`. It then runs `wasm-bindgen --target web` into `src/wasm/`, which is generated and gitignored, and prints the module size. It fails if the gzipped module exceeds the 2.5 MB budget in [10 section 5](../../docs/plan/10-repo-layout-and-dev-tooling.md#5-ci-matrix). Today the module is 1,525,931 bytes raw and 484,536 bytes gzipped. The xtask measures gzip with flate2 at its best compression; GNU `gzip -9` uses another deflate encoder and gives 483,098 bytes for the same file.

URL parameters: `?w=3&l=6` sets the first brick, and the page keeps them up to date as you change the size. `?backend=webgl2` forces the WebGL2 backend; `webgpu` and `auto` (the default) let three.js choose.

### On a phone on the same network

```sh
pnpm --dir apps/web start --host      # or, with the core already built: pnpm --dir apps/web dev --host
```

Vite prints a `Network:` URL such as `http://192.168.1.20:5173/`. Open it on the phone, which must be on the same Wi-Fi. If the page does not load, allow port 5173 through the computer's firewall. One finger orbits, two fingers pinch to zoom and pan, and the + and - buttons are 40 px touch targets. The orbit goes all the way round: drag up to look at the underside (tubes, or pins under 1-wide bricks). While the camera is below the ground, the grid and the contact shadow hide.

Browsers expose WebGPU only in secure contexts, and `http://192.168.1.20:5173/` is not one, so a phone on plain HTTP renders with WebGL2 (the status line says which). To try WebGPU on a phone, use `adb reverse tcp:5173 tcp:5173` and http://localhost:5173/ on Android. Or serve HTTPS with a locally trusted certificate made with [mkcert](https://github.com/FiloSottile/mkcert), kept outside the repository:

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
CoreClient (src/client.js) --- init(module) --> instantiate the core
                           --- brick(w, l) ---> build_nominal_brick(w, l)   (Rust, manifold-rust)
viewer (src/viewer.js)   <--- positions, indices (moved out of Rust, copied once
                              out of wasm memory, then transferred),
                              triangles, volume, fingerprints, bounds
```

| Path | What it is |
|------|------------|
| `src/main.js`, `index.html` | the page: control row, status line, wiring; light and dark themes from CSS custom properties and `prefers-color-scheme` |
| `src/client.js` | the page's side of the worker: request ids, latest-wins builds, cancelling by replacing the worker |
| `src/worker.js` | the worker: loads the core and answers requests with the handler from `src/protocol.js` |
| `src/protocol.js` | the messages, their validation and the worker's handler, unit-tested against a fake core |
| `src/viewer.js` | three.js scene: renderer, orbit controls, lattice grid, studio lights, contact shadow |
| `src/placement.js` | display transforms only: lattice placement and camera framing |
| `src/ui.js` | URL parameters, stepper limits, status text (including the note on slow builds) |
| `src/wasm/` | generated by `cargo xtask wasm` (gitignored) |
| `test/` | `node --test` unit tests; `core.test.js` loads the real core when `src/wasm/` exists and is skipped otherwise |
| `smoke.mjs` | end-to-end check in headless Chromium |

### Messages

Every request carries an `id`, and the worker answers each one with `{ id, ok: true, result }` or `{ id, ok: false, error }`. The error reply also has `fatal: true` when the core trapped (a WebAssembly `RuntimeError`, which is how a Rust panic ends on wasm32). `init` (with the compiled module) loads the core and returns `{ protocol, kernel, maxStuds, standard }`. `brick` (`width`, `length`) returns the mesh and its numbers. `checkPart` (`text`) returns the structural errors of a part document. `src/protocol.js` documents the fields. `evaluate_part` from the part evaluator joins as one more message of the same shape.

A running WebAssembly call cannot be interrupted. Builds are therefore "latest wins": a new size replaces any request still waiting. If the build in progress has run for 250 ms, the client terminates the worker and starts a new one from the already compiled module. Large bricks are slow with the pure-Rust kernel (Q01 finding 2 in [the spike report](../../docs/spikes/q01-kernel-conformance.md)). In Node on the shared build machine, 2x4 took 50 to 90 ms, 8x8 2.9 s and 16x16 about 40 s. So stepping away from a 16x16 shows the new brick at once instead of after the long build. Once a build has run for a second, the status line adds the seconds so far and says that changing the size cancels it.

After a trap, the instance's memory may be inconsistent. The worker therefore refuses every later request, and the client replaces it at once with a fresh worker from the same compiled module. The fresh worker also receives every request the old one still owed, with the original ids. The request that trapped fails with the error, and the page shows it; the next size change works as usual.

### Frames

The core returns millimetres in the kernel frame: right-handed, +Z up, with the body's minimum corner at the origin. three.js is right-handed with +Y up. The brick lives in a group rotated by -90 degrees about +X, which maps (x, y, z) to (x, z, -y). The determinant is +1, so triangle winding and back-face culling are unchanged, and one three.js unit is one millimetre. This conversion is `packages/viewport/src/frame.js`, the same module the render harness uses. Inside that group, the mesh is moved so that the brick's lattice cell corner lies on a grid line (`src/placement.js`), using the pitch and undersize the core reports from the Dimensional Standard. The grid has one cell per stud.

## Smoke test

`smoke.mjs` builds the core and the app (skip both with `--no-build`). It gets the native 2x4 and 2x2 values from `cargo run --release -p openbrick-geom --example kernel_timing` and checks the 2x4 row against LOD0 of the harness meshes. Then it serves `dist/` and opens the page in headless Chromium, once on WebGL2 and once on WebGPU (skip WebGPU with `--no-webgpu`). Each run:

- waits for the first brick and checks that the triangle count, fingerprint, canonical fingerprint and volume equal the native 2x4, and that a frame drew it;
- presses "one stud shorter" twice and checks the 2x2 values the same way;
- asks for 16x16, checks after 1.5 s that the status line shows the seconds and the cancel note, then asks for 2x4 while that build runs, and checks that the worker was replaced and the fresh worker gives the native 2x4 again;
- drags the view up by a quarter of its height, which turns the camera 90 degrees down, and checks that the camera ends below the ground and the viewer reports the underside view.

A phone-sized run (390 x 844, touch, dark theme) checks four things: there is no horizontal scrolling, every control is on screen, the dark background differs from the light one, and a tap on + and a one-finger drag (which must move the camera) both work. `--screenshots <dir>` saves what the runs saw; `apps/web/smoke-screenshots/`, the folder CI uses, is gitignored. Chromium renders with SwiftShader there, so the timings say nothing about a device.

Playwright is a pinned dev dependency (1.63.0). `pnpm install` does not download its browsers; on a fresh machine run `pnpm --dir apps/web exec playwright install chromium` once (CI adds `--with-deps`).
