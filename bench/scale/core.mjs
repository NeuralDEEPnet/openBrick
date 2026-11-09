// Scale benchmark of the core (WebAssembly build) in node: how the model operations behave as the
// model grows to 100,000 bricks (docs/plan/07 section 14). No browser, no GPU: these are core
// numbers (the Rust code as the page runs it, minus the worker's message passing), reproducible
// on any machine with node 22 and the built core.
//
//   cargo xtask wasm                      # builds apps/web/src/wasm
//   node bench/scale/core.mjs             # 1k 5k 10k 25k 100k bricks, three layouts
//   node bench/scale/core.mjs --sizes 1000,10000 --layouts city --samples 100 --json out.json --write /tmp/models
//
// For each size it reports: the .obrick size, JSON.parse time (what a page spends on its own copy),
// the core's open time, save time (what the autosave pays), stats, bill of materials and instance
// array times, snap / pick / add / remove (each with its undo) round trips as p50 and p95 in ms,
// the cost of one edit as the page paid it before the delta replies (the "full state": instances +
// stats + bom + text) and the core's linear memory.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";
import { layoutInstances, layoutText, probeRays, LAYOUTS } from "./layouts.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const wasmDir = join(here, "../../apps/web/src/wasm");
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const sizes = opt("sizes", "1000,5000,10000,25000,100000").split(",").map(Number);
const layouts = opt("layouts", LAYOUTS.join(",")).split(",");
const samples = Number(opt("samples", "60"));
const jsonOut = opt("json", null);
const writeDir = opt("write", null);

const core = await import(join(wasmDir, "openbrick_wasm.js"));
const exports = core.initSync({ module: readFileSync(join(wasmDir, "openbrick_wasm_bg.wasm")) });

const ms = (fn) => {
  const t = performance.now();
  const r = fn();
  return [performance.now() - t, r];
};
function percentile(values, p) {
  const s = [...values].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(s.length * p))];
}
const stat = (values) => ({ p50: +percentile(values, 0.5).toFixed(3), p95: +percentile(values, 0.95).toFixed(3) });
const timeEach = (n, fn) => Array.from({ length: n }, (_, i) => ms(() => fn(i))[0]);
const has = (a, name) => typeof a[name] === "function";

function bench(layout, bricks) {
  const text = layoutText(layout, bricks);
  const { instances } = layoutInstances(layout, bricks);
  if (writeDir) {
    mkdirSync(writeDir, { recursive: true });
    writeFileSync(join(writeDir, `${layout}-${bricks}.obrick`), text);
  }
  const row = { layout, bricks, instances: instances.length, inputBytes: text.length };
  row.jsonParseMs = +ms(() => JSON.parse(text))[0].toFixed(2);
  const a = new core.Assembler();
  const [open, notices] = ms(() => a.load(text));
  row.openMs = +open.toFixed(2);
  const [save, saved] = ms(() => a.save("bench"));
  row.saveMs = +save.toFixed(2);
  row.obrickBytes = saved.length;
  row.statsMs = +ms(() => a.stats())[0].toFixed(2);
  row.bomMs = +ms(() => a.bom())[0].toFixed(2);
  row.instancesMs = +ms(() => a.instances())[0].toFixed(2);
  row.fullStateMs = +ms(() => [a.instances(), a.stats(), a.bom(), a.save("Autosave")])[0].toFixed(2);
  if (has(a, "state_delta")) row.stateDeltaMs = +ms(() => a.state_delta())[0].toFixed(3);
  row.wasmMemoryMiB = +(exports.memory.buffer.byteLength / 1048576).toFixed(1);
  row.notices = notices;

  const rays = probeRays(instances, samples);
  const part = "ob.brick-2x4";
  row.snap = stat(rays.map((r) => ms(() => a.snap(part, 0, ...r.origin, ...r.dir))[0]));
  row.pick = stat(rays.map((r) => ms(() => a.pick(...r.origin, ...r.dir))[0]));

  // Add on top of a tower (the snap says where), then undo.
  const adds = [];
  const undos = [];
  const applyMs = [];
  for (const r of rays) {
    const snap = JSON.parse(a.snap(part, 0, ...r.origin, ...r.dir));
    if (!snap?.valid) continue;
    const cmd = JSON.stringify({ op: "add", part, pos: snap.pos, rot: snap.rot, colour: 4 });
    adds.push(ms(() => a.apply(cmd))[0]);
    undos.push(ms(() => a.undo())[0]);
    if (adds.length >= samples) break;
  }
  row.add = stat(adds);
  row.undoAdd = stat(undos);
  // Remove a part from the middle of the table, then undo (the position matters to the table).
  const mid = Math.floor(instances.length / 2);
  const removes = [];
  const redos = [];
  for (let k = 0; k < Math.min(samples, 30); k++) {
    const id = instances[mid + k * 7].id;
    removes.push(ms(() => a.apply(JSON.stringify({ op: "remove", id })))[0]);
    redos.push(ms(() => a.undo())[0]);
  }
  row.removeMid = stat(removes);
  row.undoRemoveMid = stat(redos);
  // Remove the last and the first part (cheap and dear ends of a table).
  const last = instances.at(-1).id;
  row.removeLast = +ms(() => a.apply(JSON.stringify({ op: "remove", id: last })))[0].toFixed(3);
  a.undo();
  const edit = JSON.stringify({ op: "recolour", id: instances[mid].id, colour: 15 });
  row.recolour = stat(timeEach(Math.min(samples, 30), () => a.apply(edit)));
  a.free();
  return row;
}

// Each case runs in its own process: the core's linear memory only grows, so memory read after a
// big model would still show the biggest model so far.
if (args.includes("--child")) {
  console.log(JSON.stringify(bench(layouts[0], sizes[0])));
  process.exit(0);
}
const { spawnSync } = await import("node:child_process");
const rows = [];
for (const layout of layouts) {
  for (const bricks of sizes) {
    const child = spawnSync(
      process.execPath,
      [fileURLToPath(import.meta.url), "--child", "--sizes", String(bricks), "--layouts", layout, "--samples", String(samples), ...(writeDir ? ["--write", writeDir] : [])],
      { encoding: "utf8", maxBuffer: 1 << 26 },
    );
    if (child.status !== 0) throw new Error(`${layout} ${bricks} failed: ${child.stderr}`);
    const row = JSON.parse(child.stdout);
    rows.push(row);
    console.error(`${layout} ${bricks}: open ${row.openMs} ms, save ${row.saveMs} ms, full state ${row.fullStateMs} ms, snap ${row.snap.p50}/${row.snap.p95}, pick ${row.pick.p50}/${row.pick.p95}, add ${row.add.p50}/${row.add.p95}`);
  }
}

const cols = [
  ["layout", (r) => r.layout],
  ["bricks", (r) => r.bricks],
  [".obrick MB", (r) => (r.obrickBytes / 1048576).toFixed(2)],
  ["JSON.parse", (r) => r.jsonParseMs],
  ["open", (r) => r.openMs],
  ["save", (r) => r.saveMs],
  ["stats", (r) => r.statsMs],
  ["bom", (r) => r.bomMs],
  ["full state", (r) => r.fullStateMs],
  ["snap p50/p95", (r) => `${r.snap.p50}/${r.snap.p95}`],
  ["pick p50/p95", (r) => `${r.pick.p50}/${r.pick.p95}`],
  ["add p50/p95", (r) => `${r.add.p50}/${r.add.p95}`],
  ["undo add", (r) => `${r.undoAdd.p50}/${r.undoAdd.p95}`],
  ["remove mid", (r) => `${r.removeMid.p50}/${r.removeMid.p95}`],
  ["undo remove", (r) => `${r.undoRemoveMid.p50}/${r.undoRemoveMid.p95}`],
  ["wasm MiB", (r) => r.wasmMemoryMiB],
];
console.log(`| ${cols.map((c) => c[0]).join(" | ")} |`);
console.log(`|${cols.map(() => "---").join("|")}|`);
for (const r of rows) console.log(`| ${cols.map((c) => c[1](r)).join(" | ")} |`);
if (jsonOut) writeFileSync(jsonOut, JSON.stringify({ node: process.version, platform: process.platform, rows }, null, 2));
