// The real core: the wasm-bindgen build in src/wasm/ (from `cargo xtask wasm`),
// loaded in Node and driven through the same dispatch() as the worker. Its 2x4
// must equal LOD0 of the harness meshes, which the native kernel wrote
// (crates/openbrick-geom/tests/harness_meshes.rs keeps those in step). Skipped,
// with a reason, when the core has not been built; the smoke test always builds it.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { cellMap, choose, chosenCells, labelText, offsetRows, recordInput, signedMm } from "../src/calibration.js";
import { controlsFor, defaultValues, paramOverrides, visibility } from "../src/params.js";
import { dispatch } from "../src/protocol.js";
import { sampleFile, sizeForSample } from "../src/samples.js";
import { findingView } from "../src/ui.js";

const glue = new URL("../src/wasm/openbrick_wasm.js", import.meta.url);
const skip = existsSync(glue) ? false : "src/wasm/ is not built; run `pnpm --dir apps/web wasm` (cargo xtask wasm)";
const manifest = JSON.parse(readFileSync(new URL("../../../bench/harness/public/meshes/manifest.json", import.meta.url), "utf8"));
const lod0 = manifest.lods.find((l) => l.lod === 0);

async function loadCore() {
  const core = await import(glue);
  core.initSync({ module: readFileSync(new URL("../src/wasm/openbrick_wasm_bg.wasm", import.meta.url)) });
  return core;
}

test("the WebAssembly core builds the native 2x4 bit for bit", { skip }, async () => {
  const core = await loadCore();
  const { reply, transfer } = dispatch(core, { id: 1, type: "brick", width: 2, length: 4 });
  assert.equal(reply.ok, true, reply.error);
  const r = reply.result;
  assert.equal(r.kernel, manifest.kernel);
  assert.equal(r.triangles, lod0.triangles);
  assert.equal(r.vertices, lod0.vertices);
  assert.equal(r.fingerprint, lod0.fingerprint);
  assert.ok(Math.abs(r.volumeMm3 - lod0.volume_mm3) < 5e-4, `${r.volumeMm3} vs ${lod0.volume_mm3}`);
  assert.equal(r.positions.length, 3 * r.vertices);
  assert.equal(r.indices.length, 3 * r.triangles);
  assert.equal(transfer.length, 2);
  for (let k = 0; k < 3; k++) {
    assert.ok(Math.abs(r.bounds[k] - lod0.bounds_mm.min[k]) < 5e-4);
    assert.ok(Math.abs(r.bounds[3 + k] - lod0.bounds_mm.max[k]) < 5e-4);
  }
});

test("the WebAssembly core reports its kernel, limits and standard, and refuses bad sizes", { skip }, async () => {
  const core = await loadCore();
  const info = dispatch(core, { id: 1, type: "info" }).reply.result;
  assert.equal(info.kernel, manifest.kernel);
  assert.equal(info.standard.pitchMm, manifest.pitch_mm);
  assert.equal(info.standard.undersizeMm, manifest.undersize_per_side_mm);
  assert.equal(`dimensions-v1 ${info.standard.version}`, manifest.standard);
  // The JavaScript check and the Rust check agree on the limits.
  const tooBig = dispatch(core, { id: 2, type: "brick", width: info.maxStuds + 1, length: 1 }).reply;
  assert.equal(tooBig.ok, false);
  assert.throws(() => core.build_nominal_brick(info.maxStuds + 1, 1), new RegExp(`1 to ${info.maxStuds} studs`));
  assert.throws(() => core.build_nominal_brick(0, 1), /1 to \d+ studs, got 0/);
  const logo = readFileSync(new URL("../../../parts/ndn.logo-tile.obpart.json", import.meta.url), "utf8");
  assert.deepEqual(dispatch(core, { id: 3, type: "checkPart", text: logo }).reply.result, []);
});

test("the WebAssembly core evaluates, validates and exports the logo tile with its sample artwork", { skip }, async () => {
  const core = await loadCore();
  const b = dispatch(core, { id: 1, type: "bundled" }).reply.result;
  const doc = JSON.parse(b.logoTilePart);
  const spec = controlsFor(doc, { maxStuds: core.max_studs() });
  const values = defaultValues(spec);
  // The CC0 sample mark, the logo tile's own default artwork.
  const cc0 = sampleFile(b.samples.find((s) => s.id === "sample"), values.size);
  assert.equal(cc0.asset, values.logo);
  const assets = [{ name: values.logo, bytes: new TextEncoder().encode(cc0.svg) }];
  const request = { doc: b.logoTilePart, params: paramOverrides(spec, values), process: "fdm-0.4", fit: b.exampleFitProfile, assets };
  const { reply, transfer } = dispatch(core, { id: 2, type: "evaluatePart", ...request });
  assert.equal(reply.ok, true, reply.error);
  const r = reply.result;
  assert.deepEqual(r.meshes.map((m) => m.role), ["body", "accent"]);
  assert.deepEqual(r.meshes.map((m) => m.color), [b.roleColors.body, b.roleColors.accent]);
  assert.equal(r.triangles, r.meshes[0].triangles + r.meshes[1].triangles);
  assert.equal(r.overlay?.role, "removed", "the sample's hairline is drawn in red");
  assert.ok(r.measurements.removedAreaPct > 0);
  assert.equal(r.exportAllowed, true, JSON.stringify(r.report));
  assert.deepEqual(r.footprint, values.size);
  assert.equal(r.kernel, manifest.kernel);
  assert.match(r.contentHash, /^[0-9a-f]{64}$/);
  assert.equal(transfer.length, 6);
  // The sample's hairline: FEATURE.MIN on the removed area, in percent, as the
  // core says (openbrick_print::finding_unit), not as the message reads.
  const area = r.report.findings.find((f) => f.rule === "FEATURE.MIN");
  assert.equal(area?.unit, "%", JSON.stringify(r.report));
  assert.equal(area.value, r.measurements.removedAreaPct);
  assert.match(findingView(area).measured, /^\d+(\.\d+)? %$/);
  const fdm = b.processProfiles.find((p) => p.id === "fdm-0.4");
  assert.equal(area.threshold, fdm.maxRemovedAreaPct);
  assert.equal(findingView(area).threshold, `${fdm.maxRemovedAreaPct} %`);

  // Without a fit profile the fit-critical tube field blocks the export.
  const none = dispatch(core, { id: 3, type: "evaluatePart", ...request, fit: "" }).reply.result;
  assert.equal(none.exportAllowed, false);
  assert.ok(none.report.findings.some((f) => f.rule === "FIT.MISSING" && f.severity === "blocker"));
  for (const f of none.report.findings) assert.equal(f.unit, f.rule === "FIT.MISSING" ? "" : "%", f.rule);
  const refused = dispatch(core, { id: 4, type: "exportPart", format: "3mf", ...request, fit: "" }).reply;
  assert.equal(refused.ok, false);
  assert.match(refused.error, /FIT\.MISSING/);

  const file = dispatch(core, { id: 5, type: "exportPart", format: "3mf", ...request, colors: {} }).reply.result;
  assert.deepEqual([...file.bytes.subarray(0, 4)], [0x50, 0x4b, 3, 4], "a zip container");

  // The page's reading of visibleWhen agrees with the core's for every relief.
  for (const relief of spec.controls.find((c) => c.name === "relief").options.map((o) => o.value)) {
    const resolved = dispatch(core, { id: 6, type: "resolveParams", doc: b.logoTilePart, params: { relief }, process: "fdm-0.4" }).reply;
    assert.equal(resolved.ok, true, resolved.error);
    assert.deepEqual(visibility(spec, { ...values, relief }).visible, resolved.result.visible, relief);
  }
});

test("the Neuraldeep.net samples are the default and print on their tiles with nothing to report", { skip }, async () => {
  const core = await loadCore();
  const b = dispatch(core, { id: 1, type: "bundled" }).reply.result;
  assert.deepEqual(
    b.samples.map((s) => s.id),
    ["neuraldeep-mark", "neuraldeep-badge", "sample"],
    "the Neuraldeep.net mark first: the playground's default",
  );
  const doc = JSON.parse(b.logoTilePart);
  const spec = controlsFor(doc, { maxStuds: core.max_studs() });
  const values = defaultValues(spec);
  let id = 2;
  for (const sample of b.samples.filter((s) => s.notice)) {
    // The first sentence of parts/assets/brand/NOTICE.md, word for word.
    assert.equal(sample.notice, "Copyright Neural Deep Network Ltd. All rights reserved.");
    assert.match(sample.noticeUrl, /\/parts\/assets\/brand\/NOTICE\.md$/);
    // The tile the page builds when the sample is chosen on the default tile.
    const size = sizeForSample(sample, values.size) ?? values.size;
    const file = sampleFile(sample, size);
    assert.deepEqual(file.tile, size, sample.id);
    const params = { ...paramOverrides(spec, { ...values, size }), logo: file.asset };
    const r = dispatch(core, {
      id: id++,
      type: "evaluatePart",
      doc: b.logoTilePart,
      params,
      process: "fdm-0.4",
      fit: b.exampleFitProfile,
      assets: [{ name: file.asset, bytes: new TextEncoder().encode(file.svg) }],
    }).reply;
    assert.equal(r.ok, true, r.error);
    assert.equal(r.result.exportAllowed, true, sample.id);
    assert.deepEqual(r.result.report.findings, [], `${sample.id}: no FEATURE.MIN or anything else`);
    assert.deepEqual(r.result.footprint, size);
  }
});

test("the WebAssembly core's layouts are the map fixtures, and it records the cells tapped on them", { skip }, async () => {
  const core = await loadCore();
  const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8"));
  const cases = [
    ["coupon-layout-fdm-0.4-cross.json", {}],
    ["coupon-layout-fdm-0.4-full-256.json", { pockets: "full", bedMm: [256, 256] }],
  ];
  for (const [name, layout] of cases) {
    const r = dispatch(core, { id: 1, type: "couponLayout", process: "fdm-0.4", layout }).reply;
    assert.equal(r.ok, true, r.error);
    assert.deepEqual(r.result, fixture(name), name);
  }
  // Tap a stud, a bore and a pocket of copy 2 on the default coupon's map.
  const map = cellMap(fixture("coupon-layout-fdm-0.4-cross.json"));
  let choices = {};
  for (const group of ["stud", "tube", "pocket"]) choices = choose(choices, map.cells.find((c) => c.group === group && c.copy === 1));
  const input = recordInput({ process: "fdm-0.4", pocketSet: "cross", printer: "Prusa MINI+", material: "PLA", nozzleMm: 0.4, date: "2026-11-12", choices });
  const r = dispatch(core, { id: 2, type: "recordFit", input }).reply;
  assert.equal(r.ok, true, r.error);
  assert.equal(r.result.ok, true, JSON.stringify(r.result.error));
  assert.equal(r.result.fileName, "prusa-mini-pla-fdm-0.4.json");
  assert.ok(r.result.text.endsWith("}\n"));
  assert.deepEqual(JSON.parse(r.result.text), r.result.fitProfile);
  assert.deepEqual(r.result.fitProfile.calibration.chosenCells, chosenCells(choices));
  assert.deepEqual(r.result.offsets.map((o) => o.cell), ["stud", "tube", "pocketTubeOD", "pocketRib"]);
  // The offsets summary of the real core's offsets: the chosen cell as its
  // label prints it, the core's nominal and offset with the CLI's signs.
  const chosenLabel = Object.fromEntries(
    ["stud", "tube", "pocket"].flatMap((group) => {
      const cell = map.cells.find((c) => c.group === group && c.copy === 1);
      return cell.cross ? [[cell.cellId, cell.lines[0]], [cell.cross.cellId, cell.lines[1]]] : [[cell.cellId, labelText(cell)]];
    }),
  );
  const rows = offsetRows(r.result.offsets);
  assert.deepEqual(
    rows.map((row) => [row.cell, row.chosen, row.offset, row.field]),
    r.result.offsets.map((o) => [o.cell, chosenLabel[o.cell], signedMm(o.offsetMm), o.offsetField]),
  );
  for (const [row, o] of rows.map((row, i) => [row, r.result.offsets[i]])) {
    assert.equal(row.nominal, o.nominalMm.toFixed(3));
    assert.ok(Math.abs(o.chosenMm - o.nominalMm - o.offsetMm) < 1e-9, `${o.cell}: the offset is the chosen cell minus the nominal`);
  }
  // The pocket set says which pockets were printed: a full-set pocket is refused on a cross coupon.
  const full = cellMap(fixture("coupon-layout-fdm-0.4-full-256.json"));
  const labels = new Set(map.cells.map((c) => c.label));
  const extra = full.cells.find((c) => c.group === "pocket" && !labels.has(c.label));
  const refused = dispatch(core, { id: 3, type: "recordFit", input: { ...input, cells: chosenCells(choose(choices, extra)) } }).reply;
  assert.equal(refused.result.ok, false);
  assert.equal(refused.result.error.field, "pocket");
  assert.match(refused.result.error.message, /not a pocket of the `cross` pocket set/);
  const empty = dispatch(core, { id: 4, type: "recordFit", input: { ...input, material: " " } }).reply;
  assert.deepEqual(empty.result.error, { field: "material", message: "material is empty; it is required" });
});

test("the WebAssembly core lays out the coupon and refuses artwork over its limit", { skip }, async () => {
  const core = await loadCore();
  const layout = dispatch(core, { id: 1, type: "couponLayout", process: "fdm-0.4", layout: {} }).reply;
  assert.equal(layout.ok, true, layout.error);
  assert.ok(layout.result.cells.length > 0);
  assert.equal(layout.result.summary.pockets, "cross");
  const b = dispatch(core, { id: 2, type: "bundled" }).reply.result;
  const r = dispatch(core, {
    id: 3,
    type: "evaluatePart",
    doc: b.logoTilePart,
    params: {},
    process: "fdm-0.4",
    fit: "",
    assets: [{ name: "assets/mark.svg", bytes: new Uint8Array(b.maxAssetBytes + 1) }],
  }).reply;
  assert.equal(r.ok, false);
  assert.match(r.error, /limit for artwork is \d+ bytes/);
});
