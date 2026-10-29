// The real core: the wasm-bindgen build in src/wasm/ (from `cargo xtask wasm`),
// loaded in Node and driven through the same dispatch() as the worker. Its 2x4
// must equal LOD0 of the harness meshes, which the native kernel wrote
// (crates/openbrick-geom/tests/harness_meshes.rs keeps those in step). Skipped,
// with a reason, when the core has not been built; the smoke test always builds it.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { controlsFor, defaultValues, paramOverrides, visibility } from "../src/params.js";
import { dispatch } from "../src/protocol.js";
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
  const assets = [{ name: values.logo, bytes: new TextEncoder().encode(b.sampleLogoSvg) }];
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
