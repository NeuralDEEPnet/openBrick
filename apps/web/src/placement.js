// Where the brick sits in the scene and how the camera frames it. These are
// display transforms of a mesh the core built; nothing here changes geometry.
//
// Frames: the mesh is in the kernel frame (millimetres, +Z up, body minimum corner
// at the origin). It is placed inside a group that maps the kernel frame to
// three.js with packages/viewport/src/frame.js, (x, y, z) -> (x, z, -y), so the
// offsets below are kernel-frame vectors and the ground is three.js y = 0.

import { kernelToThree } from "@openbrick3d/viewport/frame.js";

/**
 * Kernel-frame translation that puts a `width` x `length` brick on the stud
 * lattice of the ground grid, about its centre: the brick's lattice cell corner
 * lands on grid line -floor(n / 2) along each axis, and the body's minimum corner
 * is one undersize inside the cell corner (docs/plan/04: n studs are n pitches
 * minus the undersize on both sides).
 */
export function brickOffset({ width, length }, { pitchMm, undersizeMm }) {
  return [-Math.floor(width / 2) * pitchMm + undersizeMm, -Math.floor(length / 2) * pitchMm + undersizeMm, 0];
}

/**
 * Axis-aligned box and bounding sphere, in three.js coordinates, of kernel-frame
 * bounds [min x, y, z, max x, y, z] moved by a kernel-frame `offset`.
 */
export function placedBounds(bounds, offset) {
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  for (const x of [bounds[0], bounds[3]]) {
    for (const y of [bounds[1], bounds[4]]) {
      for (const z of [bounds[2], bounds[5]]) {
        const p = kernelToThree([x + offset[0], y + offset[1], z + offset[2]]);
        for (let k = 0; k < 3; k++) {
          lo[k] = Math.min(lo[k], p[k]);
          hi[k] = Math.max(hi[k], p[k]);
        }
      }
    }
  }
  const center = lo.map((v, k) => (v + hi[k]) / 2);
  const radius = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) / 2;
  return { min: lo.map((v) => v + 0), max: hi.map((v) => v + 0), center, radius };
}

/**
 * Camera distance from the centre of a sphere of `radius` at which the sphere
 * fits the view of a perspective camera (vertical field of view in degrees and
 * aspect ratio), times `margin`.
 */
export function framingDistance(radius, fovDeg, aspect, margin = 1.2) {
  const vertical = (fovDeg * Math.PI) / 180;
  const horizontal = 2 * Math.atan(Math.tan(vertical / 2) * aspect);
  return (margin * radius) / Math.sin(Math.min(vertical, horizontal) / 2);
}

/**
 * Cells of the square ground grid for a brick: an even count (so the grid's lines
 * fall on whole pitches from the origin) that leaves at least `margin` empty
 * studs around the brick on every side.
 */
export function gridCells({ width, length }, margin = 4) {
  const half = Math.ceil(Math.max(width, length) / 2) + margin;
  return 2 * half;
}
