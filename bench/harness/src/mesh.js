// Reader for the OBMESH v1 files written by the kernel
// (crates/openbrick-geom/examples/export_lods.rs documents the layout).
// The harness only reads geometry; it never builds or edits it (CLAUDE.md).

const MAGIC = "OBMESH\0\0";
const HEADER_BYTES = 32;
export const UNITS = { 1: "mm", 2: "m" };
export const UP_AXES = { 0: "+X", 1: "+Y", 2: "+Z" };

/**
 * Parse an OBMESH v1 buffer. Returns typed-array views into the buffer (no copy).
 * Throws with a readable message on any inconsistency.
 */
export function parseObmesh(buffer) {
  if (!(buffer instanceof ArrayBuffer)) throw new TypeError("parseObmesh expects an ArrayBuffer");
  if (buffer.byteLength < HEADER_BYTES) throw new Error(`OBMESH: file is ${buffer.byteLength} bytes, shorter than the 32-byte header`);
  const magic = String.fromCharCode(...new Uint8Array(buffer, 0, 8));
  if (magic !== MAGIC) throw new Error("OBMESH: bad magic; not a kernel mesh file");
  const view = new DataView(buffer);
  const word = (offset) => view.getUint32(offset, true);
  const version = word(8);
  if (version !== 1) throw new Error(`OBMESH: format version ${version} is not supported (expected 1)`);
  const vertexCount = word(12);
  const indexCount = word(16);
  const units = UNITS[word(20)];
  const upAxis = UP_AXES[word(24)];
  if (!units) throw new Error(`OBMESH: unknown units code ${word(20)}`);
  if (!upAxis) throw new Error(`OBMESH: unknown up-axis code ${word(24)}`);
  if (indexCount % 3 !== 0) throw new Error(`OBMESH: index count ${indexCount} is not a multiple of 3`);
  const expected = HEADER_BYTES + 12 * vertexCount + 4 * indexCount;
  if (buffer.byteLength !== expected) {
    throw new Error(`OBMESH: file is ${buffer.byteLength} bytes, header implies ${expected}`);
  }
  // Typed arrays use the host byte order and the file is little-endian. Every
  // browser the harness targets is little-endian; refuse anything else loudly.
  if (new Uint8Array(new Uint16Array([1]).buffer)[0] !== 1) throw new Error("OBMESH: big-endian hosts are not supported");
  const positions = new Float32Array(buffer, HEADER_BYTES, 3 * vertexCount);
  const indices = new Uint32Array(buffer, HEADER_BYTES + 12 * vertexCount, indexCount);
  for (let i = 0; i < indexCount; i++) {
    if (indices[i] >= vertexCount) throw new Error(`OBMESH: index ${indices[i]} at ${i} is out of range (${vertexCount} vertices)`);
  }
  return { version, vertexCount, indexCount, triangles: indexCount / 3, units, upAxis, positions, indices };
}
