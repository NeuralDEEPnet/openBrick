// The harness CSV schema: one row per run. This list is the single source of truth
// for the column names and the rule each value must meet. test/csv-schema.test.js
// checks that bench/devices/results.csv and bench/devices/README.md agree with it
// and that every row in results.csv passes rowProblems(); smoke.mjs checks every row
// a real run produces with rowProblems() too.

/** Bump when a column is added, removed or changes meaning. */
export const SCHEMA_VERSION = 1;

// Rules for one value, checked on the text that goes into the CSV (see rowProblems).
/** Non-empty text. */
const TEXT = { kind: "text" };
/** Text that may be empty. */
const OPTIONAL = { kind: "text", optional: true };
/** One of a fixed set of values. */
const oneOf = (...values) => ({ kind: "enum", values: values.map(String) });
/** An integer of at least `min`. */
const int = (min, optional = false) => ({ kind: "int", min, optional });
/** A finite number above zero. */
const positive = (optional = false) => ({ kind: "num", min: 0, exclusive: true, optional });
/** A finite number of zero or more. */
const NON_NEGATIVE = { kind: "num", min: 0, exclusive: false, optional: false };
/** Text matching a pattern. */
const pattern = (re, what) => ({ kind: "pattern", re, what });

const BACKEND = oneOf("webgpu", "webgl2");

/**
 * Column names in output order, each with a one-line description and the rule its
 * value must meet in a finished run. Only `device`, `warnings` and the `gpu_*`
 * columns may be empty; rowProblems() ties the `gpu_*` columns to each other.
 */
export const COLUMNS = [
  ["schema", "CSV schema version of the harness", oneOf(SCHEMA_VERSION)],
  ["timestamp", "UTC time the measurement finished (ISO 8601)", pattern(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z$/, "an ISO 8601 UTC time")],
  ["device", "free-text label from the device URL parameter", OPTIONAL],
  ["backend_requested", "backend asked for in the URL (webgpu or webgl2)", BACKEND],
  ["backend", "backend that actually rendered (webgpu or webgl2)", BACKEND],
  ["adapter", "GPU string: WebGPU adapter info or WEBGL_debug_renderer_info", TEXT],
  ["software", "yes when the adapter is a software rasteriser; such rows are not device results", oneOf("yes", "no")],
  ["lod", "level of detail (0, 1 or 2)", oneOf(0, 1, 2)],
  ["count", "number of brick instances", int(1)],
  ["triangles_per_brick", "triangles in one brick at this LOD (from the mesh manifest)", int(1)],
  ["triangles_per_frame", "median triangles drawn per frame (renderer.info) after chunk culling, including the one-triangle output pass", int(1)],
  ["draw_calls", "median draw calls per frame (renderer.info), including the output pass", int(1)],
  ["chunks", "number of InstancedMesh chunks in the scene", int(1)],
  ["chunk_size", "maximum instances per chunk", int(1)],
  ["frames", "frames measured after warm-up", int(1)],
  ["seconds", "measured duration in seconds", positive()],
  ["warmup", "warm-up in seconds, not measured (at least 3 frames unless 0)", NON_NEGATIVE],
  ["fps_median", "1000 / frame_ms_p50: frame rate of the median frame", positive()],
  ["fps_mean", "frames divided by measured seconds", positive()],
  ["fps_p5", "1000 / frame_ms_p95: frame rate of the slow frames (5th percentile of frame rate)", positive()],
  ["frame_ms_p50", "median frame time in milliseconds", positive()],
  ["frame_ms_p95", "95th percentile frame time in milliseconds", positive()],
  ["frame_ms_max", "longest frame in milliseconds", positive()],
  // Zero is allowed: browsers coarsen performance.now() (to 0.1 ms or more, 1 ms in
  // some), so a light scene on a fast device can read 0.
  ["cpu_ms_p50", "median JavaScript time inside renderer.render() in milliseconds", NON_NEGATIVE],
  ["cpu_ms_p95", "95th percentile JavaScript time inside renderer.render() in milliseconds", NON_NEGATIVE],
  ["gpu_samples", "GPU timer readings taken (gpu=1 only; 0 when the browser offers no GPU timer)", int(0, true)],
  ["gpu_ms_p50", "median GPU time per frame from timestamp queries in milliseconds (gpu=1 only)", positive(true)],
  ["gpu_ms_p95", "95th percentile GPU time per frame in milliseconds (gpu=1 only)", positive(true)],
  ["dpr_cap", "device pixel ratio cap from the dpr URL parameter", positive()],
  ["device_pixel_ratio", "window.devicePixelRatio of the browser", positive()],
  ["pixel_ratio", "pixel ratio actually used: min(device_pixel_ratio, dpr_cap)", positive()],
  ["canvas_width", "drawing-buffer width in pixels", int(1)],
  ["canvas_height", "drawing-buffer height in pixels", int(1)],
  ["antialias", "1 when MSAA was requested, 0 otherwise", oneOf(0, 1)],
  ["three_revision", "three.js revision the page was built with (THREE.REVISION)", pattern(/^\d+$/, "a three.js revision number")],
  ["kernel", "geometry kernel that built the meshes (from the mesh manifest)", TEXT],
  ["mesh_fingerprint", "fingerprint of the LOD mesh (from the mesh manifest)", pattern(/^0x[0-9a-f]{16}$/, "0x and 16 hex digits")],
  ["user_agent", "navigator.userAgent", TEXT],
  ["warnings", "semicolon-separated notes that qualify the row (empty when clean)", OPTIONAL],
];

export const COLUMN_NAMES = COLUMNS.map(([name]) => name);

/** Separator of the notes in the `warnings` column. */
export const WARNING_SEPARATOR = "; ";
/** Prefix of the note a failed run adds to `warnings`. */
export const ERROR_PREFIX = "error:";

/** Quote a value for CSV (RFC 4180): quotes, commas and line breaks force quoting. */
export function csvEscape(value) {
  if (value === null || value === undefined) return "";
  const s = String(value);
  return /[",\r\n]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s;
}

export function csvHeader() {
  return COLUMN_NAMES.join(",");
}

/** One CSV row from a result object keyed by column name. */
export function csvRow(result) {
  return COLUMN_NAMES.map((name) => csvEscape(result[name])).join(",");
}

/** Minimal RFC 4180 parser: returns rows of text fields. */
export function parseCsv(text) {
  const rows = [];
  let row = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"' && field === "") quoted = true;
    else if (c === ",") {
      row.push(field);
      field = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else field += c;
  }
  if (field !== "" || row.length > 0) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

/** A row object keyed by column name from the fields of one parsed CSV line. */
export function rowFromFields(fields) {
  return Object.fromEntries(COLUMN_NAMES.map((name, i) => [name, fields[i]]));
}

/**
 * True when the run failed: one of its warnings is an error note. Notes are joined
 * in the order they arose, so the error is not necessarily the first one.
 */
export function isErrorRow(row) {
  return String(row?.warnings ?? "")
    .split(WARNING_SEPARATOR)
    .some((note) => note.trimStart().startsWith(ERROR_PREFIX));
}

/** Why `text` breaks `rule`, or null when it meets it. `text` is never undefined. */
function ruleProblem(rule, text) {
  if (text === "") return rule.optional ? null : "is empty";
  switch (rule.kind) {
    case "text":
      return null;
    case "enum":
      return rule.values.includes(text) ? null : `is "${text}", expected one of ${rule.values.join(", ")}`;
    case "pattern":
      return rule.re.test(text) ? null : `is "${text}", expected ${rule.what}`;
    case "int": {
      const v = Number(text);
      return /^-?\d+$/.test(text) && Number.isSafeInteger(v) && v >= rule.min ? null : `is "${text}", expected an integer of at least ${rule.min}`;
    }
    case "num": {
      const v = Number(text);
      const ok = Number.isFinite(v) && (rule.exclusive ? v > rule.min : v >= rule.min);
      return ok ? null : `is "${text}", expected a finite number ${rule.exclusive ? "above" : "of at least"} ${rule.min}`;
    }
    default:
      throw new Error(`unknown column rule ${rule.kind}`);
  }
}

/**
 * Everything wrong with one finished run, as readable messages; an empty list means
 * the row is complete and consistent. Values are checked in the form they take in
 * the CSV (String(value)), so this accepts both a result object from the page and
 * a line parsed from results.csv (see rowFromFields).
 *
 * Besides each column's rule, it checks how columns relate:
 * - frame_ms_p50 <= frame_ms_p95 <= frame_ms_max and cpu_ms_p50 <= cpu_ms_p95;
 * - fps_median and fps_p5 are 1000 / frame_ms_p50 and 1000 / frame_ms_p95. Both
 *   sides are rounded to 0.01, so with f and m the CSV values and F * M = 1000 the
 *   unrounded ones, |f m - 1000| <= 0.005 (f + m) + 0.0001; the check allows 0.001;
 * - pixel_ratio = min(device_pixel_ratio, dpr_cap);
 * - gpu_ms_* are filled exactly when gpu_samples is above 0, with p50 <= p95.
 */
export function rowProblems(row) {
  if (row == null || typeof row !== "object") return ["no result object"];
  const problems = [];
  const text = {};
  for (const [name, , rule] of COLUMNS) {
    const value = row[name];
    if (value === undefined) {
      problems.push(`${name}: missing`);
      continue;
    }
    text[name] = value === null ? "" : String(value);
    const problem = ruleProblem(rule, text[name]);
    if (problem) problems.push(`${name}: ${problem}`);
  }
  if (problems.length > 0) return problems;

  const n = Object.fromEntries(Object.entries(text).map(([k, v]) => [k, v === "" ? null : Number(v)]));
  const order = (names) => {
    for (let i = 1; i < names.length; i++) {
      if (n[names[i - 1]] > n[names[i]]) problems.push(`${names[i - 1]} ${n[names[i - 1]]} is above ${names[i]} ${n[names[i]]}`);
    }
  };
  order(["frame_ms_p50", "frame_ms_p95", "frame_ms_max"]);
  order(["cpu_ms_p50", "cpu_ms_p95"]);
  for (const [fps, ms] of [["fps_median", "frame_ms_p50"], ["fps_p5", "frame_ms_p95"]]) {
    const f = n[fps];
    const m = n[ms];
    if (Math.abs(f * m - 1000) > 0.005 * (f + m) + 1e-3) problems.push(`${fps} ${f} is not 1000 / ${ms} (${ms} is ${m})`);
  }
  if (Math.abs(n.pixel_ratio - Math.min(n.device_pixel_ratio, n.dpr_cap)) > 1e-9) {
    problems.push(`pixel_ratio ${n.pixel_ratio} is not min(device_pixel_ratio ${n.device_pixel_ratio}, dpr_cap ${n.dpr_cap})`);
  }
  const gpuReadings = n.gpu_samples !== null && n.gpu_samples > 0;
  for (const name of ["gpu_ms_p50", "gpu_ms_p95"]) {
    if (gpuReadings && n[name] === null) problems.push(`${name}: is empty although gpu_samples is ${n.gpu_samples}`);
    if (!gpuReadings && n[name] !== null) problems.push(`${name}: is ${n[name]} although gpu_samples is ${text.gpu_samples || "empty"}`);
  }
  if (gpuReadings) order(["gpu_ms_p50", "gpu_ms_p95"]);
  return problems;
}
