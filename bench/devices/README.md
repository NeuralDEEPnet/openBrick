# Device render results

`results.csv` collects frame-time measurements of the device render harness (`bench/harness`) on real laptops and phones. They answer whether the renderer chosen in decision D10 ([03](../../docs/plan/03-tech-stack-decisions.md)) meets the budgets in [07 section 9](../../docs/plan/07-assembler-engine.md#9-rendering), and they are the trigger data for risks R02 and R11 in [13](../../docs/plan/13-risks-and-open-questions.md).

The harness renders N instanced 2x4 bricks built by the geometry kernel at one level of detail with the three.js `WebGPURenderer` (WebGPU backend, or its WebGL2 backend with `forceWebGL`), orbits the camera slowly, measures frames after a warm-up and prints one CSV row per run.

## Run it on a laptop

From the repository root:

```sh
pnpm install
pnpm --dir bench/harness dev
```

Open the printed `Local:` URL (normally http://localhost:5173/). The default run is 10,000 bricks at LOD0 on WebGPU, measured for 10 seconds after a 2-second warm-up. When it finishes, the page shows a summary, the CSV header and row, and buttons to copy or download them. "Change settings" opens a form for every parameter below.

To serve the production build instead: `pnpm --dir bench/harness build` and then `pnpm --dir bench/harness preview --host`.

## Run it on a phone on the same network

```sh
pnpm --dir bench/harness dev --host
```

Vite prints a `Network:` URL such as `http://192.168.1.20:5173/`. Open it on the phone, which must be on the same Wi-Fi; allow port 5173 through the laptop's firewall if the page does not load.

**WebGPU needs a secure context.** `http://192.168.1.20:5173/` is not one, so the phone falls back to WebGL2; the row then says `backend=webgl2` and has a warning. To measure WebGPU on a phone, use one of these:

1. **Android over USB:** `adb reverse tcp:5173 tcp:5173`, then open http://localhost:5173/ on the phone. `localhost` counts as a secure context.
2. **HTTPS with a locally trusted certificate (Android and iOS):** create one with [mkcert](https://github.com/FiloSottile/mkcert) in a directory outside the repository, so the private key can never be committed, and install mkcert's root certificate on the phone:

   ```sh
   mkcert -install
   mkdir -p ~/.config/openbrick-harness
   (cd ~/.config/openbrick-harness && mkcert 192.168.1.20)
   HARNESS_TLS_CERT="$HOME"/.config/openbrick-harness/192.168.1.20.pem \
   HARNESS_TLS_KEY="$HOME"/.config/openbrick-harness/192.168.1.20-key.pem \
   pnpm --dir bench/harness dev --host
   ```

   Open `https://192.168.1.20:5173/`. (The repository's `.gitignore` also ignores `*.pem` as a safety net.)
3. **Chrome on Android, for a quick check only:** add the LAN URL to `chrome://flags/#unsafely-treat-insecure-origin-as-secure`.

Keep the page in front and the screen on for the whole run; a hidden tab pauses rendering and the row gets a warning.

## URL parameters

| Parameter | Default | Meaning |
|-----------|---------|---------|
| `count` | 10000 | number of bricks; a `k` suffix is accepted (`100k`) |
| `lod` | 0 | 0: full brick (64-segment studs and tubes); 1: 16-segment studs, no tubes; 2: stud-less hull box |
| `backend` | webgpu | `webgpu` or `webgl2` (the WebGPURenderer with `forceWebGL`) |
| `seconds` | 10 | measured duration |
| `warmup` | 2 | warm-up before measuring; at least 3 frames are rendered first unless it is 0 |
| `dpr` | 2 | cap on the device pixel ratio; the page renders at min(devicePixelRatio, dpr) |
| `aa` | 1 | 1 requests MSAA, 0 turns it off |
| `gpu` | 0 | 1 also measures GPU time per frame with timestamp queries (WebGPU `timestamp-query`, WebGL2 `EXT_disjoint_timer_query_webgl2`) where the browser offers them; useful when fps sits at the refresh rate. Off by default because the WebGL2 path polls for results on the main thread |
| `chunk` | 4096 | maximum instances per InstancedMesh chunk (square tiles of floor(sqrt(chunk)) bricks a side) |
| `device` | empty | free-text label copied into the `device` column, for example `Pixel 8a, Chrome 141` |
| `suite` | 0 | 1 runs every combination of `counts` x `backends` x `lods`, one page load each, and shows all rows at the end |
| `counts` | 10000,100000 | bricks per suite run |
| `lods` | 0,1,2 | levels of detail per suite run |
| `backends` | webgpu,webgl2 | backends per suite run |

Unknown or out-of-range parameters stop the page with an error, so a typo cannot produce a row measured with the defaults.

Examples:

- `?suite=1&device=Pixel%208a` runs the standard matrix from [07](../../docs/plan/07-assembler-engine.md#9-rendering): 10k and 100k bricks, every LOD, both backends (12 runs, about 3 minutes on a fast device; slow configurations take longer because every run renders at least its warm-up frames).
- `?count=100k&lod=2&backend=webgl2&dpr=1` is the phone case "100k with LOD2 at reduced resolution".

## What to measure

The reference devices named in 07 section 9 are an Iris Xe laptop, a Radeon 780M laptop, a base M-series Mac, a Pixel 8a or 9a, a Galaxy A5x (Xclipse) and an iPhone 16e; any other device is welcome too. On each, run the standard suite with a `device` label. On phones, run it a second time with a lower cap such as `dpr=1`, because the phone budget in 07 assumes rendering below native resolution. Laptops should be on mains power.

## Add your rows

1. Copy the data rows (not the header) from the page, or download the CSV.
2. Append them to `results.csv` in a pull request, one row per run, unchanged except for the `device` label if you forgot to set it.
3. Do not add rows whose `software` column is `yes` or whose `warnings` contain a note starting with `error:` (notes are listed in the order they arose, so the error is not always first). Rows from a software rasteriser or a headless browser describe the CI machine, not a device. A row with other warnings (for example a WebGPU fallback) is fine; the warning explains it.
4. Run `pnpm test` from the repository root. The harness unit test checks every row of `results.csv` against the column rules in `bench/harness/src/csv.js` (every column filled except `device`, `warnings` and the `gpu_*` columns, numbers in range, columns consistent with each other) and rejects software and failed rows.

## How the numbers are measured

- **Frame time** is the interval between successive `requestAnimationFrame` timestamps (three.js `setAnimationLoop` drives rAF and passes its timestamp). Browsers cap it at the display refresh rate, so a median of 60 or 120 fps means "at least this fast"; raise `count` or pick a heavier LOD to find the limit. `frame_ms_p50` and `frame_ms_p95` are percentiles of the intervals with linear interpolation between closest ranks (the "linear" method of NumPy, R type 7). `fps_median` is 1000 / `frame_ms_p50` and `fps_p5` is 1000 / `frame_ms_p95`: frame rate falls as frame time rises, so the 5th percentile of frame rate is the 95th percentile of frame time, and deriving one from the other keeps both columns describing the same frames, also on a short run with one long stall. `fps_mean` is frames divided by measured seconds.
- **Triangles and draw calls** come from `renderer.info` after each frame, so they reflect per-chunk frustum culling. They include the renderer's output pass, which converts to the sRGB output colour space by drawing one full-screen triangle (one draw call).
- **CPU time** (`cpu_ms_*`) is the JavaScript time spent inside `renderer.render()`. Browsers coarsen `performance.now()` (often to 0.1 ms), so small values are approximate.
- **GPU time** (`gpu_ms_*`, only with `gpu=1`) comes from three.js timestamp queries, read back asynchronously, so it samples some frames rather than all of them. It is not capped by the refresh rate, which makes it the better headroom measure when `fps_median` sits at 60 or 120. Browsers may quantise these timers too.
- **Headless and software rendering** do not apply display back-pressure the way a real screen does, so their frame intervals are not meaningful; that is one more reason such rows never go into `results.csv`.
- **Scene:** kernel meshes from `bench/harness/public/meshes` (see `bench/harness/README.md`), chunked `InstancedMesh` with per-chunk bounding spheres, bricks on the stud lattice with one empty stud between neighbours, a per-instance colour from an eight-colour palette, flat shading, one hemisphere and one directional light, no shadows, and a camera that orbits the grid once every 40 seconds.

## CSV columns

`results.csv` starts with this header; `bench/harness/src/csv.js` is the source of truth, including the rule each value must meet, and a unit test keeps this table, that file and the header in step. Every column is filled in a finished run except `device` (optional label), `warnings` (empty when clean) and the `gpu_*` columns (empty without `gpu=1`; `gpu_ms_*` also empty when `gpu_samples` is 0).

| Column | Meaning |
|--------|---------|
| `schema` | CSV schema version of the harness (1) |
| `timestamp` | UTC time the measurement finished (ISO 8601) |
| `device` | free-text label from the `device` URL parameter |
| `backend_requested` | backend asked for in the URL (`webgpu` or `webgl2`) |
| `backend` | backend that actually rendered (`webgpu` or `webgl2`) |
| `adapter` | GPU string: WebGPU adapter info, or `WEBGL_debug_renderer_info` (vendor / renderer) on WebGL2 |
| `software` | `yes` when the adapter is a software rasteriser (SwiftShader, llvmpipe and similar); such rows are not device results |
| `lod` | level of detail (0, 1 or 2) |
| `count` | number of brick instances |
| `triangles_per_brick` | triangles in one brick at this LOD (from the mesh manifest) |
| `triangles_per_frame` | median triangles drawn per frame (`renderer.info`) after chunk culling, including the one-triangle output pass |
| `draw_calls` | median draw calls per frame (`renderer.info`), including the output pass |
| `chunks` | number of `InstancedMesh` chunks in the scene |
| `chunk_size` | maximum instances per chunk |
| `frames` | frames measured after warm-up |
| `seconds` | measured duration in seconds |
| `warmup` | warm-up in seconds, not measured (at least 3 frames unless 0) |
| `fps_median` | 1000 / `frame_ms_p50`: frame rate of the median frame |
| `fps_mean` | frames divided by measured seconds |
| `fps_p5` | 1000 / `frame_ms_p95`: frame rate of the slow frames (5th percentile of frame rate) |
| `frame_ms_p50` | median frame time in milliseconds |
| `frame_ms_p95` | 95th percentile frame time in milliseconds |
| `frame_ms_max` | longest frame in milliseconds |
| `cpu_ms_p50` | median JavaScript time inside `renderer.render()` in milliseconds (can read 0 where the browser coarsens its clock) |
| `cpu_ms_p95` | 95th percentile JavaScript time inside `renderer.render()` in milliseconds |
| `gpu_samples` | GPU timer readings taken; empty when `gpu=0`, 0 when the browser offers no GPU timer |
| `gpu_ms_p50` | median GPU time per frame from timestamp queries in milliseconds (`gpu=1` only), all render passes of a frame summed |
| `gpu_ms_p95` | 95th percentile GPU time per frame in milliseconds (`gpu=1` only) |
| `dpr_cap` | device pixel ratio cap from the `dpr` URL parameter |
| `device_pixel_ratio` | `window.devicePixelRatio` of the browser |
| `pixel_ratio` | pixel ratio actually used: min(`device_pixel_ratio`, `dpr_cap`) |
| `canvas_width` | drawing-buffer width in pixels |
| `canvas_height` | drawing-buffer height in pixels |
| `antialias` | 1 when MSAA was requested, 0 otherwise (WebGPU compatibility mode turns it off and adds a warning) |
| `three_revision` | three.js revision the page was built with (`THREE.REVISION`), so rows stay comparable across upgrades |
| `kernel` | geometry kernel that built the meshes (from the mesh manifest) |
| `mesh_fingerprint` | fingerprint of the LOD mesh (from the mesh manifest) |
| `user_agent` | `navigator.userAgent` |
| `warnings` | notes separated by `; ` that qualify the row, in the order they arose, empty when clean: backend fallback, insecure context, software rasteriser, hidden tab, resize, few frames, missing GPU timer, and `error: ...` when the run failed |

Values are plain numbers or text; text containing a comma, quote or line break is quoted as in RFC 4180.
