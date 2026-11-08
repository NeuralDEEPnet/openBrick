// Build mode's safety net (src/model-keeper.js): the page's copy of the model is put
// back when the client replaced the core worker, and the model a replacement set aside
// can come back.

import assert from "node:assert/strict";
import { test } from "node:test";
import { ModelKeeper } from "../src/model-keeper.js";

/** A fake client: a worker that holds one model text and is emptied when `restart()` is called. */
function fakeClient() {
  const log = [];
  const client = {
    restarts: 0,
    worker: null, // the text the "worker" holds
    log,
    restart() {
      client.restarts++;
      client.worker = null;
    },
    async assemblerLoad(text) {
      log.push(["load", text]);
      client.worker = text;
      return { instances: [], stats: { parts: 1, floating: [] }, text };
    },
    async assemblerLoadPack(bytes) {
      log.push(["pack", bytes.length]);
      return { pack: { library: "x" } };
    },
    async assemblerSnap(...args) {
      log.push(["snap", client.worker]);
      return client.worker === null ? null : { args };
    },
    async assemblerApply(command) {
      log.push(["apply", client.worker]);
      return { text: `${client.worker}+${command.op}` };
    },
  };
  return client;
}

test("a request after a worker restart finds the model put back first", async () => {
  const client = fakeClient();
  const restored = [];
  const keeper = new ModelKeeper({ client, onRestored: (s) => restored.push(s) });
  const asm = keeper.guarded();
  client.worker = "model-1";
  keeper.remember("model-1");
  assert.deepEqual(await asm.assemblerSnap("p"), { args: ["p"] });
  assert.equal(client.log.some(([k]) => k === "load"), false, "nothing is loaded while the worker is the same");

  client.restart(); // a superseded Part build cancelled the worker
  const snap = await asm.assemblerSnap("p");
  assert.deepEqual(snap, { args: ["p"] }, "the snap saw the restored model, not an empty one");
  assert.deepEqual(client.log.slice(-2), [["load", "model-1"], ["snap", "model-1"]]);
  assert.equal(restored.length, 1);
  assert.equal(restored[0].text, "model-1");
});

test("two requests right after a restart load the model once, ahead of both", async () => {
  const client = fakeClient();
  const keeper = new ModelKeeper({ client });
  const asm = keeper.guarded();
  keeper.remember("m");
  client.restart();
  const [a, b] = await Promise.all([asm.assemblerSnap("a"), asm.assemblerSnap("b")]);
  assert.ok(a && b);
  assert.equal(client.log.filter(([k]) => k === "load").length, 1);
  assert.equal(client.log[0][0], "load");
});

test("the copy follows the model: the newest text is what a restart restores", async () => {
  const client = fakeClient();
  const keeper = new ModelKeeper({ client });
  const asm = keeper.guarded();
  keeper.remember("v1");
  keeper.remember("v2");
  keeper.remember(undefined); // a state without text changes nothing
  client.restart();
  await asm.assemblerApply({ op: "add" });
  assert.equal(client.log[0][1], "v2");
  assert.equal(keeper.text, "v2");
});

test("a restart before any model exists loads nothing, and a second restart restores again", async () => {
  const client = fakeClient();
  const keeper = new ModelKeeper({ client });
  const asm = keeper.guarded();
  client.restart();
  await asm.assemblerSnap("p");
  assert.equal(client.log.some(([k]) => k === "load"), false);
  keeper.remember("m");
  client.restart();
  await asm.assemblerSnap("p");
  client.restart();
  await asm.assemblerSnap("p");
  assert.equal(client.log.filter(([k]) => k === "load").length, 2);
});

test("the replaced model is set aside, and only a well-formed one", () => {
  const keeper = new ModelKeeper({ client: fakeClient() });
  assert.equal(keeper.previous, null);
  keeper.setPrevious({ text: "old", name: "Old", dirty: true });
  assert.deepEqual(keeper.previous, { text: "old", name: "Old", dirty: true });
  keeper.setPrevious({ name: "no text" });
  assert.equal(keeper.previous, null);
  keeper.setPrevious(null);
  assert.equal(keeper.previous, null);
});

test("a replaced worker gets the LDraw pack back before the model, so the model's pack parts load", async () => {
  const client = fakeClient();
  const keeper = new ModelKeeper({ client });
  const asm = keeper.guarded();
  keeper.rememberPack(new Uint8Array(7));
  keeper.remember("m");
  assert.equal(keeper.pack.length, 7);
  client.restart();
  await asm.assemblerSnap("p");
  assert.deepEqual(
    client.log.map(([k]) => k),
    ["pack", "load", "snap"],
    "pack, then model, then the request",
  );
  // Removing the pack: later restarts send none.
  keeper.rememberPack(null);
  assert.equal(keeper.pack, null);
  client.log.length = 0;
  client.restart();
  await asm.assemblerSnap("p");
  assert.deepEqual(client.log.map(([k]) => k), ["load", "snap"]);
  keeper.rememberPack("not bytes");
  assert.equal(keeper.pack, null, "only bytes are kept");
});
