// The part being designed, as the page shows it: which file a part document
// is, the name it saves under, the drawing an opened part starts with, and
// the sentences that say which features this version cannot build yet
// (docs/plan/06 sections 3 and 11). Pure functions over what the core
// answered (open_part_json, evaluate_part), so node --test checks them; the
// core checks the document and builds it (CLAUDE.md).

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
 * The note shown when a part with features this version cannot build is
 * opened (`unsupported` from open_part_json: [{ feature, op, condition }]),
 * or "" when it has none. It names every such feature and, for each, the `if`
 * that turns it off, so the person knows what to change; the values can be
 * edited and saved either way.
 */
export function unsupportedNote(unsupported) {
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
    const conditions = sometimes.map((u) => `“${u.feature}” is built only while ${u.condition} is true`);
    parts.push(`${list(conditions)}: change the values so ${sometimes.length === 1 ? "it is" : "they are"} off, and the preview builds.`);
  }
  parts.push("You can still edit the values and save the part.");
  return parts.join(" ");
}

const UNSUPPORTED_ERROR = /^feature `([^`]+)` uses `([^`]+)`(.*?), which this evaluator does not build yet/;

/**
 * The status line's text for an evaluation that failed: for a feature this
 * version cannot build, a sentence that names it and what turns it off
 * (from `unsupported`); any other error as the core wrote it.
 */
export function buildErrorText(message, unsupported = []) {
  const m = UNSUPPORTED_ERROR.exec(String(message ?? ""));
  if (!m) return String(message ?? "");
  const [, feature, op, detail] = m;
  const known = (unsupported ?? []).find((u) => u.feature === feature);
  const off = known?.condition
    ? ` It is built only while ${known.condition} is true: change the values so it is off, and the preview builds.`
    : " Nothing in the panel turns it off, so this part cannot be previewed or downloaded here yet.";
  return `“${feature}” uses ${op}${detail}, which this version cannot build yet.${off} You can still edit the values and save the part.`;
}
