// Message protocol between the page (src/client.js) and the core worker
// (src/worker.js), docs/plan/02 section 3.
//
// The page sends requests { id, type, ...fields }. The worker answers each one with
// { id, ok: true, result } or { id, ok: false, error }, in the order received. An
// error reply carries `fatal: true` when the core trapped (a WebAssembly
// RuntimeError, which is how a Rust panic ends on wasm32): the instance's memory
// may then be inconsistent, so that worker refuses every later request and the
// page replaces it (src/client.js).
//
//   type           fields                                   result
//   init           module?                                  info (below); loads the core once per worker
//   info           -                                        { protocol, kernel, maxStuds, standard }
//   bundled        -                                        the bundled data (bundled() below)
//   brick          width, length                            a mesh result (brick() below)
//   checkPart      text                                     array of structural error messages
//   evaluatePart   doc, params, process, fit, assets        a part result (evaluatePart() below)
//   exportPart     format, doc, params, process, fit,       { format, bytes: Uint8Array, buildMs }
//                  assets, colors
//   resolveParams  doc, params, process                     { values, visible }
//   buildCoupon    process, layout                          a coupon result (buildCoupon() below)
//   couponLayout   process, layout                          { cells, summary }, without geometry
//   exportCoupon   format, process, layout, colors          { format, bytes: Uint8Array, buildMs }
//
// `doc` is the part document as JSON text, `params` an object of parameter
// overrides, `process` a bundled process profile id, `fit` a fit profile as JSON
// text ("" for none), `assets` [{ name, bytes: Uint8Array }] by the names the
// document uses, `layout` the coupon options (CouponOptions in JSON form), and
// `colors` an object of colour role to "#rrggbb" ({} for the core's colours).
//
// `module` is a compiled WebAssembly.Module, so a replacement worker starts without
// fetching or compiling again. Mesh arrays and file bytes in results travel as
// transferable ArrayBuffers (moved, not copied). Asset bytes in requests are
// copied by postMessage instead: the page keeps them, so it can send them again
// to a replacement worker. The worker is the only caller of the core; the page
// only displays what comes back (CLAUDE.md: the UI never computes geometry).
// Everything here is plain JavaScript over a `core` object with the wasm-bindgen
// exports of crates/openbrick-wasm, so the unit tests run it against a fake core
// and, when it is built, the real one.

export const PROTOCOL_VERSION = 2;

/** Requests that build geometry: the client lets a newer one supersede an older one. */
export const BUILD_TYPES = Object.freeze(["brick", "evaluatePart", "buildCoupon"]);

/** Requests that write a file: never superseded, and never cancelled by a restart. */
export const EXPORT_TYPES = Object.freeze(["exportPart", "exportCoupon"]);

/**
 * A whole number of studs from 1 to `max`, from a number or the text of an input.
 * Throws a RangeError whose message names the field and the limits.
 */
export function parseStuds(value, max, what = "studs") {
  const text = typeof value === "string" ? value.trim() : null;
  const n = text === null ? value : text === "" || !/^\d+$/.test(text) ? NaN : Number(text);
  if (!Number.isInteger(n) || n < 1 || n > max) {
    throw new RangeError(`${what} must be a whole number of studs from 1 to ${max}, got ${JSON.stringify(value)}`);
  }
  return n;
}

/**
 * The rows of the Dimensional Standard the page needs for display (lattice grid and
 * placement), read from the core's standard_json(). No dimension is written in
 * JavaScript (CLAUDE.md).
 */
export function standardSummary(json) {
  const standard = typeof json === "string" ? JSON.parse(json) : json;
  const mm = (id) => {
    const row = standard?.features?.find((f) => f.id === id);
    if (typeof row?.nominal_mm !== "number") throw new Error(`the standard has no nominal_mm for ${id}`);
    return row.nominal_mm;
  };
  return {
    version: standard.version,
    pitchMm: mm("module"),
    undersizeMm: mm("undersize_per_side"),
    brickHeightMm: mm("brick_height"),
    studHeightMm: mm("stud_height"),
  };
}

/** What the page needs to know about the core before its first request. */
export function info(core) {
  return {
    protocol: PROTOCOL_VERSION,
    kernel: core.kernel_id(),
    maxStuds: core.max_studs(),
    standard: standardSummary(core.standard_json()),
  };
}

/**
 * A wasm MeshOut as plain, transferable data, freeing the MeshOut: { role,
 * color, positions: Float32Array (x, y, z in mm, kernel frame, +Z up), indices:
 * Uint32Array, vertices, triangles, volumeMm3, fingerprint, canonicalFingerprint,
 * bounds: [min x, y, z, max x, y, z], kernel }. The arrays are moved out of the
 * MeshOut, so each is copied once, out of wasm memory.
 */
export function meshData(mesh) {
  try {
    return {
      role: mesh.role(),
      color: mesh.color(),
      positions: mesh.take_positions(),
      indices: mesh.take_indices(),
      vertices: mesh.vertex_count(),
      triangles: mesh.triangle_count(),
      volumeMm3: mesh.volume_mm3(),
      fingerprint: mesh.fingerprint(),
      canonicalFingerprint: mesh.canonical_fingerprint(),
      bounds: Array.from(mesh.bounds()),
      kernel: mesh.kernel(),
    };
  } finally {
    mesh.free();
  }
}

/**
 * Build a nominal brick with the core and copy the wasm MeshOut into a plain,
 * transferable result. `buildMs` times the kernel call alone.
 *
 * Result: meshData() plus { width, length, buildMs }.
 */
export function brick(core, width, length, now = () => performance.now()) {
  const max = core.max_studs();
  const w = parseStuds(width, max, "width");
  const l = parseStuds(length, max, "length");
  const t0 = now();
  const mesh = core.build_nominal_brick(w, l);
  const buildMs = now() - t0;
  return { width: w, length: l, ...meshData(mesh), buildMs };
}

/**
 * The data the page builds its controls from, read once: { processProfiles,
 * exampleFitProfile (JSON text), logoTilePart (JSON text), sampleLogoSvg (text),
 * disclaimer, roleColors (role -> "#rrggbb"), maxAssetBytes, couponChoices
 * ({ pocketSets, defaultPocketSet, defaults }) }.
 */
export function bundled(core) {
  return {
    processProfiles: JSON.parse(core.process_profiles_json()),
    exampleFitProfile: core.example_fit_profile_json(),
    logoTilePart: core.logo_tile_part_json(),
    sampleLogoSvg: core.sample_logo_svg(),
    disclaimer: core.disclaimer(),
    roleColors: JSON.parse(core.role_colors_json()),
    maxAssetBytes: core.max_asset_bytes(),
    couponChoices: JSON.parse(core.coupon_choices_json()),
  };
}

const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);

/** The part fields of a request, checked, with the params as JSON text. */
export function partFields(request) {
  const { doc, params = {}, process, fit = "", assets = [] } = request ?? {};
  if (typeof doc !== "string") throw new TypeError(`${request?.type} needs the part document as JSON text (doc)`);
  if (!isObject(params)) throw new TypeError("params must be an object of parameter values");
  if (typeof process !== "string" || process === "") throw new TypeError("process must be the id of a bundled process profile");
  if (typeof fit !== "string") throw new TypeError("fit must be the fit profile as JSON text, or empty for none");
  if (!Array.isArray(assets) || assets.some((a) => typeof a?.name !== "string" || !(a.bytes instanceof Uint8Array))) {
    throw new TypeError("assets must be a list of { name, bytes: Uint8Array }");
  }
  return { doc, params: JSON.stringify(params), process, fit, assets };
}

/** Colours as the core takes them: JSON text of role -> "#rrggbb", "" for its own. */
function colorsJson(colors) {
  if (colors === undefined || colors === null) return "";
  if (!isObject(colors)) throw new TypeError('colors must be an object of colour role to "#rrggbb"');
  return Object.keys(colors).length ? JSON.stringify(colors) : "";
}

/** The coupon layout as the core takes it: JSON text, "" for the defaults. */
function layoutJson(layout) {
  if (layout === undefined || layout === null) return "";
  if (!isObject(layout)) throw new TypeError("layout must be an object of coupon options");
  return Object.keys(layout).length ? JSON.stringify(layout) : "";
}

function processId(request) {
  const process = request?.process;
  if (typeof process !== "string" || process === "") throw new TypeError("process must be the id of a bundled process profile");
  return process;
}

function exportFormat(format) {
  if (format !== "3mf" && format !== "stl") throw new TypeError(`format must be "3mf" or "stl", got ${JSON.stringify(format)}`);
  return format;
}

/** Run `fn` with a core AssetSet holding `assets`, and free it afterwards. */
function withAssets(core, assets, fn) {
  const set = new core.AssetSet();
  try {
    for (const a of assets) set.add(a.name, a.bytes);
    return fn(set);
  } finally {
    set.free();
  }
}

/**
 * The validation report with each finding's `unit` (of its value and threshold:
 * "mm", "%", "mm³" or "" for a count or no numbers) set from the core's
 * report_units_json(), which lists them in report order. The page shows that
 * unit and never reads one from the wording of a message.
 */
export function reportWithUnits(reportJson, unitsJson) {
  const report = JSON.parse(reportJson);
  const units = JSON.parse(unitsJson);
  const findings = report?.findings ?? [];
  if (!Array.isArray(units) || units.length !== findings.length) {
    throw new Error(`the core gave ${Array.isArray(units) ? units.length : "no"} units for ${findings.length} findings`);
  }
  findings.forEach((f, i) => {
    f.unit = units[i];
  });
  return report;
}

/**
 * Evaluate and validate a part with the core. Result: { meshes: [meshData()]
 * (one per colour role, body first), overlay: meshData() of the red region of
 * artwork that does not print as drawn, or null, report: { findings } (blockers
 * first; each finding with its `unit`, see reportWithUnits()), exportAllowed,
 * measurements, warnings, contentHash, kernel, artwork (summary or null),
 * params: { values, visible }, footprint: [x, y] studs, bounds, volumeMm3,
 * massGPla, triangles, faceOnBed, buildMs }.
 */
export function evaluatePart(core, request, now = () => performance.now()) {
  const f = partFields(request);
  const t0 = now();
  const out = withAssets(core, f.assets, (set) => core.evaluate_part(f.doc, f.params, f.process, f.fit, set));
  const buildMs = now() - t0;
  try {
    const meshes = [];
    for (let i = 0; i < out.mesh_count(); i++) {
      const mesh = out.take_mesh(i);
      if (mesh) meshes.push(meshData(mesh));
    }
    const removed = out.take_removed();
    return {
      meshes,
      overlay: removed ? meshData(removed) : null,
      report: reportWithUnits(out.report_json(), out.report_units_json()),
      exportAllowed: out.export_allowed(),
      measurements: JSON.parse(out.measurements_json()),
      warnings: JSON.parse(out.warnings_json()),
      contentHash: out.content_hash(),
      kernel: out.kernel(),
      artwork: JSON.parse(out.artwork_json()),
      params: JSON.parse(out.params_json()),
      footprint: Array.from(out.footprint_studs()),
      bounds: Array.from(out.bounds()),
      volumeMm3: out.volume_mm3(),
      massGPla: out.mass_g_pla(),
      triangles: out.triangle_count(),
      faceOnBed: out.face_on_bed(),
      buildMs,
    };
  } finally {
    out.free();
  }
}

/** Write a part as a 3MF or STL with the core (it refuses while a blocker stands). */
export function exportPart(core, request, now = () => performance.now()) {
  const format = exportFormat(request?.format);
  const f = partFields(request);
  const colors = colorsJson(request.colors);
  const t0 = now();
  const bytes = withAssets(core, f.assets, (set) => core.export_part(format, f.doc, f.params, f.process, f.fit, set, colors));
  return { format, bytes, buildMs: now() - t0 };
}

/**
 * Build the calibration coupon. Result: { meshes: [meshData()], cells, summary,
 * estimate, warnings, contentHash, kernel, fileStem, triangles, buildMs }.
 */
export function buildCoupon(core, request, now = () => performance.now()) {
  const process = processId(request);
  const layout = layoutJson(request.layout);
  const t0 = now();
  const out = core.build_coupon(process, layout);
  const buildMs = now() - t0;
  try {
    const mesh = out.take_mesh();
    return {
      meshes: mesh ? [meshData(mesh)] : [],
      cells: JSON.parse(out.cells_json()),
      summary: JSON.parse(out.summary_json()),
      estimate: JSON.parse(out.estimate_json()),
      warnings: JSON.parse(out.warnings_json()),
      contentHash: out.content_hash(),
      kernel: out.kernel(),
      fileStem: out.file_stem(),
      triangles: out.triangle_count(),
      buildMs,
    };
  } finally {
    out.free();
  }
}

/** Write the coupon as a 3MF or STL. */
export function exportCoupon(core, request, now = () => performance.now()) {
  const format = exportFormat(request?.format);
  const process = processId(request);
  const t0 = now();
  const bytes = core.export_coupon(format, process, layoutJson(request.layout), colorsJson(request.colors));
  return { format, bytes, buildMs: now() - t0 };
}

/** The buffers of a result to transfer instead of copying: mesh arrays and file bytes. */
export function transferables(result) {
  const out = [];
  const mesh = (m) => {
    if (m?.positions instanceof Float32Array && m?.indices instanceof Uint32Array) out.push(m.positions.buffer, m.indices.buffer);
  };
  mesh(result);
  if (Array.isArray(result?.meshes)) result.meshes.forEach(mesh);
  mesh(result?.overlay);
  if (result?.bytes instanceof Uint8Array) out.push(result.bytes.buffer);
  // A buffer listed twice cannot be transferred; keep each once.
  return [...new Set(out)];
}

/** True for a WebAssembly trap: a Rust panic (panic=abort on wasm32), or a stack overflow. */
export function isTrap(err) {
  return typeof WebAssembly !== "undefined" && err instanceof WebAssembly.RuntimeError;
}

/**
 * Answer one request (any type but init) with a loaded core. Returns the reply and
 * its transfer list; a failure becomes an { ok: false } reply, never an exception,
 * so one bad request cannot take the worker down. A trap is marked `fatal`.
 */
export function dispatch(core, request, now) {
  const id = request?.id;
  try {
    let result;
    switch (request?.type) {
      case "info":
        result = info(core);
        break;
      case "bundled":
        result = bundled(core);
        break;
      case "brick":
        result = brick(core, request.width, request.length, now);
        break;
      case "checkPart":
        if (typeof request.text !== "string") throw new TypeError("checkPart needs the document as text");
        result = JSON.parse(core.check_part_json(request.text));
        break;
      case "evaluatePart":
        result = evaluatePart(core, request, now);
        break;
      case "exportPart":
        result = exportPart(core, request, now);
        break;
      case "resolveParams": {
        const f = partFields(request);
        result = JSON.parse(core.resolve_params_json(f.doc, f.params, f.process));
        break;
      }
      case "buildCoupon":
        result = buildCoupon(core, request, now);
        break;
      case "couponLayout":
        result = JSON.parse(core.coupon_layout_json(processId(request), layoutJson(request.layout)));
        break;
      case "exportCoupon":
        result = exportCoupon(core, request, now);
        break;
      default:
        throw new Error(`unknown request type ${JSON.stringify(request?.type)}`);
    }
    return { reply: { id, ok: true, result }, transfer: transferables(result) };
  } catch (err) {
    if (isTrap(err)) {
      const error = `the core stopped with a WebAssembly trap (${errorMessage(err)}), usually a Rust panic; its worker is replaced`;
      return { reply: { id, ok: false, fatal: true, error }, transfer: [] };
    }
    return { reply: { id, ok: false, error: errorMessage(err) }, transfer: [] };
  }
}

/**
 * The core worker's message handler (src/worker.js), kept here so the unit tests
 * drive it without a Worker. `load(module)` instantiates the core and resolves with
 * its exports; `post(reply, transfer)` sends a reply to the page.
 *
 * init loads the core once and answers with info(). Every other request waits for
 * the load and goes to dispatch(). After a fatal (trap) reply the handler never
 * calls the core again: it answers everything with a fatal error, so no request
 * runs on an instance whose memory may be broken.
 */
export function createHandler(load, post, now) {
  /** Resolves with the core once it is instantiated; set by the first init request. */
  let loaded = null;
  /** The reply message of the trap that ended this core, if one did. */
  let trapped = null;
  const refuse = (id) => post({ id, ok: false, fatal: true, error: `the core is unusable after an earlier trap: ${trapped}` }, []);
  return async (request) => {
    const id = request?.id;
    if (trapped) {
      refuse(id);
      return;
    }
    if (request?.type === "init") {
      try {
        loaded ??= load(request.module);
        post({ id, ok: true, result: info(await loaded) }, []);
      } catch (err) {
        loaded = null;
        post({ id, ok: false, error: `could not load the core: ${errorMessage(err)}` }, []);
      }
      return;
    }
    if (!loaded) {
      post({ id, ok: false, error: "the core is not loaded yet (send init first)" }, []);
      return;
    }
    let core;
    try {
      core = await loaded;
    } catch {
      post({ id, ok: false, error: "the core failed to load" }, []);
      return;
    }
    // Requests that waited for the load resume in order; one of them may have trapped.
    if (trapped) {
      refuse(id);
      return;
    }
    const { reply, transfer } = dispatch(core, request, now);
    if (reply.fatal) trapped = reply.error;
    post(reply, transfer);
  };
}

/** The message of anything thrown (wasm-bindgen turns a Rust CoreError into an Error). */
export function errorMessage(err) {
  return err instanceof Error ? err.message : String(err);
}
