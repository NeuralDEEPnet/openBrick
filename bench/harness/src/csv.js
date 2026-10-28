// The harness CSV schema: one row per run. This list is the single source of truth
// for the column names; test/harness.test.js checks that bench/devices/results.csv
// and bench/devices/README.md agree with it, and smoke.mjs checks that a real run
// fills every column.

/** Bump when a column is added, removed or changes meaning. */
export const SCHEMA_VERSION = 1;

/** Column names in output order, each with a one-line description. */
export const COLUMNS = [
  ["schema", "CSV schema version of the harness"],
  ["timestamp", "UTC time the measurement finished (ISO 8601)"],
  ["device", "free-text label from the device URL parameter"],
  ["backend_requested", "backend asked for in the URL (webgpu or webgl2)"],
  ["backend", "backend that actually rendered (webgpu or webgl2)"],
  ["adapter", "GPU string: WebGPU adapter info or WEBGL_debug_renderer_info"],
  ["software", "yes when the adapter is a software rasteriser; such rows are not device results"],
  ["lod", "level of detail (0, 1 or 2)"],
  ["count", "number of brick instances"],
  ["triangles_per_brick", "triangles in one brick at this LOD (from the mesh manifest)"],
  ["triangles_per_frame", "median triangles drawn per frame (renderer.info) after chunk culling, including the one-triangle output pass"],
  ["draw_calls", "median draw calls per frame (renderer.info), including the output pass"],
  ["chunks", "number of InstancedMesh chunks in the scene"],
  ["chunk_size", "maximum instances per chunk"],
  ["frames", "frames measured after warm-up"],
  ["seconds", "measured duration in seconds"],
  ["warmup", "warm-up in seconds, not measured (at least 3 frames unless 0)"],
  ["fps_median", "median of per-frame fps (1000 / frame ms)"],
  ["fps_mean", "frames divided by measured seconds"],
  ["fps_p5", "5th percentile of per-frame fps (the slow frames)"],
  ["frame_ms_p50", "median frame time in milliseconds"],
  ["frame_ms_p95", "95th percentile frame time in milliseconds"],
  ["frame_ms_max", "longest frame in milliseconds"],
  ["cpu_ms_p50", "median JavaScript time inside renderer.render() in milliseconds"],
  ["cpu_ms_p95", "95th percentile JavaScript time inside renderer.render() in milliseconds"],
  ["gpu_samples", "GPU timer readings taken (gpu=1 only; 0 when the browser offers no GPU timer)"],
  ["gpu_ms_p50", "median GPU time per frame from timestamp queries in milliseconds (gpu=1 only)"],
  ["gpu_ms_p95", "95th percentile GPU time per frame in milliseconds (gpu=1 only)"],
  ["dpr_cap", "device pixel ratio cap from the dpr URL parameter"],
  ["device_pixel_ratio", "window.devicePixelRatio of the browser"],
  ["pixel_ratio", "pixel ratio actually used: min(device_pixel_ratio, dpr_cap)"],
  ["canvas_width", "drawing-buffer width in pixels"],
  ["canvas_height", "drawing-buffer height in pixels"],
  ["antialias", "1 when MSAA was requested, 0 otherwise"],
  ["three_revision", "three.js revision the page was built with (THREE.REVISION)"],
  ["kernel", "geometry kernel that built the meshes (from the mesh manifest)"],
  ["mesh_fingerprint", "fingerprint of the LOD mesh (from the mesh manifest)"],
  ["user_agent", "navigator.userAgent"],
  ["warnings", "semicolon-separated notes that qualify the row (empty when clean)"],
];

export const COLUMN_NAMES = COLUMNS.map(([name]) => name);

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

/** The columns a result object lacks (undefined counts as missing; empty strings do not). */
export function missingColumns(result) {
  return COLUMN_NAMES.filter((name) => result == null || result[name] === undefined);
}
