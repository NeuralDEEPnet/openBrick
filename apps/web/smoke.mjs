// Smoke test of the playground: the Rust core as WebAssembly in a Web Worker,
// parts shown by the three.js viewer, validation and downloads (docs/plan/02
// sections 1 to 3, docs/plan/06 sections 11 and 13).
//
// 1. Builds the core (`cargo xtask wasm`) and the app (vite build), unless --no-build.
// 2. Gets the native values from `cargo run --release -p openbrick-wasm --example
//    native_eval`, which calls the same Rust functions the worker calls: the logo
//    tile with its sample artwork (emboss, deboss, the swap and the parts inlay
//    with the example fit profile, emboss without one) and the default coupon.
// 3. Serves dist/ on 127.0.0.1 and opens it in headless Chromium with Playwright.
//    The WebGL2 run: the first part (the sample logo, emboss, example fit) has the
//    native content hash, triangle count and per-object fingerprints and is drawn;
//    the legend says how much artwork the red overlay marks; the validation panel
//    shows the FEATURE.MIN warning with rule id, values and fix; "Use sample logo"
//    after an uploaded SVG gives the native part back; a file over the core's
//    limit is refused before it is sent; deboss and inlay change the meshes and
//    equal native; the inlay style control is shown only for an inlay, labelled
//    in words; the multi-material (parts) inlay equals native, and its 3MF
//    downloads under its own name, byte-identical to native; every part 3MF
//    downloaded (emboss, swap and parts inlay) is in the part-designer layout,
//    one build item and one components object with a component per colour role
//    (docs/spikes/q18-slicer-multimaterial-import.md), as native_eval counted;
//    "None" fit disables both downloads with FIT.MISSING shown and
//    a link to the calibration coupon; with the example fit the 3MF and STL
//    download (saved for scripts/validate-3mf.py) and every downloaded file is
//    byte-identical to the one native_eval writes with the same functions (so
//    the STL is the closed solid the openbrick-wasm tests check); a size that
//    turns invalid while a tile is being built keeps the downloads off and its
//    message up when that build answers; a fit profile the core refuses empties
//    the 3D view and the stats and says why; a 16x16 tile shows the elapsed
//    seconds and is cancelled by going back to 2x4; dragging up shows the
//    underside; the coupon mode builds the native coupon, lists its cells and
//    downloads its 3MF, a pocket layout the bed cannot take empties the view,
//    the stats and the cell table, and an invalid bed size turns the coupon
//    downloads off until it is fixed.
//    Unless --no-webgpu, a WebGPU run checks the first part again.
// 4. A phone-sized run (390 x 844, touch, dark theme): no horizontal scrolling,
//    every control inside the width, a different background from the light
//    theme, a tap on a + button, and a one-finger touch drag that orbits.
// 5. Unless --no-base-check, builds the app again with OPENBRICK_BASE=/openBrick/
//    (the GitHub Pages path) into a temporary directory, serves it only under
//    /openBrick/ and checks that the page, worker, module and assets all load
//    from there and give the native part.
//
// Usage: node smoke.mjs [--no-build] [--no-webgpu] [--no-base-check]
//                       [--screenshots <dir>] [--downloads <dir>]
//
// Downloads go to apps/web/smoke-downloads/ unless --downloads names another
// directory; the CI web job then reads every downloaded 3MF (emboss, swap and
// parts inlay, coupon) with lib3mf: python -I scripts/validate-3mf.py
// --self-test apps/web/smoke-downloads/*.3mf. Chromium renders with
// SwiftShader here, so this proves the page works; it says nothing about speed
// on a device.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";

const here = fileURLToPath(new URL(".", import.meta.url));
const repo = resolve(here, "../..");
const dist = join(here, "dist");
const argv = process.argv.slice(2);
const args = new Set(argv);
const option = (name, fallback) => {
  const at = argv.indexOf(name);
  return at >= 0 ? resolve(argv[at + 1] ?? fallback) : fallback;
};
const shots = option("--screenshots", null);
const downloads = option("--downloads", join(here, "smoke-downloads"));
/** The GitHub Pages path (https://neuraldeepnet.github.io/openBrick/). */
const PAGES_BASE = "/openBrick/";
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
  const result = spawnSync(command, commandArgs, { cwd: repo, encoding: "utf8", maxBuffer: 64 << 20, ...options });
  if (result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(" ")} failed (${result.status ?? result.signal})\n${result.stderr ?? ""}`);
  }
  return result.stdout ?? "";
}

/** The native numbers of the playground's cases (crates/openbrick-wasm/examples/native_eval.rs). */
function nativeValues() {
  const out = run("cargo", ["run", "-q", "--release", "-p", "openbrick-wasm", "--example", "native_eval"]);
  const native = JSON.parse(out);
  for (const c of ["emboss", "deboss", "inlay", "inlay-parts", "emboss-no-fit"]) if (!native.part[c]) throw new Error(`native_eval printed no ${c} case:\n${out}`);
  for (const f of ["emboss3mf", "embossStl", "inlay3mf", "inlayParts3mf", "coupon3mf"]) if (!native.exports?.[f]) throw new Error(`native_eval printed no ${f} export:\n${out}`);
  return native;
}

/** Serve `root` under `base` only; everything else is a 404, so a wrong absolute URL shows. */
function serve(root, base = "/") {
  const requests = [];
  const server = createServer(async (req, res) => {
    const { pathname } = new URL(req.url, "http://127.0.0.1");
    requests.push(pathname);
    try {
      if (!pathname.startsWith(base)) throw new Error("outside the base path");
      let path = normalize(join(root, decodeURIComponent(pathname.slice(base.length))));
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
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok({ server, requests, origin: `http://127.0.0.1:${server.address().port}` })));
}

const failures = [];
function check(condition, message) {
  if (!condition) failures.push(message);
  return condition;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function web(page) {
  return page.evaluate(() => {
    const w = window.__web;
    return w && JSON.parse(JSON.stringify({ ...w, stats: w.stats() }));
  });
}

/**
 * Wait until the page shows a finished part in `mode` that satisfies `want` and
 * came after `after` evaluations (coupon builds in coupon mode); throws on a page
 * error.
 */
async function waitFor(page, { mode = "tile", after = 0, want = () => true }, timeoutMs, label) {
  const end = Date.now() + timeoutMs;
  let w = null;
  while (Date.now() < end) {
    w = await web(page);
    if (w?.state === "error") throw new Error(`${label}: page error: ${w.error}`);
    const count = mode === "tile" ? w?.evaluations : w?.couponBuilds;
    const thing = mode === "tile" ? w?.part : w?.coupon;
    if (w?.state === "ready" && w.mode === mode && count > after && thing && !thing.error && want(thing, w)) return w;
    await sleep(100);
  }
  throw new Error(`${label}: no finished ${mode} within ${timeoutMs / 1000} s (last: ${JSON.stringify({ state: w?.state, part: w?.part, coupon: w?.coupon })})`);
}

function checkPart(part, native, label) {
  check(part.contentHash === native.contentHash, `${label}: content hash ${part.contentHash}, native ${native.contentHash}`);
  check(part.triangles === native.triangles, `${label}: ${part.triangles} triangles, native ${native.triangles}`);
  const got = part.meshes.map((m) => `${m.role} ${m.triangles} ${m.fingerprint}`);
  const want = native.meshes.map((m) => `${m.role} ${m.triangles} ${m.fingerprint}`);
  check(JSON.stringify(got) === JSON.stringify(want), `${label}: meshes ${got.join(", ")}, native ${want.join(", ")}`);
  check(part.exportAllowed === native.exportAllowed, `${label}: exportAllowed ${part.exportAllowed}, native ${native.exportAllowed}`);
  check(JSON.stringify([...part.blockers].sort()) === JSON.stringify([...native.blockers].sort()), `${label}: blockers ${part.blockers}, native ${native.blockers}`);
  check(Boolean(part.overlay) === native.removedOverlay, `${label}: red overlay ${Boolean(part.overlay)}, native ${native.removedOverlay}`);
}

async function stats(page) {
  return page.evaluate(() => window.__web.stats());
}

/** Viewer stats of the first frame that drew the newest part (frames are on demand). */
async function drawnStats(page, label) {
  const end = Date.now() + 30_000;
  while (Date.now() < end) {
    const s = await stats(page);
    if (s && s.shown > 0 && s.drawn === s.shown) return s;
    await sleep(50);
  }
  throw new Error(`${label}: the viewer did not draw the newest part within 30 s`);
}

/** Viewer stats of the first frame that satisfies `ok`, or an error with the last stats. */
async function statsWhen(page, ok, timeoutMs, label, what) {
  const end = Date.now() + timeoutMs;
  let s = await stats(page);
  while (Date.now() < end) {
    if (ok(s)) return s;
    await sleep(100);
    s = await stats(page);
  }
  throw new Error(`${label}: ${what} within ${timeoutMs / 1000} s (last ${JSON.stringify(s)})`);
}

/**
 * Wait until the camera has stopped moving. Orbit damping is counted in frames,
 * so on SwiftShader under load it can take tens of seconds; only screenshots wait
 * for it, and they are taken anyway when it does not finish.
 */
async function settle(page, timeoutMs = 30_000) {
  const end = Date.now() + timeoutMs;
  let last = await stats(page);
  while (Date.now() < end) {
    await sleep(400);
    const s = await stats(page);
    if (s.frames === last.frames) return true;
    last = s;
  }
  return false;
}

/** Click a download button and save the file; returns its name, bytes and SHA-256. */
async function download(page, selector, label) {
  const [d] = await Promise.all([page.waitForEvent("download", { timeout: 60_000 }), page.click(selector)]);
  const name = d.suggestedFilename();
  const path = join(downloads, name);
  await d.saveAs(path);
  const bytes = readFileSync(path);
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  console.log(`# ${label}: saved ${path} (${bytes.length} bytes, sha256 ${sha256.slice(0, 16)}…)`);
  return { name, path, bytes, sha256 };
}

/** A downloaded file is byte-identical to the one native_eval wrote (same name, size and SHA-256). */
function checkFile(file, native, label) {
  check(file.name === native.name, `${label}: file name ${file.name}, native ${native.name}`);
  check(
    file.bytes.length === native.bytes && file.sha256 === native.sha256,
    `${label}: ${file.bytes.length} bytes sha256 ${file.sha256}, native ${native.bytes} bytes ${native.sha256}`,
  );
}

/**
 * The model part (3D/3dmodel.model) of a 3MF the core wrote, as text: walks the
 * zip's local headers (the writer records sizes up front and uses stored or
 * deflate entries only, crates/openbrick-io/src/zip_writer.rs).
 */
function modelXml(bytes) {
  let at = 0;
  while (at + 30 <= bytes.length && bytes.readUInt32LE(at) === 0x04034b50) {
    const method = bytes.readUInt16LE(at + 8);
    const packed = bytes.readUInt32LE(at + 18);
    const nameLength = bytes.readUInt16LE(at + 26);
    const extraLength = bytes.readUInt16LE(at + 28);
    const name = bytes.toString("latin1", at + 30, at + 30 + nameLength);
    const start = at + 30 + nameLength + extraLength;
    const data = bytes.subarray(start, start + packed);
    if (name === "3D/3dmodel.model") return (method === 8 ? inflateRawSync(data) : data).toString("utf8");
    at = start + packed;
  }
  throw new Error("no 3D/3dmodel.model in the package");
}

/**
 * The part-designer layout (docs/spikes/q18-slicer-multimaterial-import.md):
 * one build item, one components object with one component per colour role,
 * as native_eval counted them.
 */
function checkLayout(file, native, roles, label) {
  const xml = modelXml(file.bytes);
  const items = (xml.match(/<item /g) ?? []).length;
  const components = (xml.match(/<component /g) ?? []).length;
  const groups = (xml.match(/<components>/g) ?? []).length;
  check(
    items === 1 && groups === 1 && components === roles,
    `${label}: ${items} build items, ${groups} components objects, ${components} components; the part-designer layout has 1, 1 and ${roles}`,
  );
  check(
    items === native.layout?.items && components === native.layout?.components,
    `${label}: ${items} items and ${components} components, native ${JSON.stringify(native.layout)}`,
  );
  return { items, components };
}

/** Wait until the page's state is `want` (or throw with the last state). */
async function waitState(page, want, timeoutMs, label) {
  try {
    await page.waitForFunction((s) => window.__web?.state === s, want, { timeout: timeoutMs, polling: 50 });
  } catch {
    throw new Error(`${label}: the page did not reach state ${want} within ${timeoutMs / 1000} s (last: ${(await web(page))?.state})`);
  }
}

const browserArgs = [
  // WebGL2 through ANGLE on SwiftShader: there is no GPU on CI runners.
  "--use-angle=swiftshader",
  "--enable-unsafe-swiftshader",
  // WebGPU on SwiftShader's Vulkan driver, which ships with Chromium.
  "--enable-unsafe-webgpu",
  "--enable-features=Vulkan",
  "--use-vulkan=swiftshader",
  "--use-webgpu-adapter=swiftshader",
];

function watchErrors(page) {
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  return errors;
}

async function fullRun(browser, origin, native) {
  const label = "webgl2";
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1, colorScheme: "light", acceptDownloads: true });
  const page = await context.newPage();
  const errors = watchErrors(page);
  const t0 = Date.now();
  await page.goto(`${origin}/?backend=webgl2`);
  let w = await waitFor(page, {}, 120_000, label);
  const firstMs = Date.now() - t0;
  check(w.backend === "webgl2", `${label}: renderer backend is ${w.backend}`);
  check(w.kernel === native.kernel, `${label}: kernel ${w.kernel}, native ${native.kernel}`);
  checkPart(w.part, native.part.emboss, `${label} emboss`);
  let s = await drawnStats(page, label);
  check(s.showing?.triangles === native.part.emboss.triangles, `${label}: the viewer shows ${s.showing?.triangles} triangles`);
  check(s.triangles >= native.part.emboss.triangles, `${label}: the frame drew ${s.triangles} triangles`);
  const statusText = await page.textContent("#status");
  check(statusText.includes(`${native.part.emboss.triangles.toLocaleString("en-US")} triangles`), `${label}: status line "${statusText}"`);
  const legend = await page.textContent("#legend");
  check(/Red: [\d.]+ % of the artwork does not print as drawn/.test(legend), `${label}: the legend lacks the removed-area entry: "${legend}"`);
  // The validation panel: FEATURE.MIN is a warning with every part of a message.
  const feature = await page.textContent('#findings li[data-rule="FEATURE.MIN"]').catch(() => "");
  check(
    feature.includes("Warning") && feature.includes("FEATURE.MIN") && /Measured [\d.]+ %/.test(feature) && /threshold [\d.]+ %/.test(feature) && feature.includes("Fix:"),
    `${label}: the FEATURE.MIN entry is "${feature}"`,
  );
  // The unit is the core's (openbrick_print::finding_unit), not read from the message.
  check(JSON.stringify(w.part.units) === JSON.stringify(["FEATURE.MIN %"]), `${label}: finding units ${JSON.stringify(w.part.units)}`);
  check(!(await page.isDisabled("#download-3mf")) && !(await page.isDisabled("#download-stl")), `${label}: downloads are disabled for the default tile`);
  const disclaimer = await page.textContent("footer");
  check(disclaimer.includes("not affiliated with") && disclaimer.includes("Source code on GitHub"), `${label}: footer "${disclaimer}"`);
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "desktop-light-emboss.png") });
  }

  // An uploaded SVG, then "Use sample logo": the native part again.
  let n = w.evaluations;
  await page.setInputFiles("#art-file", {
    name: "square.svg",
    mimeType: "image/svg+xml",
    buffer: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="1" y="1" width="8" height="8"/></svg>'),
  });
  w = await waitFor(page, { after: n, want: (p) => p.params.logo === "square.svg" }, 60_000, `${label} upload`);
  check(w.part.contentHash !== native.part.emboss.contentHash, `${label}: the uploaded square has the sample's content hash`);
  n = w.evaluations;
  await page.click("#use-sample");
  w = await waitFor(page, { after: n, want: (p) => p.params.logo === "assets/mark.svg" }, 60_000, `${label} sample`);
  checkPart(w.part, native.part.emboss, `${label} sample again`);
  // Over the core's limit: refused in the page, nothing is evaluated.
  const before = w.evaluations;
  check(Number.isInteger(w.maxAssetBytes) && w.maxAssetBytes > 0, `${label}: the page has no asset limit (${w.maxAssetBytes})`);
  await page.setInputFiles("#art-file", { name: "huge.svg", mimeType: "image/svg+xml", buffer: Buffer.alloc(w.maxAssetBytes + 1, 0x20) });
  await sleep(600);
  const note = await page.textContent("#art-note");
  w = await web(page);
  const limit = w.maxAssetBytes.toLocaleString("en-US");
  check(
    note.includes(`“huge.svg” is`) && note.includes(`artwork can be at most`) && note.includes(`(${limit} bytes)`) && note.includes("The tile keeps the sample mark"),
    `${label}: the art note for a big file is "${note}"`,
  );
  check(w.evaluations === before && w.state === "ready", `${label}: a file over the limit was evaluated (${w.evaluations} vs ${before}, ${w.state})`);

  // Deboss and inlay change the meshes and equal native.
  const relief = {};
  for (const mode of ["deboss", "inlay"]) {
    n = w.evaluations;
    await page.check(`input[name="p-relief"][value="${mode}"]`);
    w = await waitFor(page, { after: n, want: (p) => p.params.relief === mode }, 60_000, `${label} ${mode}`);
    checkPart(w.part, native.part[mode], `${label} ${mode}`);
    check(w.part.contentHash !== native.part.emboss.contentHash, `${label}: ${mode} has the emboss content hash`);
    relief[mode] = w.part;
    await drawnStats(page, `${label} ${mode}`);
  }
  check(relief.inlay.faceOnBed === "top", `${label}: the inlay is not exported face down (${relief.inlay.faceOnBed})`);
  // The inlay is exported turned face down; lib3mf checks that file too.
  const inlay = await download(page, "#download-3mf", `${label} inlay 3MF`);
  checkFile(inlay, native.exports.inlay3mf, `${label} inlay 3MF`);
  checkLayout(inlay, native.exports.inlay3mf, native.part.inlay.meshes.length, `${label} inlay 3MF`);
  check(await page.isHidden('[data-param="reliefHeight"]'), `${label}: reliefHeight is shown for an inlay`);
  const inlayNotes = await page.textContent("#notes");
  check(/filament change/.test(inlayNotes), `${label}: the inlay note on the filament change is missing: "${inlayNotes}"`);
  // The inlay style: shown only for an inlay, labelled in words, swap first.
  const styleControl = '[data-param="inlayStyle"]';
  check(await page.isVisible(styleControl), `${label}: the inlay style is hidden for an inlay`);
  const styleText = await page.textContent(styleControl);
  for (const words of ["Inlay style", "Filament swap (one extruder)", "Logo as its own part (AMS, tool changer)"]) {
    check(styleText.includes(words), `${label}: the inlay style control lacks "${words}": "${styleText}"`);
  }
  check(await page.isChecked('input[name="p-inlayStyle"][value="swap"]'), `${label}: the inlay style does not start at swap`);
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "desktop-light-inlay.png") });
  }
  // The multi-material inlay: the logo fill is its own part. It equals native,
  // downloads under its own name as one object with two parts, face down, and
  // the CI step after the smoke reads it with lib3mf like every download.
  n = w.evaluations;
  await page.check('input[name="p-inlayStyle"][value="parts"]');
  w = await waitFor(page, { after: n, want: (p) => p.params.relief === "inlay" && p.params.inlayStyle === "parts" }, 60_000, `${label} inlay parts`);
  checkPart(w.part, native.part["inlay-parts"], `${label} inlay parts`);
  check(w.part.contentHash !== relief.inlay.contentHash, `${label}: the parts inlay has the swap inlay's content hash`);
  check(w.part.faceOnBed === "top", `${label}: the parts inlay is not exported face down (${w.part.faceOnBed})`);
  await drawnStats(page, `${label} inlay parts`);
  const partsNotes = await page.textContent("#notes");
  check(/no filament change is needed/.test(partsNotes), `${label}: the parts inlay note does not say no filament change is needed: "${partsNotes}"`);
  check((await page.textContent("#legend")).includes("Logo fill"), `${label}: the legend does not name the logo fill`);
  const inlayParts = await download(page, "#download-3mf", `${label} parts inlay 3MF`);
  checkFile(inlayParts, native.exports.inlayParts3mf, `${label} parts inlay 3MF`);
  checkLayout(inlayParts, native.exports.inlayParts3mf, native.part["inlay-parts"].meshes.length, `${label} parts inlay 3MF`);
  check(inlayParts.name !== inlay.name, `${label}: both inlays download as ${inlay.name}`);
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "desktop-light-inlay-parts.png") });
  }
  n = w.evaluations;
  await page.check('input[name="p-inlayStyle"][value="swap"]');
  w = await waitFor(page, { after: n, want: (p) => p.params.inlayStyle === "swap" }, 60_000, `${label} inlay swap again`);
  checkPart(w.part, native.part.inlay, `${label} inlay swap again`);
  n = w.evaluations;
  await page.check('input[name="p-relief"][value="emboss"]');
  w = await waitFor(page, { after: n, want: (p) => p.params.relief === "emboss" }, 60_000, `${label} emboss again`);
  check(await page.isVisible('[data-param="reliefHeight"]'), `${label}: reliefHeight is hidden for an emboss`);
  check(await page.isHidden(styleControl), `${label}: the inlay style is shown for an emboss`);

  // No fit profile: FIT.MISSING blocks both downloads, with the reason and the way out.
  n = w.evaluations;
  await page.selectOption("#fit", "none");
  w = await waitFor(page, { after: n, want: (p) => p.blockers.includes("FIT.MISSING") }, 60_000, `${label} no fit`);
  checkPart(w.part, native.part["emboss-no-fit"], `${label} no fit`);
  check((await page.isDisabled("#download-3mf")) && (await page.isDisabled("#download-stl")), `${label}: downloads are enabled without a fit profile`);
  const reason = await page.textContent("#download-reason");
  check(reason.includes("FIT.MISSING"), `${label}: the download reason is "${reason}"`);
  const missing = await page.textContent('#findings li[data-rule="FIT.MISSING"]');
  check(missing.includes("Blocker") && missing.includes("Print the calibration coupon") && missing.includes("Fix:"), `${label}: the FIT.MISSING entry is "${missing}"`);
  const guide = await page.getAttribute('#findings li[data-rule="FIT.MISSING"] a', "href");
  check(guide === "https://github.com/NeuralDEEPnet/openBrick/blob/main/docs/guides/print-test.md", `${label}: guide link ${guide}`);
  const first = await page.textContent("#findings li:first-child");
  check(first.includes("Blocker"), `${label}: blockers are not listed first ("${first}")`);
  if (shots) await page.screenshot({ path: join(shots, "desktop-light-no-fit.png") });

  // The example fit: download the 3MF and the STL.
  n = w.evaluations;
  await page.selectOption("#fit", "example");
  w = await waitFor(page, { after: n, want: (p) => p.exportAllowed }, 60_000, `${label} example fit`);
  checkPart(w.part, native.part.emboss, `${label} example fit again`);
  const threemf = await download(page, "#download-3mf", `${label} 3MF`);
  check(threemf.bytes.subarray(0, 4).equals(Buffer.from([0x50, 0x4b, 3, 4])), `${label}: the 3MF is not a zip container`);
  checkFile(threemf, native.exports.emboss3mf, `${label} 3MF`);
  checkLayout(threemf, native.exports.emboss3mf, native.part.emboss.meshes.length, `${label} 3MF`);
  // The STL: byte-identical to the native export, which the openbrick-wasm
  // tests check is closed and has the kernel's volume.
  const stl = await download(page, "#download-stl", `${label} STL`);
  const count = stl.bytes.readUInt32LE(80);
  check(stl.bytes.length === 84 + 50 * count && count === native.exports.embossStl.triangles, `${label}: the STL is ${stl.bytes.length} bytes for ${count} triangles, native ${native.exports.embossStl.triangles}`);
  checkFile(stl, native.exports.embossStl, `${label} STL`);

  // A size that turns invalid while a tile is being built: when that build
  // answers, the field's message stays up and the downloads stay off.
  let raceNote;
  {
    const start = await web(page);
    await page.fill("#p-size-0", "12");
    await page.fill("#p-size-1", "12");
    await waitState(page, "evaluating", 10_000, `${label} race`);
    await page.fill("#p-size-0", "40");
    const atInvalid = await web(page);
    let now = atInvalid;
    if (atInvalid.evaluations !== start.evaluations) {
      raceNote = "the 12x12 answered before the invalid width; the race was not exercised";
    } else {
      check(atInvalid.state === "invalid", `${label}: an invalid width gives state ${atInvalid.state}`);
      // Wait for the 12x12 to answer: dropped (the fix) or shown (the bug).
      const end = Date.now() + 60_000;
      while (Date.now() < end && now.staleReplies === start.staleReplies && now.evaluations === start.evaluations) {
        await sleep(100);
        now = await web(page);
      }
      if (now.evaluations !== start.evaluations) check(false, `${label}: the 12x12 reached the screen after the width turned invalid (state ${now.state})`);
      else check(now.staleReplies > start.staleReplies, `${label}: the 12x12 did not answer within 60 s`);
      check(now.state === "invalid", `${label}: after the stale reply the state is ${now.state}, not invalid`);
      const line = await page.textContent("#status");
      check(line.includes("Width must be a whole number of studs from 1 to"), `${label}: the status line lost the invalid width message: "${line}"`);
      check((await page.isDisabled("#download-3mf")) && (await page.isDisabled("#download-stl")), `${label}: downloads came back on with an invalid width`);
      const why = await page.textContent("#download-reason");
      check(why.includes("a field is invalid"), `${label}: the download reason with an invalid width is "${why}"`);
      check((await page.getAttribute("#p-size-0", "aria-invalid")) === "true", `${label}: the width field is not marked invalid`);
      raceNote = `a stale reply after an invalid width was dropped (${now.staleReplies - start.staleReplies})`;
    }
    n = now.evaluations;
    await page.fill("#p-size-0", "2");
    await page.fill("#p-size-1", "4");
    w = await waitFor(page, { after: n, want: (p) => p.params.size.join("x") === "2x4" }, 60_000, `${label} after the race`);
    checkPart(w.part, native.part.emboss, `${label} after the race`);
  }

  // A fit profile the core refuses: nothing on screen may describe the controls,
  // so the 3D view and the stats empty and the text says why.
  n = w.evaluations;
  await page.setInputFiles("#fit-file", { name: "broken-fit.json", mimeType: "application/json", buffer: Buffer.from('{"kind":"fit","id":"x"}') });
  await waitState(page, "part-error", 60_000, `${label} broken fit`);
  s = await stats(page);
  check(s.showing?.triangles === 0 && s.showing.roles.length === 0 && s.showing.overlayTriangles === 0, `${label}: a failed build left ${JSON.stringify(s.showing)} on screen`);
  check((await page.textContent("#tile-stats")).trim() === "", `${label}: a failed build left the stats "${await page.textContent("#tile-stats")}"`);
  const emptyLegend = await page.textContent("#legend");
  check(emptyLegend.includes("Nothing to show: the tile could not be built"), `${label}: the legend after a failed build is "${emptyLegend}"`);
  check((await page.isDisabled("#download-3mf")) && (await page.isDisabled("#download-stl")), `${label}: downloads are on after a failed build`);
  check((await page.textContent("#findings")).includes("Error"), `${label}: the validation panel does not show the error`);
  await page.selectOption("#fit", "example");
  w = await waitFor(page, { after: n, want: (p) => p.exportAllowed }, 60_000, `${label} example fit after the broken one`);
  checkPart(w.part, native.part.emboss, `${label} example fit after the broken one`);

  // A 16x16 tile takes seconds: the status line shows them, and going back to
  // 2x4 cancels the build by replacing the worker.
  const restartsBefore = (await web(page)).restarts;
  n = (await web(page)).evaluations;
  await page.fill("#p-size-0", "16");
  await page.fill("#p-size-1", "16");
  let slow = "";
  let finished = false;
  const end = Date.now() + 10_000;
  while (Date.now() < end) {
    slow = await page.textContent("#status");
    if (/\d+ s/.test(slow)) break;
    const now = await web(page);
    if (now.state === "ready" && now.part?.params?.size?.[0] === 16) {
      finished = true;
      break;
    }
    await sleep(100);
  }
  let cancelNote;
  if (finished) {
    cancelNote = "16x16 finished before the slow-build note; the cancel was not exercised";
  } else {
    check(/Building the logo tile…\s*\d+ s/.test(slow) && slow.includes("Any change cancels this one."), `${label}: the 16x16 status line says "${slow}"`);
    await page.fill("#p-size-0", "2");
    await page.fill("#p-size-1", "4");
    w = await waitFor(page, { after: n, want: (p) => p.params.size.join("x") === "2x4" }, 60_000, `${label} cancel`);
    check(w.restarts > restartsBefore, `${label}: the 16x16 evaluation was not cancelled (restarts ${w.restarts})`);
    checkPart(w.part, native.part.emboss, `${label} 2x4 from a fresh worker`);
    cancelNote = `16x16 cancelled with ${w.restarts - restartsBefore} worker restart(s)`;
  }
  w = await web(page);
  if (w.part.params.size.join("x") !== "2x4") {
    n = w.evaluations;
    await page.fill("#p-size-0", "2");
    await page.fill("#p-size-1", "4");
    w = await waitFor(page, { after: n, want: (p) => p.params.size.join("x") === "2x4" }, 60_000, `${label} back to 2x4`);
  }
  check(w.traps === 0, `${label}: ${w.traps} WebAssembly trap(s)`);

  // Drag up by a quarter of the view: OrbitControls turns the camera down by
  // 90 degrees, from about 63 degrees off the zenith to about 153, below the ground.
  s = await drawnStats(page, `${label} underside`);
  const box = await page.locator("#stage canvas").boundingBox();
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy + box.height / 8);
  await page.mouse.down();
  await page.mouse.move(cx, cy - box.height / 8, { steps: 12 });
  await page.mouse.up();
  const under = await statsWhen(page, (st) => st.below && st.camera[1] < 0, 30_000, `${label} underside`, "dragging up did not take the camera below the ground");
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "desktop-light-underside.png") });
  }

  // The calibration coupon: the native coupon, its cells, its 3MF.
  const c0 = (await web(page)).couponBuilds;
  await page.click("#tab-coupon");
  w = await waitFor(page, { mode: "coupon", after: c0 }, 120_000, `${label} coupon`);
  const nc = native.coupon;
  check(w.coupon.contentHash === nc.contentHash, `${label}: coupon content hash ${w.coupon.contentHash}, native ${nc.contentHash}`);
  check(w.coupon.triangles === nc.triangles, `${label}: coupon ${w.coupon.triangles} triangles, native ${nc.triangles}`);
  check(w.coupon.fingerprint === nc.fingerprint, `${label}: coupon fingerprint ${w.coupon.fingerprint}, native ${nc.fingerprint}`);
  check(new URL(page.url()).searchParams.get("mode") === "coupon", `${label}: URL ${page.url()} does not record the mode`);
  const rows = await page.locator("#cells tr").count();
  check(rows > 0 && rows * 2 === w.coupon.cells, `${label}: ${rows} cell rows for ${w.coupon.cells} cells in two copies`);
  const command = await page.textContent("#record-command");
  check(command.startsWith("openbrick calibrate record --process fdm-0.4 --pocket-set cross"), `${label}: record command "${command}"`);
  await drawnStats(page, `${label} coupon`);
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "desktop-light-coupon.png") });
  }
  const coupon = await download(page, "#download-coupon-3mf", `${label} coupon 3MF`);
  check(coupon.name === `${nc.fileStem}.3mf`, `${label}: coupon file name ${coupon.name}`);
  checkFile(coupon, native.exports.coupon3mf, `${label} coupon 3MF`);

  // A pocket layout the bed cannot take: no coupon, and nothing of the last one
  // stays in the view, the stats or the cell table.
  await page.selectOption("#coupon-pockets", "full");
  await waitState(page, "coupon-error", 120_000, `${label} full coupon`);
  s = await stats(page);
  check(s.showing?.triangles === 0, `${label}: a failed coupon left ${JSON.stringify(s.showing)} on screen`);
  check((await page.textContent("#coupon-stats")).trim() === "", `${label}: a failed coupon left its stats`);
  const cellText = await page.textContent("#cells");
  check((await page.locator("#cells tr").count()) === 1 && cellText.includes("No cells"), `${label}: a failed coupon left the cells "${cellText.slice(0, 80)}"`);
  check((await page.isDisabled("#download-coupon-3mf")) && (await page.isDisabled("#download-coupon-stl")), `${label}: coupon downloads are on after a failed build`);
  check((await page.textContent("#coupon-download-note")).includes("could not be built"), `${label}: the coupon note does not say why`);
  check((await page.textContent("#legend")).includes("Nothing to show: the coupon could not be built"), `${label}: the legend does not say why the view is empty`);
  const c1 = (await web(page)).couponBuilds;
  await page.selectOption("#coupon-pockets", "cross");
  w = await waitFor(page, { mode: "coupon", after: c1 }, 120_000, `${label} cross coupon again`);
  check(w.coupon.contentHash === nc.contentHash, `${label}: the cross coupon again has content hash ${w.coupon.contentHash}`);
  // A bed size that is not a positive number: the coupon on screen is the last
  // valid bed's, the message stays, and the downloads are off until it is fixed.
  const bedX = await page.inputValue("#bed-x");
  await page.fill("#bed-x", "0");
  await page.dispatchEvent("#bed-x", "change");
  await waitState(page, "invalid", 10_000, `${label} bed 0`);
  check((await page.isDisabled("#download-coupon-3mf")) && (await page.isDisabled("#download-coupon-stl")), `${label}: coupon downloads are on with a bed of 0 mm`);
  check((await page.textContent("#coupon-download-note")).includes("bed size is invalid"), `${label}: the coupon note does not name the invalid bed`);
  await page.fill("#bed-x", bedX);
  await page.dispatchEvent("#bed-x", "change");
  await waitState(page, "ready", 30_000, `${label} bed back to ${bedX}`);
  check(!(await page.isDisabled("#download-coupon-3mf")), `${label}: coupon downloads stay off after the bed is fixed`);
  // Back to the tile: the part on screen is the one left there.
  await page.click("#tab-tile");
  w = await waitFor(page, { want: (p) => p.contentHash === native.part.emboss.contentHash }, 60_000, `${label} tile again`);

  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  check(w.warnings.length === 0, `${label}: page warnings: ${w.warnings.join(" | ")}`);
  console.log(
    `# ${label}: first part after ${(firstMs / 1000).toFixed(1)} s (evaluated in ${Math.round(w.part.buildMs)} ms), ` +
      `emboss ${native.part.emboss.triangles} triangles ${native.part.emboss.contentHash.slice(0, 12)}…, deboss and both inlays equal native, ` +
      `the parts inlay downloads as ${inlayParts.name} (one object with two parts), ` +
      `FIT.MISSING blocks downloads, 3MF ${threemf.bytes.length} bytes, STL ${count} triangles (both byte-identical to native), ${raceNote}, ` +
      `a refused fit profile and a refused coupon layout empty the view, ${cancelNote}, ` +
      `camera below the ground (y ${under.camera[1].toFixed(1)} mm), coupon ${nc.triangles} triangles in ${Math.round(w.coupon?.buildMs ?? 0)} ms, coupon 3MF ${coupon.bytes.length} bytes`,
  );
  await context.close();
  return [threemf.path, inlay.path, inlayParts.path, coupon.path];
}

async function webgpuRun(browser, origin, native) {
  const label = "webgpu";
  const context = await browser.newContext({ viewport: { width: 960, height: 600 }, deviceScaleFactor: 1, colorScheme: "light" });
  const page = await context.newPage();
  const errors = watchErrors(page);
  await page.goto(`${origin}/?backend=webgpu`);
  const w = await waitFor(page, {}, 120_000, label);
  check(w.backend === "webgpu", `${label}: renderer backend is ${w.backend}`);
  checkPart(w.part, native.part.emboss, `${label} emboss`);
  const s = await drawnStats(page, label);
  check(s.triangles >= native.part.emboss.triangles, `${label}: the frame drew ${s.triangles} triangles`);
  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  console.log(`# ${label}: the first part equals native and was drawn (${s.triangles} triangles in the frame)`);
  await context.close();
}

async function phoneRun(browser, origin, native) {
  const label = "phone";
  const background = async (scheme) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: scheme });
    const page = await context.newPage();
    await page.goto(`${origin}/?backend=webgl2`);
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
  const errors = watchErrors(page);
  await page.goto(`${origin}/?backend=webgl2`);
  let w = await waitFor(page, {}, 120_000, label);
  checkPart(w.part, native.part.emboss, `${label} emboss`);
  const layout = await page.evaluate(() => {
    const inside = [...document.querySelectorAll("button, input:not([type=file]), select, h1, #status, [role=tab]")]
      .filter((el) => el.offsetParent !== null)
      .map((el) => {
        const r = el.getBoundingClientRect();
        return { what: el.id || el.getAttribute("aria-label") || el.textContent.trim().slice(0, 20) || el.tagName, left: r.left, right: r.right };
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
    check(r.left >= 0 && r.right <= layout.innerWidth + 0.5, `${label}: ${r.what} is outside the width (${JSON.stringify(r)})`);
  }
  check(layout.canvas && layout.canvas.height >= layout.innerHeight * 0.4, `${label}: the 3D view is only ${layout.canvas?.height} px tall`);
  check(layout.background !== light, `${label}: dark theme background ${layout.background} equals the light one`);
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "phone-dark-emboss.png") });
    await page.screenshot({ path: join(shots, "phone-dark-full.png"), fullPage: true });
  }

  const n = w.evaluations;
  await page.tap('button[aria-label="One stud wider"]');
  w = await waitFor(page, { after: n, want: (p) => p.params.size.join("x") === "3x4" }, 60_000, `${label} tap`);

  // One-finger drag on the canvas through the DevTools protocol, so Chromium makes
  // real touch pointers (OrbitControls captures them); the camera must move.
  await page.evaluate(() => window.scrollTo(0, 0));
  const canvas = await page.evaluate(() => document.querySelector("#stage canvas").getBoundingClientRect().toJSON());
  const before = (await stats(page)).camera;
  const cdp = await context.newCDPSession(page);
  const { x, y, width, height } = canvas;
  const at = (dx) => [{ x: x + width / 2 + dx, y: y + height / 2 }];
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: at(0) });
  for (let i = 1; i <= 8; i++) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: at(i * 15) });
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await sleep(500);
  const after = (await stats(page)).camera;
  const moved = Math.hypot(after[0] - before[0], after[1] - before[1], after[2] - before[2]);
  check(moved > 1, `${label}: a touch drag moved the camera by ${moved.toFixed(3)} mm`);
  if (shots) await page.screenshot({ path: join(shots, "phone-dark-3x4-orbited.png") });
  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  console.log(`# ${label}: no horizontal scroll at ${layout.innerWidth} px, dark background ${layout.background} (light ${light}), tap to 3x4, touch drag moved the camera ${moved.toFixed(1)} mm`);
  await context.close();
}

/** Build the app for the GitHub Pages path and check it loads only from there. */
async function baseRun(browser, native) {
  const label = `base ${PAGES_BASE}`;
  const out = mkdtempSync(join(tmpdir(), "openbrick-pages-"));
  try {
    const { build } = await import("vite");
    process.env.OPENBRICK_BASE = PAGES_BASE;
    try {
      await build({ root: here, logLevel: "warn", build: { outDir: out, emptyOutDir: true } });
    } finally {
      delete process.env.OPENBRICK_BASE;
    }
    const html = readFileSync(join(out, "index.html"), "utf8");
    check(html.includes(`src="${PAGES_BASE}assets/`), `${label}: index.html does not load its script from ${PAGES_BASE}assets/`);
    const { server, requests, origin } = await serve(out, PAGES_BASE);
    try {
      const context = await browser.newContext({ viewport: { width: 960, height: 600 }, colorScheme: "light" });
      const page = await context.newPage();
      const errors = watchErrors(page);
      const failed = [];
      page.on("response", (r) => r.status() >= 400 && failed.push(`${r.status()} ${r.url()}`));
      await page.goto(`${origin}${PAGES_BASE}?backend=webgl2`);
      const w = await waitFor(page, {}, 120_000, label);
      checkPart(w.part, native.part.emboss, `${label} emboss`);
      check(failed.length === 0, `${label}: failed requests: ${failed.join(", ")}`);
      const outside = requests.filter((p) => !p.startsWith(PAGES_BASE));
      check(outside.length === 0, `${label}: requests outside ${PAGES_BASE}: ${outside.join(", ")}`);
      check(requests.some((p) => /\/assets\/worker-[\w-]+\.js$/.test(p)), `${label}: the worker was not loaded from ${PAGES_BASE}assets/ (${requests.join(", ")})`);
      check(requests.some((p) => /\/assets\/openbrick_wasm_bg-[\w-]+\.wasm$/.test(p)), `${label}: the module was not loaded from ${PAGES_BASE}assets/`);
      check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
      console.log(`# ${label}: ${requests.length} requests, all under ${PAGES_BASE}; the first part equals native`);
      await context.close();
    } finally {
      server.close();
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

async function main() {
  if (!args.has("--no-build")) {
    run("cargo", ["xtask", "wasm"], { stdio: "inherit" });
    const { build } = await import("vite");
    await build({ root: here, logLevel: "warn" });
  }
  if (shots) mkdirSync(shots, { recursive: true });
  rmSync(downloads, { recursive: true, force: true });
  mkdirSync(downloads, { recursive: true });
  const native = nativeValues();
  console.log(
    `native (${native.kernel}): emboss ${native.part.emboss.triangles} triangles ${native.part.emboss.contentHash}, ` +
      `deboss ${native.part.deboss.triangles}, inlay ${native.part.inlay.triangles}, coupon ${native.coupon.triangles} triangles ${native.coupon.contentHash}`,
  );
  const { server, origin } = await serve(dist);
  const browser = await chromium.launch({ headless: true, args: browserArgs });
  let saved = [];
  try {
    try {
      saved = await fullRun(browser, origin, native);
    } catch (err) {
      failures.push(err.message);
    }
    if (!args.has("--no-webgpu")) {
      try {
        await webgpuRun(browser, origin, native);
      } catch (err) {
        failures.push(err.message);
      }
    }
    try {
      await phoneRun(browser, origin, native);
    } catch (err) {
      failures.push(err.message);
    }
    if (!args.has("--no-base-check")) {
      try {
        await baseRun(browser, native);
      } catch (err) {
        failures.push(err.message);
      }
    }
  } finally {
    await browser.close();
    server.close();
  }
  if (failures.length > 0) {
    console.error(`\nSMOKE FAILED:\n- ${failures.join("\n- ")}`);
    process.exit(1);
  }
  console.log(`downloads for scripts/validate-3mf.py: ${saved.join(" ")}`);
  console.log("SMOKE OK");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
