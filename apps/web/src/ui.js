// Small pure helpers for the page: URL parameters, stepping the size inputs and the
// text of the status line. Kept apart from main.js so node --test can check them.

import { parseStuds } from "./protocol.js";

/** The brick the page shows first when the URL names none. */
export const DEFAULT_SIZE = Object.freeze({ width: 2, length: 4 });
export const BACKENDS = ["auto", "webgpu", "webgl2"];

/** Read ?backend= (auto, webgpu or webgl2); anything else falls back to auto with an error. */
export function readBackend(search) {
  const backend = new URLSearchParams(search).get("backend") ?? "auto";
  if (BACKENDS.includes(backend)) return { backend, errors: [] };
  return { backend: "auto", errors: [`backend must be one of ${BACKENDS.join(", ")}, got ${JSON.stringify(backend)}`] };
}

/**
 * Read ?w= and ?l= (studs). Out-of-range or malformed values fall back to the
 * defaults and are reported in `errors`.
 */
export function readSize(search, maxStuds) {
  const q = new URLSearchParams(search);
  const errors = [];
  const size = { ...DEFAULT_SIZE };
  for (const [key, field] of [["w", "width"], ["l", "length"]]) {
    if (!q.has(key)) continue;
    try {
      size[field] = parseStuds(q.get(key), maxStuds, field);
    } catch (err) {
      errors.push(err.message);
    }
  }
  return { ...size, errors };
}

/** The query string for a size, keeping any other parameters. */
export function writeQuery(search, { width, length }) {
  const q = new URLSearchParams(search);
  q.set("w", String(width));
  q.set("l", String(length));
  return `?${q}`;
}

/** The value a stepper button produces: current + step, kept within 1 to max. */
export function stepStuds(current, step, maxStuds) {
  const n = Number.isInteger(current) ? current : 1;
  return Math.min(maxStuds, Math.max(1, n + step));
}

/**
 * After this long a build counts as slow: the status line adds how long it has run
 * and how to cancel it. Large footprints take seconds to tens of seconds with the
 * pure-Rust kernel (Q01 finding 2 in docs/spikes/q01-kernel-conformance.md).
 */
export const SLOW_BUILD_MS = 1000;

/** Status line parts while a build runs; a slow one gets a note on cancelling. */
export function buildingParts({ width, length }, elapsedMs) {
  const parts = [`Building ${width} × ${length}…`];
  if (elapsedMs >= SLOW_BUILD_MS) parts.push("Large bricks take a while. Change the size to cancel.");
  return parts;
}

/** How long a build has run, for the status line: whole seconds. */
export function elapsedText(ms) {
  return `${Math.floor(Math.max(0, ms) / 1000)} s`;
}

const integer = new Intl.NumberFormat("en-US");
const volume = new Intl.NumberFormat("en-US", { minimumFractionDigits: 3, maximumFractionDigits: 3 });

/** Status line parts for a built brick, in display order. */
export function statusParts(result, backend) {
  return [
    `${result.width} × ${result.length} brick`,
    `${integer.format(result.triangles)} triangles`,
    `${volume.format(result.volumeMm3)} mm³`,
    `fingerprint ${result.fingerprint}`,
    `built in ${Math.round(result.buildMs)} ms by ${result.kernel}`,
    `${backend === "webgpu" ? "WebGPU" : "WebGL 2"}`,
  ];
}
