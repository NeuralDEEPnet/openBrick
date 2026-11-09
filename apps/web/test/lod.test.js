// Level of detail, chunking, adaptive resolution and the overlay text (src/lod.js), and the page's table
// of the model (src/model-table.js).

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CHUNK_STUDS,
  chunkLdu,
  GRID_ABOVE,
  SINGLE_BELOW,
  basePixelRatio,
  chunkCell,
  createAdaptiveResolution,
  hudLines,
  isPhone,
  nextBias,
  pickLevel,
  studPixels,
  thresholds,
  triangleBudget,
  wantGrid,
} from "../src/lod.js";
import { ModelTable, ROW } from "../src/model-table.js";

test("a model is chunked above 3000 parts and one chunk per part type again below 1500", () => {
  assert.equal(wantGrid(GRID_ABOVE, false), false);
  assert.equal(wantGrid(GRID_ABOVE + 1, false), true);
  assert.equal(wantGrid(SINGLE_BELOW, true), true);
  assert.equal(wantGrid(SINGLE_BELOW - 1, true), false);
  assert.equal(chunkLdu(20), 640, "32 studs of the standard's 20 LDU");
  assert.equal(chunkLdu(10), 320, "from the catalog's pitch, not a typed number");
  assert.equal(CHUNK_STUDS, 32);
  const edge = chunkLdu(20);
  assert.equal(chunkCell([5, -300, 10], false, edge), "all");
  assert.equal(chunkCell([5, -300, 10], true, edge), "0,-1,0");
  assert.equal(chunkCell([edge, 0, -1], true, edge), "1,0,-1");
  assert.equal(chunkCell([edge, 0, -1], true, 320), "2,0,-1");
});

test("a stud is fewer pixels the farther it is", () => {
  const near = studPixels(8, 200, 35, 800);
  const far = studPixels(8, 2000, 35, 800);
  assert.ok(near > far * 9.9 && near < far * 10.1);
  assert.ok(near > 40 && far < 6);
});

test("the level follows the stud size, with a margin so a chunk at a border does not flicker", () => {
  const big = thresholds({ small: false });
  assert.deepEqual([40, 15, 5, 1].map((px) => pickLevel(px, big)), [0, 1, 2, 3]);
  // Just under the border to level 1 (28 px): a chunk at level 0 stays there until a margin under it.
  assert.equal(pickLevel(27, big, 0), 0);
  assert.equal(pickLevel(22, big, 0), 1);
  // Just over the border: a chunk at level 1 stays until a margin over it.
  assert.equal(pickLevel(29, big, 1), 1);
  assert.equal(pickLevel(33, big, 1), 0);
  // No flicker: the answer for a sweep back and forth across a border changes at two places, not one per step.
  let level = -1;
  const changes = [];
  for (const px of [30, 29, 28, 27, 26, 28, 29, 30, 31, 33, 31, 29, 27, 24, 22]) {
    const next = pickLevel(px, big, level);
    if (next !== level) changes.push([px, next]);
    level = next;
  }
  assert.ok(changes.length <= 3, JSON.stringify(changes));
});

test("a small model keeps full detail until a stud is a speck", () => {
  const small = thresholds({ small: true });
  assert.equal(pickLevel(10, small), 0);
  assert.equal(pickLevel(2, small), 0);
  assert.equal(pickLevel(0.5, small), 3);
});

test("the level borders rise while the frame is over its triangle budget and fall back with a dead band", () => {
  const budget = 1_000_000;
  let bias = 1;
  bias = nextBias(bias, 2_000_000, budget);
  assert.ok(bias > 1);
  const raised = bias;
  assert.equal(nextBias(bias, 800_000, budget), raised, "between 60 percent and 100 percent of the budget: no change");
  assert.ok(nextBias(bias, 300_000, budget) < raised);
  assert.equal(nextBias(1, 100_000, budget), 1, "never below one");
  assert.equal(nextBias(16, 9_000_000, budget), 16, "and capped");
  assert.ok(triangleBudget({ phone: true }) < triangleBudget({ phone: false }));
});

test("the pixel ratio drops when frames are over budget, comes back when they are comfortably under, and rests in the dead band", () => {
  const a = createAdaptiveResolution({ max: 2, targetMs: 16.7, window: 4 });
  assert.equal(a.ratio, 2);
  for (let i = 0; i < 4; i++) a.frame(40);
  assert.ok(a.ratio < 2 && a.ratio >= 0.5);
  const low = a.ratio;
  // 16 ms is inside the dead band (0.7 to 1.15 of 16.7): no change.
  for (let i = 0; i < 8; i++) a.frame(16);
  assert.equal(a.ratio, low);
  for (let i = 0; i < 4; i++) a.frame(6);
  assert.ok(a.ratio > low);
  // Never below the floor or above the cap.
  for (let i = 0; i < 200; i++) a.frame(500);
  assert.equal(a.ratio, 0.5);
  for (let i = 0; i < 400; i++) a.frame(2);
  assert.equal(a.ratio, 2);
  a.frame(500);
  assert.equal(a.settle(), 2);
});

test("a phone starts at a lower pixel ratio and a lower frame budget", () => {
  assert.equal(basePixelRatio({ devicePixelRatio: 3, phone: true }), 1.5);
  assert.equal(basePixelRatio({ devicePixelRatio: 3, phone: false }), 2);
  assert.equal(basePixelRatio({ devicePixelRatio: 1, phone: false }), 1);
  assert.equal(isPhone({ coarse: true, width: 1000 }), true);
  assert.equal(isPhone({ coarse: false, width: 390 }), true);
  assert.equal(isPhone({ coarse: false, width: 1280 }), false);
});

test("the overlay says what the frame drew", () => {
  const lines = hudLines({ drawCalls: 120, triangles: 1234567, frameMs: 16.7, parts: 100000, chunks: 150, visibleChunks: 90, lod: [4, 30, 40, 16], pixelRatio: 1.25, bias: 1 });
  assert.match(lines[0], /120 draw calls, 1,234,567 triangles/);
  assert.match(lines[1], /16\.7 ms a frame \(60 fps/);
  assert.match(lines[2], /100,000 parts in 150 chunks \(90 in view\)/);
  assert.match(lines[3], /full 4, cheap 30, solid 40, hull 16/);
  assert.match(hudLines({ drawCalls: 0, triangles: 0, frameMs: 0, parts: 0, chunks: 0, visibleChunks: 0, lod: [0, 0, 0, 0], pixelRatio: 1, bias: 1 })[1], /- ms a frame/);
});

test("the table of the model is patched by deltas and equals the core's table after each", () => {
  const t = new ModelTable(2);
  const rows = Int32Array.from([1, 8, 0, -8, 0, 0, 2, 2, 5, 20, -32, 0, 0, 4, 3, 5, 60, -32, 0, 0, 4]);
  t.replaceAll(rows);
  assert.equal(t.size, 3);
  assert.deepEqual(t.get(2), { id: 2, part: 5, pos: [20, -32, 0], rot: 0, colour: 4 });
  assert.deepEqual([...t.toRows()], [...rows]);
  // A recolour and a removal.
  let r = t.applyDelta(Int32Array.from([0, 2, 5, 20, -32, 0, 0, 15, 3, -1, 0, 0, 0, 0, 0]));
  assert.deepEqual(r, { full: false, ids: [2, 3] });
  assert.equal(t.size, 2);
  assert.equal(t.get(2).colour, 15);
  assert.equal(t.has(3), false);
  // The freed slot is reused; ids may come back (undo).
  r = t.applyDelta(Int32Array.from([0, 3, 5, 60, -32, 0, 0, 4, 9, 6, 0, -8, 80, 0, 2]));
  assert.equal(t.size, 4);
  assert.deepEqual([...t.toRows()].filter((_, i) => i % ROW === 0), [1, 2, 3, 9]);
  // A whole table replaces everything and reports every id that was or is.
  r = t.applyDelta(Int32Array.from([1, 7, 1, 0, 0, 0, 0, 2]));
  assert.equal(r.full, true);
  assert.deepEqual(r.ids.sort((a, b) => a - b), [1, 2, 3, 7, 9]);
  assert.equal(t.size, 1);
  assert.throws(() => t.applyDelta(Int32Array.from([0, 1, 2])), /multiple of 7/);
  assert.throws(() => t.applyDelta(Int32Array.from([5])), /unknown delta mode/);
  const parts = Array.from({ length: 10 }, (_, i) => ({ id: `p${i}` }));
  assert.deepEqual(t.placed(parts), ["p1"]);
  assert.deepEqual(t.placed(parts, 0), [], "no list for a big model");
  assert.deepEqual([...t.countsByPart()], [[1, 1]]);
});

test("the table grows past its first capacity", () => {
  const t = new ModelTable(2);
  const rows = new Int32Array(1000 * ROW);
  for (let i = 0; i < 1000; i++) rows.set([i + 1, i % 14, i * 20, -8, 0, 0, 4], i * ROW);
  t.replaceAll(rows);
  assert.equal(t.size, 1000);
  assert.equal(t.get(1000).pos[0], 999 * 20);
  assert.deepEqual([...t.toRows()], [...rows]);
});

test("ids up to 4,294,967,295 are read back unsigned, in order, through every path", () => {
  const big = [3_000_000_000, 4_294_967_295, 2_147_483_648, 5];
  const rows = new Int32Array(big.length * ROW);
  big.forEach((id, i) => rows.set([id, 1, i * 20, -8, 0, 0, 4], i * ROW)); // the id is stored as its bits
  const t = new ModelTable(1);
  t.replaceAll(rows);
  for (const id of big) assert.equal(t.get(id)?.id, id, `get ${id}`);
  assert.deepEqual([...t.toRows()].filter((_, i) => i % ROW === 0).map((v) => v >>> 0), [5, 2_147_483_648, 3_000_000_000, 4_294_967_295], "ascending unsigned");
  // A delta: a recolour of the largest id, then its removal and its return.
  let r = t.applyDelta(Int32Array.from([0, 4_294_967_295, 1, 60, -8, 0, 0, 15]));
  assert.deepEqual(r.ids, [4_294_967_295]);
  assert.equal(t.get(4_294_967_295).colour, 15);
  r = t.applyDelta(Int32Array.from([0, 4_294_967_295, -1, 0, 0, 0, 0, 0]));
  assert.equal(t.has(4_294_967_295), false);
  t.applyDelta(Int32Array.from([0, 4_294_967_295, 1, 60, -8, 0, 0, 4]));
  assert.equal(t.get(4_294_967_295).pos[0], 60);
  assert.equal(t.size, 4);
});
