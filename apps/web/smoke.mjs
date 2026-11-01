// Smoke test of the playground: the Rust core as WebAssembly in a Web Worker,
// parts shown by the three.js viewer, validation and downloads (docs/plan/02
// sections 1 to 3, docs/plan/06 sections 11 and 13).
//
// 1. Builds the core (`cargo xtask wasm`) and the app (vite build), unless --no-build.
// 2. Gets the native values from `cargo run --release -p openbrick-wasm --example
//    native_eval`, which calls the same Rust functions the worker calls: the logo
//    tile with the playground's default artwork, the Neuraldeep.net mark (emboss,
//    deboss, the swap and the parts inlay with the example fit profile, emboss
//    without one, and with the fit profile the wizard records), the
//    Neuraldeep.net badge on 4x4, the CC0 sample mark, the default coupon with
//    its cell labels, and the calibration record (the wizard's choices, the
//    fit profile file and its offsets).
// 3. Serves dist/ on 127.0.0.1 and opens it in headless Chromium with Playwright.
//    The WebGL2 run: the first part (the Neuraldeep.net mark, emboss, example
//    fit) has the native content hash, triangle count and per-object
//    fingerprints and is drawn, with the mark's notice under the drop zone and
//    nothing to report; the legend says how much artwork the red overlay marks;
//    choosing the sample mark (CC0) in the sample chooser gives its native part,
//    hides the notice, and the validation panel shows the FEATURE.MIN warning
//    with rule id, values and fix; choosing the Neuraldeep.net mark after an
//    uploaded SVG gives the native part back; a file over the core's
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
//    the STL is the closed solid the openbrick-wasm tests check); choosing the
//    Neuraldeep.net badge on the 2x4 tile sets 4x4 and says so, equals native
//    and downloads byte-identical to native under its own name, and the mark
//    chosen again on that square tile sets 2x4; a size that
//    turns invalid while a tile is being built keeps the downloads off and its
//    message up when that build answers; a fit profile the core refuses empties
//    the 3D view and the stats and says why; a 16x16 tile shows the elapsed
//    seconds and going back to 2x4 cancels it (or, if it finished during the
//    debounce, drops its reply as stale); dragging up shows the
//    underside; the coupon mode builds the native coupon, lists its cells and
//    downloads its 3MF, a pocket layout the bed cannot take empties the view,
//    the stats and the cell table, and an invalid bed size turns the coupon
//    downloads off until it is fixed. Then the calibration wizard (recordRun):
//    "None" gives FIT.MISSING, whose Calibrate opens the coupon mode's record
//    step; its map shows every cell of the native coupon labelled as printed
//    in both copies and only the printed pockets, every tap target at least
//    24 px and every label inside its cell; a stud chosen by click, a bore
//    with the keyboard and a pocket by click in copy 2 (one checked cell per
//    line kind, the others with its value marked as twins), the printer,
//    material (an emptied one shows the core's message next to it) and date
//    filled in, the offsets summary equals native; the fit profile downloads
//    byte-identical to native_eval's record (which the CLI's
//    tests/record_parity.rs proves is `openbrick calibrate record`'s file);
//    "Use this fit profile" selects it and keeps it in localStorage; the tile
//    then equals native with that profile, has no FIT.MISSING, and its 3MF
//    names the profile and is byte-identical to native; another process gives
//    FIT.PROCESS with Calibrate; a reload keeps the profile and Forget
//    removes it; a profile kept for another process moves the tile to that
//    process at load, and the fit note says so. Then the full pocket set on
//    a 256 x 256 mm bed (copies side by side, drawn one under the other): the
//    core's cells, legible (targets of 24 px or more, labels inside their
//    cells) and tappable; and a layout answer that comes back after the
//    coupon settings changed again is dropped, never shown for other settings.
//    A designer run (designerRun) opens the Neuraldeep.net tile (bundled, then
//    from its file), edits it, chooses a preset, adds one and saves the part:
//    the saved file is byte-identical to native_eval's, and `openbrick build`
//    gives it the content hash the page then shows; the nameplate keychain,
//    dropped on the 3D view, names the features this version cannot build and
//    builds with studs on top and no ring.
//    Unless --no-webgpu, a WebGPU run checks the first part again.
// 4. A phone-sized run (390 x 844, touch, dark theme): no horizontal scrolling,
//    every control inside the width, a different background from the light
//    theme, a tap on a + button, a one-finger touch drag that orbits, and the
//    record step's map and fields inside the width, legible, with a tap
//    choosing a cell, for the default coupon and for the full pocket set on a
//    256 x 256 mm bed.
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
// parts inlay, badge, coupon, the tile with the recorded fit profile) with
// lib3mf: python -I scripts/validate-3mf.py
// --self-test apps/web/smoke-downloads/*.3mf. Chromium renders with
// SwiftShader here, so this proves the page works; it says nothing about speed
// on a device.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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
  for (const c of ["emboss", "deboss", "inlay", "inlay-parts", "emboss-no-fit", "badge-emboss", "sample-emboss"]) if (!native.part[c]) throw new Error(`native_eval printed no ${c} case:\n${out}`);
  for (const f of ["emboss3mf", "embossStl", "inlay3mf", "inlayParts3mf", "badge3mf", "coupon3mf", "fitProfile", "recorded3mf"]) if (!native.exports?.[f]) throw new Error(`native_eval printed no ${f} export:\n${out}`);
  if (!native.part["emboss-recorded-fit"] || !native.record?.input || !Array.isArray(native.coupon?.labels)) throw new Error(`native_eval printed no calibration record:\n${out}`);
  for (const c of ["opened", "edited", "saved", "savedPart", "keychain"]) if (!native.designer?.[c]) throw new Error(`native_eval printed no designer ${c}:\n${out}`);
  return native;
}

/** An offset as the record step's summary (and the CLI) shows it: +0.150. */
const signed = (v) => `${v < 0 ? "-" : "+"}${Math.abs(v).toFixed(3)}`;

/** WCAG 2.2 success criterion 2.5.8 (target size, minimum), CSS pixels. */
const MIN_TARGET_PX = 24;
/**
 * The full pocket set on a 256 x 256 mm bed, as the core lays it out (the
 * openbrick-wasm test the_web_fixtures_are_the_core_layouts keeps the file
 * equal to coupon_layout_json): two copies side by side.
 */
const FULL_256 = JSON.parse(readFileSync(join(here, "test/fixtures/coupon-layout-fdm-0.4-full-256.json"), "utf8"));

/**
 * The record step's map as drawn: no sideways scroll, the map inside the page
 * width and every cell inside the map, every tap target at least 24 px
 * (WCAG 2.2, 2.5.8) and every label line inside its cell (so nothing is
 * clipped). Returns the measurements for the log.
 */
async function mapLegibility(page, label) {
  const m = await page.evaluate(() => {
    const map = document.querySelector("#coupon-map").getBoundingClientRect().toJSON();
    const cells = [...document.querySelectorAll(".coupon-map .cell")].map((b) => {
      const r = b.getBoundingClientRect();
      const out = [...b.querySelectorAll("span")]
        .map((s) => s.getBoundingClientRect())
        .some((q) => q.left < r.left - 0.5 || q.right > r.right + 0.5 || q.top < r.top - 0.5 || q.bottom > r.bottom + 0.5);
      return { label: b.dataset.label, copy: b.dataset.copy, left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height, out, font: parseFloat(getComputedStyle(b).fontSize) };
    });
    return { map, cells, scrollWidth: document.documentElement.scrollWidth, bodyScrollWidth: document.body.scrollWidth, innerWidth };
  });
  check(m.scrollWidth <= m.innerWidth && m.bodyScrollWidth <= m.innerWidth, `${label}: the record step scrolls sideways (${m.scrollWidth} > ${m.innerWidth})`);
  check(m.map.left >= 0 && m.map.right <= m.innerWidth + 0.5, `${label}: the map is outside the width (${JSON.stringify(m.map)})`);
  const box = m.map;
  check(
    m.cells.every((c) => c.left >= box.left - 0.5 && c.right <= box.right + 0.5 && c.top >= box.top - 0.5 && c.bottom <= box.bottom + 0.5),
    `${label}: a cell lies outside the map`,
  );
  const small = m.cells.filter((c) => Math.min(c.width, c.height) < MIN_TARGET_PX);
  check(small.length === 0, `${label}: ${small.length} tap targets under ${MIN_TARGET_PX} px (${small.slice(0, 3).map((c) => `${c.label} ${c.width.toFixed(1)}x${c.height.toFixed(1)}`).join(", ")})`);
  const clipped = m.cells.filter((c) => c.out);
  check(clipped.length === 0, `${label}: ${clipped.length} labels run outside their cell (${clipped.slice(0, 3).map((c) => `${c.label} in copy ${c.copy}`).join(", ")})`);
  const smallest = Math.min(...m.cells.map((c) => Math.min(c.width, c.height)));
  const font = Math.min(...m.cells.map((c) => c.font));
  return { width: m.map.width, height: m.map.height, cells: m.cells.length, smallest, font, text: `${Math.round(m.map.width)} x ${Math.round(m.map.height)} px, ${m.cells.length} cells, smallest ${smallest.toFixed(0)} px, labels ${font.toFixed(1)} px or more` };
}

/**
 * A layout that answers after the coupon settings changed again is dropped
 * and never taken for other settings (rec.key names only a stored layout).
 * On the record step, in one go (so its layout is surely on its way): the
 * pocket layout to cross and diagonal, which the default bed cannot take,
 * then step 1 and the cross again. Once that answer is dropped, cross and
 * diagonal again and step 2 must show that coupon's refusal, not the cross
 * map; the cross is then set back. Last, cross and diagonal and straight
 * back to the cross in one go: the cross map is drawn again, not left at
 * "Loading".
 */
async function staleMapRun(page, label) {
  const before = (await web(page)).recordLayouts;
  const stale = (await web(page)).staleLayouts;
  await page.evaluate(() => {
    const pockets = document.querySelector("#coupon-pockets");
    pockets.value = "crossAndDiagonal";
    pockets.dispatchEvent(new Event("change"));
    document.querySelector("#step-print").click();
    pockets.value = "cross";
    pockets.dispatchEvent(new Event("change"));
  });
  await page.waitForFunction((n) => window.__web?.recordLayouts > n, before, { timeout: 120_000, polling: 50 });
  check((await web(page)).staleLayouts === stale + 1, `${label}: the cross-and-diagonal layout was not dropped as stale`);
  await page.selectOption("#coupon-pockets", "crossAndDiagonal");
  await page.click("#step-record");
  await page.waitForFunction(() => document.querySelector("#coupon-map .map-empty.bad") !== null, null, { timeout: 120_000, polling: 100 });
  const note = await page.textContent("#coupon-map");
  check(note.includes("cannot be laid out") && note.includes("crossAndDiagonal"), `${label}: the map of cross and diagonal on the default bed says "${note.slice(0, 120)}"`);
  check((await page.locator(".coupon-map .cell").count()) === 0, `${label}: a stale map is shown for cross and diagonal`);
  check((await web(page)).record.cells === 0, `${label}: the record step still counts cells of another coupon`);
  await page.selectOption("#coupon-pockets", "cross");
  await page.waitForFunction(() => window.__web?.record?.cells > 0 && window.__web.record.shown === "asPrinted", null, { timeout: 120_000, polling: 100 });
  // There and straight back, in one go: the cross map (stored) is drawn
  // again over the "Loading" the other layout drew, and stays when that
  // layout's answer is dropped.
  const cells = (await web(page)).record.cells;
  const asked = (await web(page)).recordLayouts;
  await page.evaluate(() => {
    const pockets = document.querySelector("#coupon-pockets");
    pockets.value = "crossAndDiagonal";
    pockets.dispatchEvent(new Event("change"));
    pockets.value = "cross";
    pockets.dispatchEvent(new Event("change"));
  });
  await page.waitForFunction((n) => window.__web?.recordLayouts > n, asked, { timeout: 120_000, polling: 50 });
  await page.waitForFunction((n) => window.__web?.record?.cells === n, cells, { timeout: 30_000, polling: 100 });
  check((await page.locator(".coupon-map .cell").count()) === cells, `${label}: after cross and diagonal and straight back the map shows ${await page.locator(".coupon-map .cell").count()} cells, not ${cells}`);
}

/**
 * The full pocket set on a 256 x 256 mm bed in the record step (the coupon
 * mode's settings are changed, then set back): the map has the core's cells,
 * says the copies lie side by side and draws them one under the other,
 * legibly; a tap on a stud of copy 2 checks it, and its twin in copy 1 is
 * marked as the same value, not checked. `tap` is "tap" on a touch screen.
 */
async function fullMapRun(page, label, tap = "click") {
  const bed = [await page.inputValue("#bed-x"), await page.inputValue("#bed-y")];
  for (const [id, mm] of [["#bed-x", 256], ["#bed-y", 256]]) {
    await page.fill(id, String(mm));
    await page.dispatchEvent(id, "change");
  }
  await page.selectOption("#coupon-pockets", "full");
  const labels = FULL_256.cells.map((c) => c.label);
  await page.waitForFunction((n) => window.__web?.record?.cells === n && window.__web.record.shown === "oneUnderTheOther", labels.length, { timeout: 120_000, polling: 100 });
  const w = await web(page);
  check(JSON.stringify(w.record.labels) === JSON.stringify(labels), `${label} full set: the map's labels differ from the core's layout`);
  const caption = await page.textContent("#map-caption-top");
  check(caption.includes("Copy 1 (left) and Copy 2 (right) lie side by side"), `${label} full set: map caption "${caption}"`);
  const bands = await page.$$eval(".coupon-map .band", (els) => els.map((e) => e.textContent));
  check(JSON.stringify(bands) === '["Copy 1 (left)","Copy 2 (right)"]', `${label} full set: copy bands ${JSON.stringify(bands)}`);
  const measured = await mapLegibility(page, `${label} full set`);
  const stud = page.locator('.coupon-map .cell[data-group="stud"][data-copy="2"]').nth(2);
  const studLabel = await stud.getAttribute("data-label");
  await stud[tap]();
  await page.waitForFunction(
    (l) => {
      const checked = [...document.querySelectorAll('.coupon-map .cell[data-group="stud"][aria-checked="true"]')];
      return checked.length === 1 && checked[0].dataset.copy === "2" && checked[0].dataset.label === l;
    },
    studLabel,
    { timeout: 10_000, polling: 100 },
  );
  const twins = await page.$$eval('.coupon-map .cell[data-group="stud"].twin', (els) => els.map((e) => [e.dataset.copy, e.dataset.label, e.getAttribute("aria-checked"), e.getAttribute("aria-describedby")]));
  check(JSON.stringify(twins) === JSON.stringify([["1", studLabel, "false", "twin-note"]]), `${label} full set: the stud's twins are ${JSON.stringify(twins)}`);
  if (shots) {
    await page.locator("#coupon-map").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(shots, `${label}-record-full-256.png`) });
  }
  // Back to the coupon as it was.
  await page.selectOption("#coupon-pockets", "cross");
  for (const [id, mm] of [["#bed-x", bed[0]], ["#bed-y", bed[1]]]) {
    await page.fill(id, mm);
    await page.dispatchEvent(id, "change");
  }
  await page.waitForFunction(() => window.__web?.record?.shown === "asPrinted", null, { timeout: 120_000, polling: 100 });
  return measured;
}

/**
 * The calibration wizard (docs/plan/05 section 4): FIT.MISSING offers
 * Calibrate, which opens the coupon mode's record step; the map shows every
 * cell of the native coupon labelled as printed, in both copies; a stud is
 * chosen by click, a bore with the keyboard and a pocket by click in copy 2;
 * printer, material and date are filled in (an emptied material shows the
 * core's message next to it); the offsets summary is the core's; the fit
 * profile downloads byte-identical to native_eval's, which
 * crates/openbrick/tests/record_parity.rs checks is the CLI's file; Use selects
 * it in the tile and keeps it in localStorage; the tile then exports with it
 * (no FIT.MISSING, its id in the 3MF, byte-identical to native), another
 * process gives FIT.PROCESS with Calibrate again, a reload keeps it, and
 * Forget removes it; a profile kept for another process moves the tile there
 * at load and says so. Then fullMapRun and staleMapRun. Returns the tile 3MF,
 * the fit profile and the map measurements.
 */
async function recordRun(page, native, label) {
  const rec = native.record;
  const input = rec.input;
  const two = (v) => v.toFixed(2);

  // No fit profile: the download reason and the finding offer Calibrate.
  let w = await web(page);
  let n = w.evaluations;
  await page.selectOption("#fit", "none");
  w = await waitFor(page, { after: n, want: (p) => p.blockers.includes("FIT.MISSING") }, 60_000, `${label} no fit for the wizard`);
  check(await page.isVisible("#download-calibrate"), `${label}: FIT.MISSING does not offer Calibrate next to the downloads`);
  // One FIT.MISSING entry per cell the part needs, each with its Calibrate.
  const entries = await page.locator('#findings li[data-rule="FIT.MISSING"]').count();
  const offered = await page.locator('#findings li[data-rule="FIT.MISSING"] button', { hasText: /^Calibrate$/ }).count();
  check(entries > 0 && offered === entries, `${label}: ${entries} FIT.MISSING entries offer Calibrate ${offered} times`);
  await page.click("#download-calibrate");
  await page.waitForFunction(() => window.__web?.mode === "coupon" && window.__web?.step === "record" && window.__web?.record?.cells > 0, null, { timeout: 120_000, polling: 100 });
  check(new URL(page.url()).searchParams.get("step") === "record", `${label}: URL ${page.url()} does not record the step`);
  check((await page.evaluate(() => document.activeElement?.id)) === "record-heading", `${label}: Calibrate did not take the focus to the record step`);
  check(await page.isHidden("#coupon-print"), `${label}: the print step is still shown`);
  w = await web(page);

  // The map: the native coupon's cells, labelled as printed, both copies.
  check(JSON.stringify(w.record.labels) === JSON.stringify(native.coupon.labels), `${label}: the map's labels differ from the native coupon's`);
  const cells = await page.$$eval(".coupon-map .cell", (bs) =>
    bs.map((b) => ({ index: Number(b.dataset.index), copy: b.dataset.copy, group: b.dataset.group, text: [...b.querySelectorAll("span")].map((s) => s.textContent).join("\n") })),
  );
  check(cells.length === native.coupon.labels.length, `${label}: the map has ${cells.length} cells, the coupon ${native.coupon.labels.length}`);
  check(cells.every((c) => c.text === native.coupon.labels[c.index]), `${label}: a cell's text is not its embossed label`);
  check(JSON.stringify([...new Set(cells.map((c) => c.copy))].sort()) === '["1","2"]', `${label}: the map does not show copy 1 and copy 2`);
  const captions = `${await page.textContent("#map-caption-top")} | ${await page.textContent("#map-caption-bottom")}`;
  check(captions.includes("Copy 1 (back)") && captions.includes("Copy 2 (front)"), `${label}: map captions "${captions}"`);
  // Only the printed pockets: the map's pockets are the coupon's.
  const pockets = new Set(cells.filter((c) => c.group === "pocket").map((c) => c.text));
  const printed = new Set(native.coupon.labels.filter((l) => l.includes("\n")));
  check(pockets.size === printed.size && [...pockets].every((p) => printed.has(p)), `${label}: the map offers pockets ${[...pockets].join(", ")}`);
  const legible = await mapLegibility(page, label);
  if (shots) await page.screenshot({ path: join(shots, "desktop-light-record-empty.png") });

  // A stud by click (copy 1), a bore with the keyboard, a pocket by click (copy 2).
  await page.click(`.coupon-map .cell[data-group="stud"][data-copy="1"][data-label="${two(input.cells.stud)}"]`);
  await page.focus('.coupon-map .cell[data-group="tube"][tabindex="0"]');
  await page.keyboard.press("Space");
  let at = null;
  for (let i = 0; i < 30; i++) {
    at = await page.evaluate(() => ({ label: document.activeElement?.dataset.label, checked: document.activeElement?.getAttribute("aria-checked") }));
    if (at.label === two(input.cells.tube)) break;
    await page.keyboard.press("ArrowRight");
  }
  check(at?.label === two(input.cells.tube) && at.checked === "true", `${label}: the arrow keys did not reach and choose the ${two(input.cells.tube)} bore (${JSON.stringify(at)})`);
  const pocketLabel = `${two(input.cells.pocketTubeOD)}/${two(input.cells.pocketRib)}`;
  await page.click(`.coupon-map .cell[data-group="pocket"][data-copy="2"][data-label="${pocketLabel}"]`);
  // One checked cell per line kind (a radio group has one checked radio); the
  // same pocket elsewhere (copy 1) is marked as a twin, not checked.
  const marks = await page.$$eval('.coupon-map .cell[data-group="pocket"]', (els) =>
    els.filter((e) => e.getAttribute("aria-checked") === "true" || e.classList.contains("twin")).map((e) => ({ copy: e.dataset.copy, label: e.dataset.label, checked: e.getAttribute("aria-checked"), twin: e.classList.contains("twin") })),
  );
  const checkedPockets = marks.filter((m) => m.checked === "true");
  check(checkedPockets.length === 1 && checkedPockets[0].copy === "2" && !checkedPockets[0].twin, `${label}: checked pockets ${JSON.stringify(checkedPockets)}`);
  check(marks.length >= 2 && marks.every((m) => m.label === pocketLabel) && marks.some((m) => m.twin && m.copy === "1"), `${label}: the pocket's twins are ${JSON.stringify(marks)}`);
  for (const group of ["stud", "tube", "pocket"]) {
    const n = await page.locator(`.coupon-map .cell[data-group="${group}"][aria-checked="true"]`).count();
    check(n === 1, `${label}: ${n} ${group} cells are checked`);
  }

  // The printer and material; the nozzle field starts at the core's default.
  check((await page.inputValue("#record-nozzle")) === String(input.nozzleMm), `${label}: the nozzle field starts at ${await page.inputValue("#record-nozzle")}`);
  await page.selectOption("#record-printer", input.printer);
  await page.fill("#record-material", " ");
  await page.waitForFunction(() => document.querySelector("#record-error-material")?.textContent !== "", null, { timeout: 30_000, polling: 100 });
  const materialError = await page.textContent("#record-error-material");
  check(materialError === "material is empty; it is required", `${label}: the material error is "${materialError}"`);
  check((await page.getAttribute("#record-material", "aria-invalid")) === "true", `${label}: the material field is not marked invalid`);
  await page.fill("#record-material", input.material);
  await page.fill("#record-date", input.date);
  await page.waitForFunction((id) => window.__web?.record?.ok && window.__web.record.id === id, rec.id, { timeout: 30_000, polling: 100 });
  w = await web(page);
  const sorted = (o) => JSON.stringify(Object.fromEntries(Object.entries(o).sort()));
  check(sorted(w.record.choices) === sorted(input.cells), `${label}: chose ${sorted(w.record.choices)}, native ${sorted(input.cells)}`);
  check(JSON.stringify(w.record.offsets) === JSON.stringify(rec.offsets), `${label}: offsets ${JSON.stringify(w.record.offsets)}, native ${JSON.stringify(rec.offsets)}`);
  check((await page.textContent("#record-error-material")) === "", `${label}: the material error stayed`);
  const table = await page.textContent("#record-offsets");
  for (const o of rec.offsets) check(table.includes(signed(o.offsetMm)) && table.includes(o.offsetField), `${label}: the summary lacks ${o.cell} ${signed(o.offsetMm)} (${table})`);
  check((await page.textContent("#record-summary")).includes(rec.fileName), `${label}: the summary does not name ${rec.fileName}`);
  if (shots) {
    await page.locator("#coupon-map").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(shots, "desktop-light-record.png") });
  }

  // The file: byte-identical to native_eval's (and so to the CLI's).
  const fit = await download(page, "#download-fit", `${label} fit profile`);
  checkFile(fit, native.exports.fitProfile, `${label} fit profile`);
  check(fit.bytes.toString("utf8") === native.exports.fitProfile.text, `${label}: the fit profile text differs from native`);

  // Use it: the tile's fit select, kept in this browser.
  await page.click("#use-fit");
  await page.waitForFunction(() => window.__web?.fit?.selected === "recorded", null, { timeout: 30_000, polling: 100 });
  check((await page.inputValue("#fit")) === "recorded", `${label}: the fit select shows ${await page.inputValue("#fit")}`);
  const stored = await page.evaluate(() => localStorage.getItem("openbrick.playground.recordedFit.v1"));
  check(stored === native.exports.fitProfile.text, `${label}: localStorage holds ${stored?.slice(0, 40)}`);
  check((await page.textContent("#record-note")).includes(rec.id), `${label}: the record note does not name ${rec.id}`);

  // The tile now exports with it.
  n = (await web(page)).evaluations;
  await page.click("#tab-tile");
  const recorded = native.part["emboss-recorded-fit"];
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === recorded.contentHash }, 60_000, `${label} tile with the recorded fit`);
  checkPart(w.part, recorded, `${label} tile with the recorded fit`);
  check(w.part.exportAllowed && !w.part.blockers.includes("FIT.MISSING"), `${label}: the recorded fit leaves blockers ${w.part.blockers}`);
  check(await page.isHidden("#download-calibrate"), `${label}: Calibrate is still offered with the recorded fit`);
  check((await page.textContent("#fit-note")).includes(rec.id), `${label}: the fit note does not name ${rec.id}`);
  const tile = await download(page, "#download-3mf", `${label} 3MF with the recorded fit`, `recorded-fit-${native.exports.recorded3mf.name}`);
  checkFile(tile, native.exports.recorded3mf, `${label} 3MF with the recorded fit`);
  const meta = `<metadata name="openbrick:fitProfile" preserve="1">${rec.id}</metadata>`;
  check(modelXml(tile.bytes).includes(meta), `${label}: the 3MF does not record ${meta}`);

  // Another process: FIT.PROCESS blocks and offers Calibrate again.
  n = w.evaluations;
  await page.selectOption("#process", "fdm-0.2");
  w = await waitFor(page, { after: n, want: (p) => p.blockers.includes("FIT.PROCESS") }, 60_000, `${label} FIT.PROCESS`);
  check(await page.isVisible("#download-calibrate"), `${label}: FIT.PROCESS does not offer Calibrate`);
  n = w.evaluations;
  await page.selectOption("#process", input.process);
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === recorded.contentHash }, 60_000, `${label} process back`);

  // A reload: this browser kept it, and the tile uses it again.
  await page.reload();
  w = await waitFor(page, {}, 120_000, `${label} reload`);
  check(w.fit?.selected === "recorded" && w.fit.recordedId === rec.id, `${label}: after a reload the fit is ${JSON.stringify(w.fit)}`);
  checkPart(w.part, recorded, `${label} recorded fit after a reload`);

  // Forget it: gone from the select and from storage, the example is back.
  n = w.evaluations;
  await page.click("#forget-fit");
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === native.part.emboss.contentHash }, 60_000, `${label} forget`);
  check((await page.inputValue("#fit")) === "example" && (await page.locator('#fit option[value="recorded"]').count()) === 0, `${label}: the recorded fit is still offered`);
  check((await page.evaluate(() => localStorage.getItem("openbrick.playground.recordedFit.v1"))) === null, `${label}: localStorage still holds the fit profile`);
  check(await page.isHidden("#fit-actions"), `${label}: Forget is still shown`);

  // A profile kept for another process: at load the tile moves to that
  // process, and the note under the fit select says so. The stored text is
  // the recorded file with its process changed (a stand-in for one recorded
  // on the fdm-0.2 coupon); Forget then removes it.
  const elsewhere = native.exports.fitProfile.text.replace(`"process": "${input.process}"`, '"process": "fdm-0.2"');
  check(elsewhere !== native.exports.fitProfile.text, `${label}: the recorded file names no process ${input.process}`);
  await page.evaluate((text) => localStorage.setItem("openbrick.playground.recordedFit.v1", text), elsewhere);
  await page.reload();
  w = await waitFor(page, {}, 120_000, `${label} reload with a profile for fdm-0.2`);
  check((await page.inputValue("#process")) === "fdm-0.2" && w.fit?.selected === "recorded", `${label}: at load the tile has process ${await page.inputValue("#process")} and fit ${JSON.stringify(w.fit)}`);
  const movedNote = await page.textContent("#fit-note");
  check(movedNote.startsWith("The process is now fdm-0.2, the one it was calibrated for."), `${label}: the fit note at load says "${movedNote}"`);
  n = w.evaluations;
  await page.click("#forget-fit");
  await page.selectOption("#process", input.process);
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === native.part.emboss.contentHash }, 60_000, `${label} forget the fdm-0.2 profile`);

  // The full pocket set on a large bed: copies side by side, still legible.
  // (The reload above started in the tile mode, so the coupon mode opens at step 1.)
  await page.click("#tab-coupon");
  await page.click("#step-record");
  await page.waitForFunction(() => window.__web?.mode === "coupon" && window.__web?.step === "record" && window.__web?.record?.cells > 0, null, { timeout: 120_000, polling: 100 });
  const full = await fullMapRun(page, label);
  await staleMapRun(page, label);
  return { tile, fit, legible, full };
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
  check(JSON.stringify([...part.warnings].sort()) === JSON.stringify([...native.warnings].sort()), `${label}: warnings ${part.warnings}, native ${native.warnings}`);
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

/**
 * Click a download button and save the file (under `saveAs` when two downloads
 * share a name); returns its name, bytes and SHA-256.
 */
async function download(page, selector, label, saveAs = null) {
  const [d] = await Promise.all([page.waitForEvent("download", { timeout: 60_000 }), page.click(selector)]);
  const name = d.suggestedFilename();
  const path = join(downloads, saveAs ?? name);
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
  // The default artwork is the Neuraldeep.net mark drawn for 2x4, with the
  // first sentence of its notice (the owner's copyright line, word for word)
  // and a link to the whole notice under the drop zone, and it has nothing to
  // report.
  check(
    w.artwork?.sample === "neuraldeep-mark" && w.part.params.logo === "assets/brand/neuraldeep-mark-2x4.svg" && w.artwork.notice,
    `${label}: the default artwork is ${JSON.stringify(w.artwork)}, logo ${w.part.params.logo}`,
  );
  check((await page.inputValue("#sample")) === "neuraldeep-mark", `${label}: the sample chooser shows ${await page.inputValue("#sample")}`);
  const notice = await page.textContent("#art-notice");
  check(
    (await page.isVisible("#art-notice")) && notice === "Copyright Neural Deep Network Ltd. All rights reserved. Read the notice",
    `${label}: the notice under the drop zone is "${notice}"`,
  );
  const noticeLink = await page.getAttribute("#art-notice a", "href");
  check(noticeLink?.endsWith("/parts/assets/brand/NOTICE.md"), `${label}: the notice links ${noticeLink}`);
  check(JSON.stringify(w.part.units) === "[]", `${label}: the Neuraldeep.net mark has findings ${JSON.stringify(w.part.units)}`);
  check((await page.locator('#findings li[data-rule="FEATURE.MIN"]').count()) === 0, `${label}: FEATURE.MIN on the Neuraldeep.net mark`);
  check(!(await page.isDisabled("#download-3mf")) && !(await page.isDisabled("#download-stl")), `${label}: downloads are disabled for the default tile`);
  const disclaimer = await page.textContent("footer");
  check(disclaimer.includes("not affiliated with") && disclaimer.includes("Source code on GitHub"), `${label}: footer "${disclaimer}"`);
  const docsLink = await page.getAttribute('footer a:text-is("Documentation")', "href");
  check(docsLink === "https://neuraldeepnet.github.io/openBrick/docs/", `${label}: the footer links the documentation site at ${docsLink}`);
  // The printer menu: none at first, so the report lists SIZE.BED as not
  // checked; a printer checks its bed (the content hash stays: the bed is not
  // part of the part), lays the coupon out for it and is kept for the next
  // visit; none again forgets it.
  check((await page.inputValue("#printer")) === "", `${label}: a printer is chosen at first`);
  check(w.part.unchecked.includes("SIZE.BED"), `${label}: SIZE.BED is not listed as not checked: ${JSON.stringify(w.part.unchecked)}`);
  check((await page.locator('#findings li.unchecked[data-rule="SIZE.BED"]').count()) === 1, `${label}: the panel does not say SIZE.BED was not checked`);
  const beforePrinter = w.evaluations;
  await page.selectOption("#printer", "prusa-xl");
  w = await waitFor(page, { after: beforePrinter, want: (p) => p.printer === "prusa-xl" }, 60_000, `${label} printer`);
  check(!w.part.unchecked.includes("SIZE.BED") && w.part.exportAllowed, `${label}: on the Prusa XL ${JSON.stringify(w.part)}`);
  check(w.part.contentHash === native.part.emboss.contentHash, `${label}: the printer changed the content hash`);
  check((await page.locator('#findings li.unchecked[data-rule="SIZE.BED"]').count()) === 0, `${label}: SIZE.BED still listed as not checked`);
  check((await page.inputValue("#bed-x")) === "360" && (await page.inputValue("#bed-y")) === "360", `${label}: the coupon bed did not follow the printer`);
  check((await page.inputValue("#coupon-printer")) === "prusa-xl", `${label}: the coupon printer did not follow`);
  const kept = await page.evaluate(() => localStorage.getItem("openbrick.playground.printer.v1"));
  check(kept === "prusa-xl", `${label}: the printer was not kept (${kept})`);
  const beforeNone = w.evaluations;
  await page.selectOption("#printer", "");
  w = await waitFor(page, { after: beforeNone, want: (p) => p.printer === "" }, 60_000, `${label} no printer`);
  check(w.part.unchecked.includes("SIZE.BED"), `${label}: without a printer SIZE.BED is checked`);
  // The coupon steps below expect the default bed (its fields are hidden in
  // the tile mode, so they are set as a typed value would be).
  await page.evaluate(() => {
    for (const id of ["bed-x", "bed-y"]) {
      const input = document.getElementById(id);
      input.value = "180";
      input.dispatchEvent(new Event("change"));
    }
  });
  check((await page.inputValue("#coupon-printer")) === "", `${label}: a typed bed size still names a printer`);
  if (shots) {
    await settle(page);
    await page.screenshot({ path: join(shots, "desktop-light-emboss.png") });
  }

  // The sample mark (CC0) from the chooser: its native part, no notice, and
  // its hairline gives the FEATURE.MIN warning with every part of a message.
  let n = w.evaluations;
  await page.selectOption("#sample", "sample");
  w = await waitFor(page, { after: n, want: (p) => p.params.logo === "assets/mark.svg" }, 60_000, `${label} CC0 sample`);
  checkPart(w.part, native.part["sample-emboss"], `${label} CC0 sample`);
  check(w.artwork?.sample === "sample" && !w.artwork.notice && (await page.isHidden("#art-notice")), `${label}: the CC0 sample shows the notice (${JSON.stringify(w.artwork)})`);
  const feature = await page.textContent('#findings li[data-rule="FEATURE.MIN"]').catch(() => "");
  check(
    feature.includes("Warning") && feature.includes("FEATURE.MIN") && /Measured [\d.]+ %/.test(feature) && /threshold [\d.]+ %/.test(feature) && feature.includes("Fix:"),
    `${label}: the FEATURE.MIN entry is "${feature}"`,
  );
  // The unit is the core's (openbrick_print::finding_unit), not read from the message.
  check(JSON.stringify(w.part.units) === JSON.stringify(["FEATURE.MIN %"]), `${label}: finding units ${JSON.stringify(w.part.units)}`);
  // An uploaded SVG, then the Neuraldeep.net mark from the chooser: the native part again.
  n = w.evaluations;
  await page.setInputFiles("#art-file", {
    name: "square.svg",
    mimeType: "image/svg+xml",
    buffer: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="1" y="1" width="8" height="8"/></svg>'),
  });
  w = await waitFor(page, { after: n, want: (p) => p.params.logo === "square.svg" }, 60_000, `${label} upload`);
  check(w.part.contentHash !== native.part.emboss.contentHash, `${label}: the uploaded square has the sample's content hash`);
  check(w.artwork?.sample === null && (await page.inputValue("#sample")) === "own" && (await page.isHidden("#art-notice")), `${label}: after an upload the chooser shows ${await page.inputValue("#sample")}`);
  n = w.evaluations;
  await page.selectOption("#sample", "neuraldeep-mark");
  w = await waitFor(page, { after: n, want: (p) => p.params.logo === "assets/brand/neuraldeep-mark-2x4.svg" }, 60_000, `${label} sample`);
  checkPart(w.part, native.part.emboss, `${label} Neuraldeep.net mark again`);
  // Over the core's limit: refused in the page, nothing is evaluated.
  const before = w.evaluations;
  check(Number.isInteger(w.maxAssetBytes) && w.maxAssetBytes > 0, `${label}: the page has no asset limit (${w.maxAssetBytes})`);
  await page.setInputFiles("#art-file", { name: "huge.svg", mimeType: "image/svg+xml", buffer: Buffer.alloc(w.maxAssetBytes + 1, 0x20) });
  await sleep(600);
  const note = await page.textContent("#art-note");
  w = await web(page);
  const limit = w.maxAssetBytes.toLocaleString("en-US");
  check(
    note.includes(`“huge.svg” is`) && note.includes(`artwork can be at most`) && note.includes(`(${limit} bytes)`) && note.includes("The part keeps the Neuraldeep.net mark"),
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

  // The Neuraldeep.net badge is drawn for square tiles: chosen on the 2x4 it
  // sets 4x4 and says so, equals native and downloads byte-identical to native
  // under its own name. The portrait mark chosen on that square sets 2x4 again.
  let badge;
  {
    n = w.evaluations;
    await page.selectOption("#sample", "neuraldeep-badge");
    w = await waitFor(page, { after: n, want: (p) => p.params.logo === "assets/brand/neuraldeep-badge-4x4.svg" }, 60_000, `${label} badge`);
    checkPart(w.part, native.part["badge-emboss"], `${label} badge 4x4`);
    check(w.part.params.size.join("x") === "4x4", `${label}: the badge is built at ${w.part.params.size.join("x")}`);
    check((await page.inputValue("#p-size-0")) === "4" && (await page.inputValue("#p-size-1")) === "4", `${label}: the size fields do not show 4 x 4`);
    const said = "The Neuraldeep.net badge is drawn for square tiles, so the size is now 4 × 4.";
    check((await page.textContent("#art-note")).includes(said), `${label}: the art note is "${await page.textContent("#art-note")}"`);
    check((await page.textContent("#status")).includes(said), `${label}: the status line is "${await page.textContent("#status")}"`);
    check(w.artwork?.notice && (await page.isVisible("#art-notice")), `${label}: the badge has no notice`);
    await drawnStats(page, `${label} badge`);
    if (shots) {
      await settle(page);
      await page.screenshot({ path: join(shots, "desktop-light-badge.png") });
    }
    badge = await download(page, "#download-3mf", `${label} badge 3MF`);
    checkFile(badge, native.exports.badge3mf, `${label} badge 3MF`);
    checkLayout(badge, native.exports.badge3mf, native.part["badge-emboss"].meshes.length, `${label} badge 3MF`);
    check(![threemf.name, inlay.name, inlayParts.name].includes(badge.name), `${label}: the badge downloads as ${badge.name}, like another file`);
    n = w.evaluations;
    await page.selectOption("#sample", "neuraldeep-mark");
    w = await waitFor(page, { after: n, want: (p) => p.params.logo === "assets/brand/neuraldeep-mark-2x4.svg" }, 60_000, `${label} mark after the badge`);
    check(w.part.params.size.join("x") === "2x4", `${label}: the mark after the badge is built at ${w.part.params.size.join("x")}`);
    check((await page.textContent("#art-note")).includes("so the size is now 2 × 4."), `${label}: the art note is "${await page.textContent("#art-note")}"`);
    checkPart(w.part, native.part.emboss, `${label} mark after the badge`);
  }

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
  check(emptyLegend.includes("Nothing to show: the part could not be built"), `${label}: the legend after a failed build is "${emptyLegend}"`);
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
    check(/Building “Logo tile”…\s*\d+ s/.test(slow) && slow.includes("Any change cancels this one."), `${label}: the 16x16 status line says "${slow}"`);
    const staleBefore = (await web(page)).staleReplies;
    await page.fill("#p-size-0", "2");
    await page.fill("#p-size-1", "4");
    w = await waitFor(page, { after: n, want: (p) => p.params.size.join("x") === "2x4" }, 60_000, `${label} cancel`);
    // Two outcomes are right. Usually the 2x4 request supersedes the running
    // 16x16 build, which is old enough to be cancelled by replacing the worker.
    // On a fast machine the 16x16 build can finish inside the debounce between
    // the edit and the 2x4 request; its reply is then dropped as stale (the
    // controls changed while it was built), and there is nothing to cancel.
    // Anything else (no restart and no stale drop) means the 16x16 result was
    // neither cancelled nor dropped.
    const cancelled = w.restarts > restartsBefore;
    const dropped = w.staleReplies > staleBefore;
    check(cancelled || dropped, `${label}: the 16x16 evaluation was neither cancelled nor dropped as stale (restarts ${w.restarts}, stale replies ${w.staleReplies})`);
    checkPart(w.part, native.part.emboss, `${label} 2x4 after the 16x16`);
    cancelNote = cancelled
      ? `16x16 cancelled with ${w.restarts - restartsBefore} worker restart(s)`
      : "16x16 finished during the debounce and its reply was dropped as stale; the cancel was not exercised";
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

  // The calibration wizard, from FIT.MISSING to a tile that exports with the recorded fit.
  const recorded = await recordRun(page, native, label);
  w = await web(page);

  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  check(w.warnings.length === 0, `${label}: page warnings: ${w.warnings.join(" | ")}`);
  console.log(
    `# ${label}: first part after ${(firstMs / 1000).toFixed(1)} s (evaluated in ${Math.round(w.part.buildMs)} ms), ` +
      `emboss ${native.part.emboss.triangles} triangles ${native.part.emboss.contentHash.slice(0, 12)}…, deboss and both inlays equal native, ` +
      `the parts inlay downloads as ${inlayParts.name} (one object with two parts), ` +
      `the badge sets 4x4 and downloads as ${badge.name}, the CC0 sample shows FEATURE.MIN, ` +
      `FIT.MISSING blocks downloads, 3MF ${threemf.bytes.length} bytes, STL ${count} triangles (both byte-identical to native), ${raceNote}, ` +
      `a refused fit profile and a refused coupon layout empty the view, ${cancelNote}, ` +
      `camera below the ground (y ${under.camera[1].toFixed(1)} mm), coupon ${nc.triangles} triangles in ${Math.round(w.coupon?.buildMs ?? 0)} ms, coupon 3MF ${coupon.bytes.length} bytes, ` +
      `the wizard recorded ${recorded.fit.name} (${recorded.fit.bytes.length} bytes, byte-identical to native) and the tile exported with it (${recorded.tile.bytes.length} bytes, byte-identical to native), ` +
      `record map ${recorded.legible.text}; full set on 256 x 256 mm ${recorded.full.text}`,
  );
  await context.close();
  return [threemf.path, inlay.path, inlayParts.path, badge.path, coupon.path, recorded.tile.path];
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

  // The record step on a phone: the map fits the width, every cell inside it,
  // and a tap chooses a cell.
  await page.tap("#tab-coupon");
  await page.tap("#step-record");
  await page.waitForFunction(() => window.__web?.record?.cells > 0, null, { timeout: 120_000, polling: 100 });
  const legible = await mapLegibility(page, label);
  const fields = await page.evaluate(() =>
    [...document.querySelectorAll("#coupon-record input, #coupon-record select, #coupon-record textarea, #coupon-record button")]
      .filter((e) => e.offsetParent !== null && !e.classList.contains("cell"))
      .map((e) => ({ what: e.id || e.tagName, ...e.getBoundingClientRect().toJSON() })),
  );
  for (const f of fields) check(f.left >= 0 && f.right <= layout.innerWidth + 0.5, `${label}: ${f.what} is outside the width (${f.left}..${f.right})`);
  const stud = page.locator('.coupon-map .cell[data-group="stud"][data-copy="2"]').nth(3);
  await stud.tap();
  await page.waitForFunction(
    () =>
      document.querySelectorAll('.coupon-map .cell[data-group="stud"][aria-checked="true"][data-copy="2"]').length === 1 &&
      document.querySelectorAll('.coupon-map .cell[data-group="stud"][aria-checked="true"]').length === 1 &&
      document.querySelectorAll('.coupon-map .cell[data-group="stud"].twin[data-copy="1"]').length === 1,
    null,
    { timeout: 10_000, polling: 100 },
  );
  if (shots) {
    await page.locator("#coupon-map").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(shots, "phone-dark-record.png") });
  }
  const full = await fullMapRun(page, label, "tap");

  // The part designer on a phone: the Neuraldeep.net tile with its preset
  // picker and the part file controls, all inside the width.
  await page.tap("#tab-tile");
  const before3 = (await web(page)).evaluations;
  await page.selectOption("#part-select", "bundled:ndn.brand-tile");
  await waitFor(page, { after: before3, want: (p, x) => x.design?.id === "ndn.brand-tile" && p.contentHash === native.designer.opened.contentHash }, 120_000, `${label} brand tile`);
  const partControls = await page.evaluate(() =>
    [...document.querySelectorAll("#panel-tile section:first-child :is(input:not([type=file]), select, button)")]
      .filter((e) => e.offsetParent !== null)
      .map((e) => ({ what: e.id || e.getAttribute("aria-label") || e.tagName, ...e.getBoundingClientRect().toJSON() })),
  );
  check(partControls.some((c) => c.what === "preset") && partControls.some((c) => c.what === "save-part"), `${label}: the preset picker or the save button is missing`);
  for (const c of partControls) {
    check(c.left >= 0 && c.right <= layout.innerWidth + 0.5, `${label}: ${c.what} is outside the width (${c.left}..${c.right})`);
    if (["preset", "part-select", "save-part", "add-preset", "preset-name"].includes(c.what)) check(c.height >= 40, `${label}: ${c.what} is ${c.height} px tall, under the 40 px touch target`);
  }
  if (shots) {
    await page.locator("#preset-row").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(shots, "phone-dark-presets.png") });
  }

  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  console.log(
    `# ${label}: no horizontal scroll at ${layout.innerWidth} px, dark background ${layout.background} (light ${light}), tap to 3x4, touch drag moved the camera ${moved.toFixed(1)} mm, ` +
      `record step map ${legible.text}, a tap chose a stud (its twin in copy 1 marked); full set on 256 x 256 mm ${full.text}, ` +
      `the Neuraldeep.net tile's preset picker and part file controls fit the width`,
  );
  await context.close();
}

/** Build a part file with the CLI from a folder holding it and its artwork; the --json answer. */
function cliBuild(text, label) {
  const dir = mkdtempSync(join(tmpdir(), "openbrick-saved-"));
  try {
    mkdirSync(join(dir, "assets/brand"), { recursive: true });
    copyFileSync(join(repo, "parts/assets/brand/neuraldeep-mark-2x4.svg"), join(dir, "assets/brand/neuraldeep-mark-2x4.svg"));
    const part = join(dir, "ndn.brand-tile.obpart.json");
    writeFileSync(part, text);
    const out = run("cargo", ["run", "-q", "--release", "-p", "openbrick", "--", "build", part, "--fit", "example", "--json", "--out", join(dir, "saved.3mf")]);
    return JSON.parse(out);
  } catch (err) {
    failures.push(`${label}: the CLI could not build the saved part: ${err.message}`);
    return {};
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * The part designer (docs/plan/06 sections 3 and 11): open, edit, preset,
 * save. The Neuraldeep.net tile is chosen from the bundled parts and then
 * opened from its file; made 2x6 and given its "Pressed-in mark" preset (the
 * picker then names it); those values kept as a new preset; and the part
 * saved: the file is byte-identical to native_eval's (add_preset_json and
 * save_part_json), the page then builds the saved document, and the CLI
 * builds the saved file with the same content hash. Then the nameplate
 * keychain, dropped on the 3D view: its text label and key ring are named as
 * features this version cannot build, with what turns them off; the font has
 * a kind note instead of a control; a text over its limit is refused in the
 * page; with studs on top and no ring it builds the native part. A fit
 * profile chosen as a part is refused with a message, and the part stays.
 */
async function designerRun(browser, origin, native) {
  const label = "designer";
  const d = native.designer;
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: "light", acceptDownloads: true });
  const page = await context.newPage();
  const errors = watchErrors(page);
  await page.goto(`${origin}/?backend=webgl2`);
  let w = await waitFor(page, {}, 120_000, label);
  check(w.design?.id === "ndn.logo-tile" && w.design.presets.length === 0, `${label}: the first part is ${JSON.stringify(w.design)}`);
  check(await page.isHidden("#preset-row"), `${label}: the logo tile has no presets, yet the picker shows`);
  check((await page.textContent("#part-heading")) === "Logo tile", `${label}: heading ${await page.textContent("#part-heading")}`);

  // The bundled Neuraldeep.net tile, then the same file opened.
  let n = w.evaluations;
  await page.selectOption("#part-select", "bundled:ndn.brand-tile");
  w = await waitFor(page, { after: n, want: (p, x) => x.design?.id === "ndn.brand-tile" }, 120_000, `${label} bundled brand tile`);
  checkPart(w.part, d.opened, `${label} bundled brand tile`);
  n = w.evaluations;
  await page.setInputFiles("#part-file", join(repo, "parts/ndn.brand-tile.obpart.json"));
  w = await waitFor(page, { after: n, want: (p, x) => x.design?.source === "file" }, 120_000, `${label} opened brand tile`);
  checkPart(w.part, d.opened, `${label} opened brand tile`);
  check(
    JSON.stringify(w.design.presets) === JSON.stringify(["Raised mark", "Pressed-in mark", "Two-colour inlay"]) && w.design.preset === "Raised mark",
    `${label}: presets ${JSON.stringify(w.design.presets)}, matching ${w.design.preset}`,
  );
  check((await page.textContent("#part-note")).startsWith("Opened “ndn.brand-tile.obpart.json”"), `${label}: part note "${await page.textContent("#part-note")}"`);
  check((await page.inputValue("#part-select")) === "file", `${label}: the part select shows ${await page.inputValue("#part-select")}`);

  // Edit: two studs longer, then the Pressed-in mark preset (it keeps the size).
  n = w.evaluations;
  await page.click('button[data-for="p-size-1"][data-step="1"]');
  await page.click('button[data-for="p-size-1"][data-step="1"]');
  await page.selectOption("#preset", "Pressed-in mark");
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === d.edited.contentHash }, 120_000, `${label} edited`);
  checkPart(w.part, d.edited, `${label} edited`);
  check(w.design.preset === "Pressed-in mark" && (await page.inputValue("#preset")) === "Pressed-in mark", `${label}: the picker shows ${w.design.preset}`);
  check((await page.inputValue("#p-size-1")) === "6", `${label}: the length field shows ${await page.inputValue("#p-size-1")}`);
  // A value no preset holds turns the picker to Custom values; emboss at
  // the preset's height is the "Raised mark" again.
  await page.fill("#p-reliefHeight", "0.8");
  await page.waitForFunction(() => window.__web?.design?.preset === "", null, { timeout: 10_000, polling: 50 });
  check((await page.inputValue("#preset")) === "", `${label}: the picker shows ${await page.inputValue("#preset")} for 0.8 mm`);
  await page.fill("#p-reliefHeight", "0.6");
  await page.click('input[name="p-relief"][value="emboss"]');
  await page.waitForFunction(() => window.__web?.design?.preset === "Raised mark", null, { timeout: 10_000, polling: 50 });
  await page.click('input[name="p-relief"][value="deboss"]');
  w = await waitFor(page, { after: w.evaluations, want: (p) => p.contentHash === d.edited.contentHash }, 120_000, `${label} back to deboss`);

  // Keep the values as a preset, then save the part.
  await page.click("#add-preset");
  check((await page.textContent("#save-note")).includes("Give the preset a name"), `${label}: an unnamed preset gives "${await page.textContent("#save-note")}"`);
  await page.fill("#preset-name", d.presetName);
  await page.click("#add-preset");
  await page.waitForFunction((name) => window.__web?.design?.presets?.includes(name), d.presetName, { timeout: 30_000, polling: 50 });
  w = await waitFor(page, { after: w.evaluations }, 120_000, `${label} preset added`);
  check(w.design.preset === "Pressed-in mark", `${label}: after adding, the picker shows ${w.design.preset}`);
  check((await page.textContent("#save-note")).includes(`Added the preset “${d.presetName}”`), `${label}: save note "${await page.textContent("#save-note")}"`);
  const saved = await download(page, "#save-part", `${label} part file`);
  checkFile(saved, d.saved, `${label} saved part`);
  w = await waitFor(page, { after: w.evaluations, want: (p) => p.contentHash === d.savedPart.contentHash }, 120_000, `${label} saved part`);
  checkPart(w.part, d.savedPart, `${label} saved part`);
  check(w.saves.length === 1 && w.saves[0].name === d.saved.name, `${label}: saves ${JSON.stringify(w.saves)}`);
  // open -> save -> CLI build: the CLI gives the hash the page shows.
  const cli = cliBuild(saved.bytes.toString("utf8"), label);
  check(cli.contentHash === d.savedPart.contentHash, `${label}: the CLI built the saved file with hash ${cli.contentHash}, the page ${d.savedPart.contentHash}`);
  // The saved file opens again with the new preset.
  n = w.evaluations;
  await page.setInputFiles("#part-file", saved.path);
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === d.savedPart.contentHash }, 120_000, `${label} saved part reopened`);
  check(w.design.presets.includes(d.presetName) && w.design.preset === "Pressed-in mark", `${label}: reopened presets ${JSON.stringify(w.design.presets)}, matching ${w.design.preset}`);

  // The keychain, dropped on the 3D view: its unsupported features are named.
  const keychainText = readFileSync(join(repo, "parts/ndn.nameplate-keychain.obpart.json"), "utf8");
  await page.evaluate((text) => {
    const dt = new DataTransfer();
    dt.items.add(new File([text], "ndn.nameplate-keychain.obpart.json", { type: "application/json" }));
    document.querySelector("#stage").dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
  }, keychainText);
  await waitState(page, "part-error", 60_000, `${label} keychain`);
  w = await web(page);
  check(
    w.design.id === "ndn.nameplate-keychain" && JSON.stringify(w.design.unsupported) === JSON.stringify(d.keychainUnsupported.map((u) => u.feature)),
    `${label}: the keychain opened as ${JSON.stringify(w.design)}`,
  );
  const partNote = await page.textContent("#part-note");
  check(partNote.includes("“label” (relief.text)") && partNote.includes("“ring” (loop.keyring)") && partNote.includes("!$studsOnTop"), `${label}: the part note says "${partNote}"`);
  const status = await page.textContent("#status");
  check(status.includes("“label” uses relief.text, which this version cannot build yet") && !status.includes("skip_unsupported"), `${label}: the status line says "${status}"`);
  check(await page.isHidden("#preset-row"), `${label}: the keychain has no presets, yet the picker shows`);
  check((await page.textContent('[data-param="font"]')).includes("font is not possible here yet"), `${label}: the font has no kind note`);
  check(await page.isDisabled("#download-3mf"), `${label}: the 3MF download is on for a part that did not build`);
  // A text over the document's limit is refused in the page.
  await page.fill("#p-text", "X".repeat(15));
  check((await page.textContent("#status")).includes("at most 14 characters"), `${label}: a long text gives "${await page.textContent("#status")}"`);
  await page.fill("#p-text", "OPENBRICK3D");
  // Studs on top turns the label off; no ring turns the ring off.
  n = w.evaluations;
  await page.check("#p-studsOnTop");
  await page.click('input[name="p-loop"][value="none"]');
  w = await waitFor(page, { after: n, want: (p) => p.contentHash === d.keychain.contentHash }, 120_000, `${label} keychain with studs`);
  checkPart(w.part, d.keychain, `${label} keychain`);
  check(w.part.params.studsOnTop === true && w.part.params.width === 2, `${label}: keychain values ${JSON.stringify(w.part.params)}`);

  // A fit profile chosen as a part is refused, and the part stays.
  await page.setInputFiles("#part-file", join(repo, "profiles/fit/example-mk4s-petg-0.4.json"));
  await page.waitForFunction(() => document.querySelector("#part-note").className.includes("bad"), null, { timeout: 30_000, polling: 50 });
  const refused = await page.textContent("#part-note");
  check(refused.includes("a `fit` document") && refused.includes("keeps Nameplate keychain"), `${label}: a fit profile as a part gives "${refused}"`);
  check((await web(page)).design.id === "ndn.nameplate-keychain", `${label}: the part changed after a refused file`);
  if (shots) await page.screenshot({ path: join(shots, "designer-keychain.png") });

  w = await web(page);
  check(errors.length === 0, `${label}: page errors: ${errors.join(" | ")}`);
  check(w.warnings.length === 0, `${label}: page warnings: ${w.warnings.join(" | ")}`);
  console.log(
    `# ${label}: the Neuraldeep.net tile opened (bundled and from its file) equals native, 2x6 with "Pressed-in mark" equals native, ` +
      `a new preset and the saved part (${saved.bytes.length} bytes) are byte-identical to native, the CLI builds it with the page's hash ${d.savedPart.contentHash.slice(0, 12)}…, ` +
      `the keychain names relief.text and loop.keyring and builds with studs on top and no ring, a fit profile is refused as a part`,
  );
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
      `deboss ${native.part.deboss.triangles}, inlay ${native.part.inlay.triangles}, badge ${native.part["badge-emboss"].triangles}, ` +
      `coupon ${native.coupon.triangles} triangles ${native.coupon.contentHash}`,
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
    try {
      await designerRun(browser, origin, native);
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
