// The worker's message protocol (src/protocol.js) against a fake core with the
// same exports as the wasm-bindgen build of crates/openbrick-wasm.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { brick, dispatch, info, parseStuds, PROTOCOL_VERSION, standardSummary, transferables } from "../src/protocol.js";

const STANDARD_TEXT = readFileSync(new URL("../../../standards/dimensions-v1.json", import.meta.url), "utf8");
const STANDARD = JSON.parse(STANDARD_TEXT);
const nominal = (id) => STANDARD.features.find((f) => f.id === id).nominal_mm;
const MAX = 16;

/** A stand-in for the wasm module: records calls and frees, makes a one-triangle mesh. */
function fakeCore({ throws = null } = {}) {
  const calls = [];
  const freed = [];
  return {
    calls,
    freed,
    kernel_id: () => "fake-kernel 1.0",
    max_studs: () => MAX,
    standard_json: () => STANDARD_TEXT,
    check_part_json: (text) => (text.includes("broken") ? JSON.stringify(["missing field `kind`"]) : "[]"),
    build_nominal_brick(width, length) {
      calls.push([width, length]);
      if (throws) throw new Error(throws);
      const mesh = {
        positions: () => new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        indices: () => new Uint32Array([0, 1, 2]),
        vertex_count: () => 3,
        triangle_count: () => 1,
        volume_mm3: () => width * length,
        fingerprint: () => "0x0000000000000001",
        canonical_fingerprint: () => "0x0000000000000002",
        bounds: () => new Float32Array([0, 0, 0, 1, 1, 0]),
        kernel: () => "fake-kernel 1.0",
        free: () => freed.push(mesh),
      };
      return mesh;
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
  assert.equal(core.freed.length, 1, "MeshOut.free() was called");
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

test("only mesh results have transferables", () => {
  assert.deepEqual(transferables(null), []);
  assert.deepEqual(transferables([]), []);
  assert.deepEqual(transferables({ positions: [1, 2, 3], indices: [0] }), []);
  const r = brick(fakeCore(), 1, 1);
  assert.equal(transferables(r).length, 2);
});
