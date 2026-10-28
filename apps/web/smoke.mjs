// Smoke test of the browser pipeline: the Rust core as WebAssembly in a Web Worker,
// meshes shown by the three.js viewer (docs/plan/02 sections 1 to 3).
//
// 1. Builds the core (`cargo xtask wasm`) and the app (vite build), unless --no-build.
// 2. Gets the native kernel values for the 2x4 and 2x2 bricks from
//    `cargo run --release -p openbrick-geom --example kernel_timing`, and checks the
//    2x4 row against LOD0 of bench/harness/public/meshes/manifest.json.
// 3. Serves dist/ on 127.0.0.1 and opens it in headless Chromium with Playwright,
//    once on the WebGL2 backend and (unless --no-webgpu) once on WebGPU. Each run
//    waits for the first brick and checks that its triangle count, fingerprint,
//    canonical fingerprint and volume equal the native values for 2x4 and that the
//    viewer drew it; then steps the length down to 2 with the - button and checks the
//    2x2 values; then asks for 16x16 and, while that runs, for 2x4 again, which must
//    cancel the long build by replacing the worker and give the native 2x4 again.
// 4. A phone-sized run (390 x 844, touch, dark theme): no horizontal scrolling,
//    every control on screen, a different background from the light theme, a tap on
//    a + button, and a one-finger touch drag that orbits the camera.
//
// Usage: node smoke.mjs [--no-build] [--no-webgpu] [--screenshots <dir>]
//
// Chromium renders with SwiftShader here, so this proves the page works; it says
// nothing about speed on a device.

import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const repo = resolve(here, "../..");
const dist = join(here, "dist");
const argv = process.argv.slice(2);
const args = new Set(argv);
const shotsAt = argv.indexOf("--screenshots");
const shots = shotsAt >= 0 ? resolve(argv[shotsAt + 1] ?? "smoke-screenshots") : null;
// CommonJS resolution finds the local devDependency first and otherwise honours
// NODE_PATH, so a globally installed Playwright works too.
const { chromium } = createRequire(import.meta.url)("playwright");

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
};

function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, { cwd: repo, encoding: "utf8", ...options });
  if (result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(" ")} failed (${result.status ?? result.signal})\n${result.stderr ?? ""}`);
  }
  return result.stdout ?? "";
}

/** Rows of kernel_timing: size, ms, triangles, volume, fingerprint, canonical. */
function nativeValues() {
  const out = run("cargo", ["run", "-q", "--release", "-p", "openbrick-geom", "--example", "kernel_timing"]);
  const rows = new Map();
  for (const line of out.split("\n")) {
    const f = line.trim().split(/\s+/);
    if (/^\d+x\d+$/.test(f[0]) && f.length >= 6) {
      rows.set(f[0], { triangles: Number(f[2]), volumeMm3: Number(f[3]), fingerprint: f[4], canonicalFingerprint: f[5] });
    }
  }
  const kernel = /^kernel: (.+)$/m.exec(out)?.[1];
  for (const size of ["2x2", "2x4"]) if (!rows.has(size)) throw new Error(`kernel_timing printed no ${size} row:\n${out}`);
  const manifest = JSON.parse(readFileSync(join(repo, "bench/harness/public/meshes/manifest.json"), "utf8"));
  const lod0 = manifest.lods.find((l) => l.lod === 0);
  const native = rows.get("2x4");
  if (lod0.triangles !== native.triangles || lod0.fingerprint !== native.fingerprint) {
    throw new Error(`kernel_timing 2x4 (${native.triangles}, ${native.fingerprint}) differs from the harness LOD0 (${lod0.triangles}, ${lod0.fingerprint})`);
  }
  return { kernel, "2x4": native, "2x2": rows.get("2x2") };
}

function serve(root) {
  const server = createServer(async (req, res) => {
    try {
      const { pathname } = new URL(req.url, "http://127.0.0.1");
      let path = normalize(join(root, decodeURIComponent(pathname)));
      if (path !== root && !path.startsWith(root + sep)) throw new Error("outside root");
      if ((await stat(path)).isDirectory()) path = join(path, "index.html");
      const body = await readFile(path);
      res.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream", "cache-control": "no-store" });
      res.end(body);
    } catch {
      res.writeHead(404, { "content-type": "text/plain" });
      res.end("not found");
    }
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

const failures = [];
function check(condition, message) {
  if (!condition) failures.push(message);
  return condition;
}

/** Wait until window.__web shows a finished brick that satisfies `want`, or an error. */
async function waitBrick(page, want, timeoutMs, label) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const web = await page.evaluate(() => {
      const w = window.__web;
      return w && { state: w.state, error: w.error, brick: w.brick, restarts: w.restarts, backend: w.backend, warnings: w.warnings };
    });
    if (web?.state === "error") throw new Error(`${label}: page error: ${web.error}`);
    if (web?.state === "ready" && web.brick && web.brick.width === want.width && web.brick.length === want.length) return web;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`${label}: no ${want.width}x${want.length} brick within ${timeoutMs / 1000} s`);
}

function checkBrick(brick, native, label) {
  check(brick.triangles === native.triangles, `${label}: ${brick.triangles} triangles, native ${native.triangles}`);
  check(brick.fingerprint === native.fingerprint, `${label}: fingerprint ${brick.fingerprint}, native ${native.fingerprint}`);
  check(
    brick.canonicalFingerprint === native.canonicalFingerprint,
    `${label}: canonical fingerprint ${brick.canonicalFingerprint}, native ${native.canonicalFingerprint}`,
  );
  // kernel_timing prints the volume to three decimals.
  check(Math.abs(brick.volumeMm3 - native.volumeMm3) <= 5e-4, `${label}: volume ${brick.volumeMm3}, native ${native.volumeMm3}`);
}

async function stats(page) {
  return page.evaluate(() => window.__web.stats());
}

/** Viewer stats of the first frame that drew the newest brick (frames are on demand). */
async function drawnStats(page, label) {
  const end = Date.now() + 30_000;
  while (Date.now() < end) {
    const s = await stats(page);
    if (s && s.shown > 0 && s.drawn === s.shown) return s;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error(`${label}: the viewer did not draw the newest brick within 30 s`);
}

async function backendRun(browser, origin, backend, native) {
  const label = backend;
  const context = await browser.newContext({ viewport: { width: 960, height: 600 }, deviceScaleFactor: 1, colorScheme: "light" });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  const t0 = Date.now();
  await page.goto(`${origin}?backend=${backend}`);
  let web = await waitBrick(page, { width: 2, length: 4 }, 120_000, label);
  const firstMs = Date.now() - t0;
  check(web.backend === backend, `${label}: renderer backend is ${web.backend}`);
  checkBrick(web.brick, native["2x4"], `${label} 2x4`);
  check(web.brick.kernel === native.kernel, `${label}: kernel ${web.brick.kernel}, native ${native.kernel}`);
  // A frame draws the brick twice (shadow map and scene), the ground and the output pass.
  let s = await drawnStats(page, label);
  check(s.triangles >= native["2x4"].triangles, `${label}: the frame with the 2x4 drew ${s.triangles} triangles`);
  const text = await page.textContent("#status");
  check(text.includes("3,620 triangles") || text.includes(`${native["2x4"].triangles.toLocaleString("en-US")} triangles`), `${label}: status line "${text}"`);
  check(text.includes(native["2x4"].fingerprint), `${label}: status line lacks the fingerprint: "${text}"`);
  if (shots) await page.screenshot({ path: join(shots, `${backend}-2x4.png`) });

  // Two taps on "one stud shorter": 2x4 -> 2x3 -> 2x2 (the 2x3 may be superseded).
  await page.click('button[data-for="length"][data-step="-1"]');
  await page.click('button[data-for="length"][data-step="-1"]');
  web = await waitBrick(page, { width: 2, length: 2 }, 60_000, `${label} 2x2`);
  check(web.brick.triangles !== native["2x4"].triangles, `${label}: 2x2 has the 2x4 triangle count`);
  checkBrick(web.brick, native["2x2"], `${label} 2x2`);
  check(new URL(page.url()).search.includes("w=2&l=2"), `${label}: URL ${page.url()} does not record the size`);
  s = await drawnStats(page, `${label} 2x2`);
  check(s.triangles >= native["2x2"].triangles, `${label}: the frame with the 2x2 drew ${s.triangles} triangles`);

  // A 16x16 build takes tens of seconds; asking for 2x4 while it runs must cancel it.
  await page.fill("#width", "16");
  await page.fill("#length", "16");
  await new Promise((r) => setTimeout(r, 1500));
  const t1 = Date.now();
  await page.fill("#width", "2");
  await page.fill("#length", "4");
  web = await waitBrick(page, { width: 2, length: 4 }, 60_000, `${label} cancel`);
  const cancelMs = Date.now() - t1;
  check(web.restarts >= 1, `${label}: the 16x16 build was not cancelled (restarts ${web.restarts})`);
  checkBrick(web.brick, native["2x4"], `${label} 2x4 from a fresh worker`);
  if (shots) await page.screenshot({ path: join(shots, `${backend}-after-cancel.png`) });

  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  check(web.warnings.length === 0, `${label}: page warnings: ${web.warnings.join(" | ")}`);
  console.log(
    `# ${label}: first brick after ${(firstMs / 1000).toFixed(1)} s (built in ${Math.round(web.brick.buildMs)} ms), ` +
      `2x4 ${web.brick.triangles} triangles ${web.brick.fingerprint}, 2x2 ${native["2x2"].triangles} triangles, ` +
      `16x16 cancelled after ${(cancelMs / 1000).toFixed(1)} s with ${web.restarts} worker restart(s)`,
  );
  await context.close();
}

async function phoneRun(browser, origin, native) {
  const label = "phone";
  const background = async (scheme) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: scheme });
    const page = await context.newPage();
    await page.goto(`${origin}?backend=webgl2`);
    const value = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await context.close();
    return value;
  };
  const light = await background("light");
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    deviceScaleFactor: 2,
    isMobile: true,
    hasTouch: true,
    colorScheme: "dark",
  });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.goto(`${origin}?backend=webgl2`);
  let web = await waitBrick(page, { width: 2, length: 4 }, 120_000, label);
  checkBrick(web.brick, native["2x4"], `${label} 2x4`);
  const layout = await page.evaluate(() => {
    const inside = [...document.querySelectorAll("button, input, h1, #status")].map((el) => {
      const r = el.getBoundingClientRect();
      return { what: el.id || el.getAttribute("aria-label") || el.tagName, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
    });
    return {
      scrollWidth: document.documentElement.scrollWidth,
      bodyScrollWidth: document.body.scrollWidth,
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      inside,
      background: getComputedStyle(document.body).backgroundColor,
      canvas: document.querySelector("#stage canvas")?.getBoundingClientRect().toJSON(),
    };
  });
  check(layout.scrollWidth <= layout.innerWidth && layout.bodyScrollWidth <= layout.innerWidth, `${label}: horizontal scroll (${layout.scrollWidth} > ${layout.innerWidth})`);
  for (const r of layout.inside) {
    check(r.left >= 0 && r.right <= layout.innerWidth + 0.5 && r.top >= 0 && r.bottom <= layout.innerHeight + 0.5, `${label}: ${r.what} is off screen (${JSON.stringify(r)})`);
  }
  check(layout.canvas && layout.canvas.height > layout.innerHeight / 2, `${label}: the 3D view is only ${layout.canvas?.height} px tall`);
  check(layout.background !== light, `${label}: dark theme background ${layout.background} equals the light one`);
  if (shots) await page.screenshot({ path: join(shots, "phone-dark-2x4.png") });

  await page.tap('button[data-for="width"][data-step="1"]');
  web = await waitBrick(page, { width: 3, length: 4 }, 60_000, `${label} tap`);

  // One-finger drag on the canvas through the DevTools protocol, so Chromium makes
  // real touch pointers (OrbitControls captures them); the camera must move.
  const before = (await stats(page)).camera;
  const cdp = await context.newCDPSession(page);
  const { x, y, width, height } = layout.canvas;
  const at = (dx) => [{ x: x + width / 2 + dx, y: y + height / 2 }];
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: at(0) });
  for (let i = 1; i <= 8; i++) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: at(i * 15) });
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await new Promise((r) => setTimeout(r, 500));
  const after = (await stats(page)).camera;
  const moved = Math.hypot(after[0] - before[0], after[1] - before[1], after[2] - before[2]);
  check(moved > 1, `${label}: a touch drag moved the camera by ${moved.toFixed(3)} mm`);
  if (shots) await page.screenshot({ path: join(shots, "phone-dark-3x4-orbited.png") });
  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  console.log(`# ${label}: no horizontal scroll at ${layout.innerWidth} px, dark background ${layout.background} (light ${light}), tap to 3x4, touch drag moved the camera ${moved.toFixed(1)} mm`);
  await context.close();
}

async function main() {
  if (!args.has("--no-build")) {
    run("cargo", ["xtask", "wasm"], { stdio: "inherit" });
    const { build } = await import("vite");
    await build({ root: here, logLevel: "warn" });
  }
  if (shots) mkdirSync(shots, { recursive: true });
  const native = nativeValues();
  console.log(`native (${native.kernel}): 2x4 ${JSON.stringify(native["2x4"])}, 2x2 ${JSON.stringify(native["2x2"])}`);
  const server = await serve(dist);
  const origin = `http://127.0.0.1:${server.address().port}/`;
  const browser = await chromium.launch({
    headless: true,
    args: [
      // WebGL2 through ANGLE on SwiftShader: there is no GPU on CI runners.
      "--use-angle=swiftshader",
      "--enable-unsafe-swiftshader",
      // WebGPU on SwiftShader's Vulkan driver, which ships with Chromium.
      "--enable-unsafe-webgpu",
      "--enable-features=Vulkan",
      "--use-vulkan=swiftshader",
      "--use-webgpu-adapter=swiftshader",
    ],
  });
  try {
    for (const backend of args.has("--no-webgpu") ? ["webgl2"] : ["webgl2", "webgpu"]) {
      try {
        await backendRun(browser, origin, backend, native);
      } catch (err) {
        failures.push(err.message);
      }
    }
    try {
      await phoneRun(browser, origin, native);
    } catch (err) {
      failures.push(err.message);
    }
  } finally {
    await browser.close();
    server.close();
  }
  if (failures.length > 0) {
    console.error(`\nSMOKE FAILED:\n- ${failures.join("\n- ")}`);
    process.exit(1);
  }
  console.log("SMOKE OK");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
