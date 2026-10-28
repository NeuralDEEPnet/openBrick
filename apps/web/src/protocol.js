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
//   type       fields               result
//   init       module?              info (below); loads the core once per worker
//   info       -                    { protocol, kernel, maxStuds, standard }
//   brick      width, length        a mesh result (see brick() below)
//   checkPart  text                 array of structural error messages
//
// `module` is a compiled WebAssembly.Module, so a replacement worker starts without
// fetching or compiling again. Mesh arrays travel as transferable ArrayBuffers
// (moved, not copied). The worker is the only caller of the core; the page only
// displays what comes back (CLAUDE.md: the UI never computes geometry). Everything
// here is plain JavaScript over a `core` object with the wasm-bindgen exports of
// crates/openbrick-wasm, so the unit tests run it against a fake core and, when
// it is built, the real one.

export const PROTOCOL_VERSION = 1;

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
 * Build a nominal brick with the core and copy the wasm MeshOut into a plain,
 * transferable result. `buildMs` times the kernel call alone.
 *
 * Result: { width, length, positions: Float32Array (x, y, z in mm, kernel frame,
 * +Z up), indices: Uint32Array, vertices, triangles, volumeMm3, fingerprint,
 * canonicalFingerprint, bounds: [min x, y, z, max x, y, z], kernel, buildMs }.
 */
export function brick(core, width, length, now = () => performance.now()) {
  const max = core.max_studs();
  const w = parseStuds(width, max, "width");
  const l = parseStuds(length, max, "length");
  const t0 = now();
  const mesh = core.build_nominal_brick(w, l);
  const buildMs = now() - t0;
  try {
    return {
      width: w,
      length: l,
      // Moved out of the MeshOut, so each array is copied once, out of wasm memory.
      positions: mesh.take_positions(),
      indices: mesh.take_indices(),
      vertices: mesh.vertex_count(),
      triangles: mesh.triangle_count(),
      volumeMm3: mesh.volume_mm3(),
      fingerprint: mesh.fingerprint(),
      canonicalFingerprint: mesh.canonical_fingerprint(),
      bounds: Array.from(mesh.bounds()),
      kernel: mesh.kernel(),
      buildMs,
    };
  } finally {
    mesh.free();
  }
}

/** The buffers of a result to transfer instead of copying. */
export function transferables(result) {
  return result?.positions instanceof Float32Array && result?.indices instanceof Uint32Array
    ? [result.positions.buffer, result.indices.buffer]
    : [];
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
      case "brick":
        result = brick(core, request.width, request.length, now);
        break;
      case "checkPart":
        if (typeof request.text !== "string") throw new TypeError("checkPart needs the document as text");
        result = JSON.parse(core.check_part_json(request.text));
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
