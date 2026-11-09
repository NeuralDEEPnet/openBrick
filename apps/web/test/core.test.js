// The real core: the wasm-bindgen build in src/wasm/ (from `cargo xtask wasm`),
// loaded in Node and driven through the same dispatch() as the worker. Its 2x4
// must equal LOD0 of the harness meshes, which the native kernel wrote
// (crates/openbrick-geom/tests/harness_meshes.rs keeps those in step). Skipped,
// with a reason, when the core has not been built; the smoke test always builds it.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { cellMap, choose, chosenCells, labelText, offsetRows, recordInput, signedMm } from "../src/calibration.js";
import { controlsFor, defaultValues, paramOverrides, visibility } from "../src/params.js";
import { withCoreDefaults } from "../src/part.js";
import { LOAD_STEP, STATS_INLINE_LIMIT, createHandler, dispatch } from "../src/protocol.js";
import { ModelTable } from "../src/model-table.js";
import { layoutText } from "../../../bench/scale/layouts.mjs";
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
    ["neuraldeep-wordmark", "neuraldeep-square", "sample"],
    "the NEURALDEEP.NET wordmark first: the playground's default",
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

test("the WebAssembly core opens, presets and saves the parts in parts/ as the page sends them", { skip }, async () => {
  const core = await loadCore();
  const read = (name) => readFileSync(new URL(`../../../parts/${name}`, import.meta.url), "utf8");
  const brand = read("ndn.brand-tile.obpart.json");
  const opened = dispatch(core, { id: 1, type: "openPart", text: brand, process: "fdm-0.4" }).reply.result;
  assert.equal(opened.ok, true, opened.error);
  assert.deepEqual(
    opened.presets.map((p) => [p.name, p.error]),
    JSON.parse(brand).presets.map((p) => [p.name, null]),
  );
  assert.deepEqual(opened.unsupported, []);
  // Every part in parts/ opens, and the values the page's controls send save.
  for (const name of ["ndn.logo-tile.obpart.json", "ndn.brand-tile.obpart.json", "ndn.brand-badge.obpart.json", "ndn.nameplate-keychain.obpart.json"]) {
    const text = read(name);
    const answer = dispatch(core, { id: 2, type: "openPart", text, process: "fdm-0.4" }).reply.result;
    assert.equal(answer.ok, true, name);
    const spec = controlsFor(withCoreDefaults(JSON.parse(text), answer.defaults), { maxStuds: core.max_studs() });
    const values = defaultValues(spec);
    const params = paramOverrides(spec, values, visibility(spec, values).visible);
    const saved = dispatch(core, { id: 3, type: "savePart", doc: text, params, process: "fdm-0.4" }).reply;
    assert.equal(saved.ok, true, `${name}: ${saved.error}`);
    // Saving the same values again changes nothing.
    const again = dispatch(core, { id: 4, type: "savePart", doc: saved.result.text, params: {}, process: "fdm-0.4" }).reply.result.text;
    assert.equal(again, saved.result.text, name);
  }
  const keychain = dispatch(core, { id: 5, type: "openPart", text: read("ndn.nameplate-keychain.obpart.json"), process: "fdm-0.4" }).reply.result;
  assert.deepEqual(
    keychain.unsupported.map((u) => u.op),
    ["relief.text", "loop.keyring"],
  );
  // A preset added through the protocol reads back as the page sent it.
  const params = { size: [2, 6], relief: "deboss", reliefHeight: 0.6 };
  const withPreset = dispatch(core, { id: 6, type: "addPreset", doc: brand, name: "Long", params, process: "fdm-0.4" }).reply.result.text;
  assert.deepEqual(JSON.parse(withPreset).presets.at(-1), { name: "Long", params });
  // Values written with units come back as the numbers the panel shows.
  const doc = JSON.parse(brand);
  doc.params.reliefHeight.default = "0.6mm";
  doc.presets = [{ name: "Higher", params: { reliefHeight: "0.8mm" } }];
  const units = dispatch(core, { id: 8, type: "openPart", text: JSON.stringify(doc), process: "fdm-0.4" }).reply.result;
  assert.equal(units.defaults.reliefHeight, 0.6);
  assert.deepEqual(units.presets, [{ name: "Higher", params: { reliefHeight: 0.8 }, error: null }]);
  const refused = dispatch(core, { id: 7, type: "savePart", doc: brand, params: { reliefHeight: 9 }, process: "fdm-0.4" }).reply;
  assert.equal(refused.ok, false);
  assert.match(refused.error, /reliefHeight/);
});

// ------------------------------------------------------------ the assembler

test("the real core's assembler: catalog, snap, place, undo, save and load through dispatch()", { skip }, async () => {
  const core = await loadCore();
  let n = 0;
  const ask = (request) => {
    const { reply, transfer } = dispatch(core, { id: ++n, ...request });
    return { reply, transfer };
  };
  const ok = (request) => {
    const { reply, transfer } = ask(request);
    assert.equal(reply.ok, true, `${request.type}: ${reply.error}`);
    return { result: reply.result, transfer };
  };
  const catalog = ok({ type: "asmInit" }).result;
  assert.equal(catalog.parts.length, 14);
  assert.equal(catalog.rotations.length, 24);
  assert.equal(catalog.lduMm, 0.4);
  const base = catalog.parts.find((p) => p.base);
  const down = (x, z) => ({ origin: [x, -1000, z], dir: [0, 1, 0] });

  const meshes = ok({ type: "asmMeshes", detail: true });
  assert.equal(meshes.result.meshes.length, 14);
  assert.equal(meshes.transfer.length, 28, "every mesh array is transferred");
  const brick = meshes.result.meshes.find((m) => m.id === "ob.brick-2x4");
  const baseplate = meshes.result.meshes.find((m) => m.id === base.id);
  assert.ok(brick.triangles > 1500, "the brick is detailed");
  assert.ok(baseplate.triangles < 50000, "the baseplate is the cheap level");

  let state = ok({
    type: "asmApply",
    command: { op: "add", part: base.id, pos: [0, -base.heightLdu, 0], rot: 0, colour: catalog.palette.defaultBase },
  }).result;
  assert.equal(state.stats.parts, 1);
  assert.equal(state.instances instanceof Int32Array, true);
  assert.equal(state.canUndo, true);

  assert.equal(ok({ type: "asmPick", ray: { origin: [0, 100, 0], dir: [0, 1, 0] } }).result, null, "below the ground: nothing");
  const snap = ok({ type: "asmSnap", part: "ob.brick-2x4", rot: 0, ray: down(3, 47) }).result;
  assert.equal(snap.valid, true);
  assert.deepEqual(snap.pos, [0, -32, 40]);
  const placed = ok({ type: "asmApply", command: { op: "add", part: "ob.brick-2x4", pos: snap.pos, rot: snap.rot, colour: 4 } });
  assert.equal(placed.result.id, 2);
  assert.equal(placed.transfer.length, 1, "the instance array is transferred");
  assert.deepEqual(placed.result.stats.floating, []);
  assert.equal(placed.result.bom.length, 2);

  // The engine refuses an overlap with a message that says what to do.
  const refused = ask({ type: "asmApply", command: { op: "add", part: "ob.brick-2x4", pos: snap.pos, rot: 0, colour: 4 } });
  assert.equal(refused.reply.ok, false);
  assert.match(refused.reply.error, /only touch/);
  assert.equal(refused.reply.fatal, undefined, "a refused edit is not a trap");

  const turned = ok({ type: "asmTurn", rot: 0, quarterTurns: 1 }).result.rot;
  assert.notEqual(turned, 0);
  assert.equal(ok({ type: "asmTurn", rot: turned, quarterTurns: 3 }).result.rot, 0);

  const saved = ok({ type: "asmSave", name: "Test model" }).result.text;
  assert.match(saved, /"catalog": "openbrick-builtin@1"/);
  state = ok({ type: "asmUndo" }).result;
  assert.equal(state.stats.parts, 1);
  assert.equal(state.canRedo, true);
  state = ok({ type: "asmRedo" }).result;
  assert.equal(state.stats.parts, 2);
  state = ok({ type: "asmApply", command: { op: "clearBuilt" } }).result;
  assert.equal(state.stats.parts, 1);
  state = ok({ type: "asmLoad", text: saved }).result;
  assert.equal(state.stats.parts, 2);
  assert.equal(state.canUndo, false, "opening a file starts a new history");
  assert.equal(ok({ type: "asmSave", name: "Test model" }).result.text, saved, "save, open, save is byte-identical");
  assert.equal(ask({ type: "asmLoad", text: "{}" }).reply.ok, false);
  assert.equal(ok({ type: "asmState" }).result.stats.parts, 2, "a failed open keeps the model");

  for (const sample of catalog.samples) {
    const text = ok({ type: "asmSample", id: sample.id }).result.text;
    const s = ok({ type: "asmLoad", text }).result;
    assert.deepEqual(s.stats.floating, [], sample.id);
    assert.ok(s.stats.parts >= 8);
  }
  assert.equal(ask({ type: "asmSnap", part: "ob.nope", rot: 0, ray: down(0, 0) }).reply.ok, false);
  assert.equal(ask({ type: "asmSnap", part: "ob.brick-2x4", rot: 0, ray: { origin: [0, 0, 0] } }).reply.ok, false);
});

// ------------------------------------------------------------ an LDraw pack

const cli = new URL("../../../target/debug/openbrick", import.meta.url).pathname;
const fixtureLibrary = new URL("../../../crates/openbrick-catalog/tests/fixtures/ldraw", import.meta.url).pathname;
const skipPack = skip || (existsSync(cli) ? false : "target/debug/openbrick is not built; run `cargo build -p openbrick`");

/** The mini pack the CLI makes from the synthetic fixture library (ours, written for the tests). */
function miniPack(extra = []) {
  const dir = mkdtempSync(join(tmpdir(), "openbrick-pack-"));
  const out = join(dir, "mini.obpack");
  const r = spawnSync(cli, ["catalog", "import", fixtureLibrary, "--out", out, "--category-filter", "Brick,Plate,Tile,Technic", ...extra], { encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
  const bytes = new Uint8Array(readFileSync(out));
  rmSync(dir, { recursive: true });
  return bytes;
}

test("the real core loads an LDraw pack: parts join the catalog, models say which pack they need", { skip: skipPack }, async () => {
  const core = await loadCore();
  let n = 0;
  const ok = (request) => {
    const { reply } = dispatch(core, { id: ++n, ...request });
    assert.equal(reply.ok, true, `${request.type}: ${reply.error}`);
    return reply.result;
  };
  const fails = (request) => {
    const { reply } = dispatch(core, { id: ++n, ...request });
    assert.equal(reply.ok, false, request.type);
    assert.equal(reply.fatal, undefined, "a refusal is not a trap");
    return reply.error;
  };
  const before = ok({ type: "asmInit" });
  assert.equal(before.pack, null);
  assert.equal(before.parts.length, 14);
  assert.ok(before.maxPackBytes >= 64 * 1024 * 1024);
  // The core is shared with the tests above: start from an empty model, then one of built-in parts to carry over the load.
  ok({
    type: "asmLoad",
    text: '{"openbrick":"1.0","kind":"model","name":"e","units":"ldu","frame":"ldraw-y-down","catalog":"openbrick-builtin@1","palette":[],"bom":[],"submodels":[{"id":"main","instances":[]}]}',
  });
  ok({ type: "asmApply", command: { op: "add", part: "ob.brick-2x4", pos: [0, -24, 0], rot: 0, colour: 4 } });

  const bytes = miniPack();
  const loaded = ok({ type: "asmLoadPack", bytes });
  assert.equal(loaded.kept, true, "the built-in model survives");
  assert.equal(loaded.stats.parts, 1);
  assert.equal(loaded.pack.library, "2026-01");
  assert.match(loaded.pack.attribution, /LDraw Parts Library/);
  assert.ok(loaded.pack.licences.length >= 1);
  const catalog = loaded.catalog;
  assert.equal(catalog.parts.length, 14 + 11);
  assert.equal(catalog.parts[0].id, "ob.brick-1x1", "built-in part numbers do not move");
  const p3003 = catalog.parts.findIndex((p) => p.id === "ld.3003");
  assert.ok(p3003 >= 14);
  assert.equal(catalog.parts[p3003].origin, "ldraw");
  assert.equal(catalog.parts[p3003].category, "Brick");
  assert.equal(catalog.parts[p3003].connectivity, "derived");
  assert.equal(catalog.parts[p3003].author, "OpenBrick3D tests");
  assert.equal(catalog.parts.find((p) => p.id === "ld.4081b").joints[0], "clip");

  // Meshes are asked for by part number, never all at once by default.
  assert.equal(ok({ type: "asmMeshes", detail: true }).meshes.length, 14);
  const m = ok({ type: "asmMeshes", detail: true, parts: [p3003] }).meshes[0];
  assert.equal(m.id, "ld.3003");
  assert.ok(m.triangles > 50 && m.positions.length > 0);
  assert.match(fails({ type: "asmMeshes", parts: [9999] }), /no catalog part number/);
  assert.match(fails({ type: "asmMeshes", parts: Array.from({ length: 300 }, (_, i) => i) }), /at most/);

  // The engine places and snaps a pack part: one more brick on top of the 2x4 has its studs connected.
  const snap = ok({ type: "asmSnap", part: "ld.3003", rot: 0, ray: { origin: [10, -1000, 10], dir: [0, 1, 0] } });
  assert.equal(snap.valid, true);
  assert.equal(snap.mates, 4);
  const placed = ok({ type: "asmApply", command: { op: "add", part: "ld.3003", pos: snap.pos, rot: snap.rot, colour: 2 } });
  assert.equal(placed.stats.parts, 2);
  assert.equal(placed.stats.connections, 4);
  assert.match(placed.text, /"requires"/);
  assert.match(placed.text, /"library":"2026-01"/);

  // The same model is described to a core without the pack: it needs ldraw, and says which parts.
  const needs = ok({ type: "asmNeeds", text: placed.text });
  assert.equal(needs.missingCount, 0, "with the pack nothing is missing");
  const gone = ok({ type: "asmUnloadPack" });
  assert.equal(gone.kept, false, "the model used pack parts, so it cannot stay");
  assert.equal(gone.catalog.parts.length, 14);
  const missing = ok({ type: "asmNeeds", text: placed.text });
  assert.equal(missing.missingCount, 1);
  assert.deepEqual(missing.missing, ["ld.3003"]);
  assert.equal(missing.requires[0].pack, "ldraw");
  assert.match(fails({ type: "asmLoad", text: placed.text }), /LDraw pack `ldraw` \(library 2026-01\), which is not loaded/);
  // A damaged or foreign file is refused with a message, and the core goes on.
  assert.match(fails({ type: "asmLoadPack", bytes: new Uint8Array([1, 2, 3]) }), /not an OpenBrick3D pack/);
  const flipped = bytes.slice();
  flipped[40] ^= 1;
  assert.match(fails({ type: "asmLoadPack", bytes: flipped }), /checksum/);
  assert.equal(ok({ type: "asmInit" }).parts.length, 14);
  // Loading it again works (the page does this when a worker is replaced).
  ok({ type: "asmLoadPack", bytes });
  const again = ok({ type: "asmLoad", text: placed.text });
  assert.equal(again.stats.parts, 2);
  assert.deepEqual(again.notices, [], "the pack the model was made with: nothing to say");
  // Another build of the pack (the low-resolution primitives): the model opens, and says so.
  ok({ type: "asmLoadPack", bytes: miniPack(["--resolution", "low"]) });
  const other = ok({ type: "asmLoad", text: placed.text });
  assert.equal(other.stats.parts, 2, "a pack of another version still opens the model");
  assert.equal(other.notices.length, 1);
  assert.match(other.notices[0], /different build of the `ldraw` pack/);
});

// ------------------------------------------------------------ big models: deltas, stepped open, levels

test("a lean edit sends what changed, and the page's table follows the core's after every edit", { skip }, async () => {
  const core = await loadCore();
  let n = 0;
  const ok = (request) => {
    const { reply } = dispatch(core, { id: ++n, ...request });
    assert.equal(reply.ok, true, `${request.type}: ${reply.error}`);
    return reply.result;
  };
  const table = new ModelTable();
  const sync = (r) => (r.instances ? table.replaceAll(r.instances) : table.applyDelta(r.delta));
  sync(ok({ type: "asmLoad", text: layoutText("city", 300) }));
  const same = () => assert.deepEqual([...table.toRows()], [...ok({ type: "asmState", lean: true }).instances], "the table equals the core's");
  assert.equal(table.size, 300 + 1);
  // (the baseplate first, then the columns)
  const placed = ok({ type: "asmApply", lean: true, command: { op: "recolour", id: 5, colour: 4 } });
  assert.equal("text" in placed, false, "a lean reply carries no model text");
  assert.equal(placed.delta[0], 0);
  assert.equal(placed.delta.length, 1 + 7, "one part changed, one row");
  assert.ok(placed.stats && placed.bom, "a small model's statistics come with the edit");
  assert.ok(STATS_INLINE_LIMIT >= 1000);
  sync(placed);
  same();
  sync(ok({ type: "asmApply", lean: true, command: { op: "remove", id: 7 } }));
  same();
  sync(ok({ type: "asmUndo", lean: true }));
  same();
  sync(ok({ type: "asmRedo", lean: true }));
  same();
  sync(ok({ type: "asmApply", lean: true, command: { op: "clearBuilt" } }));
  same();
  sync(ok({ type: "asmUndo", lean: true }));
  same();
  // The statistics on their own, as the page asks for them when editing pauses.
  const stats = ok({ type: "asmStats" });
  assert.equal(stats.parts, table.size);
  assert.equal(stats.stats.parts, table.size);
  assert.equal(stats.stats.floating.length, 0);
  // The table put back into a core: the same model.
  const rows = table.toRows();
  const restored = ok({ type: "asmLoadRows", rows });
  assert.deepEqual([...restored.instances], [...rows]);
  assert.equal(restored.canUndo, false, "putting a table back starts a new history");
  assert.equal(ok({ type: "asmStats" }).stats.connections, stats.stats.connections);
  // A bad table is refused with a message and the model stays.
  const bad = rows.slice();
  bad[5] = 99;
  assert.equal(dispatch(core, { id: ++n, type: "asmLoadRows", rows: bad }).reply.ok, false);
  assert.equal(ok({ type: "asmState", lean: true }).parts, table.size);
});

test("the worker opens a document in steps, tells how far it is, and a cancel in between stops it", { skip }, async () => {
  const core = await loadCore();
  const posted = [];
  const handle = createHandler(
    async () => core,
    (reply, transfer) => posted.push({ reply, transfer }),
    undefined,
    () => new Promise((resolve) => setTimeout(resolve, 0)),
  );
  await handle({ id: 1, type: "init" });
  const text = layoutText("city", LOAD_STEP * 2 + 500);
  await handle({ id: 2, type: "asmLoad", text, lean: true });
  const progress = posted.filter((p) => p.reply.progress && p.reply.id === 2).map((p) => p.reply.progress);
  assert.deepEqual(progress[0], { done: 0, total: progress[0].total });
  assert.ok(progress.length >= 4, `progress messages: ${JSON.stringify(progress)}`);
  assert.deepEqual(progress.map((p) => p.done), [...progress.map((p) => p.done)].sort((a, b) => a - b), "ascending");
  assert.equal(progress.at(-1).done, progress.at(-1).total);
  const done = posted.at(-1);
  assert.equal(done.reply.ok, true);
  assert.equal(done.reply.result.parts, progress.at(-1).total);
  assert.equal(done.reply.result.instances.length, progress.at(-1).total * 7);
  assert.equal("stats" in done.reply.result, false, "a big model's statistics are asked for later");
  assert.ok(done.transfer.length >= 1, "the table is transferred, not copied");
  // A cancel that arrives between steps ends the open; the model is as it was.
  posted.length = 0;
  const opening = handle({ id: 3, type: "asmLoad", text, lean: true });
  await new Promise((resolve) => setTimeout(resolve, 0));
  await handle({ id: 4, type: "asmCancel" });
  await opening;
  const last = posted.find((p) => p.reply.id === 3 && p.reply.ok !== undefined).reply;
  assert.equal(last.ok, false);
  assert.match(last.error, /cancelled/);
  const state = dispatch(core, { id: 5, type: "asmState", lean: true }).reply.result;
  assert.equal(state.parts, progress.at(-1).total, "the model from the first open is still there");
  // A document that is refused in the middle says which part and leaves the model alone.
  posted.length = 0;
  const broken = text.replace('"rot":0', '"rot":77');
  await handle({ id: 6, type: "asmLoad", text: broken, lean: true });
  const refused = posted.at(-1).reply;
  assert.equal(refused.ok, false);
  assert.match(refused.error, /rotation 77/);
  assert.equal(dispatch(core, { id: 7, type: "asmState", lean: true }).reply.result.parts, progress.at(-1).total);
});

test("the render levels come from the core and never change what the core snaps to", { skip }, async () => {
  const core = await loadCore();
  const ok = (request) => {
    const { reply } = dispatch(core, { id: 1, ...request });
    assert.equal(reply.ok, true, reply.error);
    return reply.result;
  };
  const catalog = ok({ type: "asmInit" });
  const brick = catalog.parts.findIndex((p) => p.id === "ob.brick-2x4");
  const baseplate = catalog.parts.findIndex((p) => p.base);
  const { meshes } = ok({ type: "asmMeshes", parts: [brick, baseplate], lods: [0, 1, 2, 3] });
  const by = (index) => Object.fromEntries(meshes.filter((m) => m.index === index).map((m) => [m.lod, m.triangles]));
  const t = by(brick);
  assert.ok(t[0] > t[1] && t[1] > t[2] && t[2] > t[3] && t[3] === 12, JSON.stringify(t));
  assert.equal(by(baseplate)[0], by(baseplate)[1], "the baseplate is never built at full detail");
  assert.throws(() => dispatch(core, { id: 2, type: "asmMeshes", parts: [brick], lods: [4] }).reply.ok || (() => { throw new Error("refused"); })());
  const snap = () => ok({ type: "asmSnap", part: "ob.brick-2x4", rot: 0, ray: { origin: [0, -500, 0], dir: [0, 1, 0] } });
  const before = snap();
  ok({ type: "asmMeshes", parts: [brick], lods: [3] });
  assert.deepEqual(snap(), before);
});

// ------------------------------------------------------------ the page's table against the core

/** A seeded generator, so a failure names its seed. */
function rng(seed) {
  let s = seed >>> 0 || 1;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

test("ids up to 4,294,967,295 survive load, pick, delete, undo and a restart through the page's table", { skip }, async () => {
  const core = await loadCore();
  let n = 0;
  const ok = (request) => {
    const { reply } = dispatch(core, { id: ++n, ...request });
    assert.equal(reply.ok, true, `${request.type}: ${reply.error}`);
    return reply.result;
  };
  const inst = (id, x) => ({ id, part: "ob.brick-2x4", pose: { pos: [x, -24, 0], rot: 0 }, colour: 4 });
  const text = JSON.stringify({
    openbrick: "1.0", kind: "model", name: "big ids", units: "ldu", frame: "ldraw-y-down", catalog: "openbrick-builtin@1", palette: [], bom: [],
    submodels: [{ id: "main", instances: [inst(5, 0), inst(3_000_000_000, 100), inst(4_294_967_294, 200)] }],
  });
  const table = new ModelTable();
  const sync = (r) => (r.instances ? table.replaceAll(r.instances) : table.applyDelta(r.delta));
  sync(ok({ type: "asmLoad", text, lean: true }));
  assert.deepEqual([...table.toRows()].filter((_, i) => i % 7 === 0).map((v) => v >>> 0), [5, 3_000_000_000, 4_294_967_294]);
  // The pick names the largest id and the page's table knows it.
  const hit = ok({ type: "asmPick", ray: { origin: [200, -500, 0], dir: [0, 1, 0] } });
  assert.equal(hit.instance, 4_294_967_294);
  assert.equal(table.get(hit.instance)?.pos[0], 200, "hover, outline and delete find the part");
  // Delete it (by the id the pick gave), undo, and compare with the core after each.
  const same = () => assert.deepEqual([...table.toRows()], [...ok({ type: "asmState", lean: true }).instances]);
  sync(ok({ type: "asmApply", lean: true, command: { op: "remove", id: hit.instance } }));
  assert.equal(table.has(4_294_967_294), false);
  same();
  sync(ok({ type: "asmUndo", lean: true }));
  assert.equal(table.has(4_294_967_294), true);
  same();
  // A restart: the table's rows go into an empty core, in unsigned id order, and come back the same.
  const rows = table.toRows();
  const restored = ok({ type: "asmLoadRows", rows });
  assert.deepEqual([...restored.instances], [...rows]);
  assert.equal(ok({ type: "asmPick", ray: { origin: [100, -500, 0], dir: [0, 1, 0] } }).instance, 3_000_000_000);
});

test("the page's table equals the core's after random edits, failed commands, undo, redo, clears and pack parts", { skip: skip || skipPack }, async () => {
  const core = await loadCore();
  let n = 0;
  const ask = (request) => dispatch(core, { id: ++n, ...request }).reply;
  const ok = (request) => {
    const reply = ask(request);
    assert.equal(reply.ok, true, `${request.type}: ${reply.error}`);
    return reply.result;
  };
  ok({ type: "asmLoadPack", bytes: miniPack() });
  const catalog = ok({ type: "asmInit" });
  const names = catalog.parts.filter((p) => !p.base && p.connectivity !== "none").map((p) => p.id);
  assert.ok(names.some((id) => id.startsWith("ld.")), "pack parts are in the mix");
  for (let seed = 1; seed <= 8; seed++) {
    const r = rng(seed * 7919);
    const pick = (a) => a[Math.floor(r() * a.length)];
    ok({ type: "asmLoad", text: JSON.stringify({ openbrick: "1.0", kind: "model", name: "e", units: "ldu", frame: "ldraw-y-down", catalog: "openbrick-builtin@1", palette: [], bom: [], submodels: [{ id: "main", instances: [] }] }), lean: true });
    const table = new ModelTable();
    table.replaceAll(ok({ type: "asmState", lean: true }).instances);
    let failed = 0;
    for (let step = 0; step < 120; step++) {
      const at = `seed ${seed} step ${step}`;
      let reply;
      const roll = r();
      if (roll < 0.5) {
        const part = pick(names);
        const ray = { origin: [(r() - 0.5) * 400, -300, (r() - 0.5) * 400], dir: [0.1 * (r() - 0.5), 1, 0.1 * (r() - 0.5)] };
        const snap = ok({ type: "asmSnap", part, rot: pick([0, 1, 2, 3]), ray });
        // Often the snapped pose; sometimes a pose that must be refused (the page never sends one, a hostile page could).
        const pos = snap && r() < 0.85 ? snap.pos : [Math.round(r() * 40) * 20 - 400, -24, Math.round(r() * 40) * 20 - 400];
        reply = ask({ type: "asmApply", lean: true, command: { op: "add", part, pos, rot: snap ? snap.rot : 0, colour: pick([2, 4, 15]) } });
      } else if (roll < 0.62) reply = ask({ type: "asmApply", lean: true, command: { op: "remove", id: 1 + Math.floor(r() * 40) } });
      else if (roll < 0.7) reply = ask({ type: "asmApply", lean: true, command: { op: "recolour", id: 1 + Math.floor(r() * 40), colour: pick([4, 14, 99999]) } });
      else if (roll < 0.76) reply = ask({ type: "asmApply", lean: true, command: { op: "turn", id: 1 + Math.floor(r() * 40), quarterTurns: 1 } });
      else if (roll < 0.9) reply = ask({ type: r() < 0.5 ? "asmUndo" : "asmRedo", lean: true });
      else if (roll < 0.93) reply = ask({ type: "asmApply", lean: true, command: { op: r() < 0.5 ? "clear" : "clearBuilt" } });
      else reply = ask({ type: "asmState", lean: true });
      if (!reply.ok) {
        failed++;
        assert.equal(reply.fatal, undefined, `${at}: a refusal is not a trap`);
      } else if (reply.result.instances) table.replaceAll(reply.result.instances);
      else table.applyDelta(reply.result.delta);
      const want = ok({ type: "asmState", lean: true });
      assert.deepEqual([...table.toRows()], [...want.instances], `${at}: the table equals the core's`);
    }
    assert.ok(failed > 0, "some commands failed, and the table was still right after them");
  }
});
