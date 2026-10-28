// The page's side of the core worker (src/worker.js, protocol in src/protocol.js).
//
// Requests are matched to replies by id. Brick builds are "latest wins": a new
// build request supersedes the one waiting to be sent and the one running, and
// superseded requests resolve with null. A running WebAssembly call cannot be
// interrupted, so when the superseded build has run for `restartAfterMs` (now or
// later) the client terminates the worker and starts a fresh one from the same
// compiled module; a large brick (16x16 takes tens of seconds, Q01 finding 2 in
// docs/spikes/q01-kernel-conformance.md) then never holds up the next request.
//
// The worker factory, clock and timers are injected so the unit tests drive the
// client with fake workers and a fake clock.

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
  /** The brick build the worker is running: { width, length, resolve, reject, startedAt, superseded, timer }. */
  #inflight = null;
  /** The newest brick request, not yet sent: { width, length, resolve, reject }. */
  #queued = null;
  #failure = null;
  /** How many times a worker was replaced to cancel a build. */
  restarts = 0;

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

  /** True while a brick build is running or waiting. */
  get busy() {
    return this.#inflight !== null || this.#queued !== null;
  }

  /** Structural errors of a part document (an array of messages, empty when valid). */
  checkPart(text) {
    return this.#send({ type: "checkPart", text });
  }

  /**
   * Build a nominal brick. Resolves with the mesh result of src/protocol.js, or with
   * null when a newer request superseded this one.
   */
  buildBrick(width, length) {
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
      this.#queued = { width, length, resolve, reject };
      const running = this.#inflight;
      if (running) {
        running.superseded = true;
        const age = this.#now() - running.startedAt;
        if (age >= this.#restartAfterMs) {
          this.#restart();
        } else if (running.timer === undefined) {
          running.timer = this.#setTimeout(() => {
            if (this.#inflight === running) this.#restart();
          }, this.#restartAfterMs - age);
        }
      }
      this.#pump();
    });
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

  #send(fields) {
    if (this.#failure) return Promise.reject(this.#failure);
    const request = { id: this.#nextId++, ...fields };
    return new Promise((resolve, reject) => {
      this.#pending.set(request.id, { request, resolve, reject });
      this.#worker.postMessage(request);
    });
  }

  #onReply(reply) {
    const entry = this.#pending.get(reply?.id);
    if (!entry) return; // the reply to a request that was dropped
    this.#pending.delete(reply.id);
    if (reply.ok) entry.resolve(reply.result);
    else entry.reject(new CoreError(reply.error));
  }

  /** Send the queued brick request when nothing is running. */
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
    this.#send({ type: "brick", width: job.width, length: job.length }).then(
      (result) => settle(() => job.resolve(running.superseded ? null : result)),
      (err) => settle(() => (running.superseded ? job.resolve(null) : job.reject(err))),
    );
  }

  /**
   * Cancel the running build: terminate the worker, start a fresh one from the same
   * module, and send it everything the old one still owed except the cancelled build.
   */
  #restart() {
    const running = this.#inflight;
    this.#inflight = null;
    if (running) {
      if (running.timer !== undefined) this.#clearTimeout(running.timer);
      running.resolve(null);
    }
    const owed = [...this.#pending.values()].filter((entry) => entry.request.type !== "brick");
    this.#pending.clear();
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
