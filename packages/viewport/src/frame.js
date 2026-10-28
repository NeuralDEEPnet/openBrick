// Frame conversion from the geometry kernel to three.js, shared by the device
// render harness (bench/harness) and the browser app (apps/web).
//
// Kernel meshes are millimetres in a right-handed frame with +Z up
// (crates/openbrick-geom/src/nominal.rs, crates/openbrick-wasm). three.js is
// right-handed with +Y up. A rotation of -90 degrees about +X maps (x, y, z) to
// (x, z, -y): the kernel's up axis +Z becomes +Y and its +Y (a brick's length)
// becomes -Z. The determinant is +1, so triangle winding, and with it back-face
// culling, is unchanged. There is no scaling: one three.js unit is one millimetre.
//
// Apply it once, to a group that holds everything in the kernel frame
// (group.rotation.x = KERNEL_TO_THREE_EULER_X); vertex data is never rewritten.
//
// For reference, LDraw files use a right-handed frame with -Y up in LDU
// (docs/plan/04 section on the frame); LDraw to three.js is a 180 degree rotation
// about +X, (x, y, z) -> (x, -y, -z), times 0.4 mm per LDU.

/** Euler X angle (radians) of the group that holds everything in the kernel frame. */
export const KERNEL_TO_THREE_EULER_X = -Math.PI / 2;

/** The same mapping applied to one point, for tests, documentation and camera framing. */
export function kernelToThree([x, y, z]) {
  return [x, z, -y];
}
