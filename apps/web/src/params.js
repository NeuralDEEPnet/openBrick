// From a part document's `params` to the designer's controls and back: which
// control each parameter gets, its limits and label, which parameters are shown
// (`visibleWhen`), the overrides sent to the core, and the download file name
// (docs/plan/06 sections 3 and 11). Pure functions over the document JSON, so
// node --test checks them; nothing here computes geometry (CLAUDE.md). Limits,
// steps and defaults come from the document; the only number of the page's own
// is the core's footprint limit, `maxStuds`, which caps a stud grid.

/** "reliefHeight" -> "Relief height". */
export function humanLabel(name) {
  const words = String(name)
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .trim()
    .toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** An enum option as { value, label }: options are strings or { value, label }. */
function option(o) {
  if (o !== null && typeof o === "object") return { value: o.value, label: String(o.label ?? humanLabel(o.value)) };
  return { value: o, label: humanLabel(o) };
}

/** The group of parameters whose document names none. */
export const UNGROUPED = "Details";

/** Axis names of a two-axis stud grid (x, then y). */
const GRID_AXES = ["Width", "Length"];

/** The finite number at `index` of an array or the number itself, else `fallback`. */
function pick(v, index, fallback) {
  const x = Array.isArray(v) ? v[index] : v;
  return typeof x === "number" && Number.isFinite(x) ? x : fallback;
}

/**
 * The control for one parameter: { name, kind, label, help, visibleWhen, group,
 * default, ... }. Kinds: `grid2` (two whole-number inputs with axes [{ label,
 * min, max }]), `choice` (options [{ value, label }]), `range` (a slider: min,
 * max, step, unit), `number` (min, max, step or "any", unit), `file` (an SVG
 * asset; default is the asset name), `checkbox`, `text`, `color`. `maxStuds`
 * caps a grid in studs at the core's footprint limit (`capped` says so).
 */
export function controlFor(name, p, { maxStuds = Infinity } = {}) {
  const unit = p.unit ?? "";
  const base = {
    name,
    label: humanLabel(name),
    unit,
    help: p.help ?? "",
    visibleWhen: p.visibleWhen ?? null,
    group: p.group ?? UNGROUPED,
    default: p.default,
  };
  switch (p.type) {
    case "grid2": {
      const studs = unit === "stud";
      const axes = [0, 1].map((i) => {
        const min = Math.max(1, Math.ceil(pick(p.min, i, 1)));
        const docMax = Math.floor(pick(p.max, i, studs ? maxStuds : 1000));
        const max = studs ? Math.min(docMax, maxStuds) : docMax;
        return { label: GRID_AXES[i], min, max, capped: max < docMax };
      });
      return { ...base, kind: "grid2", axes, default: [0, 1].map((i) => pick(p.default, i, axes[i].min)) };
    }
    case "enum":
      return { ...base, kind: "choice", options: (p.options ?? []).map(option) };
    case "length":
    case "number":
    case "int": {
      const min = pick(p.min, 0, undefined);
      const max = pick(p.max, 0, undefined);
      const step = typeof p.step === "number" && p.step > 0 ? p.step : p.type === "int" ? 1 : "any";
      // A slider needs both ends and a step; otherwise a number field.
      const kind = min !== undefined && max !== undefined && step !== "any" ? "range" : "number";
      return { ...base, kind, min, max, step };
    }
    case "svg":
      return { ...base, kind: "file", accept: ".svg,image/svg+xml" };
    case "bool":
      return { ...base, kind: "checkbox" };
    case "color":
      return { ...base, kind: "color" };
    default:
      return { ...base, kind: "text" };
  }
}

/**
 * Every control of a part document, grouped in document order of first use:
 * { controls: [...], groups: [{ name, controls }] }.
 */
export function controlsFor(doc, options = {}) {
  const controls = Object.entries(doc?.params ?? {}).map(([name, p]) => controlFor(name, p, options));
  const groups = [];
  for (const c of controls) {
    let g = groups.find((x) => x.name === c.group);
    if (!g) groups.push((g = { name: c.group, controls: [] }));
    g.controls.push(c);
  }
  return { controls, groups };
}

/** The document's defaults, by parameter name. */
export function defaultValues({ controls }) {
  return Object.fromEntries(controls.map((c) => [c.name, Array.isArray(c.default) ? [...c.default] : c.default]));
}

// ---------------------------------------------------------------- visibleWhen

const TOKEN = /\s*(?:(\d+(?:\.\d+)?)|'([^']*)'|"([^"]*)"|(\$?[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)|(==|!=|<=|>=|&&|\|\||[<>!()]))/y;

function tokenize(text) {
  const out = [];
  TOKEN.lastIndex = 0;
  let at = 0;
  while (at < text.length) {
    if (/^\s*$/.test(text.slice(at))) break;
    TOKEN.lastIndex = at;
    const m = TOKEN.exec(text);
    if (!m) throw new SyntaxError(`cannot read ${JSON.stringify(text.slice(at))}`);
    at = TOKEN.lastIndex;
    if (m[1] !== undefined) out.push({ value: Number(m[1]) });
    else if (m[2] !== undefined || m[3] !== undefined) out.push({ value: m[2] ?? m[3] });
    else if (m[4] !== undefined) out.push(m[4] === "true" || m[4] === "false" ? { value: m[4] === "true" } : { name: m[4] });
    else out.push({ op: m[5] });
  }
  return out;
}

/**
 * Whether a parameter with `visibleWhen` = `expr` is shown for `values`: the
 * comparisons and logic of the core's expression language (crates/openbrick-part
 * src/eval/expr.rs: ==, !=, <, <=, >, >=, &&, ||, !, parentheses, numbers,
 * quoted strings, true, false and parameter names with or without `$`).
 * Arithmetic is not read here; such an expression throws, and the page then
 * shows the control and uses the core's answer from the next evaluation
 * (`params.visible`), which is always the one that counts.
 */
export function isVisible(expr, values) {
  if (expr === null || expr === undefined || expr === "") return true;
  const tokens = tokenize(String(expr));
  let i = 0;
  const peek = () => tokens[i];
  const take = (op) => {
    if (peek()?.op === op) {
      i++;
      return true;
    }
    return false;
  };
  const lookup = (name) => {
    const key = name.startsWith("$") ? name.slice(1) : name;
    if (!(key in values)) throw new ReferenceError(`unknown parameter ${JSON.stringify(key)}`);
    return values[key];
  };
  function primary() {
    const t = tokens[i++];
    if (!t) throw new SyntaxError("the expression ends early");
    if (t.op === "(") {
      const v = or();
      if (!take(")")) throw new SyntaxError("missing )");
      return v;
    }
    if (t.op === "!") return !truthy(primary());
    if ("value" in t) return t.value;
    if ("name" in t) return lookup(t.name);
    throw new SyntaxError(`unexpected ${t.op}`);
  }
  function truthy(v) {
    if (typeof v !== "boolean") throw new TypeError(`${JSON.stringify(v)} is not true or false`);
    return v;
  }
  function compare() {
    const a = primary();
    const t = peek();
    if (!t?.op || !["==", "!=", "<", "<=", ">", ">="].includes(t.op)) return a;
    i++;
    const b = primary();
    if (typeof a !== typeof b) throw new TypeError(`cannot compare ${JSON.stringify(a)} with ${JSON.stringify(b)}`);
    switch (t.op) {
      case "==":
        return a === b;
      case "!=":
        return a !== b;
      case "<":
        return a < b;
      case "<=":
        return a <= b;
      case ">":
        return a > b;
      default:
        return a >= b;
    }
  }
  function and() {
    let v = compare();
    while (take("&&")) {
      const rhs = compare(); // always parsed, so the tokens stay in step
      v = truthy(v) && truthy(rhs);
    }
    return v;
  }
  function or() {
    let v = and();
    while (take("||")) {
      const rhs = and();
      v = truthy(v) || truthy(rhs);
    }
    return v;
  }
  const v = or();
  if (i !== tokens.length) throw new SyntaxError("unexpected text after the expression");
  return truthy(v);
}

/**
 * Which controls are shown for `values`: name -> boolean. An expression this
 * page cannot read shows its control (`errors` lists why).
 */
export function visibility({ controls }, values) {
  const visible = {};
  const errors = [];
  for (const c of controls) {
    try {
      visible[c.name] = isVisible(c.visibleWhen, values);
    } catch (err) {
      visible[c.name] = true;
      errors.push(`${c.name}: visibleWhen ${JSON.stringify(c.visibleWhen)}: ${err.message}`);
    }
  }
  return { visible, errors };
}

/**
 * The parameter overrides sent to the core: the value of every shown control.
 * A hidden parameter is left out, so it takes the document's default (the logo
 * tile hides reliefHeight for an inlay, whose depth the document fixes).
 */
export function paramOverrides(spec, values, visible = visibility(spec, values).visible) {
  const out = {};
  for (const c of spec.controls) {
    if (visible[c.name] === false) continue;
    if (values[c.name] !== undefined) out[c.name] = values[c.name];
  }
  return out;
}

// ---------------------------------------------------------------- downloads

/** Characters kept in file names; everything else becomes "-". */
function safe(text) {
  return String(text).replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "part";
}

/**
 * Whether parameter `name` is shown: the core's answer in `visible` (the
 * `params.visible` of an evaluation) when it has one, else this page's reading
 * of `visibleWhen` (shown when the page cannot read it).
 */
function shown(name, p, values, visible) {
  if (typeof visible?.[name] === "boolean") return visible[name];
  try {
    return isVisible(p?.visibleWhen, values ?? {});
  } catch {
    return true;
  }
}

/**
 * The download name: {id}-{w}x{h}-{relief}[-{inlayStyle}][-{sample}].{ext},
 * from the document id, the first two-axis grid parameter, the `relief`
 * parameter, while it is shown the `inlayStyle` parameter (each left out when
 * the document has none) and the artwork sample's `suffix` when the artwork is
 * a bundled sample (none for the user's own SVG). For example
 * ndn.logo-tile-2x4-emboss-neuraldeep.3mf and
 * ndn.logo-tile-2x4-inlay-parts-neuraldeep.3mf, so the filament-swap and the
 * multi-material inlay never share a name, and neither do two samples.
 * `visible` is the core's `params.visible`; without it the page reads
 * `visibleWhen` itself.
 */
export function downloadName(doc, values, ext, visible = null, sample = null) {
  const parts = [safe(doc?.id ?? "part")];
  const grid = Object.entries(doc?.params ?? {}).find(([, p]) => p.type === "grid2");
  if (grid && Array.isArray(values?.[grid[0]])) parts.push(values[grid[0]].map((n) => safe(n)).join("x"));
  if (doc?.params?.relief && values?.relief !== undefined) parts.push(safe(values.relief));
  const style = doc?.params?.inlayStyle;
  if (style && values?.inlayStyle !== undefined && shown("inlayStyle", style, values, visible)) parts.push(safe(values.inlayStyle));
  if (typeof sample === "string" && sample !== "") parts.push(safe(sample));
  return `${parts.join("-")}.${safe(ext)}`;
}

/** The coupon's download name: the core's file stem plus the extension. */
export function couponDownloadName(fileStem, ext) {
  return `${safe(fileStem)}.${safe(ext)}`;
}

const bytesFmt = new Intl.NumberFormat("en-US");

/**
 * Null when a file of `byteLength` bytes may be sent to the core, else a
 * friendly message naming the core's limit (`maxBytes`, from max_asset_bytes()).
 */
export function assetSizeError(name, byteLength, maxBytes) {
  if (byteLength <= maxBytes) return null;
  const mb = (n) => `${(n / (1024 * 1024)).toFixed(1)} MiB`;
  return (
    `“${name}” is ${mb(byteLength)} (${bytesFmt.format(byteLength)} bytes), and artwork can be at most ` +
    `${mb(maxBytes)} (${bytesFmt.format(maxBytes)} bytes). Simplify the drawing, or save it again without embedded images or fonts.`
  );
}

/** True when the bytes look like SVG text (an XML or <svg> start), for a friendly early message. */
export function looksLikeSvg(bytes) {
  const head = new TextDecoder("utf-8", { fatal: false }).decode(bytes.subarray(0, Math.min(bytes.length, 4096)));
  return /<svg[\s>]/i.test(head) || /^\s*(<\?xml|<!--|<!DOCTYPE svg)/i.test(head);
}
