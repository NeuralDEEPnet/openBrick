// The part being designed, as the page shows it: which file a part document
// is, the name it saves under, the drawing an opened part starts with, and
// the sentences that say which features this version cannot build yet
// (docs/plan/06 sections 3 and 11). Pure functions over what the core
// answered (open_part_json, evaluate_part), so node --test checks them; the
// core checks the document and builds it (CLAUDE.md).

import { humanLabel } from "./params.js";

/** A file the page opens as a part document rather than as artwork. */
export function isPartFile(name) {
  return /\.json$/i.test(String(name ?? ""));
}

/** The name a part saves under: `{id}.obpart.json`, as in parts/. */
export function partFileName(id) {
  const safe = String(id ?? "").replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "part";
  return `${safe}.obpart.json`;
}

/** The sample that bundles a drawing under `asset`, and that drawing, or null. */
export function sampleForAsset(samples, asset) {
  for (const sample of samples ?? []) {
    const file = sample.files?.find((f) => f.asset === asset);
    if (file) return { sample, file };
  }
  return null;
}

/** "“label” (relief.text)". */
const named = (u) => `“${u.feature}” (${u.op})`;

/** "a, b and c". */
function list(items) {
  return items.length <= 1 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items.at(-1)}`;
}

/**
 * The part document with the core's normalised defaults (`defaults` from
 * open_part_json, `"0.6mm"` read as 0.6) in place of the defaults as written,
 * so the panel shows and compares the numbers the core builds with. The
 * document is not changed; `defaults` null or missing leaves it as written.
 */
export function withCoreDefaults(doc, defaults) {
  if (!defaults || typeof defaults !== "object") return doc;
  const params = {};
  for (const [name, p] of Object.entries(doc?.params ?? {})) {
    params[name] = Object.hasOwn(defaults, name) ? { ...p, default: defaults[name] } : p;
  }
  return { ...doc, params };
}

const SIMPLE_CONDITION = /^\s*(!?)\s*\$?([A-Za-z_][A-Za-z0-9_]*)\s*(?:(==|!=)\s*(?:'([^']*)'|"([^"]*)"|(-?\d+(?:\.\d+)?)|(true|false)))?\s*$/;

/** A control's label and an option's label, from the panel's controls (params.js controlsFor). */
function labels(spec, name, value) {
  const c = (spec?.controls ?? []).find((x) => x.name === name);
  const label = c?.label ?? humanLabel(name);
  if (value === undefined) return { c, label };
  const o = (c?.options ?? []).find((x) => String(x.value) === String(value));
  return { c, label, option: o?.label ?? humanLabel(String(value)) };
}

/**
 * An `if` expression in the panel's words: `while` (when the feature is
 * built, "Studs on top is off") and `off` (what to change so it is not,
 * "turn on Studs on top"), or null for an expression this page does not
 * put into words (then the expression itself is shown).
 */
export function conditionInWords(condition, spec) {
  const m = SIMPLE_CONDITION.exec(String(condition ?? ""));
  if (!m) return null;
  const [, not, name, op, single, double, number, bool] = m;
  if (!op) {
    const { label } = labels(spec, name);
    return not ? { while: `${label} is off`, off: `turn on ${label}` } : { while: `${label} is on`, off: `turn off ${label}` };
  }
  if (not) return null;
  const value = single ?? double ?? number ?? bool;
  const { label, option } = labels(spec, name, value);
  return op === "!="
    ? { while: `${label} is not ${option}`, off: `set ${label} to ${option}` }
    : { while: `${label} is ${option}`, off: `set ${label} to something other than ${option}` };
}

/**
 * The note shown when a part with features this version cannot build is
 * opened (`unsupported` from open_part_json: [{ feature, op, condition }]),
 * or "" when it has none. It names every such feature and, for each, the
 * setting that turns it off in the panel's words (`spec`, the panel's
 * controls, gives the labels), with the part's own expressions after it; the
 * values can be edited and saved either way.
 */
export function unsupportedNote(unsupported, spec) {
  if (!Array.isArray(unsupported) || unsupported.length === 0) return "";
  const always = unsupported.filter((u) => !u.condition);
  const sometimes = unsupported.filter((u) => u.condition);
  const what = unsupported.length === 1 ? "a feature" : `${unsupported.length} features`;
  const parts = [`This part uses ${what} this version cannot build yet: ${list(unsupported.map(named))}.`];
  if (always.length > 0) {
    parts.push(
      `${list(always.map(named))} ${always.length === 1 ? "is" : "are"} always built, so the part cannot be previewed or downloaded here yet.`,
    );
  }
  if (sometimes.length > 0) {
    const words = sometimes.map((u) => ({ u, w: conditionInWords(u.condition, spec) }));
    const whiles = words.map(({ u, w }) => `“${u.feature}” is built only while ${w ? w.while : `${u.condition} is true`}`);
    const offs = words.filter(({ w }) => w).map(({ w }) => w.off);
    const todo = offs.length === words.length ? list(offs) : `change the values so ${sometimes.length === 1 ? "it is" : "they are"} off`;
    parts.push(`${list(whiles)}. To preview the part, ${todo}.`);
    if (offs.length > 0) parts.push(`(In the part file: ${sometimes.map((u) => `“${u.feature}” has if ${u.condition}`).join("; ")}.)`);
  }
  parts.push("You can still edit the values and save the part.");
  return parts.join(" ");
}

const UNSUPPORTED_ERROR = /^feature `([^`]+)` uses `([^`]+)`(.*?), which this evaluator does not build yet/;

/**
 * The status line's text for an evaluation that failed: for a feature this
 * version cannot build, a sentence that names it and what turns it off
 * (from `unsupported`, in the words of the panel's controls `spec`); any
 * other error as the core wrote it.
 */
export function buildErrorText(message, unsupported = [], spec = null) {
  const m = UNSUPPORTED_ERROR.exec(String(message ?? ""));
  if (!m) return String(message ?? "");
  const [, feature, op, detail] = m;
  const known = (unsupported ?? []).find((u) => u.feature === feature);
  const words = known?.condition ? conditionInWords(known.condition, spec) : null;
  const off = !known?.condition
    ? " Nothing in the panel turns it off, so this part cannot be previewed or downloaded here yet."
    : words
      ? ` It is built only while ${words.while}: ${words.off}, and the preview builds.`
      : ` It is built only while ${known.condition} is true: change the values so it is off, and the preview builds.`;
  return `“${feature}” uses ${op}${detail}, which this version cannot build yet.${off} You can still edit the values and save the part.`;
}
