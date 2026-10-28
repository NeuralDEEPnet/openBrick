// The committed kernel meshes: format, consistency with the manifest, closed and
// outward-oriented surfaces, and the frame conversion to three.js.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Object3D, Vector3 } from "three";
import { KERNEL_TO_THREE_EULER_X, kernelToThree } from "../src/frame.js";
import { parseObmesh } from "../src/mesh.js";

const meshes = new URL("../public/meshes/", import.meta.url);
const manifest = JSON.parse(readFileSync(new URL("manifest.json", meshes), "utf8"));

function load(file) {
  const bytes = readFileSync(new URL(file, meshes));
  // A fresh ArrayBuffer holding exactly the file (Buffer may share a larger pool).
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
}

/** FNV-1a 64 over positions then indices, as openbrick_geom::Mesh::fingerprint. */
function fingerprint(buffer) {
  const bytes = new Uint8Array(buffer, 32); // positions and indices follow the header
  let h = 0xcbf29ce484222325n;
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return `0x${h.toString(16).padStart(16, "0")}`;
}

test("the manifest describes three LODs built by the kernel from the standard", () => {
  assert.equal(manifest.format, "OBMESH");
  assert.equal(manifest.format_version, 1);
  assert.equal(manifest.units, "mm");
  assert.match(manifest.kernel, /^manifold-rust /);
  assert.match(manifest.standard, /^dimensions-v1 /);
  assert.deepEqual(manifest.lods.map((l) => l.lod), [0, 1, 2]);
  // The body is n studs minus the undersize on both sides (docs/plan/04).
  const [nx, ny] = manifest.studs;
  const u = manifest.undersize_per_side_mm;
  assert.ok(Math.abs(manifest.body_mm[0] - (nx * manifest.pitch_mm - 2 * u)) < 1e-9);
  assert.ok(Math.abs(manifest.body_mm[1] - (ny * manifest.pitch_mm - 2 * u)) < 1e-9);
  assert.equal(manifest.body_mm[2], manifest.brick_height_mm);
});

test("triangle counts fall with the LOD", () => {
  const [l0, l1, l2] = manifest.lods.map((l) => l.triangles);
  assert.ok(l0 > l1 && l1 > l2, `${l0} > ${l1} > ${l2}`);
  assert.equal(l2, 12, "a box has 12 triangles");
});

for (const entry of manifest.lods) {
  test(`LOD${entry.lod}: ${entry.file} matches its manifest entry`, () => {
    const buffer = load(entry.file);
    const mesh = parseObmesh(buffer);
    assert.equal(buffer.byteLength, entry.bytes);
    assert.equal(mesh.vertexCount, entry.vertices);
    assert.equal(mesh.triangles, entry.triangles);
    assert.equal(mesh.units, "mm");
    assert.equal(mesh.upAxis, "+Z");
    assert.equal(fingerprint(buffer), entry.fingerprint);
    const lo = [Infinity, Infinity, Infinity];
    const hi = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < mesh.positions.length; i++) {
      lo[i % 3] = Math.min(lo[i % 3], mesh.positions[i]);
      hi[i % 3] = Math.max(hi[i % 3], mesh.positions[i]);
    }
    for (let k = 0; k < 3; k++) {
      assert.ok(Math.abs(lo[k] - entry.bounds_mm.min[k]) < 5e-4, `min[${k}]`);
      assert.ok(Math.abs(hi[k] - entry.bounds_mm.max[k]) < 5e-4, `max[${k}]`);
    }
    const studTop = manifest.brick_height_mm + manifest.stud_height_mm;
    assert.ok(Math.abs(hi[2] - (entry.stud_segments ? studTop : manifest.brick_height_mm)) < 5e-4, "top height");
  });

  test(`LOD${entry.lod}: the surface is closed and wound outwards`, () => {
    const { positions: p, indices } = parseObmesh(load(entry.file));
    // Every directed edge appears once and its reverse once: closed and consistently wound.
    const edges = new Map();
    for (let t = 0; t < indices.length; t += 3) {
      for (let k = 0; k < 3; k++) {
        const key = `${indices[t + k]},${indices[t + ((k + 1) % 3)]}`;
        edges.set(key, (edges.get(key) ?? 0) + 1);
      }
    }
    for (const [key, n] of edges) {
      const [a, b] = key.split(",");
      assert.equal(n, 1, `edge ${key} is used ${n} times`);
      assert.equal(edges.get(`${b},${a}`), 1, `edge ${key} has no opposite`);
    }
    // Signed volume (divergence theorem) is positive for counter-clockwise outward
    // faces, which is what three.js treats as front faces, and equals the kernel's.
    let six = 0;
    for (let t = 0; t < indices.length; t += 3) {
      const [a, b, c] = [indices[t] * 3, indices[t + 1] * 3, indices[t + 2] * 3];
      six +=
        p[a] * (p[b + 1] * p[c + 2] - p[b + 2] * p[c + 1]) -
        p[a + 1] * (p[b] * p[c + 2] - p[b + 2] * p[c]) +
        p[a + 2] * (p[b] * p[c + 1] - p[b + 1] * p[c]);
    }
    const volume = six / 6;
    assert.ok(volume > 0, "outward winding");
    assert.ok(Math.abs(volume - entry.volume_mm3) / entry.volume_mm3 < 1e-4, `volume ${volume} vs ${entry.volume_mm3}`);
  });
}

test("parseObmesh rejects damaged files with a readable message", () => {
  const good = load(manifest.lods[2].file);
  assert.throws(() => parseObmesh(good.slice(0, 16)), /shorter than the 32-byte header/);
  assert.throws(() => parseObmesh(good.slice(0, good.byteLength - 4)), /header implies/);
  const badMagic = good.slice(0);
  new Uint8Array(badMagic)[0] = 0x58;
  assert.throws(() => parseObmesh(badMagic), /bad magic/);
  const badVersion = good.slice(0);
  new DataView(badVersion).setUint32(8, 2, true);
  assert.throws(() => parseObmesh(badVersion), /version 2/);
  const badIndex = good.slice(0);
  new DataView(badIndex).setUint32(32 + 12 * 8, 99, true);
  assert.throws(() => parseObmesh(badIndex), /out of range/);
});

test("the kernel-to-three.js rotation maps (x, y, z) to (x, z, -y) and keeps handedness", () => {
  // Same object setup as main.js: a group with rotation.x set.
  const world = new Object3D();
  world.rotation.x = KERNEL_TO_THREE_EULER_X;
  world.updateMatrix();
  assert.ok(Math.abs(world.matrix.determinant() - 1) < 1e-12);
  for (const v of [[1, 0, 0], [0, 1, 0], [0, 0, 1], [3.5, -2, 7]]) {
    const got = new Vector3(...v).applyMatrix4(world.matrix).toArray();
    const want = kernelToThree(v);
    got.forEach((g, k) => assert.ok(Math.abs(g - want[k]) < 1e-12, `${v} -> ${got} vs ${want}`));
  }
});

test("after the conversion the studs point up (+Y) in three.js", () => {
  const { positions: p } = parseObmesh(load(manifest.lods[0].file));
  let maxY = -Infinity;
  let minY = Infinity;
  for (let i = 0; i < p.length; i += 3) {
    const [, y] = kernelToThree([p[i], p[i + 1], p[i + 2]]);
    maxY = Math.max(maxY, y);
    minY = Math.min(minY, y);
  }
  assert.ok(Math.abs(minY) < 1e-6, "bottom at y = 0");
  assert.ok(Math.abs(maxY - (manifest.brick_height_mm + manifest.stud_height_mm)) < 5e-4, "stud tops are highest");
});
