// Render benchmark of Build mode in headless Chromium: opens a large model through the page's own
// file input and reports how long the page took to show it, draw calls, triangles per frame, and
// frame times while orbiting (docs/plan/07 section 9, bench/scale/README.md).
//
// IMPORTANT, read the numbers with this in mind: headless Chromium on a CI machine renders with
// SwiftShader, a CPU implementation of WebGL. Its frame times say how many triangles and pixels
// the CPU could push, not how a laptop or phone GPU does. Draw calls and triangles per frame are
// real, device independent counts; fps and frame ms are SOFTWARE numbers (the csv column
// `renderer` says so). Real-device numbers come from `?bench=1` (apps/web/README.md).
//
//   pnpm --dir apps/web build          # the page, built
//   node bench/scale/render.mjs [--sizes 1000,5000,10000] [--layout city] [--frames 30] [--viewport 1280x800]
//                               [--json out.json] [--shots dir] [--timeout 600]

import { writeFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { dirname, join, normalize, extname, sep } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { layoutText } from "./layouts.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const web = join(here, "../../apps/web");
const dist = join(web, "dist");
const { chromium } = createRequire(join(web, "package.json"))("playwright");

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const sizes = opt("sizes", "1000,5000,10000").split(",").map(Number);
const layout = opt("layout", "city");
const frames = Number(opt("frames", "3"));
const [width, height] = opt("viewport", "1280x800").split("x").map(Number);
const timeoutMs = Number(opt("timeout", "600")) * 1000;
const jsonOut = opt("json", null);
const shots = opt("shots", null);
const phone = args.includes("--phone");

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png" };
function serve(root) {
  const server = createServer(async (req, res) => {
    try {
      const { pathname } = new URL(req.url, "http://127.0.0.1");
      let path = normalize(join(root, decodeURIComponent(pathname)));
      if (path !== root && !path.startsWith(root + sep)) throw new Error("outside root");
      if ((await stat(path)).isDirectory()) path = join(path, "index.html");
      res.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream", "cache-control": "no-store" });
      res.end(await readFile(path));
    } catch {
      res.writeHead(404);
      res.end("not found");
    }
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok({ server, origin: `http://127.0.0.1:${server.address().port}` })));
}

const percentile = (v, p) => [...v].sort((a, b) => a - b)[Math.min(v.length - 1, Math.floor(v.length * p))];
const tmp = mkdtempSync(join(tmpdir(), "openbrick-scale-"));
const { server, origin } = await serve(dist);
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const rows = [];
try {
  for (const bricks of sizes) {
    const context = await browser.newContext(
      phone ? { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, hasTouch: true, isMobile: true } : { viewport: { width, height }, deviceScaleFactor: 1 },
    );
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(`${origin}/?backend=webgl2&mode=build`);
    await page.waitForFunction(() => window.__web?.build?.ready === true, null, { timeout: 120_000 });
    const file = join(tmp, `${layout}-${bricks}.obrick`);
    writeFileSync(file, layoutText(layout, bricks));
    const row = { layout, bricks, renderer: "SwiftShader (software GL), not a device", viewport: phone ? "390x844@2" : `${width}x${height}@1` };
    const t0 = Date.now();
    await page.setInputFiles("#build-file", file);
    try {
      await page.waitForFunction(() => window.__web.build.parts > 1 && window.__web.build.loading !== true, null, { timeout: timeoutMs, polling: 100 });
      row.openMs = Date.now() - t0;
      const b = await page.evaluate(() => JSON.parse(JSON.stringify({ ...window.__web.build, project: undefined, measure: undefined, bench: undefined })));
      row.parts = b.parts;
      row.openCoreMs = b.openTiming?.coreMs ?? null;
      row.openPageMs = b.openTiming?.pageMs ?? null;
      row.hudLod = b.counts?.lod ?? null;
      row.chunks = b.counts?.chunks ?? null;
      row.drawCalls = b.counts?.drawCalls;
      row.triangles = b.counts?.triangles;
      row.detail = b.detail;
      row.lod = b.lod ?? null;
      row.floating = b.stats?.floating?.length ?? null;
      // The page's own frame counter runs at the compositor's pace even when the (software) GPU is far
      // behind: frames queue up in the GPU process. So the frame time is taken as the page orbits for
      // `frames` frames and then waits (readPixels) until the GPU has drawn all of them, divided by
      // the number of frames, after draining whatever was queued before.
      const timing = frames === 0 ? null : await page.evaluate(async (n) => {
        const gl = document.querySelector("#stage canvas").getContext("webgl2");
        const px = new Uint8Array(4);
        const sync = () => gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
        const d0 = performance.now();
        sync();
        const drainMs = performance.now() - d0;
        const t0 = performance.now();
        await window.__web.build.measure(n);
        sync();
        return { drainMs, perFrameMs: (performance.now() - t0) / n };
      }, frames);
      if (timing) {
        row.frameMs = +timing.perFrameMs.toFixed(0);
        row.fps = +(1000 / timing.perFrameMs).toFixed(3);
        row.drainMs = +timing.drainMs.toFixed(0);
      }
      const bench = await page.evaluate(() => window.__web.build.bench(10));
      row.snapMs = +bench.snapMs.toFixed(2);
      row.pickMs = +bench.pickMs.toFixed(2);
      row.placeUndoMs = bench.placeUndoMs == null ? null : +bench.placeUndoMs.toFixed(2);
      row.heapMiB = await page.evaluate(() => (performance.memory ? +(performance.memory.usedJSHeapSize / 1048576).toFixed(0) : null));
      if (shots) {
        mkdirSync(shots, { recursive: true });
        await page.screenshot({ path: join(shots, `${layout}-${bricks}${phone ? "-390" : ""}.png`), timeout: 300_000 });
      }
    } catch (err) {
      row.error = `${String(err.message).split("\n")[0]} (after ${Math.round((Date.now() - t0) / 1000)} s)`;
    }
    if (errors.length) row.pageErrors = errors.slice(0, 3);
    rows.push(row);
    console.error(JSON.stringify(row));
    await context.close();
  }
} finally {
  await browser.close();
  server.close();
  rmSync(tmp, { recursive: true, force: true });
}
const cols = [
  ["layout", (r) => r.layout],
  ["bricks", (r) => r.bricks],
  ["open s (wall)", (r) => (r.openMs == null ? "-" : (r.openMs / 1000).toFixed(1))],
  ["open: core + page ms", (r) => (r.openCoreMs == null ? "-" : `${r.openCoreMs} + ${r.openPageMs}`)],
  ["draw calls", (r) => r.drawCalls ?? "-"],
  ["triangles/frame", (r) => r.triangles ?? "-"],
  ["frame ms (software)", (r) => r.frameMs ?? "-"],
  ["fps (software)", (r) => r.fps ?? "-"],
  ["snap ms", (r) => r.snapMs ?? "-"],
  ["place+undo ms", (r) => r.placeUndoMs ?? "-"],
  ["error", (r) => r.error ?? ""],
];
console.log(`| ${cols.map((c) => c[0]).join(" | ")} |`);
console.log(`|${cols.map(() => "---").join("|")}|`);
for (const r of rows) console.log(`| ${cols.map((c) => c[1](r)).join(" | ")} |`);
if (jsonOut) writeFileSync(jsonOut, JSON.stringify({ rows }, null, 2));
