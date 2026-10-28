// CSV schema test (backlog item 3, docs/plan/14): the column list in src/csv.js,
// the header of bench/devices/results.csv and the table in bench/devices/README.md
// must agree, the row validator must accept a complete row and reject incomplete
// or inconsistent ones, and results.csv must only hold valid device rows.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  COLUMNS,
  COLUMN_NAMES,
  SCHEMA_VERSION,
  csvEscape,
  csvHeader,
  csvRow,
  isErrorRow,
  parseCsv,
  rowFromFields,
  rowProblems,
} from "../src/csv.js";
import { summarize } from "../src/stats.js";

const devices = new URL("../../devices/", import.meta.url);
const manifest = JSON.parse(readFileSync(new URL("../public/meshes/manifest.json", import.meta.url), "utf8"));

/** Columns that may be empty in a finished run (see COLUMNS in src/csv.js). */
const MAY_BE_EMPTY = ["device", "gpu_samples", "gpu_ms_p50", "gpu_ms_p95", "warnings"];

/**
 * A complete, consistent row as a laptop run would produce it. The values are made
 * up for the test (this is not a measurement) but obey every rule: for example
 * fps_median 60 x frame_ms_p50 16.67 = 1000.2 and fps_p5 58.82 x frame_ms_p95 17 =
 * 999.94, both within rounding of 1000.
 */
const GOOD = Object.freeze({
  schema: SCHEMA_VERSION,
  timestamp: "2026-10-28T03:00:00.000Z",
  device: "test fixture, not a device",
  backend_requested: "webgpu",
  backend: "webgpu",
  adapter: "fixture-vendor fixture-architecture",
  software: "no",
  lod: 0,
  count: 10000,
  triangles_per_brick: manifest.lods[0].triangles,
  triangles_per_frame: 10000 * manifest.lods[0].triangles + 1,
  draw_calls: 4,
  chunks: 3,
  chunk_size: 4096,
  frames: 600,
  seconds: 10.002,
  warmup: 2,
  fps_median: 60,
  fps_mean: 59.99,
  fps_p5: 58.82,
  frame_ms_p50: 16.67,
  frame_ms_p95: 17,
  frame_ms_max: 33.3,
  cpu_ms_p50: 0.4,
  cpu_ms_p95: 0.9,
  gpu_samples: "",
  gpu_ms_p50: "",
  gpu_ms_p95: "",
  dpr_cap: 2,
  device_pixel_ratio: 2.625,
  pixel_ratio: 2,
  canvas_width: 2160,
  canvas_height: 4680,
  antialias: 1,
  three_revision: "186",
  kernel: manifest.kernel,
  mesh_fingerprint: manifest.lods[0].fingerprint,
  user_agent: "Mozilla/5.0 (fixture)",
  warnings: "",
});

/** The checks results.csv rows must pass: a valid row, from a device, from a run that worked. */
function deviceRowProblems(fields) {
  if (fields.length !== COLUMN_NAMES.length) return [`${fields.length} fields, expected ${COLUMN_NAMES.length}`];
  const row = rowFromFields(fields);
  const problems = rowProblems(row);
  if (row.software === "yes") problems.push("software-rendered rows are not device results");
  if (isErrorRow(row)) problems.push("failed runs do not belong here (warnings contain error:)");
  return problems;
}

/** The fields of `row` as they appear in a CSV line. */
const fieldsOf = (row) => parseCsv(`${csvRow(row)}\n`)[0];

test("column names are unique snake_case identifiers with descriptions and rules", () => {
  assert.equal(new Set(COLUMN_NAMES).size, COLUMN_NAMES.length);
  for (const [name, doc, rule] of COLUMNS) {
    assert.match(name, /^[a-z][a-z0-9_]*$/, name);
    assert.ok(doc.length > 10, `${name} needs a description`);
    assert.ok(rule && typeof rule.kind === "string", `${name} needs a rule`);
  }
});

test("the columns cover every measurement the backlog item asks for", () => {
  for (const name of [
    "fps_median", "fps_p5", "frame_ms_p95", "triangles_per_frame", "draw_calls", "backend", "adapter",
    "user_agent", "device_pixel_ratio", "canvas_width", "canvas_height", "lod", "count",
  ]) {
    assert.ok(COLUMN_NAMES.includes(name), `missing column ${name}`);
  }
});

test("results.csv starts with exactly the schema header", () => {
  const text = readFileSync(new URL("results.csv", devices), "utf8");
  assert.equal(text.split(/\r?\n/)[0], csvHeader());
});

test("results.csv rows are complete device rows, never software rendering or failed runs", () => {
  const rows = parseCsv(readFileSync(new URL("results.csv", devices), "utf8"));
  const [header, ...data] = rows;
  assert.deepEqual(header, COLUMN_NAMES);
  data.forEach((fields, i) => {
    const problems = deviceRowProblems(fields);
    assert.deepEqual(problems, [], `results.csv line ${i + 2}:\n- ${problems.join("\n- ")}`);
  });
});

test("results.csv guard: passes a device row, rejects software and failed runs", () => {
  assert.deepEqual(deviceRowProblems(fieldsOf(GOOD)), []);
  const software = { ...GOOD, software: "yes", warnings: "software rasteriser: not a device result" };
  assert.deepEqual(deviceRowProblems(fieldsOf(software)), ["software-rendered rows are not device results"]);
  // A phone on http asked for WebGPU, fell back, then failed: the error note is last.
  const failed = {
    ...GOOD,
    backend: "webgl2",
    warnings: "page is not a secure context, so WebGPU is unavailable; asked for webgpu, got webgl2; error: webgl2 device lost: context lost",
  };
  assert.ok(!failed.warnings.startsWith("error:"), "the case the old startsWith check missed");
  assert.deepEqual(deviceRowProblems(fieldsOf(failed)), ["failed runs do not belong here (warnings contain error:)"]);
  assert.match(deviceRowProblems(fieldsOf(GOOD).slice(1))[0], /38 fields, expected 39/);
});

test("bench/devices/README.md documents every column, in order", () => {
  const readme = readFileSync(new URL("README.md", devices), "utf8");
  const section = readme.split(/^## CSV columns$/m)[1];
  assert.ok(section, "README needs a '## CSV columns' section");
  const body = section.split(/^## /m)[0];
  const documented = [...body.matchAll(/^\| `([a-z0-9_]+)` \|/gm)].map((m) => m[1]);
  assert.deepEqual(documented, COLUMN_NAMES);
});

test("CSV quoting follows RFC 4180", () => {
  assert.equal(csvEscape("plain"), "plain");
  assert.equal(csvEscape(12.5), "12.5");
  assert.equal(csvEscape(""), "");
  assert.equal(csvEscape(undefined), "");
  assert.equal(csvEscape("a,b"), '"a,b"');
  assert.equal(csvEscape('say "hi"'), '"say ""hi"""');
  assert.equal(csvEscape("two\nlines"), '"two\nlines"');
});

test("a row round-trips through the parser with every column in place", () => {
  const result = { ...GOOD, adapter: 'ANGLE (Vendor, "GPU", driver 1,2)', warnings: "a; b, c" };
  const [header, fields] = parseCsv(`${csvHeader()}\n${csvRow(result)}\n`);
  assert.deepEqual(header, COLUMN_NAMES);
  assert.equal(fields.length, COLUMN_NAMES.length);
  const back = rowFromFields(fields);
  assert.equal(back.adapter, result.adapter);
  assert.equal(back.warnings, result.warnings);
  assert.deepEqual(rowProblems(back), []);
});

// --------------------------------------------------------------- validator

test("rowProblems accepts a complete row, as an object and as a parsed CSV line", () => {
  assert.deepEqual(rowProblems(GOOD), []);
  assert.deepEqual(rowProblems(rowFromFields(fieldsOf(GOOD))), []);
  // With the GPU timer: readings present, p50 <= p95.
  assert.deepEqual(rowProblems({ ...GOOD, gpu_samples: 12, gpu_ms_p50: 3.1, gpu_ms_p95: 4.2 }), []);
  // GPU timer requested but not offered: 0 readings and empty times.
  assert.deepEqual(rowProblems({ ...GOOD, gpu_samples: 0 }), []);
  // Coarse browser clocks can read 0 ms inside renderer.render().
  assert.deepEqual(rowProblems({ ...GOOD, cpu_ms_p50: 0, cpu_ms_p95: 0 }), []);
});

test("rowProblems rejects the half-empty result a failed page leaves behind", () => {
  // What main.js baseResult() starts from: every column "", here with only the
  // fps columns and frames set. The old missingColumns() check accepted this.
  const set = { fps_median: 60, fps_p5: 50, fps_mean: 58, frames: 100 };
  const row = { ...Object.fromEntries(COLUMN_NAMES.map((n) => [n, ""])), ...set };
  const flagged = rowProblems(row).map((p) => p.split(":")[0]);
  const required = COLUMN_NAMES.filter((n) => !MAY_BE_EMPTY.includes(n) && !(n in set));
  assert.equal(required.length, 30);
  assert.deepEqual(flagged, required);
  assert.ok(rowProblems(row).every((p) => p.endsWith(": is empty")));
});

test("rowProblems names missing, malformed and out-of-range values", () => {
  const { user_agent: _drop, ...noAgent } = GOOD;
  assert.deepEqual(rowProblems(noAgent), ["user_agent: missing"]);
  assert.deepEqual(rowProblems(null), ["no result object"]);
  const cases = [
    [{ frame_ms_p95: NaN }, /^frame_ms_p95: is "NaN"/],
    [{ frame_ms_p95: "" }, /^frame_ms_p95: is empty$/],
    [{ fps_median: 0 }, /^fps_median: is "0", expected a finite number above 0$/],
    [{ seconds: -1 }, /^seconds: is "-1"/],
    [{ frames: 12.5 }, /^frames: is "12.5", expected an integer/],
    [{ chunk_size: "" }, /^chunk_size: is empty$/],
    [{ backend: "vulkan" }, /^backend: is "vulkan", expected one of webgpu, webgl2$/],
    [{ software: "" }, /^software: is empty$/],
    [{ schema: SCHEMA_VERSION + 1 }, /^schema: /],
    [{ timestamp: "yesterday" }, /^timestamp: is "yesterday", expected an ISO 8601 UTC time$/],
    [{ mesh_fingerprint: "fa969a6c" }, /^mesh_fingerprint: /],
    [{ three_revision: "" }, /^three_revision: is empty$/],
    [{ gpu_samples: -1 }, /^gpu_samples: /],
  ];
  for (const [change, expected] of cases) {
    const problems = rowProblems({ ...GOOD, ...change });
    assert.equal(problems.length, 1, `${JSON.stringify(change)}: ${problems.join(" | ")}`);
    assert.match(problems[0], expected);
  }
});

test("rowProblems checks how the columns relate", () => {
  const cases = [
    [{ frame_ms_p95: 16, fps_p5: 62.5 }, /^frame_ms_p50 16.67 is above frame_ms_p95 16$/],
    [{ frame_ms_max: 16.9 }, /^frame_ms_p95 17 is above frame_ms_max 16.9$/],
    [{ cpu_ms_p50: 1 }, /^cpu_ms_p50 1 is above cpu_ms_p95 0.9$/],
    // The old fps_p5 next to the frame_ms_p95 of the same stalled run (see stats.js).
    [{ fps_p5: 21.45, frame_ms_p95: 8016.8, frame_ms_max: 20000 }, /^fps_p5 21.45 is not 1000 \/ frame_ms_p95/],
    [{ fps_median: 59 }, /^fps_median 59 is not 1000 \/ frame_ms_p50/],
    [{ pixel_ratio: 2.625 }, /^pixel_ratio 2.625 is not min\(device_pixel_ratio 2.625, dpr_cap 2\)$/],
    [{ gpu_ms_p50: 3, gpu_ms_p95: 4 }, /^gpu_ms_p50: is 3 although gpu_samples is empty$/],
    [{ gpu_samples: 5 }, /^gpu_ms_p50: is empty although gpu_samples is 5$/],
    [{ gpu_samples: 5, gpu_ms_p50: 5, gpu_ms_p95: 4 }, /^gpu_ms_p50 5 is above gpu_ms_p95 4$/],
  ];
  for (const [change, expected] of cases) {
    const problems = rowProblems({ ...GOOD, ...change });
    assert.ok(problems.length >= 1, `${JSON.stringify(change)} passed`);
    assert.match(problems[0], expected, problems.join(" | "));
  }
});

test("summarize() output passes the validator, including a run with one long stall", () => {
  const steady = Array(600).fill(1000 / 60);
  // 11 frames of 16.7 ms, one of 28 ms and one of 20 s: the stats.js example.
  const stalled = Array(11).fill(16.7).concat([28, 20000]);
  const p1 = Array(7).fill(0.1);
  for (const frameMs of [steady, stalled, [4.17, 4.16, 4.18, 8.33]]) {
    const n = frameMs.length;
    const stats = summarize({
      frameMs,
      cpuMs: Array(n).fill(0.3),
      triangles: Array(n).fill(GOOD.triangles_per_frame),
      drawCalls: Array(n).fill(GOOD.draw_calls),
      measuredMs: frameMs.reduce((a, b) => a + b, 0),
      gpuMs: p1,
    });
    assert.deepEqual(rowProblems({ ...GOOD, ...stats }), [], `frames ${frameMs.slice(0, 3)}...`);
  }
});

test("isErrorRow finds the error note wherever it is", () => {
  assert.equal(isErrorRow({ warnings: "" }), false);
  assert.equal(isErrorRow({ warnings: "asked for webgpu, got webgl2" }), false);
  assert.equal(isErrorRow({ warnings: "error: no adapter" }), true);
  assert.equal(isErrorRow({ warnings: "only 4 frames measured; error: webgpu device lost: destroyed" }), true);
  // "error:" inside another note is not an error note.
  assert.equal(isErrorRow({ warnings: "device label says error: none" }), false);
});
