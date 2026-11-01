// Desktop smoke test (apps/desktop): starts the debug desktop app with
// tauri-driver and drives it over WebDriver, the way docs/plan/09 tier 1 asks
// for, on Linux under Xvfb (WebKitGTK and its WebKitWebDriver):
//
//   xvfb-run -a -s "-screen 0 1600x1000x24" pnpm --dir apps/desktop smoke [--no-build] [--screenshots <dir>]
//
// It checks that the playground runs inside the app (the core in its worker,
// the first tile built and equal to the native evaluation, and shown: the
// tile's colour is in the screenshot of the 3D view), that the strict
// Content Security Policy is in force and the page needs nothing it forbids,
// and that the desktop adapter (apps/web/src/files.js) routes every save and
// open through the app's commands: Download 3MF and STL and the coupon 3MF
// write byte-identical copies of what `native_eval` exports (the coupon 3MF
// also over Tauri's postMessage fallback, which carries the bytes as a JSON
// array of numbers), an SVG and a fit profile open from the open dialog, a
// file over the core's limit is refused before it is read, a cancelled open
// changes nothing, a part document opens from the open dialog and, given a
// preset and saved through the save dialog, is byte-identical to what
// `native_eval` saves, and a project link does not navigate the window away.
//
// WebDriver cannot reach a native dialog, so the debug app is started with
// OPENBRICK_DESKTOP_TEST_DIALOGS set to a temporary folder: saves land there
// under the suggested name, and an open picks open.svg, open.json (a fit
// profile) or open.obpart.json (a part) from it
// (src-tauri/src/app.rs; release builds do not contain this). The native file
// dialogs themselves are checked by hand on each OS (README.md).
//
// Needs: the debug app (built here unless --no-build: `cargo xtask wasm`, then
// `pnpm build:debug`), tauri-driver (`cargo install tauri-driver --locked`, or
// TAURI_DRIVER=<path>), WebKitWebDriver (Debian and Ubuntu: webkit2gtk-driver),
// and a display (Xvfb).

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..");
const argv = process.argv.slice(2);
const option = (name, fallback) => {
  const i = argv.indexOf(name);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : fallback;
};
const build = !argv.includes("--no-build");
const shots = option("--screenshots", null);
const port = Number(option("--port", "4444"));
const exe = join(repo, "target", "debug", process.platform === "win32" ? "openbrick-desktop.exe" : "openbrick-desktop");
const driverBin = process.env.TAURI_DRIVER || "tauri-driver";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repo,
    encoding: "utf8",
    maxBuffer: 64 << 20,
    shell: process.platform === "win32",
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed (${result.status ?? result.signal})\n${result.stderr ?? ""}`);
  }
  return result.stdout ?? "";
}

const failures = [];
function check(condition, message) {
  if (!condition) failures.push(message);
  return condition;
}

/** A minimal W3C WebDriver client over fetch. */
class Session {
  constructor(origin) {
    this.origin = origin;
    this.id = null;
  }

  async cmd(method, path, body) {
    const response = await fetch(`${this.origin}${path}`, {
      method,
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const json = await response.json().catch(() => ({}));
    if (!response.ok || json.value?.error) throw new Error(`${method} ${path}: ${response.status} ${JSON.stringify(json.value ?? json)}`);
    return json.value;
  }

  async start(application) {
    const value = await this.cmd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application } } } });
    this.id = value.sessionId;
  }

  exec(script, ...args) {
    return this.cmd("POST", `/session/${this.id}/execute/sync`, { script, args });
  }

  /**
   * Click the element `css` as a person would: wait until it is enabled, scroll
   * it into the middle of its scroll containers (the side panel scrolls on
   * its own, and WebKitWebDriver does not always scroll nested containers
   * before a click; how far down a button sits depends on the fonts, so it
   * differs between machines), then click. A failed click reports where the
   * element was.
   */
  async click(css) {
    const where = await this.waitFor(
      `const el = document.querySelector(arguments[0]);
       if (!el || el.disabled) return null;
       el.scrollIntoView({ block: "center", inline: "center" });
       const r = el.getBoundingClientRect();
       return { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height), viewport: [innerWidth, innerHeight], hidden: el.closest("[hidden]") !== null };`,
      `${css} enabled`,
      30_000,
      css,
    );
    const element = await this.cmd("POST", `/session/${this.id}/element`, { using: "css selector", value: css });
    try {
      await this.cmd("POST", `/session/${this.id}/element/${Object.values(element)[0]}/click`, {});
    } catch (err) {
      throw new Error(`clicking ${css} failed (${JSON.stringify(where)}): ${err.message}`);
    }
  }

  async screenshot(path) {
    writeFileSync(path, Buffer.from(await this.cmd("GET", `/session/${this.id}/screenshot`), "base64"));
  }

  /** Poll `script` until it returns something truthy; throws with the last value after `timeoutMs`. */
  async waitFor(script, label, timeoutMs = 60_000, ...args) {
    const end = Date.now() + timeoutMs;
    let last;
    while (Date.now() < end) {
      last = await this.exec(script, ...args);
      if (last && !last.error) return last;
      if (last?.error) throw new Error(`${label}: ${last.error}`);
      await sleep(100);
    }
    throw new Error(`${label}: not within ${timeoutMs / 1000} s (last: ${JSON.stringify(last)})`);
  }

  async quit() {
    if (this.id) await this.cmd("DELETE", `/session/${this.id}`).catch(() => {});
  }
}

/** Decode an 8-bit RGB or RGBA PNG without interlacing (what WebKitWebDriver returns). */
function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error("the screenshot is not a PNG");
  let at = 8;
  let header = null;
  const idat = [];
  while (at + 8 <= buf.length) {
    const length = buf.readUInt32BE(at);
    const kind = buf.toString("latin1", at + 4, at + 8);
    const body = buf.subarray(at + 8, at + 8 + length);
    if (kind === "IHDR") header = { width: body.readUInt32BE(0), height: body.readUInt32BE(4), depth: body[8], type: body[9], interlace: body[12] };
    else if (kind === "IDAT") idat.push(body);
    else if (kind === "IEND") break;
    at += 12 + length;
  }
  const channels = { 2: 3, 6: 4 }[header?.type];
  if (header?.depth !== 8 || !channels || header.interlace !== 0) throw new Error(`unsupported PNG ${JSON.stringify(header)}`);
  const { width, height } = header;
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const data = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const out = data.subarray(y * stride, (y + 1) * stride);
    const prev = y > 0 ? data.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? out[x - channels] : 0;
      const b = prev ? prev[x] : 0;
      const c = prev && x >= channels ? prev[x - channels] : 0;
      let v = line[x];
      if (filter === 1) v += a;
      else if (filter === 2) v += b;
      else if (filter === 3) v += (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      } else if (filter !== 0) throw new Error(`PNG filter ${filter}`);
      out[x] = v & 255;
    }
  }
  return { width, height, channels, data };
}

/**
 * The share of the 3D view's pixels in the screenshot that are clearly blue,
 * as the body of the default tile is (the background, the grid and the white
 * logo are not). A WebGL canvas the webview does not show reads as 0.
 */
async function bodyShare(session) {
  const png = decodePng(Buffer.from(await session.cmd("GET", `/session/${session.id}/screenshot`), "base64"));
  const r = await session.exec("const r = document.getElementById('stage').getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height, dpr: devicePixelRatio };");
  const x0 = Math.round(r.x * r.dpr);
  const y0 = Math.round(r.y * r.dpr);
  const x1 = Math.min(png.width, Math.round((r.x + r.w) * r.dpr));
  const y1 = Math.min(png.height, Math.round((r.y + r.h) * r.dpr));
  let blue = 0;
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      const i = (y * png.width + x) * png.channels;
      if (png.data[i + 2] - png.data[i] > 80) blue++;
    }
  }
  return blue / Math.max(1, (x1 - x0) * (y1 - y0));
}

/** The page summary (window.__web), without functions. */
const WEB = "const w = window.__web; return w && JSON.parse(JSON.stringify({ ...w, stats: null }));";
/** Wait until a finished build in `mode` came after `after` builds. */
const READY = `
  const [mode, after] = arguments;
  const w = window.__web;
  if (!w) return null;
  if (w.state === "error") return { error: w.error };
  const count = mode === "tile" ? w.evaluations : w.couponBuilds;
  const thing = mode === "tile" ? w.part : w.coupon;
  return w.state === "ready" && w.mode === mode && count > after && thing && !thing.error ? JSON.parse(JSON.stringify({ ...w, stats: null })) : null;`;

function fileOf(dir, name) {
  const path = join(dir, name);
  if (!existsSync(path)) return null;
  const bytes = readFileSync(path);
  return { name, path, bytes, sha256: createHash("sha256").update(bytes).digest("hex") };
}

/** A saved file is byte-identical to the one native_eval wrote (same name, size and SHA-256). */
function checkFile(file, native, label) {
  if (!check(file !== null, `${label}: nothing was saved as ${native.name}`)) return;
  check(
    file.bytes.length === native.bytes && file.sha256 === native.sha256,
    `${label}: ${file.bytes.length} bytes sha256 ${file.sha256}, native ${native.bytes} bytes ${native.sha256}`,
  );
}

/** Click a save button and wait for the page to record the save; returns the file. */
async function save(session, selector, dir, native, label) {
  const before = (await session.exec(WEB)).downloads.length;
  await session.click(selector);
  const w = await session.waitFor(
    "const [n] = arguments; const w = window.__web; return w.downloads.length > n ? JSON.parse(JSON.stringify({ ...w, stats: null })) : null;",
    label,
    60_000,
    before,
  );
  const saved = w.downloads.at(-1);
  check(saved.name === native.name, `${label}: saved as ${saved.name}, native ${native.name}`);
  const status = await session.exec("return document.getElementById('status').textContent;");
  check(status.includes(join(dir, native.name)), `${label}: the status line does not name the saved path: "${status}"`);
  const file = fileOf(dir, native.name);
  checkFile(file, native, label);
  console.log(`# ${label}: saved ${join(dir, native.name)} (${file?.bytes.length} bytes)`);
  return file;
}

/**
 * Save what the run sees: WebDriver's screenshot of the page and, on Linux,
 * the X display as composited (ImageMagick `import`, if installed), which
 * also shows the WebGL canvas when the WebDriver snapshot leaves it out.
 */
async function screenshots(session, name) {
  await session.screenshot(join(shots, `${name}.png`));
  if (process.platform === "linux") {
    await sleep(500);
    spawnSync("import", ["-window", "root", join(shots, `${name}-display.png`)], { stdio: "ignore" });
  }
}

async function startDriver(env) {
  const driver = spawn(driverBin, ["--port", String(port), "--native-port", String(port + 1)], { env, stdio: ["ignore", "inherit", "inherit"] });
  const exited = new Promise((resolve) => driver.on("exit", (code) => resolve(code)));
  driver.on("error", (err) => console.error(`tauri-driver: ${err.message} (install it with cargo install tauri-driver --locked, or set TAURI_DRIVER)`));
  // tauri-driver answers /status by asking WebKitWebDriver, which starts a
  // moment later; asking too early only logs a refused connection.
  await sleep(1000);
  const end = Date.now() + 30_000;
  while (Date.now() < end) {
    const status = await fetch(`http://127.0.0.1:${port}/status`).then((r) => r.ok, () => false);
    if (status) return driver;
    if (driver.exitCode !== null) break;
    await sleep(200);
  }
  driver.kill();
  throw new Error(`tauri-driver did not answer on port ${port} (exit ${await Promise.race([exited, sleep(1000).then(() => "none")])})`);
}

async function main() {
  if (process.platform === "linux" && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    throw new Error("no display: run under Xvfb, for example `xvfb-run -a pnpm --dir apps/desktop smoke`");
  }
  if (build) {
    console.log("# building the core (cargo xtask wasm) and the debug app (pnpm build:debug)");
    run("cargo", ["xtask", "wasm"], { stdio: "inherit" });
    run("pnpm", ["build:debug"], { cwd: here, stdio: "inherit" });
  }
  if (!existsSync(exe)) throw new Error(`${exe} is missing: build it with pnpm --dir apps/desktop build:debug`);
  const native = JSON.parse(run("cargo", ["run", "-q", "--release", "-p", "openbrick-wasm", "--example", "native_eval"]));

  const dialogs = mkdtempSync(join(tmpdir(), "openbrick-desktop-smoke-"));
  if (shots) mkdirSync(shots, { recursive: true });
  const env = {
    ...process.env,
    OPENBRICK_DESKTOP_TEST_DIALOGS: dialogs,
  };
  // WebKitGTK's defaults draw WebGL under Xvfb (in software). The safe
  // rendering variables do not: with either set, WebKitGTK 2.52 under Xvfb
  // leaves the 3D view empty, so the test runs without them.
  for (const v of ["WEBKIT_DISABLE_DMABUF_RENDERER", "WEBKIT_DISABLE_COMPOSITING_MODE", "OPENBRICK_SAFE_RENDERING"]) delete env[v];
  const driver = await startDriver(env);
  const session = new Session(`http://127.0.0.1:${port}`);
  const label = "desktop";
  try {
    await session.start(exe);
    // The first tile: the Neuraldeep.net mark, emboss, example fit profile.
    let w = await session.waitFor(READY, `${label} first tile`, 180_000, "tile", 0);
    check(w.desktop === true, `${label}: the page does not see the desktop app (window.__web.desktop ${w.desktop})`);
    const where = await session.exec("return { protocol: location.protocol, host: location.host, tauri: typeof window.__TAURI__?.core?.invoke };");
    check(
      (where.protocol === "tauri:" && where.host === "localhost") || where.host === "tauri.localhost",
      `${label}: the page is served from ${where.protocol}//${where.host}, not the app's own origin`,
    );
    check(where.tauri === "function", `${label}: window.__TAURI__.core.invoke is ${where.tauri}`);
    const part = native.part.emboss;
    check(w.part.contentHash === part.contentHash, `${label}: content hash ${w.part.contentHash}, native ${part.contentHash}`);
    check(w.part.triangles === part.triangles, `${label}: ${w.part.triangles} triangles, native ${part.triangles}`);
    const got = w.part.meshes.map((m) => `${m.role} ${m.triangles} ${m.fingerprint}`).join(", ");
    const want = part.meshes.map((m) => `${m.role} ${m.triangles} ${m.fingerprint}`).join(", ");
    check(got === want, `${label}: meshes ${got}, native ${want}`);
    // The viewer drew the newest part (three.js counts what each frame submitted).
    const drawn = await session.waitFor(
      "const s = window.__web.stats(); return s && s.shown > 0 && s.drawn === s.shown ? s : null;",
      `${label} first frame`,
      30_000,
    );
    check(drawn.triangles >= part.triangles, `${label}: the frame drew ${drawn.triangles} triangles, the part has ${part.triangles}`);
    // And the webview shows it: the tile's blue body covers part of the 3D
    // view (8 percent at the default camera and window; the legend's swatch alone
    // is far below 1 percent).
    const share = await bodyShare(session);
    check(share > 0.03, `${label}: the 3D view shows no tile (${(share * 100).toFixed(2)} % blue pixels): WebGL draws but the webview does not show it`);
    console.log(`# ${label}: first tile ${w.part.triangles} triangles, frame ${drawn.triangles} triangles in ${drawn.drawCalls} draw calls, ${(share * 100).toFixed(1)} % of the 3D view is the tile's body, ${w.backend}, kernel ${w.kernel}`);

    // The Content Security Policy is in force (no eval, no style="" attributes),
    // and the page needs neither: the legend swatches get their colour through
    // the CSSOM.
    const csp = await session.exec(`
      let evalBlocked = false;
      try { new Function("return 1")(); } catch { evalBlocked = true; }
      const probe = document.createElement("div");
      probe.setAttribute("style", "color: rgb(1, 2, 3)");
      document.body.append(probe);
      const inlineStyle = getComputedStyle(probe).color === "rgb(1, 2, 3)";
      probe.remove();
      const swatch = document.querySelector("#legend .swatch");
      return { evalBlocked, inlineStyle, swatch: swatch && getComputedStyle(swatch).backgroundColor };`);
    check(csp.evalBlocked, `${label}: eval is allowed, so the Content Security Policy is not in force`);
    check(!csp.inlineStyle, `${label}: a style="" attribute applied, so style-src is not strict`);
    check(csp.swatch && csp.swatch !== "rgba(0, 0, 0, 0)", `${label}: the legend swatch has no colour (${csp.swatch})`);
    if (shots) await screenshots(session, "desktop-tile");

    // Saves go through the save dialog (scripted here) and write the core's bytes.
    await save(session, "#download-3mf", dialogs, native.exports.emboss3mf, `${label} 3MF`);
    await save(session, "#download-stl", dialogs, native.exports.embossStl, `${label} STL`);

    // Open an SVG from the open dialog: the CC0 sample mark under another name.
    const art = () => session.exec("const n = document.getElementById('art-note'); return { text: n.textContent, bad: n.classList.contains('bad'), artwork: window.__web.artwork, evaluations: window.__web.evaluations };");
    copyFileSync(join(repo, "parts", "assets", "mark.svg"), join(dialogs, "open.svg"));
    let before = (await art()).evaluations;
    await session.click("#drop");
    w = await session.waitFor(READY, `${label} opened SVG`, 60_000, "tile", before);
    let note = await art();
    check(note.artwork?.asset === "open.svg" && note.artwork.sample === null, `${label}: the opened SVG is not in use: ${JSON.stringify(note.artwork)}`);
    check(!note.bad && note.text.includes("open.svg"), `${label}: the art note after opening is "${note.text}"`);
    check(w.part.params.logo === "open.svg", `${label}: the tile was built from ${w.part.params.logo}`);

    // A file over the core's limit is refused before it is read; the tile keeps its artwork.
    writeFileSync(join(dialogs, "open.svg"), `<svg xmlns="http://www.w3.org/2000/svg">${" ".repeat(w.maxAssetBytes)}</svg>`);
    await session.click("#drop");
    note = await session.waitFor("const n = document.getElementById('art-note'); return n.classList.contains('bad') ? n.textContent : null;", `${label} too large`, 30_000);
    check(/open\.svg/.test(note) && /keeps/.test(note), `${label}: the too-large note is "${note}"`);
    check((await art()).artwork.asset === "open.svg", `${label}: a refused file replaced the artwork`);

    // A cancelled open (no open.svg) changes nothing.
    rmSync(join(dialogs, "open.svg"));
    before = await art();
    await session.click("#drop");
    await sleep(1000);
    const after = await art();
    check(after.text === before.text && after.artwork.asset === before.artwork.asset && after.evaluations === before.evaluations, `${label}: a cancelled open changed the page: ${JSON.stringify(after)}`);

    // A fit profile from the open dialog.
    copyFileSync(join(repo, "profiles", "fit", "example-mk4s-petg-0.4.json"), join(dialogs, "open.json"));
    await session.exec("const s = document.getElementById('fit'); s.value = 'load'; s.dispatchEvent(new Event('change'));");
    const fit = await session.waitFor("const s = document.getElementById('fit'); return s.value === 'mine' ? s.selectedOptions[0].textContent : null;", `${label} fit profile`, 30_000);
    check(fit.startsWith("My fit profile"), `${label}: the fit chooser shows "${fit}"`);

    // The part designer through the native dialogs: open a part document
    // (scripted: open.obpart.json), give it the "Pressed-in mark" preset on a
    // 2x6 tile, keep those values as a preset and save the part file, which
    // must be byte-identical to native_eval's (the same file the playground
    // saves, and the CLI builds with the hash the page then shows).
    const d = native.designer;
    copyFileSync(join(repo, "parts", "ndn.brand-tile.obpart.json"), join(dialogs, "open.obpart.json"));
    await session.exec("const s = document.getElementById('part-select'); s.value = 'open'; s.dispatchEvent(new Event('change'));");
    w = await session.waitFor(
      `const w = window.__web; return w.state === "ready" && w.design?.source === "file" && w.part?.contentHash === arguments[0] ? JSON.parse(JSON.stringify({ ...w, stats: null })) : null;`,
      `${label} opened part`,
      120_000,
      d.opened.contentHash,
    );
    check(w.design.id === "ndn.brand-tile" && w.design.file === "open.obpart.json", `${label}: opened ${JSON.stringify(w.design)}`);
    await session.click('button[data-for="p-size-1"][data-step="1"]');
    await session.click('button[data-for="p-size-1"][data-step="1"]');
    await session.exec("const s = document.getElementById('preset'); s.value = 'Pressed-in mark'; s.dispatchEvent(new Event('change'));");
    w = await session.waitFor(
      `const w = window.__web; return w.state === "ready" && w.part?.contentHash === arguments[0] ? JSON.parse(JSON.stringify({ ...w, stats: null })) : null;`,
      `${label} edited part`,
      120_000,
      d.edited.contentHash,
    );
    await session.exec("document.getElementById('preset-name').value = arguments[0];", d.presetName);
    await session.click("#add-preset");
    await session.waitFor("return window.__web.design?.presets?.includes(arguments[0]) && window.__web.state === 'ready';", `${label} preset added`, 60_000, d.presetName);
    await session.click("#save-part");
    w = await session.waitFor(
      `const w = window.__web; return w.saves.length > 0 && w.state === "ready" && w.part?.contentHash === arguments[0] ? JSON.parse(JSON.stringify({ ...w, stats: null })) : null;`,
      `${label} saved part`,
      120_000,
      d.savedPart.contentHash,
    );
    check(w.saves.at(-1).name === d.saved.name, `${label}: the part saved as ${w.saves.at(-1).name}, native ${d.saved.name}`);
    const savedNote = await session.exec("return document.getElementById('save-note').textContent;");
    check(savedNote.includes(join(dialogs, d.saved.name)), `${label}: the save note does not name the saved path: "${savedNote}"`);
    checkFile(fileOf(dialogs, d.saved.name), d.saved, `${label} saved part`);
    console.log(`# ${label}: opened a part from the open dialog, gave it a preset and saved it byte-identical to native (${d.saved.bytes} bytes)`);

    // A project link opens outside (the system browser, if there is one); the window stays on the app.
    await session.exec("document.querySelector('footer a[href^=\"https://github.com/\"]').click();");
    await sleep(1000);
    const still = await session.exec("return location.protocol + '//' + location.host;");
    check(still === `${where.protocol}//${where.host}`, `${label}: a link navigated the window to ${still}`);

    // The coupon and its 3MF.
    await session.click("#tab-coupon");
    w = await session.waitFor(READY, `${label} coupon`, 120_000, "coupon", 0);
    check(w.coupon.contentHash === native.coupon.contentHash, `${label}: coupon hash ${w.coupon.contentHash}, native ${native.coupon.contentHash}`);
    await save(session, "#download-coupon-3mf", dialogs, native.exports.coupon3mf, `${label} coupon 3MF`);
    if (shots) await screenshots(session, "desktop-coupon");

    // Tauri's IPC fallback. When a request over its custom protocol fails,
    // Tauri's IPC script switches the page to postMessage for good, and a
    // Uint8Array then travels as a JSON array of numbers; save_file accepts
    // that too (src-tauri/src/app.rs, save_bytes). Make the page's fetch
    // refuse the IPC origin, as a blocked custom protocol would, and save the
    // coupon 3MF (the largest file) again: it must arrive byte-identical, and
    // the fetch must have been refused, so the save really took the fallback.
    // This is the last step, because the page stays on postMessage.
    rmSync(join(dialogs, native.exports.coupon3mf.name));
    await session.exec(`
      const fetch = window.fetch;
      window.__ipcRefused = 0;
      window.fetch = function (input, init) {
        const url = String(input instanceof Request ? input.url : input);
        if (url.startsWith("ipc:") || url.startsWith("http://ipc.localhost")) {
          window.__ipcRefused += 1;
          return Promise.reject(new TypeError("smoke test: the IPC custom protocol is blocked"));
        }
        return fetch.call(window, input, init);
      };`);
    await save(session, "#download-coupon-3mf", dialogs, native.exports.coupon3mf, `${label} coupon 3MF over the IPC fallback`);
    const refused = await session.exec("return window.__ipcRefused;");
    check(refused >= 1, `${label}: the save never tried the custom protocol (${refused} refused requests), so the fallback was not tested`);
    console.log(`# ${label}: the page's fetch refused ${refused} IPC request(s); the save went over postMessage`);
  } finally {
    await session.quit();
    driver.kill();
    rmSync(dialogs, { recursive: true, force: true });
  }
}

main().then(
  () => {
    if (failures.length) {
      for (const f of failures) console.error(`FAIL ${f}`);
      process.exit(1);
    }
    console.log("desktop smoke: ok");
  },
  (err) => {
    for (const f of failures) console.error(`FAIL ${f}`);
    console.error(err.stack ?? err);
    process.exit(1);
  },
);
