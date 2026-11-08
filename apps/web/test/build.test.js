// Build mode's pure logic (src/build.js): what the core sends, keys, texts, files
// and thumbnails. Nothing here computes where a part goes: that is the core's.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  aimRay,
  aimStep,
  bomRows,
  clearable,
  colourHex,
  colourName,
  decodeInstances,
  frameSummary,
  ghostState,
  groupByPart,
  INSTANCE_STRIDE,
  instanceMatrix,
  isClick,
  isModelFile,
  keyAction,
  ORBIT_STEP_DEG,
  replaceNeedsConfirm,
  usesPackParts,
  paletteName,
  MAX_PALETTE_NAME,
  stageKeyAction,
  ZOOM_STEP,
  DETAIL_BELOW,
  LOW_DETAIL_ABOVE,
  modelFileName,
  modelNameFromFile,
  shadeHex,
  snapText,
  stateSentence,
  statsRows,
  thumbnailFit,
  thumbnailTriangles,
  typingTarget,
  wantDetail,
} from "../src/build.js";

const palette = { colours: [{ code: 4, name: "Red", rgb: "#c4281f" }, { code: 15, name: "White", rgb: "#f2f1ec" }] };
const IDENTITY = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
const YAW = [[0, 0, 1], [0, 1, 0], [-1, 0, 0]];

test("instances decode from the core's flat array and group by part", () => {
  const flat = Int32Array.from([1, 13, 0, -8, 0, 0, 2, 2, 4, 10, -32, 10, 0, 4, 3, 4, 30, -32, 10, 5, 15]);
  assert.equal(flat.length, 3 * INSTANCE_STRIDE);
  const list = decodeInstances(flat);
  assert.deepEqual(list[1], { id: 2, part: 4, pos: [10, -32, 10], rot: 0, colour: 4 });
  const groups = groupByPart(list);
  assert.deepEqual([...groups.keys()], [13, 4]);
  assert.deepEqual(groups.get(4).map((i) => i.id), [2, 3], "placement order is kept");
  assert.throws(() => decodeInstances(Int32Array.from([1, 2, 3])), /not a multiple of 7/);
  assert.deepEqual(decodeInstances(new Int32Array(0)), []);
});

test("an instance matrix is the core's rotation and the position, column-major", () => {
  assert.deepEqual(instanceMatrix([IDENTITY], 0, [5, -8, 7]), [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, -8, 7, 1]);
  // Row-major [[0,0,1],[0,1,0],[-1,0,0]] has the columns (0,0,-1), (0,1,0), (1,0,0).
  assert.deepEqual(instanceMatrix([IDENTITY, YAW], 1, [0, 0, 0]).slice(0, 12), [0, 0, -1, 0, 0, 1, 0, 0, 1, 0, 0, 0]);
  assert.throws(() => instanceMatrix([IDENTITY], 5, [0, 0, 0]), /rotation 5/);
});

test("colours come from the palette the core sent", () => {
  assert.equal(colourHex(palette, 4), "#c4281f");
  assert.equal(colourHex(palette, 99), "#808080");
  assert.equal(colourName(palette, 15), "White");
  assert.equal(colourName(palette, 99), "colour 99");
});

test("keys: undo and redo with Ctrl or Cmd, R rotates, Delete removes", () => {
  assert.equal(keyAction({ key: "z", ctrlKey: true }), "undo");
  assert.equal(keyAction({ key: "z", metaKey: true }), "undo");
  assert.equal(keyAction({ key: "Z", ctrlKey: true, shiftKey: true }), "redo");
  assert.equal(keyAction({ key: "y", ctrlKey: true }), "redo");
  assert.equal(keyAction({ key: "r" }), "rotate");
  assert.equal(keyAction({ key: "R", shiftKey: true }), "rotateBack");
  assert.equal(keyAction({ key: "Delete" }), "delete");
  assert.equal(keyAction({ key: "Backspace" }), "delete");
  assert.equal(keyAction({ key: "Escape" }), "deselect");
  assert.equal(keyAction({ key: "r", ctrlKey: true }), null, "Ctrl+R reloads the page");
  assert.equal(keyAction({ key: "a" }), null);
  assert.equal(typingTarget({ tagName: "INPUT" }), true);
  assert.equal(typingTarget({ tagName: "SELECT" }), true);
  assert.equal(typingTarget({ tagName: "BUTTON" }), false);
  assert.equal(typingTarget(null), false);
});

test("a click is a short gesture that hardly moved; the rest is orbiting", () => {
  assert.equal(isClick({ x: 10, y: 10, t: 0 }, { x: 12, y: 11, t: 120 }), true);
  assert.equal(isClick({ x: 10, y: 10, t: 0 }, { x: 40, y: 10, t: 120 }), false, "dragged");
  assert.equal(isClick({ x: 10, y: 10, t: 0 }, { x: 10, y: 10, t: 900 }), false, "held");
});

test("the ghost's state and the status line follow the engine's snap", () => {
  const ok = { pos: [0, -32, 0], rot: 0, valid: true, mates: 8, grounded: false, collides: null, target: 1 };
  assert.equal(ghostState(null), "none");
  assert.equal(ghostState(ok), "valid");
  assert.equal(ghostState({ ...ok, valid: false }), "invalid");
  assert.match(snapText(ok, "Brick 2 x 4")[0], /click or tap to place \(8 studs connected\)/);
  assert.match(snapText({ ...ok, mates: 1 }, "Plate")[0], /1 stud connected/);
  assert.match(snapText({ ...ok, grounded: true, mates: 0 }, "Plate")[0], /on the ground/);
  const hit = snapText({ ...ok, valid: false, collides: 7 }, "Brick 2 x 4", { 7: "Plate 1 x 2" });
  assert.deepEqual(hit, ["Brick 2 x 4 would overlap the Plate 1 x 2", "Move so the two only touch"]);
  assert.match(snapText({ ...ok, valid: false, mates: 0 }, "Tile")[1], /Press it onto studs/);
  assert.match(snapText(null, "Tile")[0], /Point at the baseplate/);
});

test("stats, the live sentence and the bill of materials", () => {
  const stats = { parts: 3, connections: 8, components: 2, grounded: 1, floating: [3] };
  assert.deepEqual(statsRows(stats).map((r) => r[0]), ["Parts", "Connections", "Connected groups", "On the ground", "Floating"]);
  assert.match(statsRows(stats)[4][1], /^1 /);
  assert.equal(statsRows({ ...stats, floating: [] })[4][1], "none");
  assert.equal(stateSentence({ parts: 1, floating: [] }), "1 part, all connected to the ground");
  assert.equal(stateSentence(stats), "3 parts, 1 floating");
  const { rows, total } = bomRows([
    { part: "ob.brick-2x4", name: "Brick 2 x 4", colour: 4, colourName: "Red", count: 5 },
    { part: "ob.plate-1x2", name: "Plate 1 x 2", colour: 15, colourName: "", count: 2 },
  ]);
  assert.equal(total, 7);
  assert.deepEqual(rows[1], { count: 2, part: "Plate 1 x 2", colour: "colour 15", code: 15 });
});

test("model files: names, names from files, what counts as a model file", () => {
  assert.equal(modelFileName("My house"), "My-house.obrick.json");
  assert.equal(modelFileName("a/b:c*"), "abc.obrick.json");
  assert.equal(modelFileName("house.obrick.json"), "house.obrick.json");
  assert.equal(modelFileName("   "), "model.obrick.json");
  assert.equal(modelFileName("x".repeat(100)).length, 60 + ".obrick.json".length);
  assert.equal(modelNameFromFile("my-house.obrick.json"), "my house");
  assert.equal(modelNameFromFile("tower.obrick"), "tower");
  assert.equal(isModelFile("a.obrick"), true);
  assert.equal(isModelFile("a.OBRICK.JSON"), true);
  assert.equal(isModelFile("a.svg"), false);
});

test("big models use the cheap mesh level, with a gap so it does not flicker", () => {
  assert.equal(wantDetail(0), true);
  assert.equal(wantDetail(LOW_DETAIL_ABOVE, true), true);
  assert.equal(wantDetail(LOW_DETAIL_ABOVE + 1, true), false);
  assert.equal(wantDetail(LOW_DETAIL_ABOVE + 1, false), false);
  assert.equal(wantDetail(DETAIL_BELOW, false), false, "still cheap inside the gap");
  assert.equal(wantDetail(DETAIL_BELOW - 1, false), true);
  assert.equal(wantDetail(DETAIL_BELOW, true), true, "still detailed inside the gap");
});

test("frame times summarise as fps and the 95th percentile", () => {
  assert.deepEqual(frameSummary([]), { fps: 0, p95Ms: 0, frames: 0 });
  const s = frameSummary(Array.from({ length: 20 }, (_, i) => (i === 19 ? 50 : 10)));
  assert.equal(s.frames, 20);
  assert.ok(Math.abs(s.fps - 1000 / 12) < 1e-9);
  assert.equal(s.p95Ms, 50);
});

test("thumbnails keep the visible faces, far to near, and fit the square", () => {
  // A cube from -10 to 10 in the part frame (Y down), 12 triangles counter-clockwise from outside.
  const p = [];
  for (const x of [-10, 10]) for (const y of [-10, 10]) for (const z of [-10, 10]) p.push(x, y, z);
  const v = (x, y, z) => ((x > 0) << 2) | ((y > 0) << 1) | (z > 0);
  const quad = (a, b, c, d) => [a, b, c, a, c, d];
  const idx = [
    ...quad(v(-1, -1, -1), v(-1, 1, -1), v(1, 1, -1), v(1, -1, -1)),
    ...quad(v(-1, -1, 1), v(1, -1, 1), v(1, 1, 1), v(-1, 1, 1)),
    ...quad(v(-1, -1, -1), v(-1, -1, 1), v(-1, 1, 1), v(-1, 1, -1)),
    ...quad(v(1, -1, -1), v(1, 1, -1), v(1, 1, 1), v(1, -1, 1)),
    ...quad(v(-1, -1, -1), v(1, -1, -1), v(1, -1, 1), v(-1, -1, 1)),
    ...quad(v(-1, 1, -1), v(-1, 1, 1), v(1, 1, 1), v(1, 1, -1)),
  ];
  // The test cube really is counter-clockwise from outside: every geometric normal points away from the centre.
  for (let t = 0; t < idx.length; t += 3) {
    const at = (k) => [p[idx[t + k] * 3], p[idx[t + k] * 3 + 1], p[idx[t + k] * 3 + 2]];
    const [a, b, c] = [at(0), at(1), at(2)];
    const u = b.map((x, i) => x - a[i]);
    const w = c.map((x, i) => x - a[i]);
    const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
    const centroid = a.map((x, i) => (x + b[i] + c[i]) / 3);
    assert.ok(n[0] * centroid[0] + n[1] * centroid[1] + n[2] * centroid[2] > 0, `triangle ${t / 3} faces outward`);
  }
  const { tris, box } = thumbnailTriangles(Float32Array.from(p), Uint32Array.from(idx));
  assert.ok(tris.length > 0 && tris.length < 12, `back faces are dropped (${tris.length})`);
  // Seen from above and the side: the top (Y down, so negative) is visible, the bottom never.
  assert.ok(tris.some((t) => t.ny < -0.9), "the top face is drawn");
  assert.ok(tris.every((t) => t.ny < 0.5), "the bottom face is not drawn");
  for (let i = 1; i < tris.length; i++) assert.ok(tris[i - 1].depth <= tris[i].depth, "painter's order: far first, nearest last");
  assert.ok(tris.every((t) => t.shade > 0 && t.shade <= 1));
  const fit = thumbnailFit(box, 48, 3);
  for (const t of tris) {
    for (let k = 0; k < 6; k += 2) {
      const x = t.points[k] * fit.scale + fit.offsetX;
      const y = t.points[k + 1] * fit.scale + fit.offsetY;
      assert.ok(x >= 2.9 && x <= 45.1 && y >= 2.9 && y <= 45.1, `${x},${y}`);
    }
  }
  assert.equal(shadeHex("#808080", 0.5), "#404040");
  assert.equal(shadeHex("#ffffff", 2), "#ffffff");
});

test("the 3D view's own keys: aim, place, orbit, zoom, frame; never Ctrl or Cmd combinations", () => {
  assert.deepEqual(stageKeyAction({ key: "ArrowUp" }), { type: "aim", key: "ArrowUp" });
  assert.deepEqual(stageKeyAction({ key: "Enter" }), { type: "place" });
  assert.deepEqual(stageKeyAction({ key: " " }), { type: "place" });
  assert.deepEqual(stageKeyAction({ key: "ArrowLeft", shiftKey: true }), { type: "orbit", azimuth: ORBIT_STEP_DEG, polar: -0 });
  assert.deepEqual(stageKeyAction({ key: "ArrowDown", shiftKey: true }), { type: "orbit", azimuth: -0, polar: ORBIT_STEP_DEG });
  assert.equal(stageKeyAction({ key: "+" }).factor < 1, true, "plus brings the camera closer");
  assert.equal(stageKeyAction({ key: "-" }).factor, ZOOM_STEP);
  assert.deepEqual(stageKeyAction({ key: "Home" }), { type: "reframe" });
  assert.equal(stageKeyAction({ key: "z", ctrlKey: true }), null);
  assert.equal(stageKeyAction({ key: "ArrowUp", ctrlKey: true }), null);
  assert.equal(stageKeyAction({ key: "Enter", metaKey: true }), null);
  assert.equal(stageKeyAction({ key: "q" }), null);
});

test("the aim moves one stud along the nearer ground axis, as the camera looks", () => {
  // The first view looks from the front right: towards minus X and minus Z of the scene (Y up).
  const first = [-1, -1.5];
  assert.deepEqual(aimStep("ArrowUp", first), [0, 1], "away from you is plus Z of the model frame (the frame flips Z)");
  assert.deepEqual(aimStep("ArrowDown", first), [0, -1]);
  assert.deepEqual(aimStep("ArrowRight", first), [1, 0]);
  assert.deepEqual(aimStep("ArrowLeft", first), [-1, 0]);
  // Turned a quarter: looking along minus X, Up goes to minus X and Right to the model's minus Z... check each quadrant is a rotation.
  const looks = [[0, -1], [-1, 0], [0, 1], [1, 0]];
  const seen = new Set();
  for (const f of looks) {
    const up = aimStep("ArrowUp", f);
    const right = aimStep("ArrowRight", f);
    assert.equal(Math.abs(up[0]) + Math.abs(up[1]), 1);
    assert.ok(up[0] * right[0] + up[1] * right[1] === 0, "right is across up");
    assert.deepEqual(aimStep("ArrowDown", f), [-up[0] || 0, -up[1] || 0]);
    seen.add(up.join());
  }
  assert.equal(seen.size, 4, "the four views give four different Up directions");
  // Looking straight down gives a direction too (no NaN).
  assert.equal(aimStep("ArrowUp", [0, 0]).every(Number.isFinite), true);
  assert.equal(aimStep("PageUp", first), null);
  assert.deepEqual(aimRay(40, -20), { origin: [40, -1_000_000, -20], dir: [0, 1, 0] });
});

test("replacing the model asks only when there are unsaved edits", () => {
  assert.equal(replaceNeedsConfirm({ dirty: true }), true);
  assert.equal(replaceNeedsConfirm({ dirty: false }), false);
  assert.equal(replaceNeedsConfirm({}), false);
});

test("only a model that uses parts of the loaded pack can lose them when the pack changes", () => {
  const parts = [{ origin: "builtin" }, { origin: "ldraw" }, undefined];
  assert.equal(usesPackParts([], parts), false);
  assert.equal(usesPackParts([{ part: 0 }], parts), false);
  assert.equal(usesPackParts([{ part: 0 }, { part: 1 }], parts), true);
  assert.equal(usesPackParts([{ part: 2 }, { part: 99 }], parts), false, "a part number past the catalog is not a pack part");
});

test("a part name in the palette is printable and short, whatever the pack says", () => {
  assert.equal(paletteName("Brick 2 x 4"), "Brick 2 x 4");
  assert.equal(paletteName("a\u202Eb\u200Bc\u001bd\ne"), "a\uFFFDb\uFFFDc\uFFFDd\uFFFDe");
  assert.equal(paletteName("é日本語"), "é日本語");
  const long = paletteName("x".repeat(5000));
  assert.equal([...long].length, MAX_PALETTE_NAME);
  assert.ok(long.endsWith("…"));
  assert.equal(paletteName(undefined), "");
});

test("Clear has something to do unless only ground baseplates are left", () => {
  const parts = [{ base: true, heightLdu: 8 }, { base: false, heightLdu: 24 }];
  const ground = { part: 0, pos: [0, -8, 0] };
  assert.equal(clearable([], parts), false);
  assert.equal(clearable([ground], parts), false);
  assert.equal(clearable([ground, { part: 1, pos: [10, -32, 10] }], parts), true);
  assert.equal(clearable([{ part: 0, pos: [0, -32, 0] }], parts), true, "a baseplate on bricks goes too");
});
