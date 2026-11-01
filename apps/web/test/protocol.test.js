// The worker's message protocol (src/protocol.js) against a fake core with the
// same exports as the wasm-bindgen build of crates/openbrick-wasm.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  brick,
  bundled,
  BUILD_TYPES,
  createHandler,
  EXPORT_TYPES,
  dispatch,
  info,
  isTrap,
  parseStuds,
  PROTOCOL_VERSION,
  reportWithUnits,
  standardSummary,
  transferables,
} from "../src/protocol.js";

const STANDARD_TEXT = readFileSync(new URL("../../../standards/dimensions-v1.json", import.meta.url), "utf8");
const STANDARD = JSON.parse(STANDARD_TEXT);
const nominal = (id) => STANDARD.features.find((f) => f.id === id).nominal_mm;
const MAX = 16;

/** A one-triangle stand-in for a wasm MeshOut that records takes and frees. */
function fakeMesh({ role = "body", volume = 1, taken = [], freed = [] } = {}) {
  const mesh = {
    role: () => role,
    color: () => (role === "accent" ? "#f4f1e8" : role === "removed" ? "#e5242b" : "#2f6db5"),
    take_positions: () => (taken.push(`${role} positions`), new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0])),
    take_indices: () => (taken.push(`${role} indices`), new Uint32Array([0, 1, 2])),
    vertex_count: () => 3,
    triangle_count: () => 1,
    volume_mm3: () => volume,
    fingerprint: () => "0x0000000000000001",
    canonical_fingerprint: () => "0x0000000000000002",
    bounds: () => new Float32Array([0, 0, 0, 1, 1, 0]),
    kernel: () => "fake-kernel 1.0",
    free: () => freed.push(mesh),
  };
  return mesh;
}

/**
 * A stand-in for the wasm module: records calls, takes and frees, makes
 * one-triangle meshes. `throws` is an error (or a message) the building calls
 * throw.
 */
function fakeCore({ throws = null } = {}) {
  const calls = [];
  const freed = [];
  const taken = [];
  const assetSets = [];
  const fail = () => {
    if (throws) throw typeof throws === "string" ? new Error(throws) : throws;
  };
  class AssetSet {
    added = [];
    freed = false;
    constructor() {
      assetSets.push(this);
    }
    add(name, bytes) {
      this.added.push([name, bytes.length]);
    }
    free() {
      this.freed = true;
    }
  }
  return {
    calls,
    freed,
    taken,
    assetSets,
    AssetSet,
    kernel_id: () => "fake-kernel 1.0",
    max_studs: () => MAX,
    standard_json: () => STANDARD_TEXT,
    check_part_json: (text) => (text.includes("broken") ? JSON.stringify(["missing field `kind`"]) : "[]"),
    process_profiles_json: () => JSON.stringify([{ id: "fdm-0.4" }]),
    example_fit_profile_json: () => '{"kind":"fit","id":"example"}',
    logo_tile_part_json: () => '{"id":"ndn.logo-tile"}',
    bundled_parts_json: () => '[{"id":"ndn.logo-tile","label":"Logo tile","file":"ndn.logo-tile.obpart.json","text":"{}"}]',
    max_part_bytes: () => 4194304,
    open_part_json(text, process) {
      calls.push(["open_part_json", text, process]);
      return text.startsWith("{") ? JSON.stringify({ ok: true, id: "t", presets: [], unsupported: [] }) : JSON.stringify({ ok: false, error: "the file is not JSON" });
    },
    save_part_json(doc, params, process) {
      calls.push(["save_part_json", doc, params, process]);
      fail();
      return `${doc}\n`;
    },
    add_preset_json(doc, name, params, process) {
      calls.push(["add_preset_json", doc, name, params, process]);
      if (!name.trim()) throw new Error("a preset needs a name");
      return `${doc} ${name}\n`;
    },
    artwork_samples_json: () => '[{"id":"neuraldeep-wordmark","files":[{"asset":"assets/brand/neuraldeep-wordmark-2x6.svg","tile":[2,6],"svg":"<svg/>"}]}]',
    disclaimer: () => "not affiliated",
    role_colors_json: () => '{"body":"#2f6db5","accent":"#f4f1e8"}',
    max_asset_bytes: () => 4194304,
    coupon_choices_json: () => '{"pocketSets":["cross"],"defaultPocketSet":"cross","defaults":{}}',
    build_nominal_brick(width, length) {
      calls.push([width, length]);
      fail();
      return fakeMesh({ volume: width * length, taken, freed });
    },
    evaluate_part(doc, params, process, fit, set) {
      calls.push(["evaluate_part", doc, params, process, fit, set.added]);
      fail();
      const meshes = [fakeMesh({ role: "body", taken, freed }), fakeMesh({ role: "accent", taken, freed })];
      let removed = fakeMesh({ role: "removed", taken, freed });
      const out = {
        mesh_count: () => meshes.length,
        take_mesh: (i) => {
          const m = meshes[i];
          meshes[i] = undefined;
          return m;
        },
        take_removed: () => {
          const m = removed;
          removed = undefined;
          return m;
        },
        report_json: () => JSON.stringify({ findings: [REMOVED_AREA] }),
        report_units_json: () => '["%"]',
        export_allowed: () => true,
        measurements_json: () => '{"process":"fdm-0.4"}',
        warnings_json: () => '["a note"]',
        content_hash: () => "c0ffee",
        kernel: () => "fake-kernel 1.0",
        artwork_json: () => '{"mode":"emboss","removedPolygons":1}',
        params_json: () => '{"values":{"size":[2,4]},"visible":{"size":true}}',
        footprint_studs: () => new Uint32Array([2, 4]),
        bounds: () => new Float64Array([0, 0, 0, 12.5, 25.5, 2.5]),
        volume_mm3: () => 900,
        mass_g_pla: () => 1.116,
        triangle_count: () => 2,
        face_on_bed: () => "bottom",
        free: () => freed.push(out),
      };
      return out;
    },
    export_part(format, doc, params, process, fit, set, colors) {
      calls.push(["export_part", format, params, process, fit, set.added, colors]);
      fail();
      return new Uint8Array([0x50, 0x4b, 3, 4]);
    },
    evaluate_part_on_printer(doc, params, process, fit, set, printer) {
      const out = this.evaluate_part(doc, params, process, fit, set);
      calls[calls.length - 1] = ["evaluate_part_on_printer", printer];
      return out;
    },
    export_part_on_printer(format, doc, params, process, fit, set, colors, printer) {
      calls.push(["export_part_on_printer", format, printer]);
      fail();
      return new Uint8Array([0x50, 0x4b, 3, 4]);
    },
    printers_json: () => '[{"id":"prusa-mini-plus","name":"Original Prusa MINI+","buildVolumeMm":[180,180,180]}]',
    resolve_params_json(doc, params, process) {
      calls.push(["resolve_params_json", params, process]);
      return '{"values":{},"visible":{"reliefHeight":false}}';
    },
    build_coupon(process, layout) {
      calls.push(["build_coupon", process, layout]);
      fail();
      let mesh = fakeMesh({ taken, freed });
      const out = {
        take_mesh: () => {
          const m = mesh;
          mesh = undefined;
          return m;
        },
        cells_json: () => '[{"cellId":"stud","label":"4.60"}]',
        summary_json: () => '{"process":"fdm-0.4"}',
        estimate_json: () => '{"massGPla":32.2}',
        warnings_json: () => "[]",
        content_hash: () => "beef",
        kernel: () => "fake-kernel 1.0",
        file_stem: () => "coupon-fdm-0.4-cross",
        triangle_count: () => 1,
        free: () => freed.push(out),
      };
      return out;
    },
    coupon_layout_json(process, layout) {
      calls.push(["coupon_layout_json", process, layout]);
      return '{"cells":[],"summary":{}}';
    },
    record_fit(input) {
      calls.push(["record_fit", input]);
      fail();
      const parsed = JSON.parse(input);
      return parsed.material
        ? JSON.stringify({ ok: true, fileName: "x.json", text: "{}\n", fitProfile: { id: "x" }, offsets: [] })
        : JSON.stringify({ ok: false, error: { field: "material", message: "material is empty; it is required" } });
    },
    export_coupon(format, process, layout, colors) {
      calls.push(["export_coupon", format, process, layout, colors]);
      fail();
      return new Uint8Array([1, 2, 3]);
    },
  };
}

test("parseStuds accepts whole numbers of studs within the core's limit", () => {
  assert.equal(parseStuds(1, MAX), 1);
  assert.equal(parseStuds(MAX, MAX), MAX);
  assert.equal(parseStuds("4", MAX), 4);
  assert.equal(parseStuds(" 7 ", MAX), 7);
});

test("parseStuds rejects everything else with the field and the limits in the message", () => {
  for (const bad of [0, -1, MAX + 1, 2.5, NaN, Infinity, "", " ", "abc", "2e1", "0x4", "4.0", "-3", null, undefined, {}]) {
    assert.throws(() => parseStuds(bad, MAX, "width"), (err) => {
      assert.ok(err instanceof RangeError);
      assert.match(err.message, /^width must be a whole number of studs from 1 to 16/);
      return true;
    }, `accepted ${JSON.stringify(bad)}`);
  }
});

test("standardSummary reads the lattice numbers from the standard, not from JavaScript", () => {
  const s = standardSummary(STANDARD_TEXT);
  assert.equal(s.version, STANDARD.version);
  assert.equal(s.pitchMm, nominal("module"));
  assert.equal(s.undersizeMm, nominal("undersize_per_side"));
  assert.equal(s.brickHeightMm, nominal("brick_height"));
  assert.equal(s.studHeightMm, nominal("stud_height"));
  const withoutModule = { ...STANDARD, features: STANDARD.features.filter((f) => f.id !== "module") };
  assert.throws(() => standardSummary(withoutModule), /no nominal_mm for module/);
});

test("info reports the protocol, kernel, limits and standard", () => {
  assert.deepEqual(info(fakeCore()), {
    protocol: PROTOCOL_VERSION,
    kernel: "fake-kernel 1.0",
    maxStuds: MAX,
    standard: standardSummary(STANDARD_TEXT),
  });
});

test("a brick request returns plain data with transferable arrays and frees the wasm mesh", () => {
  const core = fakeCore();
  let clock = 100;
  const { reply, transfer } = dispatch(core, { id: 7, type: "brick", width: 2, length: "4" }, () => (clock += 5));
  assert.equal(reply.id, 7);
  assert.equal(reply.ok, true);
  const r = reply.result;
  assert.deepEqual(core.calls, [[2, 4]]);
  assert.deepEqual(core.taken, ["body positions", "body indices"], "each array is moved out once, never cloned");
  assert.equal(core.freed.length, 1, "MeshOut.free() was called");
  assert.deepEqual([r.role, r.color], ["body", "#2f6db5"]);
  assert.deepEqual([r.width, r.length, r.vertices, r.triangles, r.volumeMm3], [2, 4, 3, 1, 8]);
  assert.equal(r.fingerprint, "0x0000000000000001");
  assert.equal(r.canonicalFingerprint, "0x0000000000000002");
  assert.deepEqual(r.bounds, [0, 0, 0, 1, 1, 0]);
  assert.equal(r.kernel, "fake-kernel 1.0");
  assert.equal(r.buildMs, 5, "only the kernel call is timed");
  assert.ok(r.positions instanceof Float32Array && r.indices instanceof Uint32Array);
  assert.deepEqual(transfer, [r.positions.buffer, r.indices.buffer]);
  // postMessage moves transferred buffers: after the structured clone the worker's
  // copies are detached and the page owns the only ones.
  const received = structuredClone(reply, { transfer });
  assert.equal(r.positions.byteLength, 0);
  assert.equal(received.result.positions.length, 9);
  assert.equal(received.result.indices.length, 3);
});

test("a bad size is refused before the core is called", () => {
  const core = fakeCore();
  for (const [width, length] of [[0, 4], [2, MAX + 1], [1.5, 2], ["x", 2]]) {
    const { reply, transfer } = dispatch(core, { id: 1, type: "brick", width, length });
    assert.equal(reply.ok, false);
    assert.match(reply.error, /must be a whole number of studs from 1 to 16/);
    assert.deepEqual(transfer, []);
  }
  assert.deepEqual(core.calls, []);
});

test("a core error becomes an error reply, never an exception", () => {
  const { reply } = dispatch(fakeCore({ throws: "manifold-rust 0.19.1 returned a solid that is not manifold" }), {
    id: 3,
    type: "brick",
    width: 2,
    length: 2,
  });
  assert.deepEqual(reply, { id: 3, ok: false, error: "manifold-rust 0.19.1 returned a solid that is not manifold" });
});

test("a WebAssembly trap becomes a fatal error reply; other errors are not fatal", () => {
  const trap = new WebAssembly.RuntimeError("unreachable");
  assert.ok(isTrap(trap));
  assert.equal(isTrap(new Error("unreachable")), false);
  assert.equal(isTrap("unreachable"), false);
  const { reply, transfer } = dispatch(fakeCore({ throws: trap }), { id: 4, type: "brick", width: 2, length: 4 });
  assert.equal(reply.id, 4);
  assert.equal(reply.ok, false);
  assert.equal(reply.fatal, true);
  assert.match(reply.error, /WebAssembly trap \(unreachable\)/);
  assert.deepEqual(transfer, []);
  assert.equal("fatal" in dispatch(fakeCore({ throws: "not manifold" }), { id: 5, type: "brick", width: 2, length: 4 }).reply, false);
});

/** A worker handler over `core`, recording what it posts. */
function handlerFor(core, { loadFails = null } = {}) {
  const posted = [];
  const loads = [];
  const handle = createHandler(
    async (module) => {
      loads.push(module);
      if (loadFails) throw new Error(loadFails);
      return core;
    },
    (reply, transfer) => posted.push({ reply, transfer }),
  );
  return { handle, posted, loads, replies: () => posted.map((p) => p.reply) };
}

test("the worker handler loads the core once on init, then serves requests", async () => {
  const core = fakeCore();
  const h = handlerFor(core);
  await h.handle({ id: 1, type: "brick", width: 2, length: 4 });
  assert.deepEqual(h.replies()[0], { id: 1, ok: false, error: "the core is not loaded yet (send init first)" });
  await h.handle({ id: 2, type: "init", module: "compiled" });
  await h.handle({ id: 3, type: "init", module: "compiled" });
  assert.deepEqual(h.loads, ["compiled"], "instantiated once");
  assert.deepEqual(h.replies()[1], { id: 2, ok: true, result: info(core) });
  await h.handle({ id: 4, type: "brick", width: 2, length: 4 });
  const last = h.posted.at(-1);
  assert.equal(last.reply.ok, true);
  assert.equal(last.transfer.length, 2, "mesh arrays are transferred");
});

test("the worker handler reports a failed load and allows another init", async () => {
  const h = handlerFor(fakeCore(), { loadFails: "bad magic" });
  await h.handle({ id: 1, type: "init" });
  assert.deepEqual(h.replies()[0], { id: 1, ok: false, error: "could not load the core: bad magic" });
  await h.handle({ id: 2, type: "init" });
  assert.equal(h.loads.length, 2);
});

test("after a trap the worker handler never calls the core again", async () => {
  const core = fakeCore({ throws: new WebAssembly.RuntimeError("unreachable") });
  const h = handlerFor(core);
  await h.handle({ id: 1, type: "init" });
  // Both pass the entry check and wait for the load before either runs; the second
  // must not reach the core after the first traps.
  const waiting = [h.handle({ id: 2, type: "brick", width: 2, length: 4 }), h.handle({ id: 3, type: "brick", width: 1, length: 1 })];
  await Promise.all(waiting);
  await h.handle({ id: 4, type: "checkPart", text: "{}" });
  await h.handle({ id: 5, type: "init" });
  assert.deepEqual(core.calls, [[2, 4]], "only the request that trapped reached the core");
  const [, trapped, ...refused] = h.replies();
  assert.equal(trapped.fatal, true);
  assert.match(trapped.error, /trap \(unreachable\)/);
  assert.deepEqual(refused.map((r) => [r.id, r.ok, r.fatal]), [[3, false, true], [4, false, true], [5, false, true]]);
  for (const r of refused) assert.match(r.error, /unusable after an earlier trap/);
});

test("checkPart returns the structural errors as an array", () => {
  const core = fakeCore();
  assert.deepEqual(dispatch(core, { id: 1, type: "checkPart", text: "{}" }).reply, { id: 1, ok: true, result: [] });
  assert.deepEqual(dispatch(core, { id: 2, type: "checkPart", text: "broken" }).reply.result, ["missing field `kind`"]);
  assert.match(dispatch(core, { id: 3, type: "checkPart" }).reply.error, /needs the document as text/);
});

test("unknown and malformed requests get an error reply", () => {
  const core = fakeCore();
  assert.deepEqual(dispatch(core, { id: 9, type: "explode" }).reply, { id: 9, ok: false, error: 'unknown request type "explode"' });
  assert.equal(dispatch(core, null).reply.ok, false);
  assert.equal(dispatch(core, { id: 1, type: "info" }).reply.result.kernel, "fake-kernel 1.0");
});

test("only mesh arrays and file bytes are transferables, each buffer once", () => {
  assert.deepEqual(transferables(null), []);
  assert.deepEqual(transferables([]), []);
  assert.deepEqual(transferables({ positions: [1, 2, 3], indices: [0] }), []);
  const r = brick(fakeCore(), 1, 1);
  assert.equal(transferables(r).length, 2);
  const bytes = new Uint8Array(4);
  assert.deepEqual(transferables({ bytes }), [bytes.buffer]);
  const m = { positions: new Float32Array(3), indices: new Uint32Array(3) };
  assert.equal(transferables({ meshes: [m, m], overlay: m }).length, 2, "a buffer listed twice is transferred once");
});

// ------------------------------------------------------------ part designer

/** A FEATURE.MIN finding on the removed area, as schemas/report.schema.json has it. */
const REMOVED_AREA = Object.freeze({
  rule: "FEATURE.MIN",
  severity: "warning",
  message: "the morphological opening removed 1.03 % of the artwork area",
  value: 1.03,
  threshold: 1,
  fix: "make those lines wider",
  feature: null,
});
const PART = { doc: '{"id":"ndn.logo-tile"}', params: { size: [2, 4], relief: "emboss" }, process: "fdm-0.4", fit: "" };
const SVG = new TextEncoder().encode("<svg/>");

test("evaluatePart sends JSON text and the assets to the core and returns plain, transferable data", () => {
  const core = fakeCore();
  let clock = 0;
  const request = { id: 5, type: "evaluatePart", ...PART, fit: '{"kind":"fit"}', assets: [{ name: "assets/mark.svg", bytes: SVG }] };
  const { reply, transfer } = dispatch(core, request, () => (clock += 10));
  assert.equal(reply.ok, true, reply.error);
  const r = reply.result;
  assert.deepEqual(core.calls[0], [
    "evaluate_part",
    PART.doc,
    JSON.stringify(PART.params),
    "fdm-0.4",
    '{"kind":"fit"}',
    [["assets/mark.svg", SVG.length]],
  ]);
  assert.ok(core.assetSets[0].freed, "the AssetSet is freed");
  assert.deepEqual(r.meshes.map((m) => [m.role, m.color, m.triangles]), [
    ["body", "#2f6db5", 1],
    ["accent", "#f4f1e8", 1],
  ]);
  assert.equal(r.overlay.role, "removed");
  assert.deepEqual(r.report, { findings: [{ ...REMOVED_AREA, unit: "%" }], unchecked: [] }, "each finding carries the core's unit");
  assert.equal(r.exportAllowed, true);
  assert.deepEqual(r.measurements, { process: "fdm-0.4" });
  assert.deepEqual(r.warnings, ["a note"]);
  assert.equal(r.contentHash, "c0ffee");
  assert.deepEqual(r.artwork, { mode: "emboss", removedPolygons: 1 });
  assert.deepEqual(r.params.values.size, [2, 4]);
  assert.deepEqual(r.footprint, [2, 4]);
  assert.deepEqual(r.bounds, [0, 0, 0, 12.5, 25.5, 2.5]);
  assert.deepEqual([r.volumeMm3, r.massGPla, r.triangles, r.faceOnBed], [900, 1.116, 2, "bottom"]);
  assert.equal(r.buildMs, 10, "only the core call is timed");
  // Every MeshOut and the PartOut are freed; six arrays move to the page.
  assert.equal(core.freed.length, 4);
  assert.equal(transfer.length, 6);
  const received = structuredClone(reply, { transfer });
  assert.equal(r.meshes[0].positions.byteLength, 0, "moved, not copied");
  assert.equal(received.result.overlay.indices.length, 3);
});

test("units come from the core, one per finding, in report order", () => {
  const report = JSON.stringify({ findings: [REMOVED_AREA, { ...REMOVED_AREA, rule: "WALL.MIN" }] });
  assert.deepEqual(
    reportWithUnits(report, '["%","mm"]').findings.map((f) => [f.rule, f.unit]),
    [
      ["FEATURE.MIN", "%"],
      ["WALL.MIN", "mm"],
    ],
  );
  assert.deepEqual(reportWithUnits('{"findings":[]}', "[]"), { findings: [], unchecked: [] });
  // The rules the core could not check travel as they are.
  const unchecked = [{ rule: "SIZE.BED", reason: "no printer or bed was given" }];
  assert.deepEqual(reportWithUnits(JSON.stringify({ findings: [], unchecked }), "[]").unchecked, unchecked);
  // A count that does not match is a core bug, not something to guess around.
  assert.throws(() => reportWithUnits(report, '["%"]'), /1 units for 2 findings/);
  assert.throws(() => reportWithUnits(report, "null"), /no units for 2 findings/);
});

test("part requests are checked before the core is called", () => {
  const core = fakeCore();
  const bad = [
    [{ ...PART, doc: { id: "x" } }, /needs the part document as JSON text/],
    [{ ...PART, params: [1] }, /params must be an object/],
    [{ ...PART, process: "" }, /process must be the id/],
    [{ ...PART, fit: null }, /fit must be the fit profile as JSON text/],
    [{ ...PART, printer: 3 }, /printer must be the id of a bundled printer/],
    [{ ...PART, assets: [{ name: "a.svg", bytes: [1, 2] }] }, /assets must be a list of \{ name, bytes: Uint8Array \}/],
  ];
  for (const [fields, message] of bad) {
    for (const type of ["evaluatePart", "exportPart", "resolveParams"]) {
      const { reply } = dispatch(core, { id: 1, type, format: "3mf", ...fields });
      assert.equal(reply.ok, false, type);
      assert.match(reply.error, message);
    }
  }
  assert.match(dispatch(core, { id: 1, type: "exportPart", format: "obj", ...PART }).reply.error, /format must be "3mf" or "stl"/);
  assert.match(dispatch(core, { id: 1, type: "exportPart", format: "3mf", ...PART, colors: "red" }).reply.error, /colors must be an object/);
  assert.deepEqual(core.calls, []);
});

test("exportPart returns the file bytes, transferred, and passes the colours", () => {
  const core = fakeCore();
  const request = { id: 2, type: "exportPart", format: "stl", ...PART, assets: [{ name: "a.svg", bytes: SVG }], colors: { accent: "#ff8000" } };
  const { reply, transfer } = dispatch(core, request);
  assert.equal(reply.ok, true, reply.error);
  assert.equal(reply.result.format, "stl");
  assert.deepEqual([...reply.result.bytes], [0x50, 0x4b, 3, 4]);
  assert.deepEqual(transfer, [reply.result.bytes.buffer]);
  assert.deepEqual(core.calls[0], ["export_part", "stl", JSON.stringify(PART.params), "fdm-0.4", "", [["a.svg", SVG.length]], '{"accent":"#ff8000"}']);
  dispatch(core, { ...request, colors: {} });
  assert.equal(core.calls[1][6], "", "no colours: the core's own");
  assert.ok(core.assetSets.every((s) => s.freed));
});

test("a printer goes to the core's printer calls; none keeps the plain ones", () => {
  const core = fakeCore();
  const request = { id: 7, type: "evaluatePart", ...PART, printer: "prusa-mini-plus", assets: [{ name: "a.svg", bytes: SVG }] };
  assert.equal(dispatch(core, request).reply.ok, true);
  assert.deepEqual(core.calls[0], ["evaluate_part_on_printer", "prusa-mini-plus"]);
  assert.equal(dispatch(core, { ...request, printer: "" }).reply.ok, true);
  assert.equal(core.calls[1][0], "evaluate_part");
  const exported = dispatch(core, { ...request, type: "exportPart", format: "3mf" });
  assert.equal(exported.reply.ok, true, exported.reply.error);
  assert.deepEqual(core.calls[2], ["export_part_on_printer", "3mf", "prusa-mini-plus"]);
  assert.deepEqual(bundled(core).printers.map((p) => p.id), ["prusa-mini-plus"]);
});

test("a refused export is an error reply with the core's message", () => {
  const core = fakeCore({ throws: "export is disabled while the validation report has 1 blocker: FIT.MISSING: ..." });
  const { reply } = dispatch(core, { id: 3, type: "exportPart", format: "3mf", ...PART });
  assert.deepEqual(reply, { id: 3, ok: false, error: "export is disabled while the validation report has 1 blocker: FIT.MISSING: ..." });
  assert.ok(core.assetSets[0].freed, "freed on failure too");
});

test("bundled and resolveParams read the core's data", () => {
  const core = fakeCore();
  const b = dispatch(core, { id: 1, type: "bundled" }).reply.result;
  assert.deepEqual(b.processProfiles, [{ id: "fdm-0.4" }]);
  assert.equal(b.exampleFitProfile, '{"kind":"fit","id":"example"}', "kept as text: it is sent back as the fit");
  assert.equal(b.logoTilePart, '{"id":"ndn.logo-tile"}');
  assert.deepEqual(b.samples.map((s) => [s.id, s.files[0].asset]), [["neuraldeep-wordmark", "assets/brand/neuraldeep-wordmark-2x6.svg"]], "parsed");
  assert.equal(b.disclaimer, "not affiliated");
  assert.deepEqual(b.roleColors, { body: "#2f6db5", accent: "#f4f1e8" });
  assert.equal(b.maxAssetBytes, 4194304);
  assert.equal(b.couponChoices.defaultPocketSet, "cross");
  assert.deepEqual(b.parts.map((p) => [p.id, p.file]), [["ndn.logo-tile", "ndn.logo-tile.obpart.json"]], "parsed");
  assert.equal(b.maxPartBytes, 4194304);
  const r = dispatch(core, { id: 2, type: "resolveParams", ...PART, params: { relief: "inlay" } }).reply.result;
  assert.deepEqual(r.visible, { reliefHeight: false });
  assert.deepEqual(core.calls[0], ["resolve_params_json", '{"relief":"inlay"}', "fdm-0.4"]);
});

test("openPart, savePart and addPreset pass the document through the core", () => {
  const core = fakeCore();
  const opened = dispatch(core, { id: 1, type: "openPart", text: '{"a":1}', process: "fdm-0.4" }).reply;
  assert.equal(opened.ok, true);
  assert.deepEqual(opened.result, { ok: true, id: "t", presets: [], unsupported: [] });
  // A file that is not a part is an answer, not a failed request.
  const refused = dispatch(core, { id: 2, type: "openPart", text: "nope", process: "fdm-0.4" }).reply;
  assert.deepEqual([refused.ok, refused.result.ok, refused.result.error], [true, false, "the file is not JSON"]);
  assert.match(dispatch(core, { id: 3, type: "openPart", process: "fdm-0.4" }).reply.error, /needs the document as text/);
  assert.match(dispatch(core, { id: 4, type: "openPart", text: "{}" }).reply.error, /process/);

  const saved = dispatch(core, { id: 5, type: "savePart", ...PART, params: { relief: "deboss" } }).reply;
  assert.deepEqual(saved.result, { text: `${PART.doc}\n` });
  assert.deepEqual(core.calls.at(-1), ["save_part_json", PART.doc, '{"relief":"deboss"}', "fdm-0.4"]);
  const preset = dispatch(core, { id: 6, type: "addPreset", ...PART, name: "Mine", params: {} }).reply;
  assert.deepEqual(preset.result, { text: `${PART.doc} Mine\n` });
  assert.deepEqual(core.calls.at(-1), ["add_preset_json", PART.doc, "Mine", "{}", "fdm-0.4"]);
  // The core's refusals come back as errors with its message.
  assert.match(dispatch(core, { id: 7, type: "addPreset", ...PART, name: " ", params: {} }).reply.error, /needs a name/);
  assert.match(dispatch(core, { id: 8, type: "addPreset", ...PART, params: {} }).reply.error, /preset's name/);
  const failing = fakeCore({ throws: "parameter `reliefHeight`: 9 is above the maximum 1.5" });
  assert.match(dispatch(failing, { id: 9, type: "savePart", ...PART, params: {} }).reply.error, /reliefHeight/);
});

// ------------------------------------------------------------ calibration coupon

test("buildCoupon returns the mesh, the cells and the file stem, and frees the core objects", () => {
  const core = fakeCore();
  const { reply, transfer } = dispatch(core, { id: 1, type: "buildCoupon", process: "fdm-0.4", layout: { pockets: "full", bedMm: [256, 256] } });
  assert.equal(reply.ok, true, reply.error);
  const r = reply.result;
  assert.deepEqual(core.calls[0], ["build_coupon", "fdm-0.4", '{"pockets":"full","bedMm":[256,256]}']);
  assert.equal(r.meshes.length, 1);
  assert.deepEqual(r.cells, [{ cellId: "stud", label: "4.60" }]);
  assert.deepEqual([r.summary.process, r.estimate.massGPla, r.contentHash, r.fileStem, r.triangles], ["fdm-0.4", 32.2, "beef", "coupon-fdm-0.4-cross", 1]);
  assert.equal(core.freed.length, 2);
  assert.equal(transfer.length, 2);
  dispatch(core, { id: 2, type: "buildCoupon", process: "fdm-0.4" });
  assert.equal(core.calls[1][2], "", "no layout: the defaults");
  assert.match(dispatch(core, { id: 3, type: "buildCoupon", layout: {} }).reply.error, /process must be the id/);
  assert.match(dispatch(core, { id: 4, type: "buildCoupon", process: "fdm-0.4", layout: "full" }).reply.error, /layout must be an object/);
});

test("couponLayout and exportCoupon pass the layout as JSON text", () => {
  const core = fakeCore();
  assert.deepEqual(dispatch(core, { id: 1, type: "couponLayout", process: "fdm-0.2", layout: {} }).reply.result, { cells: [], summary: {} });
  const { reply, transfer } = dispatch(core, { id: 2, type: "exportCoupon", format: "3mf", process: "fdm-0.4", layout: { copies: 1 } });
  assert.deepEqual([...reply.result.bytes], [1, 2, 3]);
  assert.deepEqual(transfer, [reply.result.bytes.buffer]);
  assert.deepEqual(core.calls, [
    ["coupon_layout_json", "fdm-0.2", ""],
    ["export_coupon", "3mf", "fdm-0.4", '{"copies":1}', ""],
  ]);
  assert.match(dispatch(core, { id: 3, type: "exportCoupon", format: "step", process: "fdm-0.4" }).reply.error, /format must be/);
});

test("recordFit sends the wizard's choices as JSON text and answers the core's record or refusal", () => {
  const core = fakeCore();
  const input = { process: "fdm-0.4", printer: "Prusa MINI+", material: "PLA", cells: { stud: 1.25 } }; // a fake core: any number of millimetres
  const ok = dispatch(core, { id: 1, type: "recordFit", input }).reply;
  assert.equal(ok.ok, true);
  assert.deepEqual(ok.result, { ok: true, fileName: "x.json", text: "{}\n", fitProfile: { id: "x" }, offsets: [] });
  assert.deepEqual(core.calls[0], ["record_fit", JSON.stringify(input)]);
  // A refusal is an answer, not a failed request: the page shows it by its field.
  const refused = dispatch(core, { id: 2, type: "recordFit", input: { ...input, material: "" } }).reply;
  assert.equal(refused.ok, true);
  assert.deepEqual(refused.result.error, { field: "material", message: "material is empty; it is required" });
  for (const bad of [undefined, "{}", [], null]) {
    assert.match(dispatch(core, { id: 3, type: "recordFit", input: bad }).reply.error, /recordFit needs the wizard's choices/);
  }
});

test("build and export request types are listed for the client", () => {
  assert.deepEqual([...BUILD_TYPES], ["brick", "evaluatePart", "buildCoupon"]);
  assert.deepEqual([...EXPORT_TYPES], ["exportPart", "exportCoupon"]);
});
