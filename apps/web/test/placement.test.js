// Display transforms (src/placement.js) and page helpers (src/ui.js): the brick on
// the lattice in the three.js frame, camera framing, the grid, the backend URL
// parameter, the size steppers and the slow-build seconds.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Object3D, Vector3 } from "three";
import { KERNEL_TO_THREE_EULER_X } from "@openbrick3d/viewport/frame.js";
import { brickOffset, framingDistance, gridCells, placedBounds } from "../src/placement.js";
import { elapsedText, readBackend, stepStuds } from "../src/ui.js";
import { standardSummary } from "../src/protocol.js";

const standard = standardSummary(readFileSync(new URL("../../../standards/dimensions-v1.json", import.meta.url), "utf8"));
const { pitchMm: pitch, undersizeMm: undersize, brickHeightMm: height, studHeightMm: studHeight } = standard;
const close = (a, b, what) => assert.ok(Math.abs(a - b) < 1e-9, `${what}: ${a} vs ${b}`);

/** Kernel-frame bounds of a nominal brick, as the core reports them. */
function kernelBounds(width, length) {
  return [0, 0, 0, width * pitch - 2 * undersize, length * pitch - 2 * undersize, height + studHeight];
}

test("the brick's lattice cell corner lands on a grid line and its body one undersize inside", () => {
  for (const [width, length] of [[1, 1], [2, 4], [3, 5], [16, 16]]) {
    const [x, y, z] = brickOffset({ width, length }, standard);
    const cellX = x - undersize;
    const cellY = y - undersize;
    close(cellX / pitch, Math.round(cellX / pitch), `${width}x${length} cell x on a grid line`);
    close(cellY / pitch, Math.round(cellY / pitch), `${width}x${length} cell y on a grid line`);
    assert.equal(z, 0, "on the ground");
    // Centred: the footprint's middle is within half a stud of the origin.
    assert.ok(Math.abs(cellX + (width * pitch) / 2) <= pitch / 2 + 1e-9);
    assert.ok(Math.abs(cellY + (length * pitch) / 2) <= pitch / 2 + 1e-9);
  }
});

test("placed bounds match three.js's own transform of the scene graph the viewer builds", () => {
  const size = { width: 3, length: 5 };
  const bounds = kernelBounds(size.width, size.length);
  const offset = brickOffset(size, standard);
  // As viewer.js: a group rotated into three.js holding the mesh at the offset.
  const world = new Object3D();
  world.rotation.x = KERNEL_TO_THREE_EULER_X;
  const mesh = new Object3D();
  mesh.position.set(...offset);
  world.add(mesh);
  world.updateMatrixWorld(true);
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  for (const x of [bounds[0], bounds[3]]) {
    for (const y of [bounds[1], bounds[4]]) {
      for (const z of [bounds[2], bounds[5]]) {
        const p = mesh.localToWorld(new Vector3(x, y, z)).toArray();
        p.forEach((v, k) => {
          lo[k] = Math.min(lo[k], v);
          hi[k] = Math.max(hi[k], v);
        });
      }
    }
  }
  const placed = placedBounds(bounds, offset);
  for (let k = 0; k < 3; k++) {
    close(placed.min[k], lo[k], `min[${k}]`);
    close(placed.max[k], hi[k], `max[${k}]`);
  }
});

test("after placement the brick stands on the ground with its studs up (+Y)", () => {
  const size = { width: 2, length: 4 };
  const placed = placedBounds(kernelBounds(2, 4), brickOffset(size, standard));
  close(placed.min[1], 0, "bottom at y = 0");
  close(placed.max[1], height + studHeight, "stud tops highest");
  // The kernel's +Y (length) runs along three.js -Z, so the long side is along Z.
  close(placed.max[2] - placed.min[2], 4 * pitch - 2 * undersize, "length along z");
  close(placed.max[0] - placed.min[0], 2 * pitch - 2 * undersize, "width along x");
  close(placed.radius, Math.hypot(...placed.max.map((v, k) => v - placed.min[k])) / 2, "radius");
});

test("framing puts the bounding sphere inside the narrower field of view", () => {
  const r = 10;
  const fov = 35;
  // Landscape: the vertical angle is the narrower one.
  const wide = framingDistance(r, fov, 16 / 9, 1);
  close(wide * Math.sin(((fov / 2) * Math.PI) / 180), r, "sphere touches the vertical edges");
  // Portrait (a phone): the horizontal angle is narrower, so the camera backs off.
  const tall = framingDistance(r, fov, 9 / 19.5, 1);
  const half = Math.atan(Math.tan(((fov / 2) * Math.PI) / 180) * (9 / 19.5));
  close(tall * Math.sin(half), r, "sphere touches the horizontal edges");
  assert.ok(tall > wide);
  close(framingDistance(r, fov, 1, 1.2), 1.2 * framingDistance(r, fov, 1, 1), "margin scales the distance");
});

test("the grid has an even cell count and room around the brick", () => {
  for (const [width, length] of [[1, 1], [2, 4], [5, 3], [16, 16]]) {
    const cells = gridCells({ width, length });
    assert.equal(cells % 2, 0, "even, so lines fall on whole pitches from the origin");
    // The brick reaches ceil(n / 2) studs from the origin; four empty studs beyond.
    assert.ok(cells / 2 >= Math.ceil(Math.max(width, length) / 2) + 4, `${width}x${length}: ${cells} cells`);
  }
});

test("the backend URL parameter comes from a fixed list", () => {
  assert.deepEqual(readBackend("?backend=webgl2"), { backend: "webgl2", errors: [] });
  assert.deepEqual(readBackend(""), { backend: "auto", errors: [] });
  const bad = readBackend("?backend=vulkan");
  assert.equal(bad.backend, "auto");
  assert.match(bad.errors[0], /backend must be one of auto, webgpu, webgl2, got "vulkan"/);
});

test("stepper buttons stay within 1 to the limit", () => {
  assert.equal(stepStuds(2, 1, 16), 3);
  assert.equal(stepStuds(16, 1, 16), 16);
  assert.equal(stepStuds(1, -1, 16), 1);
  assert.equal(stepStuds(Number.NaN, 1, 16), 2, "an emptied input steps from 1");
});

test("a slow build shows its whole seconds", () => {
  assert.equal(elapsedText(999), "0 s");
  assert.equal(elapsedText(12_345), "12 s");
  assert.equal(elapsedText(-5), "0 s");
});
