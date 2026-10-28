// The page: compiles the core once, starts the core worker, shows the viewer and
// a control row for the brick size, and reports what the core built in the status
// line. Geometry comes only from the worker (src/worker.js).

import wasmUrl from "./wasm/openbrick_wasm_bg.wasm?url";
import { CoreClient } from "./client.js";
import { parseStuds } from "./protocol.js";
import {
  buildingParts,
  elapsedText,
  readBackend,
  readSize,
  SLOW_BUILD_MS,
  statusParts,
  stepStuds,
  writeQuery,
} from "./ui.js";
import { createViewer } from "./viewer.js";

const $ = (id) => document.getElementById(id);
const status = $("status");
const form = $("controls");
const inputs = { width: $("width"), length: $("length") };

/**
 * State for the smoke test (apps/web/smoke.mjs) and for anyone poking at the page
 * from the console. `brick` is the last brick shown, without its arrays.
 */
const page = {
  state: "loading",
  kernel: null,
  backend: null,
  brick: null,
  builds: 0,
  restarts: 0,
  traps: 0,
  warnings: [],
  error: null,
  stats: () => null,
};
window.__web = page;

function setState(state) {
  page.state = state;
  document.body.dataset.state = state;
}

function showStatus(parts, kind = "") {
  status.className = kind;
  status.replaceChildren(
    ...parts.map((text) => {
      const span = document.createElement("span");
      span.textContent = text;
      return span;
    }),
  );
}

/** Updates the status line while the newest build runs; one at a time. */
let ticker = null;

function stopTicker() {
  clearInterval(ticker);
  ticker = null;
}

/**
 * Show "Building w × l…" and, once the build is slow, a note on cancelling and the
 * seconds it has run. The note is added once, so the live region announces it
 * once; the seconds then change only in a span hidden from assistive technology.
 */
function showBuilding(size, startedAt) {
  stopTicker();
  showStatus(buildingParts(size, 0));
  let seconds = null;
  ticker = setInterval(() => {
    const ms = performance.now() - startedAt;
    if (ms < SLOW_BUILD_MS) return;
    if (!seconds) {
      showStatus(buildingParts(size, ms));
      seconds = document.createElement("span");
      seconds.setAttribute("aria-hidden", "true");
      status.firstElementChild.after(seconds);
    }
    seconds.textContent = elapsedText(ms);
  }, 250);
}

function fail(err) {
  stopTicker();
  page.error = err.message;
  setState("error");
  showStatus([`Error: ${err.message}`], "bad");
}

/** Compile the module once; every worker (including replacements) instantiates it. */
async function compileCore(url) {
  try {
    return await WebAssembly.compileStreaming(fetch(url));
  } catch {
    // compileStreaming needs the application/wasm content type; some static hosts
    // do not send it.
    const response = await fetch(url);
    if (!response.ok) throw new Error(`could not load ${url}: HTTP ${response.status}`);
    return WebAssembly.compile(await response.arrayBuffer());
  }
}

async function main() {
  const { backend: wanted, errors: backendErrors } = readBackend(location.search);
  page.warnings.push(...backendErrors);
  showStatus(["Loading the core…"]);
  const [module, viewer] = await Promise.all([
    compileCore(wasmUrl),
    createViewer($("stage"), { backend: wanted, onDeviceLost: fail }),
  ]);
  page.backend = viewer.backend;
  page.stats = () => viewer.stats();
  if (wanted !== "auto" && viewer.backend !== wanted) {
    page.warnings.push(`asked for ${wanted}, got ${viewer.backend}`);
  }

  const client = new CoreClient({
    createWorker: () => new Worker(new URL("./worker.js", import.meta.url), { type: "module" }),
    module,
  });
  const info = await client.start();
  page.kernel = info.kernel;
  viewer.setStandard(info.standard);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => viewer.applyTheme());

  // Limits come from the core, not from this page.
  const { width, length, errors } = readSize(location.search, info.maxStuds);
  page.warnings.push(...errors);
  for (const [field, input] of Object.entries(inputs)) {
    input.min = "1";
    input.max = String(info.maxStuds);
    input.value = String(field === "width" ? width : length);
    input.disabled = false;
  }
  for (const button of form.querySelectorAll("button")) button.disabled = false;
  $("limits").textContent = `1 to ${info.maxStuds} studs`;

  /** The size in the inputs, or null (with the bad input marked) if one is invalid. */
  function sizeFromInputs() {
    const size = {};
    for (const [field, input] of Object.entries(inputs)) {
      try {
        size[field] = parseStuds(input.value, info.maxStuds, field);
        input.removeAttribute("aria-invalid");
      } catch {
        input.setAttribute("aria-invalid", "true");
        return null;
      }
    }
    return size;
  }

  async function build() {
    const size = sizeFromInputs();
    if (!size) {
      stopTicker();
      showStatus([`Width and length must be whole numbers from 1 to ${info.maxStuds}.`], "bad");
      return;
    }
    history.replaceState(null, "", writeQuery(location.search, size));
    setState("building");
    showBuilding(size, performance.now());
    let result;
    try {
      result = await client.buildBrick(size.width, size.length);
    } catch (err) {
      fail(err);
      return;
    } finally {
      page.restarts = client.restarts;
      page.traps = client.traps;
    }
    if (result === null) return; // superseded by a newer request, which owns the status line
    stopTicker();
    viewer.showBrick(result);
    const { positions, indices, ...summary } = result;
    page.brick = summary;
    page.builds++;
    page.error = null;
    setState(client.busy ? "building" : "ready");
    showStatus(statusParts(result, viewer.backend));
  }

  form.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-step]");
    if (!button) return;
    const input = inputs[button.dataset.for];
    const current = Number.parseInt(input.value, 10);
    input.value = String(stepStuds(current, Number(button.dataset.step), info.maxStuds));
    build();
  });
  for (const input of Object.values(inputs)) input.addEventListener("input", build);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    build();
  });

  await build();
}

main().catch(fail);
