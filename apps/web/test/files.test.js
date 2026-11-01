// Saving and opening (src/files.js): browser downloads unchanged, and the
// desktop adapter that sends saves and opens to the desktop app's commands
// (apps/desktop/src-tauri/src/app.rs) when window.__TAURI__ exists. The Tauri
// API is a mock that records every call; the command names and headers are
// the ones apps/desktop/src-tauri/src/files.rs reads.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  COMMANDS,
  FORMAT_HEADER,
  MIME,
  NAME_HEADER,
  browserDownload,
  createFileAccess,
  desktopBridge,
  errorText,
  pickedFile,
  replyBytes,
  savedText,
} from "../src/files.js";

/** A mock of the Tauri bridge: answers by command name and records each call. */
function mockBridge(answers) {
  const calls = [];
  return {
    calls,
    async invoke(cmd, args, options) {
      calls.push({ cmd, args, options });
      const answer = answers[cmd];
      if (answer instanceof Error) throw answer.message; // Tauri rejects with the Rust error string
      return typeof answer === "function" ? answer(args, options) : answer;
    },
  };
}

test("only a page with the global Tauri API is the desktop app", () => {
  assert.equal(desktopBridge(undefined), null);
  assert.equal(desktopBridge({}), null);
  assert.equal(desktopBridge({ __TAURI__: {} }), null);
  assert.equal(desktopBridge({ __TAURI__: { core: { invoke: "not a function" } } }), null);
  const invoke = async () => null;
  assert.equal(desktopBridge({ __TAURI__: { core: { invoke } } }).invoke, invoke);
  assert.equal(createFileAccess().desktop, false);
  assert.equal(createFileAccess({ bridge: { invoke } }).desktop, true);
});

test("in a browser a save is a download, exactly as before", async () => {
  const downloads = [];
  const files = createFileAccess({ download: (...a) => downloads.push(a) });
  const bytes = new Uint8Array([1, 2, 3]);
  assert.deepEqual(await files.save(bytes, "ndn.logo-tile-2x4-emboss-neuraldeep.3mf", "3mf"), {
    saved: true,
    name: "ndn.logo-tile-2x4-emboss-neuraldeep.3mf",
    path: null,
  });
  assert.deepEqual(downloads, [[bytes, "ndn.logo-tile-2x4-emboss-neuraldeep.3mf", "3mf"]]);
  assert.equal(files.open, null, "the browser keeps its file inputs");
});

test("the browser download is an <a download> with the file's media type", () => {
  const made = [];
  const revoked = [];
  const clicked = [];
  const body = { append: (a) => made.push(a) };
  const doc = {
    body,
    createElement: (tag) => ({ tag, click() { clicked.push(this.download); }, remove() { this.removed = true; } }),
  };
  const blobs = [];
  const urls = {
    createObjectURL: (blob) => (blobs.push(blob), "blob:test/1"),
    revokeObjectURL: (u) => revoked.push(u),
  };
  let pending = null;
  browserDownload(new Uint8Array([7]), "coupon.stl", "stl", { doc, urls, later: (fn) => (pending = fn) });
  assert.equal(made.length, 1);
  assert.equal(made[0].tag, "a");
  assert.equal(made[0].href, "blob:test/1");
  assert.equal(made[0].download, "coupon.stl");
  assert.equal(made[0].hidden, true);
  assert.equal(made[0].removed, true);
  assert.deepEqual(clicked, ["coupon.stl"]);
  assert.equal(blobs[0].type, MIME.stl);
  assert.equal(blobs[0].size, 1);
  pending();
  assert.deepEqual(revoked, ["blob:test/1"]);
});

test("in the desktop app a save goes to the save_file command with the bytes and two headers", async () => {
  const bridge = mockBridge({ [COMMANDS.save]: { name: "tile copy.3mf", path: "/home/u/Downloads/tile copy.3mf" } });
  const files = createFileAccess({ bridge, download: () => assert.fail("no browser download in the desktop app") });
  const bytes = new Uint8Array([80, 75, 3, 4]);
  const saved = await files.save(bytes, "ndn.logo-tile-2x4 Über.3mf", "3mf");
  assert.deepEqual(saved, { saved: true, name: "tile copy.3mf", path: "/home/u/Downloads/tile copy.3mf" });
  assert.equal(bridge.calls.length, 1);
  const [call] = bridge.calls;
  assert.equal(call.cmd, "save_file");
  assert.equal(call.args, bytes, "the bytes are the raw request body, not JSON");
  assert.deepEqual(call.options, {
    headers: { [NAME_HEADER]: "ndn.logo-tile-2x4%20%C3%9Cber.3mf", [FORMAT_HEADER]: "3mf" },
  });
  // Header values must be ASCII; the Rust side decodes the name (files.rs decode_name).
  assert.match(call.options.headers[NAME_HEADER], /^[\x21-\x7e]*$/);
});

test("a cancelled save dialog saves nothing and says so", async () => {
  const files = createFileAccess({ bridge: mockBridge({ [COMMANDS.save]: null }) });
  const saved = await files.save(new Uint8Array([1]), "coupon.stl", "stl");
  assert.deepEqual(saved, { saved: false, name: "coupon.stl", path: null });
  assert.deepEqual(savedText(saved, "1 KB"), ["Not saved: the save dialog was closed without choosing a file."]);
});

test("the status line names the saved file: the name in a browser, the path in the desktop app", () => {
  assert.deepEqual(savedText({ saved: true, name: "a.3mf", path: null }, "12 KB"), ["Saved a.3mf", "12 KB"]);
  assert.deepEqual(savedText({ saved: true, name: "a.3mf", path: "/tmp/a.3mf" }, "12 KB"), ["Saved /tmp/a.3mf", "12 KB"]);
});

test("the desktop adapter saves only the formats the app knows and passes its errors on", async () => {
  const bridge = mockBridge({ [COMMANDS.save]: new Error("cannot save /x/tile.3mf: permission denied") });
  const files = createFileAccess({ bridge });
  await assert.rejects(files.save(new Uint8Array([1]), "x.exe", "exe"), /cannot save a exe file/);
  assert.equal(bridge.calls.length, 0, "an unknown format never reaches the app");
  for (const format of ["3mf", "stl", "json"]) assert.ok(format in MIME);
  const err = await files.save(new Uint8Array([1]), "tile.3mf", "3mf").catch((e) => e);
  assert.equal(errorText(err), "cannot save /x/tile.3mf: permission denied");
});

test("opening asks pick_file, then reads the bytes only when the page asks for them", async () => {
  const svg = new TextEncoder().encode("<svg xmlns='http://www.w3.org/2000/svg'/>");
  const bridge = mockBridge({
    [COMMANDS.pick]: { name: "logo.svg", size: svg.length },
    [COMMANDS.read]: () => svg.buffer.slice(0),
  });
  const files = createFileAccess({ bridge });
  const file = await files.open("svg", 4 * 1024 * 1024);
  assert.equal(file.name, "logo.svg");
  assert.equal(file.size, svg.length);
  assert.deepEqual(bridge.calls.map((c) => c.cmd), ["pick_file"]);
  assert.deepEqual(bridge.calls[0].args, { kind: "svg" });
  const buffer = await file.arrayBuffer();
  assert.ok(buffer instanceof ArrayBuffer);
  assert.deepEqual(new Uint8Array(buffer), svg);
  assert.deepEqual(bridge.calls[1], { cmd: "read_picked_file", args: { kind: "svg", maxBytes: 4 * 1024 * 1024 }, options: undefined });
});

test("a fit profile opens as text", async () => {
  const json = '{"kind":"fit","id":"mine"}';
  const bridge = mockBridge({
    [COMMANDS.pick]: { name: "fit.json", size: json.length },
    // The postMessage fallback answers raw bytes as an array of numbers.
    [COMMANDS.read]: () => Array.from(new TextEncoder().encode(json)),
  });
  const file = await createFileAccess({ bridge }).open("fit", 1024 * 1024);
  assert.equal(await file.text(), json);
  assert.deepEqual(bridge.calls[1].args, { kind: "fit", maxBytes: 1024 * 1024 });
});

test("a cancelled open dialog gives no file, and unknown kinds never reach the app", async () => {
  const bridge = mockBridge({ [COMMANDS.pick]: null });
  const files = createFileAccess({ bridge });
  assert.equal(await files.open("svg", 10), null);
  await assert.rejects(files.open("exe", 10), /cannot open a exe file/);
  assert.equal(bridge.calls.length, 1);
});

test("a file too large for the page is never read", async () => {
  // The page compares `size` with the core's limit before it reads (main.js
  // useFile and useFitFile); the read command is only called on demand.
  const bridge = mockBridge({ [COMMANDS.pick]: { name: "huge.svg", size: 50 * 1024 * 1024 }, [COMMANDS.read]: () => assert.fail("read") });
  const file = await createFileAccess({ bridge }).open("svg", 4 * 1024 * 1024);
  assert.equal(file.size, 50 * 1024 * 1024);
  assert.deepEqual(bridge.calls.map((c) => c.cmd), ["pick_file"]);
});

test("reply bytes come from an ArrayBuffer, a view or an array of numbers", () => {
  assert.deepEqual(replyBytes(new Uint8Array([1, 2]).buffer), new Uint8Array([1, 2]));
  assert.deepEqual(replyBytes(new Uint8Array([0, 1, 2, 3]).subarray(1, 3)), new Uint8Array([1, 2]));
  assert.deepEqual(replyBytes([1, 2]), new Uint8Array([1, 2]));
  assert.throws(() => replyBytes(null), /no file bytes/);
  assert.throws(() => replyBytes({ name: "x" }), /no file bytes/);
});

test("picked files keep the File shape the page reads", async () => {
  const bridge = mockBridge({ [COMMANDS.read]: () => new Uint8Array([104, 105]) });
  const file = pickedFile(bridge, "fit", { name: "p.json", size: "2" }, 10);
  assert.equal(file.size, 2);
  assert.equal(await file.text(), "hi");
});

test("errors read the same whether Tauri rejects with a string or an Error", () => {
  assert.equal(errorText("no file is chosen"), "no file is chosen");
  assert.equal(errorText(new Error("boom")), "boom");
  assert.equal(errorText(42), "42");
});
