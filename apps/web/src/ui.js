// Small pure helpers for the page: URL parameters, stepping the size inputs, the
// text of the status line, the stats and the validation panel. Kept apart from
// main.js so node --test can check them. Every number shown comes from the core,
// and so does every unit.

export const BACKENDS = ["auto", "webgpu", "webgl2"];
export const MODES = ["tile", "coupon"];

/** The repository, and the print-test guide the validation panel links to. */
export const REPO_URL = "https://github.com/NeuralDEEPnet/openBrick";
export const PRINT_TEST_GUIDE_URL = `${REPO_URL}/blob/main/docs/guides/print-test.md`;

/** Read ?backend= (auto, webgpu or webgl2); anything else falls back to auto with an error. */
export function readBackend(search) {
  const backend = new URLSearchParams(search).get("backend") ?? "auto";
  if (BACKENDS.includes(backend)) return { backend, errors: [] };
  return { backend: "auto", errors: [`backend must be one of ${BACKENDS.join(", ")}, got ${JSON.stringify(backend)}`] };
}

/** Read ?mode= (tile, the default, or coupon). */
export function readMode(search) {
  const mode = new URLSearchParams(search).get("mode") ?? "tile";
  if (MODES.includes(mode)) return { mode, errors: [] };
  return { mode: "tile", errors: [`mode must be one of ${MODES.join(", ")}, got ${JSON.stringify(mode)}`] };
}

/** The query string with ?mode= set (left out for the default), keeping other parameters. */
export function writeMode(search, mode) {
  const q = new URLSearchParams(search);
  if (mode === "tile") q.delete("mode");
  else q.set("mode", mode);
  const s = q.toString();
  return s ? `?${s}` : "";
}

/** The value a stepper button produces: current + step, kept within min and max. */
export function stepStuds(current, step, maxStuds, minStuds = 1) {
  const n = Number.isInteger(current) ? current : minStuds;
  return Math.min(maxStuds, Math.max(minStuds, n + step));
}

/**
 * After this long a build counts as slow: the status line adds how long it has run
 * and how to cancel it. Large footprints take seconds to tens of seconds with the
 * pure-Rust kernel (Q01 finding 2 in docs/spikes/q01-kernel-conformance.md).
 */
export const SLOW_BUILD_MS = 1000;

/** Status line parts while an evaluation runs; a slow one says how to cancel it. */
export function evaluatingParts(what, elapsedMs) {
  const parts = [`${what}…`];
  if (elapsedMs >= SLOW_BUILD_MS) parts.push("Large parts take a while. Any change cancels this one.");
  return parts;
}

/** How long a build has run, for the status line: whole seconds. */
export function elapsedText(ms) {
  return `${Math.floor(Math.max(0, ms) / 1000)} s`;
}

const integer = new Intl.NumberFormat("en-US");
const oneDecimal = new Intl.NumberFormat("en-US", { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const upToThree = new Intl.NumberFormat("en-US", { maximumFractionDigits: 3 });

/** "12.5 × 25.5 × 2.5 mm" from bounds [min x, y, z, max x, y, z]. */
export function sizeText(bounds) {
  const d = [0, 1, 2].map((k) => bounds[k + 3] - bounds[k]);
  return `${d.map((x) => oneDecimal.format(x)).join(" × ")} mm`;
}

/** The stats of an evaluated part, as [label, value] rows. */
export function partStats(result) {
  return [
    ["Triangles", integer.format(result.triangles)],
    ["Size", sizeText(result.bounds)],
    ["Mass", `about ${oneDecimal.format(result.massGPla)} g of PLA, printed solid`],
    ["Built in", `${integer.format(Math.round(result.buildMs))} ms`],
  ];
}

/** The stats of the coupon, as [label, value] rows. */
export function couponStats(result) {
  const s = result.summary;
  const e = result.estimate;
  return [
    ["Triangles", integer.format(result.triangles)],
    ["Size", `${s.footprintMm.map((x) => oneDecimal.format(x)).join(" × ")} × ${oneDecimal.format(s.heightMm)} mm`],
    ["Cells", `${integer.format(result.cells.length)} (${s.copies} copies)`],
    ["Mass", `about ${integer.format(Math.round(e.massGPla))} g of PLA`],
    ["Print time", `about ${oneDecimal.format(e.timeS / 3600)} h at ${oneDecimal.format(e.flowMm3S)} mm³/s (the slicer knows better)`],
    ["Built in", `${integer.format(Math.round(result.buildMs))} ms`],
  ];
}

/** The legend line of the red overlay: how much of the artwork does not print as drawn. */
export function removedText(result) {
  const pct = result.measurements?.removedAreaPct;
  if (typeof pct !== "number") return null;
  return `Red: ${upToThree.format(pct)} % of the artwork does not print as drawn (lines thinner than the minimum feature, or face material too thin between cuts)`;
}

const num = (x, unit) => (typeof x === "number" ? `${upToThree.format(x)}${unit ? ` ${unit}` : ""}` : "not measured");

/**
 * What the validation panel shows for one finding: severity in words and as an
 * icon (never colour alone), rule id, message, measured value, threshold and
 * fix hint, and whether it asks for calibration (FIT.MISSING, FIT.PROCESS).
 * The unit of the numbers is the core's (`f.unit`, from
 * openbrick_print::finding_unit through the worker); without one the numbers
 * are shown bare, never with a guessed unit.
 */
export function findingView(f) {
  const blocker = f.severity === "blocker";
  const unit = typeof f.unit === "string" ? f.unit : "";
  return {
    blocker,
    icon: blocker ? "⛔" : "⚠️",
    severity: blocker ? "Blocker" : "Warning",
    rule: f.rule,
    message: f.message,
    measured: f.value === null && f.threshold === null ? null : num(f.value, unit),
    threshold: f.threshold === null ? null : num(f.threshold, unit),
    fix: f.fix,
    feature: f.feature ?? null,
    calibrate: f.rule === "FIT.MISSING" || f.rule === "FIT.PROCESS",
  };
}

/** Findings for the panel: blockers first, then warnings, each in report order. */
export function sortedFindings(report) {
  const findings = report?.findings ?? [];
  return [...findings.filter((f) => f.severity === "blocker"), ...findings.filter((f) => f.severity !== "blocker")];
}

/**
 * Why the download buttons are disabled, or null when they are enabled: the
 * blockers, by rule id.
 */
export function downloadBlockedReason(report) {
  const rules = [...new Set(sortedFindings(report).filter((f) => f.severity === "blocker").map((f) => f.rule))];
  if (rules.length === 0) return null;
  const list = rules.length === 1 ? rules[0] : `${rules.slice(0, -1).join(", ")} and ${rules.at(-1)}`;
  return `Download is off until the ${rules.length === 1 ? "blocker" : "blockers"} ${list} ${rules.length === 1 ? "is" : "are"} fixed (see Validation).`;
}

/** The summary line above the findings. */
export function reportSummary(report) {
  const all = report?.findings ?? [];
  const b = all.filter((f) => f.severity === "blocker").length;
  const w = all.length - b;
  if (all.length === 0) return "No findings: every implemented rule passes.";
  const part = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  return `${part(b, "blocker")}, ${part(w, "warning")}.`;
}

/** The label of a fit profile in the fit select: "Example fit profile (Prusa MK4S, PETG)". */
export function fitLabel(prefix, fit) {
  const material = typeof fit?.material === "object" ? fit.material?.type : fit?.material;
  const what = [fit?.printer, material].filter(Boolean).join(", ");
  return what ? `${prefix} (${what})` : prefix;
}

/** A pocket set as the command line spells it: crossAndDiagonal -> cross-and-diagonal. */
export function pocketSetFlag(name) {
  return String(name).replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
}

/**
 * The `openbrick calibrate record` line for a process and pocket set
 * (docs/plan/05 sections 4 and 9). `example` is a fit profile's
 * `calibration.chosenCells` (the bundled example's), shown as sample values;
 * without it the values are placeholders.
 */
export function recordCommand(process, pocketSet, example = {}) {
  const cell = (id, fallback) => (typeof example[id] === "number" ? example[id].toFixed(2) : fallback);
  return [
    "openbrick calibrate record",
    `--process ${process}`,
    `--pocket-set ${pocketSetFlag(pocketSet)}`,
    '--printer "Your printer"',
    "--material PLA",
    `--stud ${cell("stud", "D")}`,
    `--tube ${cell("tube", "D")}`,
    `--pocket ${cell("pocketTubeOD", "OD")}/${cell("pocketRib", "RIB")}`,
    "--out my-fit.json",
  ].join(" ");
}

/** The coupon's cells as table rows for its first copy: line, label, what to record. */
export function couponRows(cells) {
  const flag = { stud: "--stud", tube: "--tube" };
  return cells
    .filter((c) => c.copy === 0)
    .map((c) => {
      const value = c.valueMm.toFixed(2);
      if (c.cross) {
        const rib = c.cross.valueMm.toFixed(2);
        return { line: "2x2 pocket", label: `${value} / ${rib}`, record: `--pocket ${value}/${rib}` };
      }
      return {
        line: c.cellId === "stud" ? "Stud" : c.cellId === "tube" ? "Tube bore" : c.cellId,
        label: value,
        record: flag[c.cellId] ? `${flag[c.cellId]} ${value}` : "",
      };
    });
}
