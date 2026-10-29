// The playground page (docs/plan/06 sections 5, 11 and 13; docs/plan/05 sections
// 4 to 7): compiles the core once, starts the core worker, builds the logo tile
// controls from the part document, and shows what the core returns: the part in
// 3D (one mesh per colour role and the red overlay of artwork that does not
// print as drawn), its stats, the validation report and the downloads; or the
// calibration coupon with its cell table. Geometry, validation and the files
// come only from the worker (src/worker.js); this page never computes them.

import wasmUrl from "./wasm/openbrick_wasm_bg.wasm?url";
import { CoreClient } from "./client.js";
import {
  assetSizeError,
  controlsFor,
  couponDownloadName,
  defaultValues,
  downloadName,
  looksLikeSvg,
  paramOverrides,
  visibility,
} from "./params.js";
import {
  couponRows,
  couponStats,
  downloadBlockedReason,
  elapsedText,
  evaluatingParts,
  findingView,
  fitLabel,
  partStats,
  PRINT_TEST_GUIDE_URL,
  readBackend,
  readMode,
  recordCommand,
  removedText,
  reportSummary,
  SLOW_BUILD_MS,
  sortedFindings,
  stepStuds,
  writeMode,
} from "./ui.js";
import { sampleFile, sampleLabel, sizeForSample, sizeNote } from "./samples.js";
import { createViewer } from "./viewer.js";

const $ = (id) => document.getElementById(id);

/** A DOM element with attributes (`on*` functions become listeners) and children. */
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith("on") && typeof v === "function") node.addEventListener(k.slice(2), v);
    else if (k === "text") node.textContent = v;
    else if (k === "className") node.className = v;
    else node.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children.flat()) if (c !== null && c !== undefined && c !== false) node.append(c);
  return node;
}

/** Debounce of re-evaluation after a change (docs/plan/06 section 11: live preview, debounced). */
const EVALUATE_DELAY_MS = 150;
/** Largest fit profile file read; real ones are about 2 KB. */
const MAX_FIT_BYTES = 1024 * 1024;
const MIME = { "3mf": "model/3mf", stl: "model/stl" };
const POCKET_LABELS = {
  cross: "Cross: the default, the smallest",
  crossAndDiagonal: "Cross and diagonal: needs a larger bed",
  full: "Full cross: every pocket, needs a large bed",
};

/**
 * State for the smoke test (apps/web/smoke.mjs) and for anyone poking at the page
 * from the console. `part` and `coupon` summarise what is shown, without arrays.
 */
const page = {
  state: "loading",
  mode: "tile",
  kernel: null,
  backend: null,
  part: null,
  coupon: null,
  evaluations: 0,
  /** Tile replies dropped because the controls changed while they were built. */
  staleReplies: 0,
  couponBuilds: 0,
  restarts: 0,
  traps: 0,
  warnings: [],
  error: null,
  downloads: [],
  /** The artwork in use: { sample (its id, null for the user's SVG), asset, notice (shown) }. */
  artwork: null,
  /** The core's artwork size limit (max_asset_bytes()), once loaded. */
  maxAssetBytes: null,
  stats: () => null,
};
window.__web = page;

function setState(s) {
  page.state = s;
  document.body.dataset.state = s;
}

const status = $("status");
function showStatus(parts, kind = "") {
  status.className = kind;
  status.replaceChildren(...parts.map((text) => el("span", { text })));
}

/** Updates the status line while the newest build runs; one at a time. */
let ticker = null;
function stopTicker() {
  clearInterval(ticker);
  ticker = null;
}

/**
 * Show "<what>…" and, once the build is slow, a note on cancelling and the
 * seconds it has run. The note is added once, so the live region announces it
 * once; the seconds then change only in a span hidden from assistive technology.
 */
function showBuilding(what, startedAt) {
  stopTicker();
  showStatus(evaluatingParts(what, 0));
  let seconds = null;
  ticker = setInterval(() => {
    const ms = performance.now() - startedAt;
    if (ms < SLOW_BUILD_MS) return;
    if (!seconds) {
      showStatus(evaluatingParts(what, ms));
      seconds = el("span", { "aria-hidden": "true" });
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

/** Hand bytes to the browser as a download. */
function save(bytes, name, format) {
  const url = URL.createObjectURL(new Blob([bytes], { type: MIME[format] ?? "application/octet-stream" }));
  const a = el("a", { href: url, download: name, hidden: true });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

const kb = (n) => `${new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 }).format(n / 1024)} KB`;

async function main() {
  const { backend: wanted, errors: backendErrors } = readBackend(location.search);
  const { mode: firstMode, errors: modeErrors } = readMode(location.search);
  page.warnings.push(...backendErrors, ...modeErrors);
  showStatus(["Loading the core…"]);
  const [module, viewer] = await Promise.all([
    compileCore(wasmUrl),
    createViewer($("stage"), { backend: wanted, onDeviceLost: fail }),
  ]);
  page.backend = viewer.backend;
  page.stats = () => viewer.stats();
  if (wanted !== "auto" && viewer.backend !== wanted) page.warnings.push(`asked for ${wanted}, got ${viewer.backend}`);

  const client = new CoreClient({
    createWorker: () => new Worker(new URL("./worker.js", import.meta.url), { type: "module" }),
    module,
  });
  const info = await client.start();
  page.kernel = info.kernel;
  viewer.setStandard(info.standard);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => viewer.applyTheme());
  const data = await client.bundled();
  page.maxAssetBytes = data.maxAssetBytes;
  const backendName = viewer.backend === "webgpu" ? "WebGPU" : "WebGL 2";
  const syncCounters = () => {
    page.restarts = client.restarts;
    page.traps = client.traps;
  };

  // ------------------------------------------------------------------ state

  const doc = JSON.parse(data.logoTilePart);
  const spec = controlsFor(doc, { maxStuds: info.maxStuds });
  const fileControl = spec.controls.find((c) => c.kind === "file");
  /** The stud grid a sample's tile shape applies to (the logo tile's size). */
  const gridControl = spec.controls.find((c) => c.kind === "grid2");
  const exampleFit = JSON.parse(data.exampleFitProfile);
  /**
   * The artwork samples, the Neuraldeep.net mark first: the playground is
   * Neural Deep Network's public demo, so it is the default. A sample's
   * drawing is sent under its asset name in parts/ (src/samples.js picks the
   * drawing for the tile), so the native tools give the same content hash.
   */
  const samples = data.samples;
  const encoder = new TextEncoder();
  /** The artwork of `sample` for a tile of `size`. */
  const sampleArtwork = (sample, size) => {
    const file = sampleFile(sample, size);
    return { name: file.asset, bytes: encoder.encode(file.svg), label: sampleLabel(sample, file), sample };
  };
  const defaults = defaultValues(spec);
  const state = {
    values: defaults,
    artwork: sampleArtwork(samples[0], gridControl ? defaults[gridControl.name] : null),
    /** A note for the status line after the next build (a sample changed the size). */
    statusNote: null,
    process: data.processProfiles.some((p) => p.id === exampleFit.process) ? exampleFit.process : data.processProfiles[0].id,
    fit: "example",
    fits: { example: data.exampleFitProfile, mine: null },
    colors: { body: data.roleColors.body, accent: data.roleColors.accent },
    /** The request and result on screen, and whether the controls changed since. */
    request: null,
    result: null,
    resultArtwork: null,
    dirty: true,
    error: null,
    exporting: false,
    coupon: {
      process: exampleFit.process,
      pockets: data.couponChoices.defaultPocketSet,
      bed: [...data.couponChoices.defaults.bedMm],
      key: null,
      request: null,
      result: null,
      error: null,
      /** A bed size field holds something that is not a positive number. */
      invalid: false,
      exporting: false,
    },
  };

  // ------------------------------------------------------------------ tile controls

  const form = $("tile-form");
  /** Wrappers by parameter name, to show or hide them. */
  const wrappers = new Map();
  /** Inputs whose value is invalid, by id, with the message to show. */
  const invalid = new Map();

  const artNote = el("p", { className: "note", id: "art-note", "aria-live": "polite" });
  const fileInput = el("input", { type: "file", id: "art-file", accept: fileControl?.accept ?? ".svg", hidden: true });
  /** The sample chooser; "own" (never chosen, only shown) stands for the user's SVG. */
  const sampleSelect = el(
    "select",
    { id: "sample", "aria-describedby": "art-note" },
    samples.map((s) => el("option", { value: s.id, text: s.label })),
    el("option", { value: "own", disabled: true, text: "Your SVG (drop or choose it above)" }),
  );
  sampleSelect.addEventListener("change", () => useSample(sampleSelect.value));
  /** The notice of a Neuraldeep.net mark, under the drop zone while one is in use. */
  const artNotice = el("p", { className: "help", id: "art-notice", hidden: true });
  /** The number inputs of each stud grid, by parameter name, so a sample can set the size. */
  const gridInputs = new Map();

  function describe(c, id) {
    return c.help ? el("p", { className: "help", id: `${id}-help`, text: c.help }) : null;
  }

  function stepper(c, axis, i) {
    const id = `p-${c.name}-${i}`;
    const input = el("input", {
      id,
      type: "number",
      inputmode: "numeric",
      step: 1,
      min: axis.min,
      max: axis.max,
      value: state.values[c.name][i],
      "aria-describedby": `${c.name}-limits`,
    });
    if (!gridInputs.has(c.name)) gridInputs.set(c.name, []);
    gridInputs.get(c.name)[i] = input;
    const set = (n) => {
      input.value = String(n);
      readGrid();
    };
    const less = i === 0 ? "One stud narrower" : "One stud shorter";
    const more = i === 0 ? "One stud wider" : "One stud longer";
    const step = (d) => set(stepStuds(Number.parseInt(input.value, 10), d, axis.max, axis.min));
    const readGrid = () => {
      const text = input.value.trim();
      const n = /^\d+$/.test(text) ? Number(text) : NaN;
      if (!Number.isInteger(n) || n < axis.min || n > axis.max) {
        input.setAttribute("aria-invalid", "true");
        invalid.set(id, `${axis.label} must be a whole number of studs from ${axis.min} to ${axis.max}.`);
      } else {
        input.removeAttribute("aria-invalid");
        invalid.delete(id);
        state.values[c.name] = state.values[c.name].map((v, k) => (k === i ? n : v));
      }
      changed();
    };
    input.addEventListener("input", readGrid);
    return el(
      "div",
      { className: "stepper" },
      el("label", { for: id, text: `${axis.label} (studs)` }),
      el("button", { type: "button", className: "icon", "aria-label": less, "data-for": id, "data-step": -1, onclick: () => step(-1) }, "−"),
      input,
      el("button", { type: "button", className: "icon", "aria-label": more, "data-for": id, "data-step": 1, onclick: () => step(1) }, "+"),
    );
  }

  function control(c) {
    const id = `p-${c.name}`;
    switch (c.kind) {
      case "grid2": {
        const limits = c.axes.map((a) => `${a.label.toLowerCase()} ${a.min} to ${a.max}`).join(", ");
        const capped = c.axes.some((a) => a.capped) ? " (the browser core's limit)" : "";
        return el(
          "div",
          { className: "field" },
          el("div", { className: "row" }, c.axes.map((a, i) => stepper(c, a, i))),
          el("p", { className: "help", id: `${c.name}-limits`, text: `Studs: ${limits}${capped}.` }),
        );
      }
      case "choice":
        return el(
          "div",
          { className: "field" },
          el("span", { className: "label", id: `${id}-label`, text: c.label }),
          el(
            "div",
            { className: "choices", role: "radiogroup", "aria-labelledby": `${id}-label`, "aria-describedby": c.help ? `${id}-help` : null },
            c.options.map((o) =>
              el(
                "label",
                {},
                el("input", {
                  type: "radio",
                  name: id,
                  value: o.value,
                  checked: state.values[c.name] === o.value,
                  onchange: (e) => {
                    state.values[c.name] = e.target.value;
                    changed();
                  },
                }),
                o.label,
              ),
            ),
          ),
          describe(c, id),
        );
      case "range": {
        const out = el("output", { for: id, id: `${id}-out` });
        const show = () => {
          out.textContent = `${Number(state.values[c.name]).toFixed(2)} ${c.unit}`.trim();
        };
        show();
        return el(
          "div",
          { className: "field" },
          el("label", { for: id, text: c.unit ? `${c.label} (${c.unit})` : c.label }),
          el(
            "div",
            { className: "slider" },
            el("input", {
              id,
              type: "range",
              min: c.min,
              max: c.max,
              step: c.step,
              value: state.values[c.name],
              "aria-describedby": c.help ? `${id}-help` : null,
              oninput: (e) => {
                state.values[c.name] = Number(e.target.value);
                show();
                changed();
              },
            }),
            out,
          ),
          describe(c, id),
        );
      }
      case "number": {
        const range = [c.min, c.max].every((x) => typeof x === "number") ? `${c.min} to ${c.max}` : null;
        const input = el("input", {
          id,
          type: "number",
          inputmode: "decimal",
          min: c.min,
          max: c.max,
          step: c.step,
          value: state.values[c.name],
          "aria-describedby": `${id}-help`,
          oninput: (e) => {
            const n = Number(e.target.value);
            const ok = e.target.value.trim() !== "" && Number.isFinite(n) && !(n < c.min) && !(n > c.max);
            if (ok) {
              e.target.removeAttribute("aria-invalid");
              invalid.delete(id);
              state.values[c.name] = n;
            } else {
              e.target.setAttribute("aria-invalid", "true");
              invalid.set(id, `${c.label} must be a number${range ? ` from ${range}` : ""}${c.unit ? ` ${c.unit}` : ""}.`);
            }
            changed();
          },
        });
        const help = [range ? `${range} ${c.unit}`.trim() : "", c.help].filter(Boolean).join(". ");
        return el(
          "div",
          { className: "field" },
          el("label", { for: id, text: c.unit ? `${c.label} (${c.unit})` : c.label }),
          input,
          el("p", { className: "help", id: `${id}-help`, text: help }),
        );
      }
      case "file": {
        const drop = el(
          "button",
          {
            type: "button",
            id: "drop",
            "aria-describedby": "art-note",
            onclick: () => fileInput.click(),
          },
          el("strong", { text: "Drop an SVG here, or choose a file" }),
          el("span", { text: c.help || "Single-colour SVG" }),
        );
        drop.addEventListener("dragenter", (e) => {
          e.preventDefault();
          drop.classList.add("over");
        });
        drop.addEventListener("dragover", (e) => {
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
        });
        drop.addEventListener("dragleave", () => drop.classList.remove("over"));
        drop.addEventListener("drop", (e) => {
          e.preventDefault();
          drop.classList.remove("over");
          useFile(e.dataTransfer.files?.[0]);
        });
        return el(
          "div",
          { className: "field" },
          el("span", { className: "label", text: c.label }),
          drop,
          artNotice,
          fileInput,
          el("div", { className: "artwork-row" }, el("label", { for: "sample", text: "Sample artwork" }), sampleSelect),
          artNote,
        );
      }
      default:
        return el("p", { className: "note", text: `${c.label}: this parameter type (${c.kind}) has no control here yet; its default is used.` });
    }
  }

  for (const g of spec.groups) {
    const fieldset = el("fieldset", {}, el("legend", { text: g.name }));
    for (const c of g.controls) {
      const wrap = el("div", { "data-param": c.name }, control(c));
      wrappers.set(c.name, wrap);
      fieldset.append(wrap);
    }
    form.append(fieldset);
  }
  form.addEventListener("submit", (e) => e.preventDefault());

  function showArtNote(text, bad = false) {
    artNote.textContent = text;
    artNote.className = bad ? "note bad" : "note";
  }

  /** The chooser, the notice line and the page summary follow the artwork in use. */
  function renderArtwork() {
    const sample = state.artwork.sample ?? null;
    sampleSelect.value = sample?.id ?? "own";
    if (sample?.notice) {
      artNotice.replaceChildren(`${sample.notice} `, el("a", { href: sample.noticeUrl, target: "_blank", rel: "noopener" }, "Read the notice"));
      artNotice.hidden = false;
    } else {
      artNotice.replaceChildren();
      artNotice.hidden = true;
    }
    page.artwork = { sample: sample?.id ?? null, asset: state.artwork.name, notice: !artNotice.hidden };
  }
  renderArtwork();
  showArtNote(`Using ${state.artwork.label}.`);

  /** Put `size` in the stud grid's fields, as if typed, and clear their messages. */
  function setGrid(size) {
    if (!gridControl) return;
    gridInputs.get(gridControl.name)?.forEach((input, i) => {
      input.value = String(size[i]);
      input.removeAttribute("aria-invalid");
      invalid.delete(input.id);
    });
    state.values[gridControl.name] = [...size];
  }

  /**
   * Use a bundled sample. Choosing the badge on a tile that is not square sets
   * the tile to 4x4, and the portrait mark on a tile that is not longer than
   * wide sets 2x4; the art note and the next status line say so. Later size
   * changes are the user's own; only the drawing follows them (sampleFile).
   */
  function useSample(id) {
    const sample = samples.find((s) => s.id === id);
    if (!sample) return;
    const size = sizeForSample(sample, gridControl ? state.values[gridControl.name] : null);
    let note = "";
    if (size) {
      setGrid(size);
      note = sizeNote(sample, size);
      state.statusNote = note;
    }
    state.artwork = sampleArtwork(sample, gridControl ? state.values[gridControl.name] : null);
    renderArtwork();
    showArtNote(`Using ${state.artwork.label}.${note ? ` ${note}` : ""}`);
    changed();
  }

  /**
   * While a sample is in use, send the drawing made for the current tile (the
   * Neuraldeep.net mark has one for 2x4 and one for 4x6). Called on every change.
   */
  function followSample() {
    const sample = state.artwork.sample;
    if (!sample || !gridControl) return;
    const next = sampleArtwork(sample, state.values[gridControl.name]);
    if (next.name === state.artwork.name) return;
    state.artwork = next;
    renderArtwork();
    showArtNote(`Using ${state.artwork.label}.`);
  }

  async function useFile(file) {
    if (!file) return;
    const still = ` The tile keeps ${state.artwork.label}.`;
    const tooBig = assetSizeError(file.name, file.size, data.maxAssetBytes);
    if (tooBig) {
      showArtNote(tooBig + still, true);
      return;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (!looksLikeSvg(bytes)) {
      showArtNote(`“${file.name}” does not look like an SVG drawing. Choose an .svg file; tracing PNG and JPG images is planned.${still}`, true);
      return;
    }
    state.artwork = { name: file.name, bytes, label: `“${file.name}” (${kb(bytes.length)})` };
    renderArtwork();
    showArtNote(`Using ${state.artwork.label}.`);
    changed();
  }
  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    fileInput.value = "";
    useFile(file);
  });
  // Dropping a file anywhere on the 3D view works too.
  const stage = $("stage");
  stage.addEventListener("dragover", (e) => {
    if (page.mode === "tile") e.preventDefault();
  });
  stage.addEventListener("drop", (e) => {
    if (page.mode !== "tile") return;
    e.preventDefault();
    useFile(e.dataTransfer.files?.[0]);
  });

  function applyVisibility(visible) {
    for (const [name, wrap] of wrappers) wrap.hidden = visible[name] === false;
  }

  // ------------------------------------------------------------------ printer

  const processSelect = $("process");
  const couponProcess = $("coupon-process");
  for (const select of [processSelect, couponProcess]) {
    for (const p of data.processProfiles) select.append(el("option", { value: p.id, text: `${p.id}: ${p.name}` }));
    select.disabled = false;
  }
  processSelect.value = state.process;
  processSelect.addEventListener("change", () => {
    state.process = processSelect.value;
    changed();
  });

  const fitSelect = $("fit");
  const fitFile = $("fit-file");
  const fitNote = $("fit-note");
  fitSelect.append(
    el("option", { value: "example", text: fitLabel("Example fit profile", exampleFit) }),
    el("option", { value: "load", text: "Load my fit profile…" }),
    el("option", { value: "none", text: "None (uncalibrated)" }),
  );
  fitSelect.value = state.fit;
  fitSelect.disabled = false;

  function showFitNote() {
    fitNote.className = "note";
    if (state.fit === "example") {
      fitNote.textContent = `${exampleFit.notes} Calibrated for ${exampleFit.process}.`;
    } else if (state.fit === "mine") {
      const mine = JSON.parse(state.fits.mine);
      fitNote.textContent = `${mine.id}: ${[mine.printer, mine.material?.type ?? mine.material].filter(Boolean).join(", ")}, calibrated for ${mine.process}.`;
    } else {
      fitNote.replaceChildren(
        "Without a fit profile the tile's tubes are not compensated for your printer, so downloads stay off. ",
        el("button", { type: "button", className: "link", onclick: () => setMode("coupon", { focus: true }) }, "Print the calibration coupon"),
        " first.",
      );
    }
  }
  showFitNote();

  function fitError(text) {
    fitNote.className = "note bad";
    fitNote.textContent = text;
  }
  fitSelect.addEventListener("change", () => {
    if (fitSelect.value === "load") {
      fitSelect.value = state.fit; // until a file is read
      fitFile.click();
      return;
    }
    state.fit = fitSelect.value;
    showFitNote();
    changed();
  });
  fitFile.addEventListener("change", async () => {
    const file = fitFile.files?.[0];
    fitFile.value = "";
    if (!file) return;
    if (file.size > MAX_FIT_BYTES) {
      fitError(`“${file.name}” is ${kb(file.size)}; a fit profile is a small JSON file (about 2 KB) written by openbrick calibrate record.`);
      return;
    }
    const text = await file.text();
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch {
      fitError(`“${file.name}” is not JSON. Choose the file that openbrick calibrate record wrote.`);
      return;
    }
    if (parsed?.kind !== "fit") {
      fitError(`“${file.name}” is ${parsed?.kind ? `a ${parsed.kind} profile` : "not a profile"}, not a fit profile. Choose the file that openbrick calibrate record wrote.`);
      return;
    }
    state.fits.mine = text;
    let option = fitSelect.querySelector('option[value="mine"]');
    if (!option) {
      option = el("option", { value: "mine" });
      fitSelect.insertBefore(option, fitSelect.querySelector('option[value="load"]'));
    }
    option.textContent = fitLabel(`My fit profile ${parsed.id ?? ""}`.trim(), parsed);
    fitSelect.value = "mine";
    state.fit = "mine";
    showFitNote();
    changed();
  });

  for (const role of ["body", "accent"]) {
    const input = $(`color-${role}`);
    input.value = state.colors[role];
    input.disabled = false;
    input.addEventListener("input", () => {
      state.colors[role] = input.value;
      viewer.setColors({ [role]: input.value });
      if (state.coupon.result && role === "body") renderCouponLegend();
      renderLegend();
    });
  }

  // ------------------------------------------------------------------ evaluation

  let timer = null;
  /**
   * Counts changes to the tile controls. An evaluation remembers the count it
   * started at; a reply for an older count describes controls that have changed
   * since (a newer evaluation follows, or a field is now invalid), so it never
   * reaches the screen and never turns the downloads back on.
   */
  let edits = 0;
  /** The controls changed: evaluate again after a short pause. */
  function changed() {
    edits++;
    state.dirty = true;
    renderDownloads();
    followSample();
    if (invalid.size > 0) {
      clearTimeout(timer);
      timer = null;
      stopTicker();
      setState("invalid");
      showStatus([...invalid.values()], "bad");
      return;
    }
    const { visible, errors } = visibility(spec, state.values);
    for (const e of errors) if (!page.warnings.includes(e)) page.warnings.push(e);
    applyVisibility(visible);
    if (page.mode !== "tile") return; // evaluated when the tile is shown again
    setState("pending");
    clearTimeout(timer);
    timer = setTimeout(evaluate, EVALUATE_DELAY_MS);
  }

  function currentRequest() {
    const { visible } = visibility(spec, state.values);
    const params = paramOverrides(spec, state.values, visible);
    if (fileControl) params[fileControl.name] = state.artwork.name;
    return {
      doc: data.logoTilePart,
      params,
      process: state.process,
      fit: state.fit === "none" ? "" : state.fits[state.fit] ?? "",
      assets: [{ name: state.artwork.name, bytes: state.artwork.bytes }],
    };
  }

  async function evaluate() {
    timer = null;
    if (invalid.size > 0) return; // changed() shows why; nothing to build
    const request = currentRequest();
    const artwork = state.artwork;
    const started = edits;
    setState("evaluating");
    showBuilding("Building the logo tile", performance.now());
    let result;
    try {
      result = await client.evaluatePart(request);
    } catch (err) {
      syncCounters();
      if (started !== edits) {
        page.staleReplies++;
        return;
      }
      // In the coupon mode the tile stays dirty and is evaluated again when shown.
      if (page.mode !== "tile") return;
      state.error = err.message;
      state.dirty = false;
      state.result = null;
      page.part = { error: err.message };
      presentTileError();
      return;
    }
    syncCounters();
    if (result === null) return; // superseded; the newer request owns the status line
    if (started !== edits) {
      page.staleReplies++;
      return;
    }
    state.request = request;
    state.result = result;
    /** The artwork the result was built from: its sample names the downloads. */
    state.resultArtwork = artwork;
    state.dirty = false;
    state.error = null;
    // The core's reading of visibleWhen is the one that counts.
    const { visible } = visibility(spec, state.values);
    for (const [name, shown] of Object.entries(result.params.visible ?? {})) {
      if (visible[name] !== undefined && visible[name] !== shown) {
        page.warnings.push(`visibleWhen of ${name}: the page read ${visible[name]}, the core ${shown}`);
      }
    }
    const keepView = page.evaluations > 0;
    page.evaluations++;
    page.part = {
      contentHash: result.contentHash,
      triangles: result.triangles,
      kernel: result.kernel,
      meshes: result.meshes.map(({ role, triangles, fingerprint, canonicalFingerprint, volumeMm3 }) => ({ role, triangles, fingerprint, canonicalFingerprint, volumeMm3 })),
      overlay: result.overlay ? { triangles: result.overlay.triangles } : null,
      exportAllowed: result.exportAllowed,
      blockers: result.report.findings.filter((f) => f.severity === "blocker").map((f) => f.rule),
      warnings: result.report.findings.filter((f) => f.severity !== "blocker").map((f) => f.rule),
      units: result.report.findings.map((f) => `${f.rule} ${f.unit}`),
      params: result.params.values,
      faceOnBed: result.faceOnBed,
      removedAreaPct: result.measurements.removedAreaPct ?? null,
      buildMs: result.buildMs,
    };
    page.error = null;
    // In the coupon mode the result waits; setMode("tile") shows it.
    if (page.mode === "tile") presentTile({ keepView });
  }

  /** Put the tile result on screen: the part, the status line, stats, validation, downloads. */
  function presentTile({ keepView }) {
    const result = state.result;
    stopTicker();
    applyVisibility(result.params.visible ?? visibility(spec, state.values).visible);
    showTile({ keepView });
    setState("ready");
    const v = result.params.values;
    // Once: a sample set the size (its note also stays in the art note).
    const note = state.statusNote;
    state.statusNote = null;
    showStatus([
      ...(note ? [note] : []),
      `Logo tile ${v.size?.join(" × ") ?? ""}${v.relief ? `, ${v.relief}` : ""}`,
      `${result.triangles.toLocaleString("en-US")} triangles`,
      `content hash ${result.contentHash.slice(0, 12)}…`,
      `built in ${Math.round(result.buildMs)} ms by ${result.kernel}`,
      backendName,
    ]);
    renderStats();
    renderValidation();
    renderDownloads();
  }

  /**
   * The tile could not be built: nothing on screen may describe the controls
   * any more, so the 3D view and the stats are emptied, and the legend, the
   * status line and the validation panel say why.
   */
  function presentTileError() {
    stopTicker();
    viewer.clear();
    setState("part-error");
    showStatus([`The tile could not be built: ${state.error}`], "bad");
    renderLegend();
    renderStats();
    renderValidation();
    renderDownloads();
  }

  function showTile({ keepView }) {
    const r = state.result;
    if (!r) return;
    viewer.showPart({ meshes: r.meshes, overlay: r.overlay, footprint: r.footprint }, { keepView });
    viewer.setColors(state.colors);
    renderLegend();
  }

  // ------------------------------------------------------------------ panels

  const legend = $("legend");
  function swatch(colour, text, id) {
    return el("li", { id }, el("span", { className: "swatch", style: `background:${colour}`, "aria-hidden": "true" }), el("span", { text }));
  }
  /** The legend's line when a build failed: why the 3D view is empty. */
  const emptyNote = (what) => el("li", { id: "legend-empty", text: `Nothing to show: the ${what} could not be built (see the status line).` });
  function renderLegend() {
    if (page.mode !== "tile") return;
    const r = state.result;
    if (!r) {
      legend.replaceChildren(...(state.error ? [emptyNote("tile")] : []));
      return;
    }
    const relief = r.params.values.relief;
    // What the accent part holds: the face slab of a filament-swap inlay, the
    // logo fill of a multi-material (parts) inlay, the logo otherwise.
    const accent =
      relief !== "inlay"
        ? "Logo"
        : r.params.values.inlayStyle === "parts"
          ? "Logo fill (its own part, face down)"
          : "Face slab (printed first, face down)";
    const items = r.meshes.map((m) =>
      m.role === "body"
        ? swatch(state.colors.body, "Body")
        : m.role === "accent"
          ? swatch(state.colors.accent, accent)
          : swatch(m.color, m.role),
    );
    const removed = removedText(r);
    if (r.overlay && removed) items.push(swatch(r.overlay.color, removed, "legend-removed"));
    legend.replaceChildren(...items);
  }

  function statsList(target, rows) {
    target.replaceChildren(...rows.flatMap(([k, v]) => [el("dt", { text: k }), el("dd", { text: v })]));
  }
  function renderStats() {
    const r = state.result;
    if (!r) {
      $("tile-stats").replaceChildren();
      return;
    }
    const rows = partStats(r);
    if (r.faceOnBed === "top") rows.push(["Prints", "face down: the downloads are laid out that way"]);
    statsList($("tile-stats"), rows);
  }

  const findingsList = $("findings");
  function renderValidation() {
    const summary = $("summary");
    const notes = $("notes");
    if (state.error) {
      summary.className = "error";
      summary.textContent = "The tile could not be built, so it was not checked.";
      findingsList.replaceChildren(
        el(
          "li",
          { className: "finding blocker" },
          el("span", { className: "sev", "aria-hidden": "true", text: "⛔" }),
          el("div", {}, el("p", { className: "head" }, el("strong", { text: "Error" })), el("p", { text: state.error })),
        ),
      );
      notes.hidden = true;
      return;
    }
    const r = state.result;
    if (!r) return;
    summary.textContent = reportSummary(r.report);
    summary.className = r.report.findings.length === 0 ? "ok" : "";
    findingsList.replaceChildren(
      ...sortedFindings(r.report).map((f) => {
        const v = findingView(f);
        const nums = [v.measured !== null ? `Measured ${v.measured}` : null, v.threshold !== null ? `threshold ${v.threshold}` : null].filter(Boolean).join(", ");
        return el(
          "li",
          { className: `finding ${v.blocker ? "blocker" : "warning"}`, "data-rule": v.rule },
          el("span", { className: "sev", "aria-hidden": "true", text: v.icon }),
          el(
            "div",
            {},
            el("p", { className: "head" }, el("strong", { text: v.severity }), el("code", { text: v.rule }), v.feature ? ` on ${v.feature}` : null),
            el("p", { text: v.message }),
            nums ? el("p", { className: "nums", text: `${nums}.` }) : null,
            el("p", {}, el("strong", { text: "Fix: " }), v.fix),
            v.calibrate
              ? el(
                  "p",
                  {},
                  el("button", { type: "button", className: "link", onclick: () => setMode("coupon", { focus: true }) }, "Print the calibration coupon"),
                  " · ",
                  el("a", { href: PRINT_TEST_GUIDE_URL, target: "_blank", rel: "noopener" }, "How to print and measure it"),
                )
              : null,
          ),
        );
      }),
    );
    const warnings = r.warnings ?? [];
    notes.hidden = warnings.length === 0;
    notes.querySelector("summary").textContent = `Notes from the evaluator (${warnings.length})`;
    notes.querySelector("ul").replaceChildren(...warnings.map((w) => el("li", { text: w })));
  }

  const download3mf = $("download-3mf");
  const downloadStl = $("download-stl");
  function renderDownloads() {
    const reason = $("download-reason");
    let why = null;
    if (state.exporting) why = "Preparing the file…";
    else if (invalid.size > 0) why = `Download is off while a field is invalid: ${[...invalid.values()][0]}`;
    else if (state.error) why = "Download is off until the tile builds (see Validation).";
    else if (!state.result) why = "Waiting for the first build…";
    else if (state.dirty) why = "Updating the preview…";
    else why = downloadBlockedReason(state.result.report);
    const enabled = why === null;
    download3mf.disabled = !enabled;
    downloadStl.disabled = !enabled;
    reason.textContent = why ?? "";
    reason.className = why && ((state.result && !state.dirty && !state.exporting) || invalid.size > 0) ? "error" : "";
  }

  async function downloadPart(format) {
    if (!state.result?.exportAllowed || state.dirty || state.exporting) return;
    const suffix = state.resultArtwork?.sample?.suffix ?? null;
    const name = downloadName(doc, state.result.params.values, format, state.result.params.visible, suffix);
    state.exporting = true;
    renderDownloads();
    try {
      const { bytes } = await client.exportPart({ ...state.request, format, colors: { ...state.colors } });
      save(bytes, name, format);
      page.downloads.push({ name, format, bytes: bytes.byteLength });
      $("download-reason").textContent = "";
      showStatus([`Saved ${name}`, kb(bytes.byteLength)]);
    } catch (err) {
      showStatus([`The download failed: ${err.message}`], "bad");
    } finally {
      syncCounters();
      state.exporting = false;
      renderDownloads();
    }
  }
  download3mf.addEventListener("click", () => downloadPart("3mf"));
  downloadStl.addEventListener("click", () => downloadPart("stl"));

  // ------------------------------------------------------------------ coupon

  const pocketsSelect = $("coupon-pockets");
  for (const name of data.couponChoices.pocketSets) pocketsSelect.append(el("option", { value: name, text: POCKET_LABELS[name] ?? name }));
  pocketsSelect.value = state.coupon.pockets;
  couponProcess.value = state.coupon.process;
  const bedInputs = [$("bed-x"), $("bed-y")];
  bedInputs.forEach((input, i) => {
    input.value = String(state.coupon.bed[i]);
    input.disabled = false;
  });
  pocketsSelect.disabled = false;

  function couponChanged() {
    state.coupon.process = couponProcess.value;
    state.coupon.pockets = pocketsSelect.value;
    let ok = true;
    state.coupon.bed = bedInputs.map((input, i) => {
      const n = Number(input.value);
      if (!Number.isFinite(n) || n <= 0) {
        input.setAttribute("aria-invalid", "true");
        ok = false;
        return state.coupon.bed[i];
      }
      input.removeAttribute("aria-invalid");
      return n;
    });
    $("record-command").textContent = recordCommand(state.coupon.process, state.coupon.pockets, exampleFit.calibration?.chosenCells);
    state.coupon.invalid = !ok;
    if (!ok) {
      showBedInvalid();
      renderCouponDownloads();
      return;
    }
    if (page.mode === "coupon") buildCoupon();
  }
  couponProcess.addEventListener("change", couponChanged);
  pocketsSelect.addEventListener("change", couponChanged);
  for (const input of bedInputs) input.addEventListener("change", couponChanged);
  $("record-command").textContent = recordCommand(state.coupon.process, state.coupon.pockets, exampleFit.calibration?.chosenCells);

  const couponButtons = [$("download-coupon-3mf"), $("download-coupon-stl")];
  function renderCouponDownloads() {
    const c = state.coupon;
    const ok = c.result !== null && c.request !== null && !c.exporting && !c.invalid && c.key === couponKey();
    for (const b of couponButtons) b.disabled = !ok;
    const note = c.exporting
      ? "Preparing the file…"
      : c.invalid
        ? "Download is off while the bed size is invalid."
        : c.error
          ? `The coupon could not be built: ${c.error}`
          : "";
    $("coupon-download-note").textContent = note;
    $("coupon-download-note").className = c.error || c.invalid ? "note bad" : "note";
  }

  function couponRequest() {
    return { process: state.coupon.process, layout: { pockets: state.coupon.pockets, bedMm: state.coupon.bed } };
  }
  const couponKey = () => JSON.stringify(couponRequest());

  function renderCouponLegend() {
    if (page.mode !== "coupon") return;
    if (!state.coupon.result) {
      legend.replaceChildren(...(state.coupon.error ? [emptyNote("coupon")] : []));
      return;
    }
    legend.replaceChildren(swatch(state.colors.body, "Coupon (body colour)"));
  }

  /** The coupon's stats and cell table, or empty ones (with a note) after a failed build. */
  function renderCouponTables() {
    const r = state.coupon.result;
    if (!r) {
      $("coupon-stats").replaceChildren();
      $("cells").replaceChildren(
        ...(state.coupon.error ? [el("tr", {}, el("td", { colspan: 3, text: "No cells: the coupon could not be built (see the note under Download)." }))] : []),
      );
      return;
    }
    statsList($("coupon-stats"), couponStats(r));
    $("cells").replaceChildren(
      ...couponRows(r.cells).map((row) =>
        el("tr", {}, el("td", { text: row.line }), el("td", { text: row.label }), el("td", {}, row.record ? el("code", { text: row.record }) : "")),
      ),
    );
  }

  function showCoupon({ keepView }) {
    const r = state.coupon.result;
    if (!r) return;
    viewer.showPart({ meshes: r.meshes }, { keepView });
    viewer.setColors({ body: state.colors.body });
    renderCouponLegend();
  }

  /** The bed size is invalid: a build that answers keeps this message (the coupon shown is the last valid bed's). */
  function showBedInvalid() {
    setState("invalid");
    showStatus(["The bed size must be a positive number of millimetres."], "bad");
  }

  function showCouponStatus() {
    const r = state.coupon.result;
    showStatus([
      `Calibration coupon, ${r.summary.process}, ${r.summary.pockets} pockets`,
      `${r.triangles.toLocaleString("en-US")} triangles`,
      `built in ${Math.round(r.buildMs)} ms by ${r.kernel}`,
      backendName,
    ]);
  }

  async function buildCoupon() {
    const request = couponRequest();
    const key = couponKey();
    if (state.coupon.key === key && state.coupon.result) {
      showCoupon({ keepView: false });
      if (state.coupon.invalid) showBedInvalid();
      else {
        setState("ready");
        showCouponStatus();
      }
      renderCouponDownloads();
      return;
    }
    // Until the new coupon arrives the view keeps the last one, or nothing.
    if (state.coupon.result) showCoupon({ keepView: true });
    else viewer.clear();
    setState("building-coupon");
    showBuilding("Building the calibration coupon", performance.now());
    renderCouponDownloads();
    let result;
    try {
      result = await client.buildCoupon(request);
    } catch (err) {
      syncCounters();
      if (page.mode !== "coupon" || couponKey() !== key) return;
      stopTicker();
      state.coupon.error = err.message;
      state.coupon.result = null;
      state.coupon.key = null;
      page.coupon = { error: err.message };
      // Nothing on screen may describe the old layout: empty the view and tables.
      viewer.clear();
      setState("coupon-error");
      showStatus([`The coupon could not be built: ${err.message}`], "bad");
      renderCouponLegend();
      renderCouponTables();
      renderCouponDownloads();
      return;
    }
    syncCounters();
    if (result === null || couponKey() !== key) return;
    stopTicker();
    Object.assign(state.coupon, { result, request, key, error: null });
    page.couponBuilds++;
    page.coupon = {
      contentHash: result.contentHash,
      triangles: result.triangles,
      fileStem: result.fileStem,
      cells: result.cells.length,
      fingerprint: result.meshes[0]?.fingerprint,
      buildMs: result.buildMs,
    };
    if (page.mode === "coupon") {
      showCoupon({ keepView: false });
      if (state.coupon.invalid) showBedInvalid();
      else {
        setState("ready");
        showCouponStatus();
      }
    }
    renderCouponTables();
    renderCouponDownloads();
  }

  async function downloadCoupon(format) {
    const c = state.coupon;
    if (!c.result || c.exporting) return;
    const name = couponDownloadName(c.result.fileStem, format);
    c.exporting = true;
    renderCouponDownloads();
    try {
      const { bytes } = await client.exportCoupon({ ...c.request, format, colors: { body: state.colors.body } });
      save(bytes, name, format);
      page.downloads.push({ name, format, bytes: bytes.byteLength });
      showStatus([`Saved ${name}`, kb(bytes.byteLength)]);
    } catch (err) {
      showStatus([`The download failed: ${err.message}`], "bad");
    } finally {
      syncCounters();
      c.exporting = false;
      renderCouponDownloads();
    }
  }
  couponButtons[0].addEventListener("click", () => downloadCoupon("3mf"));
  couponButtons[1].addEventListener("click", () => downloadCoupon("stl"));

  // ------------------------------------------------------------------ modes

  const tabs = { tile: $("tab-tile"), coupon: $("tab-coupon") };
  const panels = { tile: $("panel-tile"), coupon: $("panel-coupon") };

  function setMode(mode, { focus = false } = {}) {
    // The status line's seconds belong to the mode being left; a build the new
    // mode starts shows its own.
    stopTicker();
    page.mode = mode;
    document.body.dataset.mode = mode;
    for (const [m, tab] of Object.entries(tabs)) {
      tab.setAttribute("aria-selected", String(m === mode));
      tab.tabIndex = m === mode ? 0 : -1;
      panels[m].hidden = m !== mode;
    }
    if (focus) tabs[mode].focus();
    document.querySelector("aside").scrollTop = 0;
    history.replaceState(null, "", `${location.pathname}${writeMode(location.search, mode)}${location.hash}`);
    if (mode === "tile") {
      legend.replaceChildren();
      if (state.dirty || !state.result) {
        // Until the evaluation answers, the view keeps the last tile, or nothing.
        if (state.result) showTile({ keepView: false });
        else viewer.clear();
        changed();
      } else presentTile({ keepView: false });
    } else {
      clearTimeout(timer);
      timer = null;
      renderCouponLegend();
      buildCoupon();
    }
  }

  for (const [mode, tab] of Object.entries(tabs)) {
    tab.addEventListener("click", () => setMode(mode));
    tab.addEventListener("keydown", (e) => {
      const order = Object.keys(tabs);
      const i = order.indexOf(mode);
      const next = { ArrowRight: order[(i + 1) % order.length], ArrowLeft: order[(i + order.length - 1) % order.length], Home: order[0], End: order.at(-1) }[e.key];
      if (next) {
        e.preventDefault();
        setMode(next, { focus: true });
      }
    });
  }

  setMode(firstMode);
}

main().catch(fail);
