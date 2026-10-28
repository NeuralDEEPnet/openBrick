// Runs the kernel WebAssembly probe (crates/openbrick-geom/examples/wasm_probe.rs)
// under Node and prints volume, triangles, timing and module size.
// Usage: node scripts/wasm-probe.mjs <path-to-wasm_probe.wasm>
import { readFileSync, statSync } from "node:fs";
import { gzipSync } from "node:zlib";

const path = process.argv[2];
const bytes = readFileSync(path);
const { instance } = await WebAssembly.instantiate(bytes, {});
const t0 = performance.now();
const volume = instance.exports.ob_brick_2x4_volume();
const t1 = performance.now();
const triangles = instance.exports.ob_brick_2x4_triangles();
const fingerprint = instance.exports.ob_brick_2x4_fingerprint();
console.log(JSON.stringify({
  volume_mm3: Number(volume.toFixed(3)),
  triangles,
  fingerprint: "0x" + BigInt.asUintN(64, fingerprint).toString(16).padStart(16, "0"),
  first_build_ms: Number((t1 - t0).toFixed(1)),
  wasm_bytes: statSync(path).size,
  wasm_gzip_bytes: gzipSync(bytes, { level: 9 }).length,
}));
if (!(volume > 0)) process.exit(1);
