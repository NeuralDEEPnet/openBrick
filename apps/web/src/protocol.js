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
//   evaluatePart   doc, params, process, fit, assets,       a part result (evaluatePart() below)
//                  printer?
//   exportPart     format, doc, params, process, fit,       { format, bytes: Uint8Array, buildMs }
//                  assets, colors, printer?
//   resolveParams  doc, params, process                     { values, visible }
//   buildCoupon    process, layout                          a coupon result (buildCoupon() below)
//   couponLayout   process, layout                          { cells, summary }, without geometry
//   exportCoupon   format, process, layout, colors          { format, bytes: Uint8Array, buildMs }
//   recordFit      input                                    the calibration record (recordFit() below)
//   openPart       text, process                            { ok, id, name, version, license, credit,
//                                                           defaults, presets, unsupported } or { ok: false, error }
//   savePart       doc, params, process                     { text }: the document with params as defaults
//   addPreset      doc, name, params, process               { text }: the document with a preset `name`
//   asmInit        -                                        the assembler catalog (assemblerCatalog() below)
//   asmMeshes      detail, parts?, lods?                    { meshes: [{ index, lod?, positions, indices, triangles }] }:
//                                                           the listed catalog part numbers, else the built-in ones;
//                                                           `lods` (0 to 3) asks for those render levels of each
//   asmLoadPack    bytes                                    { pack, kept, ...state }: an LDraw pack (.obpack) joins the
//                                                           catalog; the model is carried over when it still loads
//   asmUnloadPack  -                                        { kept, ...state }
//   asmNeeds       text                                     { requires, missing, missingCount } of a .obrick document
//   asmSnap        part, rot, ray                           a snap or null (the engine's Snap)
//   asmPick        ray                                      a hit or null
//   asmApply       command, lean?                           { id, ...state }; the engine refuses with a message
//   asmUndo, asmRedo, asmState  lean?                       the state (assemblerState() below); with `lean` what the
//                                                           edit changed, not the model (assemblerEdit() below)
//   asmStats       -                                        { stats, bom, parts, revision } (lean replies leave them out of big models)
//   asmLoad        text, lean?                              the state of the model in a .obrick document, and `notices`
//                                                            (sentences: the model opened with another build of its pack);
//                                                            with `lean` it is read in steps, with { id, progress: { done,
//                                                            total } } messages between them
//   asmLoadRows    rows                                     the model from the table (Int32Array, seven numbers a part)
//   asmCancel      -                                        stops a document being opened in steps
//   asmSave        name                                     { text }
//   asmSample      id                                       { text } of a sample model
//   asmTurn        rot, quarterTurns                        { rot }: the rotation index turned about the vertical axis
//

// `doc` is the part document as JSON text, `params` an object of parameter
// overrides, `process` a bundled process profile id, `fit` a fit profile as JSON
// text ("" for none), `printer` a bundled printer id whose bed rule SIZE.BED
// checks ("" or absent for none: the report then lists SIZE.BED as not
// checked), `assets` [{ name, bytes: Uint8Array }] by the names the
// document uses, `layout` the coupon options (CouponOptions in JSON form), and
// `colors` an object of colour role to "#rrggbb" ({} for the core's colours),
// and `input` the calibration wizard's choices (RecordFitInput in
// crates/openbrick-wasm/src/record.rs: process, pocketSet, printer, material,
// nozzleMm, slicer, date, notes, cells).
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

export const PROTOCOL_VERSION = 5;

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
 * exampleFitProfile (JSON text), logoTilePart (JSON text), parts (the bundled
 * part documents, the default first: [{ id, label, file, text }]),
 * maxPartBytes (the largest part document the core reads), samples (the
 * artwork samples, the default first: [{ id, label, title, suffix, shape,
 * size, notice, noticeUrl, files: [{ asset, tile, svg }] }], see
 * src/samples.js), disclaimer, roleColors (role -> "#rrggbb"), maxAssetBytes,
 * couponChoices ({ pocketSets, defaultPocketSet, defaults }), printers (the
 * bundled printers: [{ id, name, maker, buildVolumeMm: [x, y, z], toolheads,
 * filaments, source: { url, accessed, quote } }], sorted by id) }.
 */
export function bundled(core) {
  return {
    processProfiles: JSON.parse(core.process_profiles_json()),
    exampleFitProfile: core.example_fit_profile_json(),
    logoTilePart: core.logo_tile_part_json(),
    parts: JSON.parse(core.bundled_parts_json()),
    maxPartBytes: core.max_part_bytes(),
    samples: JSON.parse(core.artwork_samples_json()),
    disclaimer: core.disclaimer(),
    roleColors: JSON.parse(core.role_colors_json()),
    maxAssetBytes: core.max_asset_bytes(),
    couponChoices: JSON.parse(core.coupon_choices_json()),
    printers: typeof core.printers_json === "function" ? JSON.parse(core.printers_json()) : [],
  };
}

const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);

/** The part fields of a request, checked, with the params as JSON text. */
export function partFields(request) {
  const { doc, params = {}, process, fit = "", assets = [], printer = "" } = request ?? {};
  if (typeof doc !== "string") throw new TypeError(`${request?.type} needs the part document as JSON text (doc)`);
  if (!isObject(params)) throw new TypeError("params must be an object of parameter values");
  if (typeof process !== "string" || process === "") throw new TypeError("process must be the id of a bundled process profile");
  if (typeof fit !== "string") throw new TypeError("fit must be the fit profile as JSON text, or empty for none");
  if (typeof printer !== "string") throw new TypeError("printer must be the id of a bundled printer, or empty for none");
  if (!Array.isArray(assets) || assets.some((a) => typeof a?.name !== "string" || !(a.bytes instanceof Uint8Array))) {
    throw new TypeError("assets must be a list of { name, bytes: Uint8Array }");
  }
  return { doc, params: JSON.stringify(params), process, fit, assets, printer };
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
  if (!Array.isArray(report.unchecked)) report.unchecked = [];
  return report;
}

/**
 * Evaluate and validate a part with the core. Result: { meshes: [meshData()]
 * (one per colour role, body first), overlay: meshData() of the red region of
 * artwork that does not print as drawn, or null, report: { findings } (blockers
 * first; each finding with its `unit`, see reportWithUnits(); and `unchecked`,
 * the rules not checked with the reason, [] when every rule was), exportAllowed,
 * measurements, warnings, contentHash, kernel, artwork (summary or null),
 * params: { values, visible }, footprint: [x, y] studs, bounds, volumeMm3,
 * massGPla, triangles, faceOnBed, buildMs }.
 */
export function evaluatePart(core, request, now = () => performance.now()) {
  const f = partFields(request);
  const t0 = now();
  // With a printer, rule SIZE.BED checks its bed; without, the plain call.
  const out = withAssets(core, f.assets, (set) =>
    f.printer ? core.evaluate_part_on_printer(f.doc, f.params, f.process, f.fit, set, f.printer) : core.evaluate_part(f.doc, f.params, f.process, f.fit, set),
  );
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
  const bytes = withAssets(core, f.assets, (set) =>
    f.printer
      ? core.export_part_on_printer(format, f.doc, f.params, f.process, f.fit, set, colors, f.printer)
      : core.export_part(format, f.doc, f.params, f.process, f.fit, set, colors),
  );
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

/**
 * Record the calibration wizard's choices as a fit profile with the core's
 * record_fit, which writes the file `openbrick calibrate record` writes for the
 * same choices. Result: { ok: true, fitProfile, text (the file, byte for byte),
 * fileName, offsets: [{ cell, chosenMm, nominalMm, offsetField, offsetMm }] },
 * or { ok: false, error: { field, message } } when the core refuses a choice.
 */
export function recordFit(core, request) {
  const input = request?.input;
  if (!isObject(input)) throw new TypeError("recordFit needs the wizard's choices as an object (input)");
  return JSON.parse(core.record_fit(JSON.stringify(input)));
}

/**
 * Check a part document the user opened, with the core's open_part_json:
 * { ok: true, id, name, version, license, credit, defaults, presets: [{ name,
 * params, error }], unsupported: [{ feature, op, condition }] }, or { ok: false,
 * error } for a file that is not a valid part document (an answer, not a
 * failed request). `defaults` and the `params` of a preset that fits are
 * normalised as the evaluator reads them ("0.6mm" is 0.6).
 */
export function openPart(core, request) {
  if (typeof request?.text !== "string") throw new TypeError("openPart needs the document as text");
  return JSON.parse(core.open_part_json(request.text, processId(request)));
}

/**
 * The part document with the parameter values as its defaults (savePart), or
 * with a preset `name` holding them (addPreset): { text }, in the core's saved
 * layout. The core refuses values its evaluator refuses.
 */
export function savePart(core, request) {
  const f = partFields(request);
  return { text: core.save_part_json(f.doc, f.params, f.process) };
}

export function addPreset(core, request) {
  const f = partFields(request);
  if (typeof request.name !== "string") throw new TypeError("addPreset needs the preset's name");
  return { text: core.add_preset_json(f.doc, request.name, f.params, f.process) };
}

// ---------------------------------------------------------------- assembler

/** The one assembler of a core (a model and its history); a replacement worker starts with an empty one. */
const assemblers = new WeakMap();

function assembler(core) {
  let a = assemblers.get(core);
  if (!a) {
    a = new core.Assembler();
    assemblers.set(core, a);
  }
  return a;
}

const isRay = (r) => Array.isArray(r?.origin) && r.origin.length === 3 && Array.isArray(r?.dir) && r.dir.length === 3;

function rayArgs(ray) {
  if (!isRay(ray)) throw new TypeError("a pointer ray is { origin: [x, y, z], dir: [x, y, z] } in LDU");
  return [...ray.origin, ...ray.dir];
}

/** The catalog, palette, rotation matrices and samples (assembler_catalog_json). */
export function assemblerCatalog(core) {
  assembler(core);
  return JSON.parse(core.assembler_catalog_json());
}

/**
 * The model on screen: { instances: Int32Array (seven numbers per part: id,
 * catalog part number, x, y, z in LDU, rotation index, colour code), stats
 * ({ parts, connections, components, grounded, floating }), bom, canUndo,
 * canRedo, revision, text (the model as .obrick text) }.
 */
export function assemblerState(core) {
  const a = assembler(core);
  return {
    instances: a.instances(),
    stats: JSON.parse(a.stats()),
    bom: JSON.parse(a.bom()),
    canUndo: a.can_undo(),
    canRedo: a.can_redo(),
    revision: a.revision(),
    // The model as .obrick text, for the page's copy (src/model-keeper.js): a replaced worker starts empty.
    text: a.save("Autosave"),
  };
}

/**
 * Models up to this many parts get their statistics and bill of materials in every edit's reply (they cost
 * milliseconds); bigger ones are asked for with asmStats when editing pauses, because the statistics are
 * counted from the whole model again after a removal (about 30 ms for 100,000 parts).
 */
export const STATS_INLINE_LIMIT = 5000;

/** The parts a delta's rows are for, in the unit of the core's table. */
function leanSummary(a) {
  const parts = a.len();
  const out = { parts, canUndo: a.can_undo(), canRedo: a.can_redo(), revision: a.revision(), depth: a.history_depth() };
  if (parts <= STATS_INLINE_LIMIT) {
    out.stats = JSON.parse(a.stats());
    out.bom = JSON.parse(a.bom());
  }
  return out;
}

/** What an edit changed, without the model: { delta (Int32Array: mode, rows), parts, canUndo, canRedo, revision, stats?, bom? }. */
export function assemblerEdit(core) {
  const a = assembler(core);
  return { delta: a.delta(), ...leanSummary(a) };
}

/** The whole table without the model text: { instances (Int32Array), parts, canUndo, canRedo, revision, stats?, bom? }. */
export function assemblerTable(core) {
  const a = assembler(core);
  a.delta(); // a whole table is being sent: forget the change list
  return { instances: a.instances(), ...leanSummary(a) };
}

/** The statistics and bill of materials of the model as it is: { stats, bom, parts, revision }. */
export function assemblerStats(core) {
  const a = assembler(core);
  return { stats: JSON.parse(a.stats()), bom: JSON.parse(a.bom()), parts: a.len(), revision: a.revision() };
}

/** Parts the core reads per step when it opens a document: about 25 ms of work, so a cancel or a snap is answered between steps. */
export const LOAD_STEP = 4000;

/**
 * Open a document in steps, telling the page how far it is after each (`post({ id, progress: { done, total } })`)
 * and giving other requests (a cancel, a snap) a turn in between. Resolves with the reply to send.
 */
export async function assemblerLoadSteps(core, request, id, post, yieldToEvents) {
  const a = assembler(core);
  const total = a.load_begin(request.text);
  post({ id, progress: { done: 0, total } }, []);
  let done = 0;
  try {
    while (done < total) {
      done = a.load_step(LOAD_STEP);
      post({ id, progress: { done, total } }, []);
      await yieldToEvents();
    }
  } catch (err) {
    a.load_cancel();
    // A cancel ends the loop from outside: the next step finds no document being loaded.
    if (/no document is being loaded/.test(errorMessage(err))) throw new Error("opening was cancelled");
    throw err;
  }
  const notices = JSON.parse(a.load_finish());
  return { ...assemblerTable(core), notices };
}

/**
 * The render meshes of catalog parts: the baseplate always at the cheap level, the others at `detail`
 * or not. `request.parts` lists the part numbers wanted (at most MAX_MESHES_PER_REQUEST); without it
 * the answer is the built-in parts, so a loaded pack's hundreds of parts are never all built at once.
 */
export function assemblerMeshes(core, request) {
  const catalog = assemblerCatalog(core);
  let wanted;
  if (Array.isArray(request?.parts)) {
    if (request.parts.length > MAX_MESHES_PER_REQUEST) throw new RangeError(`at most ${MAX_MESHES_PER_REQUEST} meshes per request`);
    wanted = request.parts.map((i) => {
      if (!Number.isInteger(i) || i < 0 || i >= catalog.parts.length) throw new RangeError(`there is no catalog part number ${i}`);
      return i;
    });
  } else {
    wanted = catalog.parts.flatMap((p, i) => (p.origin === "ldraw" ? [] : [i]));
  }
  // `request.lods` asks for render levels 0 to 3 (0 full studs, 1 cheap, 2 solid underside, 3 hull) of each
  // part: one mesh per part and level, with `lod` on it. The baseplate is never built at level 0 (it takes a
  // second); its level 0 is level 1.
  if (Array.isArray(request?.lods)) {
    const lods = request.lods.map((l) => {
      if (!Number.isInteger(l) || l < 0 || l > 3) throw new RangeError(`render level ${l} is not one of 0 to 3`);
      return l;
    });
    if (wanted.length * lods.length > MAX_MESHES_PER_REQUEST * 4) throw new RangeError("too many meshes in one request");
    const meshes = wanted.flatMap((index) => {
      const part = catalog.parts[index];
      return lods.map((lod) => {
        const mesh = core.assembler_part_lod(index, part.base ? Math.max(1, lod) : lod);
        try {
          return { index, lod, id: part.id, positions: mesh.take_positions(), indices: mesh.take_indices(), triangles: mesh.triangle_count() };
        } finally {
          mesh.free();
        }
      });
    });
    return { meshes };
  }
  const meshes = wanted.map((index) => {
    const part = catalog.parts[index];
    const mesh = core.assembler_part_mesh(index, part.base ? false : request?.detail !== false);
    try {
      return { index, id: part.id, positions: mesh.take_positions(), indices: mesh.take_indices(), triangles: mesh.triangle_count() };
    } finally {
      mesh.free();
    }
  });
  return { meshes };
}

/** The most meshes one request may ask for. */
export const MAX_MESHES_PER_REQUEST = 256;

/**
 * Put the model into a new assembler, which has the catalog of the core as it is now (an assembler keeps the
 * catalog it was made with). Resolves { kept }: false when the model's text no longer loads (it uses parts the
 * catalog does not have), in which case the new assembler is empty.
 */
function renewAssembler(core) {
  const old = assemblers.get(core);
  const text = old ? old.save("Autosave") : null;
  assemblers.delete(core);
  const fresh = assembler(core);
  if (text === null) return { kept: true };
  try {
    fresh.load(text);
    return { kept: true };
  } catch {
    return { kept: false };
  }
}

/** Load an LDraw pack: its parts join the catalog after the built-in ones. */
export function assemblerLoadPack(core, request) {
  if (!(request?.bytes instanceof Uint8Array)) throw new TypeError("asmLoadPack needs the pack file as bytes");
  core.assembler_load_pack(request.bytes);
  const { kept } = renewAssembler(core);
  const catalog = assemblerCatalog(core);
  return { pack: catalog.pack, catalog, kept, ...(request.lean ? assemblerTable(core) : assemblerState(core)) };
}

export function assemblerUnloadPack(core, request) {
  core.assembler_unload_pack();
  const { kept } = renewAssembler(core);
  return { kept, catalog: assemblerCatalog(core), ...(request?.lean ? assemblerTable(core) : assemblerState(core)) };
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
  if (result?.instances instanceof Int32Array) out.push(result.instances.buffer);
  if (result?.delta instanceof Int32Array) out.push(result.delta.buffer);
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
      case "recordFit":
        result = recordFit(core, request);
        break;
      case "openPart":
        result = openPart(core, request);
        break;
      case "savePart":
        result = savePart(core, request);
        break;
      case "addPreset":
        result = addPreset(core, request);
        break;
      case "asmInit":
        result = assemblerCatalog(core);
        break;
      case "asmMeshes":
        result = assemblerMeshes(core, request);
        break;
      case "asmLoadPack":
        result = assemblerLoadPack(core, request);
        break;
      case "asmUnloadPack":
        result = assemblerUnloadPack(core, request);
        break;
      case "asmNeeds":
        if (typeof request.text !== "string") throw new TypeError("asmNeeds needs the .obrick document as text");
        result = JSON.parse(core.assembler_model_needs(request.text));
        break;
      case "asmSnap": {
        if (typeof request.part !== "string") throw new TypeError("asmSnap needs the catalog id of the part");
        result = JSON.parse(assembler(core).snap(request.part, request.rot ?? 0, ...rayArgs(request.ray)));
        break;
      }
      case "asmPick":
        result = JSON.parse(assembler(core).pick(...rayArgs(request.ray)));
        break;
      case "asmApply": {
        const { id } = JSON.parse(assembler(core).apply(JSON.stringify(request.command)));
        // `lean`: what changed, not the model (a big model is not sent back with every edit).
        result = { id, ...(request.lean ? assemblerEdit(core) : assemblerState(core)) };
        break;
      }
      case "asmUndo":
        assembler(core).undo();
        result = request.lean ? assemblerEdit(core) : assemblerState(core);
        break;
      case "asmRedo":
        assembler(core).redo();
        result = request.lean ? assemblerEdit(core) : assemblerState(core);
        break;
      case "asmState":
        result = request.lean ? assemblerTable(core) : assemblerState(core);
        break;
      case "asmStats":
        result = assemblerStats(core);
        break;
      case "asmLoadRows":
        if (!(request.rows instanceof Int32Array)) throw new TypeError("asmLoadRows needs the table as an Int32Array");
        assembler(core).load_rows(request.rows);
        result = { ...assemblerTable(core), notices: [] };
        break;
      case "asmCancel":
        assembler(core).load_cancel();
        result = {};
        break;
      case "asmLoad":
        if (typeof request.text !== "string") throw new TypeError("asmLoad needs the .obrick document as text");
        {
          // `notices`: sentences about a model that opened with another build of the pack it was made with.
          const notices = JSON.parse(assembler(core).load(request.text));
          result = { ...assemblerState(core), notices };
        }
        break;
      case "asmSave":
        result = { text: assembler(core).save(String(request.name ?? "Model")) };
        break;
      case "asmSample":
        result = { text: core.assembler_sample(String(request.id)) };
        break;
      case "asmTurn":
        result = { rot: core.assembler_turn_rotation(request.rot ?? 0, request.quarterTurns ?? 1) };
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
export function createHandler(load, post, now, yieldToEvents = () => new Promise((resolve) => setTimeout(resolve, 0))) {
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
    if (request?.type === "asmLoad" && request.lean) {
      // Opened in steps: progress messages go out between them and other requests get a turn.
      let reply;
      let transfer = [];
      try {
        if (typeof request.text !== "string") throw new TypeError("asmLoad needs the .obrick document as text");
        const result = await assemblerLoadSteps(core, request, id, post, yieldToEvents);
        reply = { id, ok: true, result };
        transfer = transferables(result);
      } catch (err) {
        reply = isTrap(err)
          ? { id, ok: false, fatal: true, error: `the core stopped with a WebAssembly trap (${errorMessage(err)}), usually a Rust panic; its worker is replaced` }
          : { id, ok: false, error: errorMessage(err) };
      }
      if (reply.fatal) trapped = reply.error;
      post(reply, transfer);
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
