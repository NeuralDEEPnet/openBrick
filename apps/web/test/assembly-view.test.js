// The chunked scene (src/assembly-view.js) with a fake viewer: what is drawn must equal the model after every
// edit, whatever happens to a chunk's meshes (growth past its capacity, a replaced mesh, a removal, a level change).

import assert from "node:assert/strict";
import { test } from "node:test";
import * as THREE from "three/webgpu";
import { createAssemblyView } from "../src/assembly-view.js";
import { ModelTable } from "../src/model-table.js";

globalThis.document ??= { documentElement: {} };
globalThis.getComputedStyle ??= () => ({ getPropertyValue: () => "" });

const rotations = Array.from({ length: 24 }, () => [[1, 0, 0], [0, 1, 0], [0, 0, 1]]);
const box = { min: [-20, 0, -20], max: [20, 24, 20] };
const part = (id) => ({ id, name: id, origin: "builtin", pickBox: box, bodyBox: box, base: false, heightLdu: 24 });
const catalog = { parts: [part("a"), part("b")], palette: { colours: [{ code: 4, rgb: "#ff0000" }] }, rotations, studPitchLdu: 20 };
const mesh = (index, lod, triangles) => ({ index, lod, positions: new Float32Array(9), indices: new Uint32Array([0, 1, 2]), triangles });

function setup() {
  const camera = new THREE.PerspectiveCamera(35, 1.6, 1, 100000);
  camera.position.set(0, 400, 800);
  camera.lookAt(0, 0, 0);
  const hooks = [];
  const viewer = {
    internals: {
      THREE,
      scene: new THREE.Scene(),
      camera,
      canvas: { getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 500 }) },
      markDirty() {},
      setBuildActive() {},
      frameSphere() {},
      onBeforeRender: (fn) => hooks.push(fn),
      stageSize: () => ({ widthPx: 800, heightPx: 500 }),
    },
  };
  const view = createAssemblyView(viewer, { catalog, lduMm: 0.4 });
  view.addMeshes([0, 1].flatMap((i) => [0, 1, 2, 3].map((l) => mesh(i, l, 12 * (4 - l)))));
  const table = new ModelTable();
  const frame = () => hooks.forEach((h) => h());
  const add = (id, p = 0, x = id * 4) => {
    const r = table.applyDelta(Int32Array.from([0, id, p, x, -24, 0, 0, 4]));
    view.update(r.ids);
  };
  const check = (what) => {
    frame();
    assert.equal(view.drawnInstances(), table.size, `${what}: drawn instances`);
    assert.equal(view.counts().parts, table.size, `${what}: the counts say what is drawn`);
  };
  view.setModel(table);
  return { view, table, add, check, frame, camera };
}

for (const n of [17, 40, 200]) {
  test(`${n} parts placed one by one are all drawn after every placement (a chunk grows past its capacity of 16)`, () => {
    const { add, check } = setup();
    for (let id = 1; id <= n; id++) {
      add(id);
      check(`after part ${id}`);
    }
  });
}

test("a removal back below the capacity, undo and redo keep every part drawn", () => {
  const { table, view, add, check } = setup();
  for (let id = 1; id <= 20; id++) add(id);
  check("20 placed");
  const remove = (id) => view.update(table.applyDelta(Int32Array.from([0, id, -1, 0, 0, 0, 0, 0])).ids);
  for (let id = 20; id >= 10; id--) {
    remove(id);
    check(`removed ${id}`);
  }
  for (let id = 10; id <= 25; id++) {
    add(id); // undo of a removal, then more parts: grows past the capacity again
    check(`back ${id}`);
  }
});

test("a replaced mesh (a pack part arriving, a level change) leaves every part drawn", () => {
  const { view, add, check, camera, frame } = setup();
  for (let id = 1; id <= 30; id++) add(id);
  check("30 placed");
  view.addMeshes([mesh(0, 0, 100), mesh(0, 3, 12)]);
  check("meshes replaced");
  for (let id = 31; id <= 40; id++) add(id, 1);
  check("a second part type");
  // Levels change as the camera moves away and back, after growth.
  for (const z of [3000, 60000, 800, 20000, 500]) {
    camera.position.set(0, z / 2, z);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld(true);
    frame();
    check(`camera at ${z}`);
  }
  view.release([1]);
});
