// The worker's message protocol (src/protocol.js) against a fake core with the
// same exports as the wasm-bindgen build of crates/openbrick-wasm.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  brick,
  createHandler,
  dispatch,
  info,
  isTrap,
  parseStuds,
  PROTOCOL_VERSION,
  standardSummary,
  transferables,
} from "../src/protocol.js";

const STANDARD_TEXT = readFileSync(new URL("../../../standards/dimensions-v1.json", import.meta.url), "utf8");
const STANDARD = JSON.parse(STANDARD_TEXT);
const nominal = (id) => STANDARD.features.find((f) => f.id === id).nominal_mm;
const MAX = 16;

/**
 * A stand-in for the wasm module: records calls, takes and frees, makes a
 * one-triangle mesh. `throws` is an error (or a message) build_nominal_brick throws.
 */
function fakeCore({ throws = null } = {}) {
  const calls = [];
  const freed = [];
  const taken = [];
  return {
    calls,
    freed,
    taken,
    kernel_id: () => "fake-kernel 1.0",
    max_studs: () => MAX,
    standard_json: () => STANDARD_TEXT,
    check_part_json: (text) => (text.includes("broken") ? JSON.stringify(["missing field `kind`"]) : "[]"),
    build_nominal_brick(width, length) {
      calls.push([width, length]);
      if (throws) throw typeof throws === "string" ? new Error(throws) : throws;
      const mesh = {
        take_positions: () => (taken.push("positions"), new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0])),
        take_indices: () => (taken.push("indices"), new Uint32Array([0, 1, 2])),
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
  assert.deepEqual(core.taken, ["positions", "indices"], "each array is moved out once, never cloned");
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

test("only mesh results have transferables", () => {
  assert.deepEqual(transferables(null), []);
  assert.deepEqual(transferables([]), []);
  assert.deepEqual(transferables({ positions: [1, 2, 3], indices: [0] }), []);
  const r = brick(fakeCore(), 1, 1);
  assert.equal(transferables(r).length, 2);
});
