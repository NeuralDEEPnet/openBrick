// The kernel frame (millimetres, +Z up) to three.js (+Y up) conversion, checked
// against three's own matrices.

import assert from "node:assert/strict";
import { test } from "node:test";
import { Object3D, Vector3 } from "three";
import { KERNEL_TO_THREE_EULER_X, kernelToThree } from "../src/frame.js";

/** +0 turns -0 into 0, so deepEqual compares values rather than signed zeros. */
const plain = (v) => v.map((x) => x + 0);

function kernelGroup() {
  // As bench/harness and apps/web set it up: one group with rotation.x set.
  const group = new Object3D();
  group.rotation.x = KERNEL_TO_THREE_EULER_X;
  group.updateMatrix();
  return group;
}

test("the group rotation maps (x, y, z) to (x, z, -y)", () => {
  const { matrix } = kernelGroup();
  for (const v of [[1, 0, 0], [0, 1, 0], [0, 0, 1], [3.5, -2, 7]]) {
    const got = new Vector3(...v).applyMatrix4(matrix).toArray();
    const want = kernelToThree(v);
    got.forEach((g, k) => assert.ok(Math.abs(g - want[k]) < 1e-12, `${v} -> ${got} vs ${want}`));
  }
});

test("it is a proper rotation, so handedness and triangle winding are kept", () => {
  const { matrix } = kernelGroup();
  assert.ok(Math.abs(matrix.determinant() - 1) < 1e-12);
  // A triangle counter-clockwise seen from kernel +Z (up) is counter-clockwise seen
  // from three.js +Y (up): its normal turns from +Z into +Y.
  const [a, b, c] = [[0, 0, 0], [1, 0, 0], [0, 1, 0]].map((p) => new Vector3(...kernelToThree(p)));
  const normal = new Vector3().subVectors(b, a).cross(new Vector3().subVectors(c, a));
  assert.deepEqual(plain(normal.toArray()), [0, 1, 0]);
});

test("kernel up is three.js up and the kernel's +Y (a brick's length) runs along -Z", () => {
  assert.deepEqual(plain(kernelToThree([0, 0, 1])), [0, 1, 0]);
  assert.deepEqual(plain(kernelToThree([0, 1, 0])), [0, 0, -1]);
  assert.deepEqual(plain(kernelToThree([1, 0, 0])), [1, 0, 0]);
});
