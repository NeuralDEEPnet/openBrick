// URL query parameters of the harness. Pure functions so they run under node --test.

export const BACKENDS = ["webgpu", "webgl2"];
export const LODS = [0, 1, 2];

/** Defaults for a single run (docs: bench/devices/README.md). */
export const DEFAULTS = Object.freeze({
  count: 10000,
  lod: 0,
  backend: "webgpu",
  seconds: 10,
  warmup: 2,
  dpr: 2,
  aa: 1,
  gpu: 0,
  chunk: 4096,
  device: "",
  // Suite mode runs every combination of counts x lods x backends, one page load each.
  suite: 0,
  counts: [10000, 100000],
  lods: [0, 1, 2],
  backends: ["webgpu", "webgl2"],
  // Internal to suite mode: suite id and run index.
  sid: "",
  run: -1,
});

const int = (min, max) => ({ kind: "int", min, max });
const num = (min, max) => ({ kind: "num", min, max });
const oneOf = (values) => ({ kind: "enum", values });
const listOf = (item) => ({ kind: "list", item });

const SPEC = {
  count: int(1, 2_000_000),
  lod: oneOf(LODS),
  backend: oneOf(BACKENDS),
  seconds: num(0.5, 600),
  warmup: num(0, 120),
  dpr: num(0.25, 8),
  aa: oneOf([0, 1]),
  gpu: oneOf([0, 1]),
  chunk: int(1, 1_048_576),
  device: { kind: "text", max: 80 },
  suite: oneOf([0, 1]),
  counts: listOf(int(1, 2_000_000)),
  lods: listOf(oneOf(LODS)),
  backends: listOf(oneOf(BACKENDS)),
  sid: { kind: "text", max: 32 },
  run: int(0, 10_000),
};

/** Parse one value; returns [value, error]. Integers accept a k suffix (10k = 10000). */
function parseValue(spec, raw) {
  const s = raw.trim();
  switch (spec.kind) {
    case "int": {
      const m = /^(\d+)(k?)$/i.exec(s);
      const v = m ? Number(m[1]) * (m[2] ? 1000 : 1) : NaN;
      if (!Number.isSafeInteger(v) || v < spec.min || v > spec.max) return [undefined, `expected an integer from ${spec.min} to ${spec.max}`];
      return [v];
    }
    case "num": {
      const v = s === "" ? NaN : Number(s);
      if (!Number.isFinite(v) || v < spec.min || v > spec.max) return [undefined, `expected a number from ${spec.min} to ${spec.max}`];
      return [v];
    }
    case "enum": {
      const v = spec.values.find((x) => String(x) === s.toLowerCase());
      if (v === undefined) return [undefined, `expected one of ${spec.values.join(", ")}`];
      return [v];
    }
    case "list": {
      const items = s.split(",").filter((x) => x.trim() !== "");
      if (items.length === 0) return [undefined, "expected a comma-separated list"];
      const out = [];
      for (const item of items) {
        const [v, err] = parseValue(spec.item, item);
        if (err) return [undefined, `${item}: ${err}`];
        out.push(v);
      }
      return [out];
    }
    case "text":
      if (s.length > spec.max) return [undefined, `at most ${spec.max} characters`];
      return [s];
    default:
      throw new Error(`unknown parameter kind ${spec.kind}`);
  }
}

/**
 * Parse a query string such as location.search. Unknown parameters are errors, so
 * a typo cannot silently produce a row measured with the defaults.
 */
export function parseParams(search) {
  const query = new URLSearchParams(search);
  const params = { ...DEFAULTS };
  const errors = [];
  for (const [key, raw] of query) {
    const spec = SPEC[key];
    if (!spec) {
      errors.push(`unknown parameter "${key}" (known: ${Object.keys(SPEC).join(", ")})`);
      continue;
    }
    const [value, err] = parseValue(spec, raw);
    if (err) errors.push(`${key}=${raw}: ${err}`);
    else params[key] = value;
  }
  return { params, errors };
}

/** The query string that reproduces a single run (defaults included, for readable URLs). */
export function runQuery(p) {
  const q = new URLSearchParams();
  for (const key of ["count", "lod", "backend", "seconds", "warmup", "dpr", "aa"]) q.set(key, String(p[key]));
  if (p.gpu !== DEFAULTS.gpu) q.set("gpu", String(p.gpu));
  if (p.chunk !== DEFAULTS.chunk) q.set("chunk", String(p.chunk));
  if (p.device) q.set("device", p.device);
  return q.toString();
}

/** Every run of a suite, in order: counts outermost, then backends, then lods. */
export function suitePlan(p) {
  const runs = [];
  for (const count of p.counts) {
    for (const backend of p.backends) {
      for (const lod of p.lods) runs.push({ count, backend, lod });
    }
  }
  return runs;
}

/** Query string of run `index` of the suite described by `p`, tagged with suite id `sid`. */
export function suiteRunQuery(p, index, sid) {
  const run = suitePlan(p)[index];
  const q = new URLSearchParams(runQuery({ ...p, ...run }));
  q.set("suite", "1");
  q.set("counts", p.counts.join(","));
  q.set("lods", p.lods.join(","));
  q.set("backends", p.backends.join(","));
  q.set("sid", sid);
  q.set("run", String(index));
  return q.toString();
}
