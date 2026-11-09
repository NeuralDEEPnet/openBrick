// Keeps Build mode's model safe (src/build-ui.js). The model lives in the core
// worker, but the page replaces that worker whenever it cancels a superseded Part or
// coupon build (src/client.js #restart) or after a trap, and a new worker starts with
// an empty assembler. So the page keeps a copy of the model and, before the next request
// to a replaced worker, loads it back. The copy is whatever `remember` was given: the
// page's own table of the model (a function that returns its rows, read only when a worker
// was replaced, so keeping it costs nothing per edit however big the model is), the rows
// themselves, or the model's text. The undo history lives in the worker and is lost by a restart.
//
// It also remembers the model that Open or a sample replaced, so "Restore previous
// model" can bring it back (and bring the replacement back again).

export class ModelKeeper {
  #client;
  #seen;
  #text = null;
  #pack = null;
  #previous = null;
  #onRestored;

  /**
   * @param {object} options
   * @param {object} options.client  the CoreClient (`restarts`, `assemblerLoad`, ...)
   * @param {(state: object) => void|Promise<void>} [options.onRestored]  called with the state
   *   the restored model gives, after a restart
   */
  constructor({ client, onRestored = () => {} }) {
    this.#client = client;
    this.#seen = client.restarts ?? 0;
    this.#onRestored = onRestored;
  }

  /** The copy as last given: a function, rows (Int32Array) or text, or null before there is one. */
  get text() {
    return this.#text;
  }

  /**
   * Remember the model: a function that returns the rows of the table (an Int32Array, seven numbers a part),
   * the rows, or the model's text. Anything else changes nothing (a state without a copy).
   */
  remember(copy) {
    if (typeof copy === "string" || copy instanceof Int32Array || typeof copy === "function") this.#text = copy;
  }

  /** Remember the bytes of the LDraw pack that is loaded (or null), to send them again to a replaced worker. */
  rememberPack(bytes) {
    this.#pack = bytes instanceof Uint8Array ? bytes : null;
  }

  /** The bytes of the loaded pack, or null. */
  get pack() {
    return this.#pack;
  }

  /**
   * If the worker was replaced since the last request, load the loaded pack and then the remembered
   * model into the new one. Resolves with true when it did. The restart is noted before anything is
   * awaited, and the load is sent in the same turn, so a request made right after goes
   * behind it in the worker's queue.
   */
  async ensure() {
    const now = this.#client.restarts ?? 0;
    if (now === this.#seen) return false;
    this.#seen = now;
    if (this.#pack !== null) await this.#client.assemblerLoadPack(this.#pack);
    if (this.#text === null) return false;
    const copy = typeof this.#text === "function" ? this.#text() : this.#text;
    const state = typeof copy === "string" ? await this.#client.assemblerLoad(copy) : await this.#client.assemblerLoadRows(copy);
    await this.#onRestored(state);
    return true;
  }

  /**
   * The client's assembler methods, each preceded by [`ensure`]. Use these for every
   * request, so none reaches a replaced worker before the model does.
   */
  guarded() {
    const wrap =
      (name) =>
      async (...args) => {
        await this.ensure();
        return this.#client[name](...args);
      };
    return Object.fromEntries(
      [
        "assemblerCatalog",
        "assemblerMeshes",
        "assemblerLodMeshes",
        "assemblerSnap",
        "assemblerPick",
        "assemblerApply",
        "assemblerUndo",
        "assemblerRedo",
        "assemblerState",
        "assemblerLoad",
        "assemblerLoadRows",
        "assemblerStats",
        "assemblerCancel",
        "assemblerLoadPack",
        "assemblerUnloadPack",
        "assemblerNeeds",
        "assemblerSave",
        "assemblerSample",
        "assemblerTurn",
      ].map((name) => [name, wrap(name)]),
    );
  }

  /** Set aside the model about to be replaced: { rows (or text), name, dirty }. */
  setPrevious(previous) {
    this.#previous = previous && (previous.rows instanceof Int32Array || typeof previous.text === "string") ? previous : null;
  }

  /** The model that was replaced, or null. */
  get previous() {
    return this.#previous;
  }
}
