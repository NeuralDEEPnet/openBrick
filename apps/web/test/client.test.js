// The page's side of the worker (src/client.js), driven with fake workers, a fake
// clock and fake timers: request ids, latest-wins brick builds, cancelling a long
// build by replacing the worker, and failures.

import assert from "node:assert/strict";
import { test } from "node:test";
import { CoreClient, CoreError } from "../src/client.js";

const INFO = { protocol: 1, kernel: "fake-kernel 1.0", maxStuds: 16, standard: {} };
const MODULE = { compiled: "module" };

/** Everything a test needs: the client, its workers, the clock and pending timers. */
function setup({ restartAfterMs = 250 } = {}) {
  const workers = [];
  const timers = [];
  const env = {
    workers,
    timers,
    time: 0,
    get worker() {
      return workers.at(-1);
    },
  };
  class FakeWorker {
    sent = [];
    terminated = false;
    postMessage(message) {
      if (this.terminated) throw new Error("posted to a terminated worker");
      this.sent.push(message);
    }
    terminate() {
      this.terminated = true;
    }
    /** Answer the request with this id (the last one of `type` when id is a string). */
    reply(idOrType, result) {
      const id = typeof idOrType === "number" ? idOrType : this.sent.findLast((m) => m.type === idOrType).id;
      this.onmessage({ data: { id, ok: true, result } });
    }
    fail(id, error) {
      this.onmessage({ data: { id, ok: false, error } });
    }
    /** Answer with the fatal reply the worker sends after a WebAssembly trap. */
    trap(idOrType) {
      const id = typeof idOrType === "number" ? idOrType : this.sent.findLast((m) => m.type === idOrType).id;
      this.onmessage({ data: { id, ok: false, fatal: true, error: "the core stopped with a WebAssembly trap (unreachable)" } });
    }
  }
  env.client = new CoreClient({
    createWorker: () => {
      const w = new FakeWorker();
      workers.push(w);
      return w;
    },
    module: MODULE,
    now: () => env.time,
    restartAfterMs,
    setTimeout: (fn, ms) => {
      const timer = { fn, at: env.time + ms, cleared: false };
      timers.push(timer);
      return timer;
    },
    clearTimeout: (timer) => {
      timer.cleared = true;
    },
  });
  /** Advance the clock and run the timers that are due. */
  env.advance = (ms) => {
    env.time += ms;
    for (const t of timers.filter((t) => !t.cleared && !t.ran && t.at <= env.time)) {
      t.ran = true;
      t.fn();
    }
  };
  return env;
}

async function started(options) {
  const env = setup(options);
  const info = env.client.start();
  assert.deepEqual(env.worker.sent, [{ id: 1, type: "init", module: MODULE }]);
  env.worker.reply(1, INFO);
  assert.deepEqual(await info, INFO);
  return env;
}

/** Let promise callbacks run. */
const settle = () => new Promise((r) => setImmediate(r));
const bricks = (worker) => worker.sent.filter((m) => m.type === "brick");

test("start loads the core in a worker with the compiled module and reports its info", async () => {
  const env = await started();
  assert.equal(env.client.info, INFO);
  await assert.rejects(env.client.start(), /already called/);
});

test("a failed start rejects, and so does every later call", async () => {
  const env = setup();
  const info = env.client.start();
  env.worker.fail(1, "could not load the core: bad magic");
  await assert.rejects(info, (err) => err instanceof CoreError && /bad magic/.test(err.message));
  assert.ok(env.worker.terminated);
  await assert.rejects(env.client.buildBrick(2, 4), /bad magic/);
});

test("building before start is an error", async () => {
  await assert.rejects(setup().client.buildBrick(2, 4), /start\(\) first/);
});

test("a brick request resolves with the worker's result", async () => {
  const env = await started();
  const p = env.client.buildBrick(2, 4);
  assert.ok(env.client.busy);
  assert.deepEqual(bricks(env.worker), [{ id: 2, type: "brick", width: 2, length: 4 }]);
  env.worker.reply(2, { triangles: 3620 });
  assert.deepEqual(await p, { triangles: 3620 });
  assert.equal(env.client.busy, false);
});

test("a core error rejects the request with its message", async () => {
  const env = await started();
  const p = env.client.buildBrick(0, 4);
  env.worker.fail(2, "brick width must be 1 to 16 studs, got 0");
  await assert.rejects(p, (err) => err instanceof CoreError && err.message === "brick width must be 1 to 16 studs, got 0");
});

test("latest wins: only one build runs, waiting requests are replaced, superseded ones resolve null", async () => {
  const env = await started();
  const a = env.client.buildBrick(2, 4);
  const b = env.client.buildBrick(2, 3);
  const c = env.client.buildBrick(2, 2);
  assert.equal(await b, null, "b was replaced before it was sent");
  assert.equal(bricks(env.worker).length, 1, "c waits until a is done");
  env.advance(50); // a young build is left to finish
  env.worker.reply(2, { size: "2x4" });
  assert.equal(await a, null, "a finished but was superseded");
  await settle();
  assert.deepEqual(bricks(env.worker).map((m) => [m.width, m.length]), [[2, 4], [2, 2]]);
  env.worker.reply("brick", { size: "2x2" });
  assert.deepEqual(await c, { size: "2x2" });
  assert.equal(env.client.restarts, 0);
  assert.equal(env.workers.length, 1);
});

test("superseding a build that has run restartAfterMs replaces the worker at once", async () => {
  const env = await started({ restartAfterMs: 250 });
  const big = env.client.buildBrick(16, 16);
  env.advance(1000);
  const small = env.client.buildBrick(2, 4);
  assert.equal(await big, null, "the cancelled build resolves null");
  assert.equal(env.client.restarts, 1);
  assert.equal(env.client.traps, 0, "a cancel is not a trap");
  assert.equal(env.workers.length, 2);
  const [old, fresh] = env.workers;
  assert.ok(old.terminated);
  // Ids: 1 the first init, 2 the cancelled build; the queued 2x4 had none yet.
  assert.deepEqual(fresh.sent, [
    { id: 3, type: "init", module: MODULE },
    { id: 4, type: "brick", width: 2, length: 4 },
  ]);
  // A late reply from the terminated worker is ignored.
  old.onmessage({ data: { id: 2, ok: true, result: { size: "16x16" } } });
  fresh.reply(3, INFO);
  fresh.reply(4, { size: "2x4" });
  assert.deepEqual(await small, { size: "2x4" });
});

test("a young superseded build gets a timer; if it is still running then, the worker is replaced", async () => {
  const env = await started({ restartAfterMs: 250 });
  const big = env.client.buildBrick(16, 16);
  env.advance(100);
  const small = env.client.buildBrick(2, 4);
  assert.equal(env.client.restarts, 0);
  assert.equal(env.timers.length, 1);
  assert.equal(env.timers[0].at, 250, "fires when the build is restartAfterMs old");
  env.advance(150);
  assert.equal(await big, null);
  assert.equal(env.client.restarts, 1);
  const fresh = env.worker;
  assert.deepEqual(bricks(fresh), [{ id: 4, type: "brick", width: 2, length: 4 }]);
  fresh.reply("init", INFO);
  fresh.reply("brick", { size: "2x4" });
  assert.deepEqual(await small, { size: "2x4" });
});

test("a young superseded build that finishes in time clears its timer and keeps the worker", async () => {
  const env = await started({ restartAfterMs: 250 });
  const first = env.client.buildBrick(2, 3);
  env.advance(100);
  const second = env.client.buildBrick(2, 4);
  env.worker.reply(2, { size: "2x3" });
  assert.equal(await first, null);
  assert.ok(env.timers[0].cleared);
  env.advance(1000);
  assert.equal(env.client.restarts, 0);
  await settle();
  env.worker.reply("brick", { size: "2x4" });
  assert.deepEqual(await second, { size: "2x4" });
});

test("a replacement worker also answers the other requests the old one owed", async () => {
  const env = await started({ restartAfterMs: 0 });
  const big = env.client.buildBrick(16, 16);
  const check = env.client.checkPart("{}");
  const small = env.client.buildBrick(1, 1);
  assert.equal(await big, null);
  const fresh = env.worker;
  assert.deepEqual(
    fresh.sent.map((m) => m.type),
    ["init", "checkPart", "brick"],
    "init first, then what the old worker owed, then the newest build",
  );
  const checkId = fresh.sent[1].id;
  assert.equal(checkId, 3, "re-sent with its original id");
  fresh.reply("init", INFO);
  fresh.reply(checkId, []);
  fresh.reply("brick", { size: "1x1" });
  assert.deepEqual(await check, []);
  assert.deepEqual(await small, { size: "1x1" });
});

test("a trap fails its request and replaces the worker before the next build", async () => {
  const env = await started();
  const build = env.client.buildBrick(2, 4);
  const old = env.worker;
  old.trap("brick");
  await assert.rejects(build, (err) => err instanceof CoreError && /WebAssembly trap/.test(err.message));
  assert.ok(old.terminated, "the trapped instance is never used again");
  assert.equal(env.client.restarts, 1);
  assert.equal(env.client.traps, 1);
  const fresh = env.worker;
  assert.notEqual(fresh, old);
  assert.deepEqual(fresh.sent, [{ id: 3, type: "init", module: MODULE }], "the failed build is not retried");
  assert.equal(env.client.busy, false);
  fresh.reply("init", INFO);
  const next = env.client.buildBrick(2, 2);
  assert.deepEqual(bricks(fresh), [{ id: 4, type: "brick", width: 2, length: 2 }]);
  fresh.reply("brick", { size: "2x2" });
  assert.deepEqual(await next, { size: "2x2" });
});

test("after a trap the fresh worker gets everything else the old one owed", async () => {
  const env = await started();
  const check = env.client.checkPart("{}");
  const build = env.client.buildBrick(2, 4);
  const queued = env.client.buildBrick(1, 1);
  const old = env.worker;
  assert.deepEqual(old.sent.map((m) => [m.id, m.type]), [[1, "init"], [2, "checkPart"], [3, "brick"]]);
  old.trap(2);
  await assert.rejects(check, /WebAssembly trap/);
  assert.ok(old.terminated);
  const fresh = env.worker;
  assert.deepEqual(
    fresh.sent.map((m) => [m.id, m.type]),
    [[4, "init"], [3, "brick"]],
    "init first, then the build the old worker owed, with its original id",
  );
  // A late reply from the old worker is ignored.
  old.onmessage({ data: { id: 3, ok: true, result: { size: "stale" } } });
  fresh.reply(4, INFO);
  fresh.reply(3, { size: "2x4" });
  assert.equal(await build, null, "superseded by the queued 1x1");
  await settle();
  fresh.reply("brick", { size: "1x1" });
  assert.deepEqual(await queued, { size: "1x1" });
  assert.deepEqual([env.client.restarts, env.client.traps], [1, 1]);
});

test("a crashed worker rejects everything waiting and every later call", async () => {
  const env = await started();
  const build = env.client.buildBrick(2, 4);
  const check = env.client.checkPart("{}");
  env.worker.onerror({ message: "out of memory" });
  await assert.rejects(build, /the core worker failed: out of memory/);
  await assert.rejects(check, /out of memory/);
  assert.ok(env.worker.terminated);
  await assert.rejects(env.client.buildBrick(1, 1), /out of memory/);
  await assert.rejects(env.client.checkPart("{}"), /out of memory/);
});

test("dispose rejects what is waiting", async () => {
  const env = await started();
  const build = env.client.buildBrick(2, 4);
  env.client.dispose();
  await assert.rejects(build, /disposed/);
});
