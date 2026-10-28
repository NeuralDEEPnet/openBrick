// Smoke test of the device render harness (backlog item 3 in docs/plan/14).
//
// Builds the page with vite, serves dist/ on 127.0.0.1, opens it in headless
// Chromium through Playwright and checks every row a run produces, both
// window.__harnessResult and the CSV text on the page, with rowProblems() from
// src/csv.js: every column filled except device, warnings and (without a GPU timer)
// the gpu_* columns, numbers finite and in range, and the columns consistent with
// each other. It also checks fps > 0 and the renderer.info counts against the mesh
// manifest. Runs: WebGL2 and (unless --no-webgpu) WebGPU, a short WebGPU run with
// the GPU timer, and a two-step suite to check the reload chain of suite mode.
//
// Usage: node smoke.mjs [--no-webgpu] [--no-build]
//
// The numbers it prints come from a software rasteriser in a headless browser. They
// prove the page works; they are not device results and must never be added to
// bench/devices/results.csv.

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { COLUMN_NAMES, csvHeader, parseCsv, rowFromFields, rowProblems } from "./src/csv.js";

const here = fileURLToPath(new URL(".", import.meta.url));
const dist = join(here, "dist");
const args = new Set(process.argv.slice(2));
// CommonJS resolution finds the local devDependency first and otherwise honours
// NODE_PATH, so a globally installed Playwright works too.
const { chromium } = createRequire(import.meta.url)("playwright");

// Software rasterisers are slow at the 3.6 M triangles of 1000 LOD0 bricks, so the
// smoke runs use a small viewport and no MSAA; the checks do not depend on either.
const VIEWPORT = { width: 640, height: 360 };
const RUNS = [
  {
    name: "webgl2, 1000 bricks, LOD0",
    query: "count=1000&lod=0&backend=webgl2&seconds=2&aa=0",
    expect: { backend: "webgl2", count: 1000, lod: 0 },
  },
  {
    name: "suite: 200 bricks, LOD1 and LOD2, webgl2",
    query: "suite=1&counts=200&lods=1,2&backends=webgl2&seconds=0.5&warmup=0.2&aa=0",
    suiteRows: 2,
    expect: { backend: "webgl2", count: 200 },
  },
];
if (!args.has("--no-webgpu")) {
  RUNS.push(
    {
      name: "webgpu, 1000 bricks, LOD0",
      query: "count=1000&lod=0&backend=webgpu&seconds=2&aa=0",
      expect: { backend: "webgpu", count: 1000, lod: 0 },
    },
    {
      name: "webgpu with the GPU timer, 200 bricks, LOD2",
      query: "count=200&lod=2&backend=webgpu&seconds=1&warmup=0.5&aa=0&gpu=1",
      expect: { backend: "webgpu", count: 200, lod: 2 },
      gpuTimer: true,
    },
  );
}

/**
 * The WebGPURenderer converts to the sRGB output colour space in an extra pass that
 * draws one full-screen triangle (Renderer.needsFrameBufferTarget in three r186),
 * so renderer.info counts one draw call and one triangle more than the bricks.
 */
const OUTPUT_PASS = { drawCalls: 1, triangles: 1 };

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".bin": "application/octet-stream",
};

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

/** Poll for window.__harnessDone; survives the page reloads of suite mode. */
async function waitDone(page, timeoutMs) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const done = await page.evaluate(() => window.__harnessDone === true).catch(() => false);
    if (done) return;
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`no result within ${timeoutMs / 1000} s`);
}

function check(condition, message, failures) {
  if (!condition) failures.push(message);
}

function checkRow(row, expect, manifest, failures, label) {
  check(row && !row.error, `${label}: page reported an error: ${row?.error}`, failures);
  if (!row || row.error) return;
  // Empty, missing, malformed, out-of-range and inconsistent values (src/csv.js).
  for (const problem of rowProblems(row)) failures.push(`${label}: ${problem}`);
  for (const [k, v] of Object.entries(expect)) check(row[k] === v, `${label}: ${k} is ${row[k]}, expected ${v}`, failures);
  const lod = manifest.lods.find((l) => l.lod === row.lod);
  check(row.triangles_per_brick === lod?.triangles, `${label}: triangles_per_brick ${row.triangles_per_brick} vs manifest ${lod?.triangles}`, failures);
  check(row.mesh_fingerprint === lod?.fingerprint, `${label}: mesh_fingerprint ${row.mesh_fingerprint} vs manifest ${lod?.fingerprint}`, failures);
  const fps = [row.fps_median, row.fps_p5, row.fps_mean];
  check(fps.every((f) => typeof f === "number" && f > 0), `${label}: fps must be numbers > 0, got ${fps.join(", ")}`, failures);
  const maxTriangles = row.count * row.triangles_per_brick + OUTPUT_PASS.triangles;
  check(row.triangles_per_frame > 0 && row.triangles_per_frame <= maxTriangles,
    `${label}: triangles_per_frame ${row.triangles_per_frame} outside (0, ${maxTriangles}]`, failures);
  const maxDraws = row.chunks + OUTPUT_PASS.drawCalls;
  check(row.draw_calls >= 1 && row.draw_calls <= maxDraws, `${label}: draw_calls ${row.draw_calls} outside [1, ${maxDraws}]`, failures);
  check(row.canvas_width > 0 && row.canvas_height > 0, `${label}: empty canvas`, failures);
  check(typeof row.adapter === "string" && row.adapter.length > 0, `${label}: no adapter string`, failures);
  check(row.kernel === manifest.kernel, `${label}: kernel ${row.kernel} vs manifest ${manifest.kernel}`, failures);
}

async function main() {
  if (!args.has("--no-build")) {
    const { build } = await import("vite");
    await build({ root: here, logLevel: "warn" });
  }
  const manifest = JSON.parse(await readFile(join(dist, "meshes", "manifest.json"), "utf8"));
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
  const failures = [];
  try {
    for (const run of RUNS) {
      const context = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 1, acceptDownloads: true });
      const page = await context.newPage();
      const pageErrors = [];
      page.on("pageerror", (e) => pageErrors.push(e.message));
      page.on("console", (m) => {
        if (m.type() === "error") pageErrors.push(m.text());
      });
      const t0 = Date.now();
      await page.goto(`${origin}?${run.query}`);
      await waitDone(page, 300_000);
      const { result, suite, csv, shown } = await page.evaluate(() => ({
        result: window.__harnessResult,
        suite: window.__harnessSuite,
        csv: window.__harnessCsv,
        shown: document.getElementById("csv").value,
      }));
      const label = run.name;
      check(pageErrors.length === 0, `${label}: page errors: ${pageErrors.join(" | ")}`, failures);
      const rows = run.suiteRows ? suite ?? [] : [result];
      if (run.suiteRows) check(rows.length === run.suiteRows, `${label}: ${rows.length} suite rows, expected ${run.suiteRows}`, failures);
      rows.forEach((row, i) => checkRow(row, run.expect, manifest, failures, rows.length > 1 ? `${label} [${i}]` : label));
      if (run.gpuTimer && result && !result.error) {
        // Where the browser offers a GPU timer, the harness must turn it into readings.
        const offered = !result.warnings.includes("no GPU timer");
        check(!offered || (result.gpu_samples > 0 && result.gpu_ms_p50 > 0), `${label}: GPU timer offered but gave ${result.gpu_samples} readings`, failures);
      }
      check(csv === shown, `${label}: CSV on the page differs from window.__harnessCsv`, failures);
      const lines = (csv ?? "").trimEnd().split("\n");
      check(lines[0] === csvHeader(), `${label}: CSV header differs from the schema`, failures);
      check(lines.length === rows.length + 1, `${label}: CSV has ${lines.length - 1} rows, expected ${rows.length}`, failures);
      // The same rules on the text a person copies or downloads.
      parseCsv(csv ?? "").slice(1).forEach((fields, i) => {
        check(fields.length === COLUMN_NAMES.length, `${label}: CSV row ${i + 1} has ${fields.length} fields`, failures);
        for (const problem of rowProblems(rowFromFields(fields))) failures.push(`${label}: CSV row ${i + 1}: ${problem}`);
      });
      // The download button must hand out exactly the CSV shown.
      const [download] = await Promise.all([page.waitForEvent("download"), page.click("#download")]);
      const downloaded = await readFile(await download.path(), "utf8");
      check(downloaded === csv, `${label}: downloaded CSV differs from the page`, failures);
      console.log(`\n# ${label} (${((Date.now() - t0) / 1000).toFixed(1)} s)`);
      for (const row of rows) {
        console.log(
          `  backend=${row.backend} adapter="${row.adapter}" software=${row.software} lod=${row.lod} count=${row.count}` +
            ` fps_median=${row.fps_median} fps_p5=${row.fps_p5} frame_ms_p95=${row.frame_ms_p95}` +
            ` triangles_per_frame=${row.triangles_per_frame} draw_calls=${row.draw_calls} chunks=${row.chunks}` +
            ` frames=${row.frames} canvas=${row.canvas_width}x${row.canvas_height}` +
            (row.gpu_samples !== "" ? ` gpu_samples=${row.gpu_samples} gpu_ms_p50=${row.gpu_ms_p50}` : "") +
            (row.warnings ? `\n  warnings: ${row.warnings}` : ""),
        );
      }
      console.log(csv.trimEnd().replace(/^/gm, "  | "));
      await context.close();
    }
  } finally {
    await browser.close();
    server.close();
  }
  console.log(`\nAll numbers above are SOFTWARE RENDERING in headless Chromium, not device results.`);
  console.log(`Checked every row against the ${COLUMN_NAMES.length}-column rules of src/csv.js, as objects and as CSV text.`);
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
