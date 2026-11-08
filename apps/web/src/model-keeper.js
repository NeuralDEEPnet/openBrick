// Keeps Build mode's model safe (src/build-ui.js). The model lives in the core
// worker, but the page replaces that worker whenever it cancels a superseded Part or
// coupon build (src/client.js #restart) or after a trap, and a new worker starts with
// an empty assembler. So the page keeps the text of the model after every change (the
// core hands it back with the state) and, before the next request to a replaced worker,
// loads it back. The undo history lives in the worker and is lost by a restart.
//
// It also remembers the model that Open or a sample replaced, so "Restore previous
// model" can bring it back (and bring the replacement back again).

export class ModelKeeper {
  #client;
  #seen;
  #text = null;
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

  /** The text of the model as of the last change, or null before there is one. */
  get text() {
    return this.#text;
  }

  /** Remember the model's text (the `text` of every state the core answers with). */
  remember(text) {
    if (typeof text === "string") this.#text = text;
  }

  /**
   * If the worker was replaced since the last request, load the remembered model into the
   * new one. Resolves with true when it did. The restart is noted before anything is
   * awaited, and the load is sent in the same turn, so a request made right after goes
   * behind it in the worker's queue.
   */
  async ensure() {
    const now = this.#client.restarts ?? 0;
    if (now === this.#seen) return false;
    this.#seen = now;
    if (this.#text === null) return false;
    const state = await this.#client.assemblerLoad(this.#text);
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
        "assemblerSnap",
        "assemblerPick",
        "assemblerApply",
        "assemblerUndo",
        "assemblerRedo",
        "assemblerState",
        "assemblerLoad",
        "assemblerSave",
        "assemblerSample",
        "assemblerTurn",
      ].map((name) => [name, wrap(name)]),
    );
  }

  /** Set aside the model about to be replaced: { text, name, dirty }. */
  setPrevious(previous) {
    this.#previous = previous && typeof previous.text === "string" ? previous : null;
  }

  /** The model that was replaced, or null. */
  get previous() {
    return this.#previous;
  }
}
