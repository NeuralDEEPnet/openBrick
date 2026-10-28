// URL parameters, lattice layout and frame statistics.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { gridLayout } from "../src/layout.js";
import { DEFAULTS, parseParams, runQuery, suitePlan, suiteRunQuery } from "../src/params.js";
import { percentile, summarize } from "../src/stats.js";

const manifest = JSON.parse(readFileSync(new URL("../public/meshes/manifest.json", import.meta.url), "utf8"));

// ------------------------------------------------------------------ params

test("defaults match the backlog item", () => {
  const { params, errors } = parseParams("");
  assert.deepEqual(errors, []);
  assert.equal(params.count, 10000);
  assert.equal(params.lod, 0);
  assert.equal(params.backend, "webgpu");
  assert.equal(params.seconds, 10);
  assert.equal(params.warmup, 2);
  assert.equal(params.gpu, 0, "GPU timing is opt-in");
});

test("valid parameters parse to typed values", () => {
  const { params, errors } = parseParams("?count=100k&lod=2&backend=WebGL2&seconds=2.5&warmup=0&dpr=0.75&aa=0&device=Pixel%208a");
  assert.deepEqual(errors, []);
  assert.equal(params.count, 100000);
  assert.equal(params.lod, 2);
  assert.equal(params.backend, "webgl2");
  assert.equal(params.seconds, 2.5);
  assert.equal(params.warmup, 0);
  assert.equal(params.dpr, 0.75);
  assert.equal(params.aa, 0);
  assert.equal(params.device, "Pixel 8a");
});

test("bad and unknown parameters are reported, not silently defaulted", () => {
  const { errors } = parseParams("?count=-5&lod=3&backend=vulkan&seconds=abc&lods=1&cuont=7");
  assert.equal(errors.length, 5, errors.join("\n"));
  assert.ok(errors.some((e) => e.includes('unknown parameter "cuont"')));
  assert.ok(errors.some((e) => e.startsWith("count=-5")));
  assert.ok(errors.some((e) => e.startsWith("lod=3")));
  assert.ok(errors.some((e) => e.startsWith("backend=vulkan")));
  assert.ok(errors.some((e) => e.startsWith("seconds=abc")));
});

test("runQuery reproduces the parameters it was given", () => {
  const { params } = parseParams("?count=2500&lod=1&backend=webgl2&device=Laptop%2C%20Iris%20Xe&chunk=1024&gpu=1");
  const again = parseParams(`?${runQuery(params)}`).params;
  for (const key of ["count", "lod", "backend", "seconds", "warmup", "dpr", "aa", "gpu", "chunk", "device"]) {
    assert.equal(again[key], params[key], key);
  }
});

test("the standard suite is 10k and 100k at every LOD on both backends", () => {
  const plan = suitePlan(DEFAULTS);
  assert.equal(plan.length, 12);
  assert.deepEqual(plan[0], { count: 10000, backend: "webgpu", lod: 0 });
  assert.deepEqual(plan[11], { count: 100000, backend: "webgl2", lod: 2 });
  assert.equal(new Set(plan.map((r) => `${r.count}/${r.backend}/${r.lod}`)).size, 12);
});

test("each suite run URL parses back to that run and keeps the suite", () => {
  const { params } = parseParams("?suite=1&counts=200,300&lods=1,2&backends=webgl2&seconds=1&device=x");
  const plan = suitePlan(params);
  assert.equal(plan.length, 4);
  plan.forEach((run, i) => {
    const { params: p, errors } = parseParams(`?${suiteRunQuery(params, i, "abc")}`);
    assert.deepEqual(errors, []);
    assert.deepEqual({ count: p.count, backend: p.backend, lod: p.lod }, run);
    assert.equal(p.suite, 1);
    assert.equal(p.run, i);
    assert.equal(p.sid, "abc");
    assert.equal(p.seconds, 1);
    assert.equal(p.device, "x");
    assert.deepEqual(suitePlan(p), plan);
  });
});

// ------------------------------------------------------------------ layout

test("every brick is placed once, on the lattice, without overlap", () => {
  for (const count of [1, 7, 1000, 10000, 12345]) {
    const layout = gridLayout({ count, studs: manifest.studs, chunkSize: 4096 });
    const seen = new Set();
    for (const xy of layout.chunks) {
      assert.ok(xy.length / 2 <= layout.chunkCapacity);
      for (let i = 0; i < xy.length; i += 2) {
        assert.ok(Number.isInteger(xy[i]) && Number.isInteger(xy[i + 1]));
        // Spacing is at least the footprint, so cells on a grid of that spacing are disjoint.
        assert.equal((xy[i] - xy[0]) % layout.spacing[0], 0);
        assert.equal((xy[i + 1] - xy[1]) % layout.spacing[1], 0);
        seen.add(`${xy[i]},${xy[i + 1]}`);
      }
    }
    assert.equal(seen.size, count, `count ${count}`);
    assert.ok(layout.spacing[0] >= manifest.studs[0] && layout.spacing[1] >= manifest.studs[1]);
  }
});

test("the grid is about square and centred", () => {
  const layout = gridLayout({ count: 100000, studs: manifest.studs, chunkSize: 4096 });
  const [ex, ey] = layout.extent;
  assert.ok(Math.abs(ex - ey) / Math.max(ex, ey) < 0.02, `${ex} x ${ey}`);
  let sx = 0;
  let sy = 0;
  let n = 0;
  for (const xy of layout.chunks) {
    for (let i = 0; i < xy.length; i += 2) {
      sx += xy[i];
      sy += xy[i + 1];
      n++;
    }
  }
  assert.ok(Math.abs(sx / n) < layout.spacing[0] && Math.abs(sy / n) < 2 * layout.spacing[1], "centroid near the origin");
});

test("chunks are compact square tiles, so per-chunk culling can work", () => {
  const layout = gridLayout({ count: 100000, studs: manifest.studs, chunkSize: 4096 });
  assert.equal(layout.tileSide, 64);
  assert.equal(layout.chunks.length, Math.ceil(layout.cols / 64) * Math.ceil(layout.rows / 64));
  for (const xy of layout.chunks) {
    const xs = xy.filter((_, i) => i % 2 === 0);
    const ys = xy.filter((_, i) => i % 2 === 1);
    assert.ok(Math.max(...xs) - Math.min(...xs) < 64 * layout.spacing[0]);
    assert.ok(Math.max(...ys) - Math.min(...ys) < 64 * layout.spacing[1]);
  }
});

// ------------------------------------------------------------------- stats

test("percentile interpolates between closest ranks", () => {
  const xs = [10, 20, 30, 40, 50];
  assert.equal(percentile(xs, 0), 10);
  assert.equal(percentile(xs, 50), 30);
  assert.equal(percentile(xs, 100), 50);
  assert.equal(percentile(xs, 25), 20);
  assert.equal(percentile([1, 2], 50), 1.5);
  assert.ok(Number.isNaN(percentile([], 50)));
});

test("summarize: a steady 60 Hz run with one hitch", () => {
  const frameMs = Array(99).fill(1000 / 60).concat([100]);
  const total = frameMs.reduce((a, b) => a + b, 0);
  const s = summarize({
    frameMs,
    cpuMs: Array(100).fill(0.5),
    triangles: Array(100).fill(1234),
    drawCalls: Array(100).fill(7),
    measuredMs: total,
  });
  assert.equal(s.frames, 100);
  assert.equal(s.fps_median, 60);
  assert.equal(s.frame_ms_p50, 16.67);
  assert.equal(s.frame_ms_max, 100);
  // 100 frames in 1750 ms: 99 x 16.667 + 100 = 1750.
  assert.equal(s.fps_mean, 57.14);
  // p95 of the frame times: rank 0.95 x 99 = 94.05 lies among the 16.667 ms frames.
  assert.equal(s.frame_ms_p95, 16.67);
  assert.equal(s.fps_p5, 60);
  assert.equal(s.triangles_per_frame, 1234);
  assert.equal(s.draw_calls, 7);
  assert.equal(s.cpu_ms_p50, 0.5);
  assert.deepEqual([s.gpu_samples, s.gpu_ms_p50, s.gpu_ms_p95], ["", "", ""], "GPU columns stay empty when timing is off");
});

test("summarize: fps columns are 1000 / the frame-time percentiles, also with a stall", () => {
  // 11 frames of 16.7 ms, then 28 ms and a 20 s stall (13 frames). Sorted frame
  // times: p50 is rank 6 = 16.7 ms; p95 is rank 0.95 x 12 = 11.4, so
  // 28 + 0.4 x (20000 - 28) = 8016.8 ms, and fps_p5 = 1000 / 8016.8 = 0.1247.
  // Interpolating in fps space instead gave 21.45 for the same frames.
  const frameMs = Array(11).fill(16.7).concat([28, 20000]);
  const s = summarize({
    frameMs,
    cpuMs: Array(13).fill(1),
    triangles: Array(13).fill(1),
    drawCalls: Array(13).fill(1),
    measuredMs: frameMs.reduce((a, b) => a + b, 0),
  });
  assert.equal(s.frame_ms_p50, 16.7);
  assert.equal(s.fps_median, 59.88);
  assert.equal(s.frame_ms_p95, 8016.8);
  assert.equal(s.fps_p5, 0.12);
  assert.equal(s.frame_ms_max, 20000);
  // An even count: the median frame time is interpolated, then inverted.
  const even = summarize({ frameMs: [10, 20], cpuMs: [1, 1], triangles: [1, 1], drawCalls: [1, 1], measuredMs: 30 });
  assert.equal(even.frame_ms_p50, 15);
  assert.equal(even.fps_median, 66.67);
});

test("summarize: GPU timer readings", () => {
  const base = { frameMs: [16, 17], cpuMs: [1, 1], triangles: [1, 1], drawCalls: [1, 1], measuredMs: 33 };
  const on = summarize({ ...base, gpuMs: [4, 2, 3, 5, 1] });
  assert.equal(on.gpu_samples, 5);
  assert.equal(on.gpu_ms_p50, 3);
  assert.equal(on.gpu_ms_p95, 4.8);
  const none = summarize({ ...base, gpuMs: [] });
  assert.equal(none.gpu_samples, 0);
  assert.equal(none.gpu_ms_p50, "");
});
