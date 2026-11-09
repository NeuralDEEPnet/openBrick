// The page's side of the core worker (src/worker.js, protocol in src/protocol.js).
//
// Requests are matched to replies by id. Builds (a brick, a part evaluation, the
// coupon) are "latest wins": a new build request of any kind supersedes the one
// waiting to be sent and the one running, and superseded requests resolve with
// null. A running WebAssembly call cannot be interrupted, so when the superseded
// build has run for `restartAfterMs` (now or later) the client terminates the
// worker and starts a fresh one from the same compiled module; a large part
// (a 16x16 brick takes tens of seconds, Q01 finding 2 in
// docs/spikes/q01-kernel-conformance.md) then never holds up the next request.
//
// Exports (exportPart, exportCoupon) are never superseded: a build does not
// replace them, and while one is owed the client does not restart the worker to
// cancel a build (that would start the export over). It cancels once the export
// has its reply, if the superseded build is still running then.
//
// A reply marked `fatal` means the core trapped (src/protocol.js). The request it
// answers fails, and the client replaces the worker at once and sends the fresh one
// everything the old one still owed (exports included), so no later request runs
// on a broken instance.
//
// The worker factory, clock and timers are injected so the unit tests drive the
// client with fake workers and a fake clock.

import { BUILD_TYPES, EXPORT_TYPES } from "./protocol.js";

/**
 * Requests whose answer depends on the model in the worker. A replaced worker starts with an empty model, so one of
 * these owed by the old worker is not sent again (it would be answered from an empty model, and an edit's delta
 * would patch the page's table with rows from it): it fails with a message, and the page, which puts its copy of
 * the model back into the new worker before its next request (src/model-keeper.js), asks again.
 */
export const MODEL_TYPES = ["asmSnap", "asmPick", "asmApply", "asmUndo", "asmRedo", "asmState", "asmStats", "asmLoad", "asmLoadRows", "asmSave", "asmCancel", "asmLoadPack", "asmUnloadPack"];

export class CoreError extends Error {
  constructor(message) {
    super(message);
    this.name = "CoreError";
  }
}

export class CoreClient {
  #createWorker;
  #module;
  #now;
  #restartAfterMs;
  #setTimeout;
  #clearTimeout;
  #worker = null;
  #nextId = 1;
  /** id -> { request, resolve, reject } for every request the current worker owes a reply. */
  #pending = new Map();
  #info = null;
  /** The build the worker is running: { fields, resolve, reject, startedAt, superseded, timer }. */
  #inflight = null;
  /** The newest build request, not yet sent: { fields, resolve, reject }. */
  #queued = null;
  #failure = null;
  /** How many times the worker was replaced: to cancel a build, or after a trap. */
  restarts = 0;
  /** How many of those replacements followed a trap. */
  traps = 0;

  /**
   * @param {object} options
   * @param {() => Worker} options.createWorker  makes a worker running src/worker.js
   * @param {WebAssembly.Module} [options.module]  the compiled core, shared by every worker
   * @param {number} [options.restartAfterMs]  age at which a superseded build is cancelled
   */
  constructor({
    createWorker,
    module,
    now = () => performance.now(),
    restartAfterMs = 250,
    setTimeout: setTimer = globalThis.setTimeout.bind(globalThis),
    clearTimeout: clearTimer = globalThis.clearTimeout.bind(globalThis),
  }) {
    this.#createWorker = createWorker;
    this.#module = module;
    this.#now = now;
    this.#restartAfterMs = restartAfterMs;
    this.#setTimeout = setTimer;
    this.#clearTimeout = clearTimer;
  }

  /** Start the worker and load the core; resolves with the core's info. */
  async start() {
    if (this.#worker) throw new Error("CoreClient.start() was already called");
    this.#spawn();
    try {
      this.#info = await this.#send({ type: "init", module: this.#module });
    } catch (err) {
      this.#fail(err);
      throw err;
    }
    return this.#info;
  }

  /** The info the core reported at start: { protocol, kernel, maxStuds, standard }. */
  get info() {
    return this.#info;
  }

  /** True while a build is running or waiting. */
  get busy() {
    return this.#inflight !== null || this.#queued !== null;
  }

  /** True while an export is owed. */
  get exporting() {
    return this.#exportsPending();
  }

  /** Structural errors of a part document (an array of messages, empty when valid). */
  checkPart(text) {
    return this.#send({ type: "checkPart", text });
  }

  /** The bundled data the page builds its controls from (protocol.js bundled()). */
  bundled() {
    return this.#send({ type: "bundled" });
  }

  /** The core's parameter resolution, { values, visible }, without building anything. */
  resolveParams(fields) {
    return this.#send({ ...fields, type: "resolveParams" });
  }

  /** The coupon's cells and summary without geometry. */
  couponLayout(fields) {
    return this.#send({ ...fields, type: "couponLayout" });
  }

  /**
   * The calibration wizard's choices as a fit profile (protocol.js
   * recordFit()): { ok, fitProfile, text, fileName, offsets } or { ok: false,
   * error: { field, message } }. A plain request: it is never superseded.
   */
  recordFit(input) {
    return this.#send({ type: "recordFit", input });
  }

  /** A part document the user opened, checked: { ok, ... } (protocol.js openPart). */
  openPart(text, process) {
    return this.#send({ type: "openPart", text, process });
  }

  /** The part document with the values as defaults: { text }. */
  savePart(fields) {
    return this.#send({ ...fields, type: "savePart" });
  }

  /** The part document with a preset `name` holding the values: { text }. */
  addPreset(fields) {
    return this.#send({ ...fields, type: "addPreset" });
  }

  /**
   * Build a nominal brick. Resolves with the mesh result of src/protocol.js, or with
   * null when a newer build superseded this one.
   */
  buildBrick(width, length) {
    return this.#build({ type: "brick", width, length });
  }

  /**
   * Evaluate and validate a part ({ doc, params, process, fit, assets }). Resolves
   * with the part result of src/protocol.js, or null when superseded.
   */
  evaluatePart(fields) {
    return this.#build({ ...fields, type: "evaluatePart" });
  }

  /** Build the coupon ({ process, layout }); null when superseded. */
  buildCoupon(fields) {
    return this.#build({ ...fields, type: "buildCoupon" });
  }

  /** Write a part file ({ format, doc, params, process, fit, assets, colors }): { format, bytes }. */
  exportPart(fields) {
    return this.#export({ ...fields, type: "exportPart" });
  }

  /** Write the coupon file ({ format, process, layout, colors }): { format, bytes }. */
  exportCoupon(fields) {
    return this.#export({ ...fields, type: "exportCoupon" });
  }

  // The assembler (Build mode): plain requests, never superseded. The model lives
  // in the worker's core; every change answers with the state to draw.

  /** The built-in catalog, palette, rotation matrices and samples. */
  assemblerCatalog() {
    return this.#send({ type: "asmInit" });
  }

  /**
   * The render meshes ({ meshes }): the built-in parts, or the catalog part numbers in `parts`
   * (at most 256 at a time); `detail` false for the cheap level of the built-in ones.
   */
  assemblerMeshes(detail = true, parts = undefined) {
    return this.#send({ type: "asmMeshes", detail, parts });
  }

  /** The render levels (0 full studs to 3 the hull) of catalog parts: { meshes: [{ index, lod, ... }] }. */
  assemblerLodMeshes(parts, lods) {
    return this.#send({ type: "asmMeshes", parts, lods });
  }

  /** Add an LDraw pack (the bytes of an .obpack) to the catalog: { pack, catalog, kept, ...state }. */
  assemblerLoadPack(bytes) {
    return this.#send({ type: "asmLoadPack", bytes, lean: true });
  }

  /** Drop the pack: { kept, catalog, ...state }. */
  assemblerUnloadPack() {
    return this.#send({ type: "asmUnloadPack", lean: true });
  }

  /** What a .obrick document needs the core does not have: { requires, missing, missingCount }. */
  assemblerNeeds(text) {
    return this.#send({ type: "asmNeeds", text });
  }

  /** The engine's snapped pose for a part under a pointer ray (LDU, model frame), or null. */
  assemblerSnap(part, rot, ray) {
    return this.#send({ type: "asmSnap", part, rot, ray });
  }

  /** What the ray hits first, or null. */
  assemblerPick(ray) {
    return this.#send({ type: "asmPick", ray });
  }

  /**
   * Apply a command (see crates/openbrick-engine/src/history.rs); resolves with { id, delta, parts, canUndo,
   * canRedo, revision, stats?, bom? }: what changed (src/model-table.js applyDelta), not the model.
   */
  assemblerApply(command) {
    return this.#send({ type: "asmApply", command, lean: true });
  }

  assemblerUndo() {
    return this.#send({ type: "asmUndo", lean: true });
  }

  assemblerRedo() {
    return this.#send({ type: "asmRedo", lean: true });
  }

  /** The whole table: { instances (Int32Array), parts, canUndo, canRedo, revision, stats?, bom? }. */
  assemblerState() {
    return this.#send({ type: "asmState", lean: true });
  }

  /** The statistics and bill of materials of the model as it is: { stats, bom, parts, revision }. */
  assemblerStats() {
    return this.#send({ type: "asmStats" });
  }

  /**
   * Replace the model with a .obrick document, read in steps; `onProgress({ done, total })` is called between
   * them. Resolves with the whole table (as assemblerState) and `notices`.
   */
  assemblerLoad(text, onProgress) {
    return this.#send({ type: "asmLoad", text, lean: true }, { onProgress });
  }

  /** Replace the model with the table the page holds (the Int32Array of seven numbers a part). */
  assemblerLoadRows(rows) {
    return this.#send({ type: "asmLoadRows", rows });
  }

  /** Stop a document being opened. */
  assemblerCancel() {
    return this.#send({ type: "asmCancel" });
  }

  /** The model as .obrick text: { text }. */
  assemblerSave(name) {
    return this.#send({ type: "asmSave", name });
  }

  /** The text of a sample model: { text }. */
  assemblerSample(id) {
    return this.#send({ type: "asmSample", id });
  }

  /** A rotation index turned about the vertical axis: { rot }. */
  assemblerTurn(rot, quarterTurns) {
    return this.#send({ type: "asmTurn", rot, quarterTurns });
  }

  /** Stop the worker; anything still waiting is rejected. */
  dispose() {
    this.#fail(new CoreError("the core client was disposed"));
  }

  // ------------------------------------------------------------------ internals

  #spawn() {
    const worker = this.#createWorker();
    worker.onmessage = (event) => {
      if (this.#worker === worker) this.#onReply(event.data);
    };
    worker.onerror = (event) => {
      if (this.#worker === worker) this.#fail(new CoreError(`the core worker failed: ${event?.message ?? "unknown error"}`));
    };
    worker.onmessageerror = () => {
      if (this.#worker === worker) this.#fail(new CoreError("a message from the core worker could not be read"));
    };
    this.#worker = worker;
  }

  #send(fields, { onProgress } = {}) {
    if (this.#failure) return Promise.reject(this.#failure);
    if (!this.#worker) return Promise.reject(new Error("call CoreClient.start() first"));
    const request = { id: this.#nextId++, ...fields };
    return new Promise((resolve, reject) => {
      this.#pending.set(request.id, { request, resolve, reject, onProgress });
      this.#worker.postMessage(request);
    });
  }

  /** An export: sent at once, never superseded; when it settles, a pending cancel may run. */
  #export(fields) {
    const promise = this.#send(fields);
    const after = () => this.#maybeCancel();
    promise.then(after, after);
    return promise;
  }

  #exportsPending() {
    for (const entry of this.#pending.values()) if (EXPORT_TYPES.includes(entry.request.type)) return true;
    return false;
  }

  /** Queue a build, superseding the waiting one and marking the running one superseded. */
  #build(fields) {
    return new Promise((resolve, reject) => {
      if (this.#failure) {
        reject(this.#failure);
        return;
      }
      if (!this.#worker) {
        reject(new Error("call CoreClient.start() first"));
        return;
      }
      this.#queued?.resolve(null);
      this.#queued = { fields, resolve, reject };
      if (this.#inflight) {
        this.#inflight.superseded = true;
        this.#maybeCancel();
      }
      this.#pump();
    });
  }

  /**
   * Cancel the running build if it is superseded and old enough, by replacing the
   * worker; arm a timer if it is younger. Never while an export is owed: the
   * export's reply calls this again.
   */
  #maybeCancel() {
    const running = this.#inflight;
    if (!running?.superseded || this.#failure || this.#exportsPending()) return;
    const age = this.#now() - running.startedAt;
    if (age >= this.#restartAfterMs) {
      this.#restart({ cancelBuild: true });
    } else if (running.timer === undefined) {
      running.timer = this.#setTimeout(() => {
        running.timer = undefined;
        if (this.#inflight === running) this.#maybeCancel();
      }, this.#restartAfterMs - age);
    }
  }

  #onReply(reply) {
    if (reply?.progress) {
      // Not the answer: a long request says how far it is. The request stays owed.
      this.#pending.get(reply.id)?.onProgress?.(reply.progress);
      return;
    }
    const entry = this.#pending.get(reply?.id);
    if (entry) this.#pending.delete(reply.id);
    if (reply?.fatal) {
      // Replace the worker before settling, so whatever the settled request
      // triggers next already goes to the fresh one.
      this.traps++;
      this.#restart({ cancelBuild: false });
    }
    if (!entry) return; // the reply to a request that was dropped
    if (reply.ok) entry.resolve(reply.result);
    else entry.reject(new CoreError(reply.error));
  }

  /** Send the queued build when nothing is running. */
  #pump() {
    if (this.#inflight || !this.#queued || this.#failure) return;
    const job = this.#queued;
    this.#queued = null;
    const running = { ...job, startedAt: this.#now(), superseded: false, timer: undefined };
    this.#inflight = running;
    const settle = (finish) => {
      if (this.#inflight !== running) return; // cancelled by a restart; already resolved
      this.#inflight = null;
      if (running.timer !== undefined) this.#clearTimeout(running.timer);
      finish();
      this.#pump();
    };
    this.#send(job.fields).then(
      (result) => settle(() => job.resolve(running.superseded ? null : result)),
      (err) => settle(() => (running.superseded ? job.resolve(null) : job.reject(err))),
    );
  }

  /**
   * Terminate the worker, start a fresh one from the same module, and send it
   * everything the old one still owed, with the original ids. With `cancelBuild`
   * the running build is dropped (it resolves null) instead of re-sent.
   */
  #restart({ cancelBuild }) {
    if (cancelBuild) {
      const running = this.#inflight;
      this.#inflight = null;
      if (running) {
        if (running.timer !== undefined) this.#clearTimeout(running.timer);
        running.resolve(null);
      }
    }
    const all = [...this.#pending.values()];
    const stale = all.filter((entry) => MODEL_TYPES.includes(entry.request.type));
    const owed = all.filter((entry) => !stale.includes(entry) && !(cancelBuild && BUILD_TYPES.includes(entry.request.type)));
    this.#pending.clear();
    for (const entry of stale) {
      entry.reject(new CoreError("the core was restarted while this was running, so it was not repeated on an empty model; your model is put back before the next request, then try again"));
    }
    this.#worker.terminate();
    this.restarts++;
    this.#spawn();
    this.#send({ type: "init", module: this.#module }).catch((err) => this.#fail(err));
    for (const entry of owed) {
      this.#pending.set(entry.request.id, entry);
      this.#worker.postMessage(entry.request);
    }
    this.#pump();
  }

  /** Reject everything and stop the worker; later calls fail with the same error. */
  #fail(err) {
    this.#failure ??= err;
    const running = this.#inflight;
    const queued = this.#queued;
    this.#inflight = null;
    this.#queued = null;
    if (running?.timer !== undefined) this.#clearTimeout(running.timer);
    running?.reject(err);
    queued?.reject(err);
    for (const entry of this.#pending.values()) entry.reject(err);
    this.#pending.clear();
    this.#worker?.terminate();
  }
}
