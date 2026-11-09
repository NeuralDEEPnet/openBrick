// The scale benchmark page's pure parts (src/bench.js): options from the URL, CSV, percentiles, warnings.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { BENCH_DEFAULTS, CSV_COLUMNS, MAX_BENCH_BRICKS, benchOptions, benchWarnings, csvField, csvHeader, csvLine, frameStats, percentile } from "../src/bench.js";

test("the options come from the URL, with defaults and limits", () => {
  assert.equal(benchOptions("").enabled, false);
  assert.equal(benchOptions("?bench=1").enabled, true);
  assert.deepEqual(benchOptions("?bench=1").sizes, BENCH_DEFAULTS.sizes);
  const o = benchOptions("?bench=1&sizes=1k, 10000,100k&layout=towers&seconds=5&device=Pixel%208a");
  assert.deepEqual(o.sizes, [1000, 10000, 100000]);
  assert.equal(o.layout, "towers");
  assert.equal(o.seconds, 5);
  assert.equal(o.device, "Pixel 8a");
  // Nonsense falls back, and a size over the limit is dropped.
  const bad = benchOptions(`?bench=1&sizes=abc,0,${MAX_BENCH_BRICKS + 1},-5&layout=sphere&seconds=9999`);
  assert.deepEqual(bad.sizes, BENCH_DEFAULTS.sizes);
  assert.equal(bad.layout, "city");
  assert.equal(bad.seconds, BENCH_DEFAULTS.seconds);
  assert.equal(benchOptions(`?bench=1&device=${"x".repeat(500)}`).device.length, 80);
});

test("the CSV has one header and one line per row, quoting what needs it", () => {
  assert.equal(csvHeader().split(",").length, CSV_COLUMNS.length);
  assert.equal(csvField(null), "");
  assert.equal(csvField(12.5), "12.5");
  assert.equal(csvField('Pixel 8a, "Chrome"'), '"Pixel 8a, ""Chrome"""');
  const line = csvLine({ date: "2026-11-09", device: "a,b", bricks: 1000, warnings: "software renderer" });
  const fields = line.split(",");
  assert.equal(fields[0], "2026-11-09");
  assert.ok(line.includes('"a,b"'));
  assert.equal(CSV_COLUMNS.indexOf("bricks"), 9);
  assert.equal(new Set(CSV_COLUMNS).size, CSV_COLUMNS.length, "no column twice");
});

test("text that a spreadsheet would run as a formula is neutralised, numbers are not", () => {
  for (const bad of ['=HYPERLINK("http://evil","x")', "@SUM(1+1)", "+1", "-1", "\tcmd", "\rcmd", "=1+1"]) {
    const field = csvField(bad);
    assert.ok(field.replace(/^"/, "").startsWith("'"), `${JSON.stringify(bad)} became ${JSON.stringify(field)}`);
  }
  assert.equal(csvField('=HYPERLINK("a,b","x")'), `"'=HYPERLINK(""a,b"",""x"")"`);
  assert.equal(csvField("Pixel 8a"), "Pixel 8a");
  assert.equal(csvField("a=b"), "a=b", "only a leading character makes a formula");
  assert.equal(csvField(-1), "-1", "a negative number stays a number");
  assert.equal(csvField(0.5), "0.5");
  // Through a whole line: device and user agent come from outside.
  const line = csvLine({ device: "=cmd|' /C calc'!A0", user_agent: "@evil", bricks: 1000, warnings: "-x" });
  assert.ok(line.includes("'=cmd") && line.includes("'@evil") && line.includes("'-x") && line.includes(",1000,"));
});

test("frame statistics and percentiles", () => {
  assert.deepEqual(frameStats([]), { frames: 0, p50: 0, p95: 0, fps: 0 });
  const s = frameStats(Array.from({ length: 100 }, (_, i) => (i < 95 ? 16.7 : 40)));
  assert.equal(s.frames, 100);
  assert.equal(s.p50, 16.7);
  assert.equal(s.p95, 40);
  assert.ok(s.fps > 55 && s.fps < 60);
  assert.equal(percentile([], 0.5), 0);
  assert.equal(percentile([3, 1, 2], 0), 1);
});

test("a run that may not mean what it seems says so", () => {
  assert.deepEqual(benchWarnings({ hidden: false, gpu: "Apple M2", backend: "webgpu", fpsMean: 60, frames: 400 }), []);
  assert.ok(benchWarnings({ gpu: "Google SwiftShader", backend: "webgl2", fpsMean: 60, frames: 400 }).some((w) => /software renderer/.test(w)));
  assert.ok(benchWarnings({ hidden: true, gpu: "x", fpsMean: 60, frames: 400 }).some((w) => /hidden/.test(w)));
  assert.ok(benchWarnings({ gpu: "x", fpsMean: 3, frames: 10 }).some((w) => /fewer than 30/.test(w)));
  assert.ok(benchWarnings({ gpu: "x", fpsMean: 3, frames: 100 }).some((w) => /under 5 fps/.test(w)));
  assert.ok(benchWarnings({ error: "boom", gpu: "x", fpsMean: 0, frames: 0 }).some((w) => /error: boom/.test(w)));
});

test("the results file's header is the CSV's, so rows pasted from the page fit", () => {
  const file = readFileSync(new URL("../../../bench/devices/scale-results.csv", import.meta.url), "utf8");
  assert.equal(file.split("\n")[0], csvHeader());
});
