// The playground page (docs/plan/06 sections 5, 11 and 13; docs/plan/05 sections
// 4 to 7): compiles the core once, starts the core worker, builds the logo tile
// controls from the part document, and shows what the core returns: the part in
// 3D (one mesh per colour role and the red overlay of artwork that does not
// print as drawn), its stats, the validation report and the downloads; or the
// calibration coupon with its cell table. Geometry, validation and the files
// come only from the worker (src/worker.js); this page never computes them.

import wasmUrl from "./wasm/openbrick_wasm_bg.wasm?url";
import { CoreClient } from "./client.js";
import { createFileAccess, desktopBridge, errorText, savedText } from "./files.js";
import {
  applyPreset,
  assetSizeError,
  controlsFor,
  couponDownloadName,
  defaultValues,
  downloadName,
  looksLikeSvg,
  matchingPreset,
  paramOverrides,
  visibility,
} from "./params.js";
import { buildErrorText, isPartFile, partFileName, sampleForAsset, unsupportedNote, withCoreDefaults } from "./part.js";
import {
  couponRows,
  couponStats,
  downloadBlockedReason,
  elapsedText,
  evaluatingParts,
  findingView,
  fitLabel,
  needsCalibration,
  partStats,
  PRINT_TEST_GUIDE_URL,
  readBackend,
  readMode,
  readStep,
  recordCommand,
  removedText,
  reportPasses,
  reportSummary,
  SLOW_BUILD_MS,
  sortedFindings,
  stepStuds,
  writeMode,
  writeStep,
} from "./ui.js";
import {
  boxPercent,
  cellAccessibleName,
  cellMap,
  choiceText,
  choose,
  chosenCells,
  errorPlace,
  FORM_FIELDS,
  groupName,
  keepPrinted,
  mapSelection,
  MATERIALS,
  moveInGroup,
  offsetRows,
  OTHER_PRINTER,
  parseNozzle,
  PRINTERS,
  recordInput,
  todayLocal,
} from "./calibration.js";
import { forgetRecordedFit, loadPrinter, loadRecordedFit, savePrinter, saveRecordedFit } from "./storage.js";
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
/**
 * Saving and opening: browser downloads and file inputs, or the native dialogs
 * of the desktop app (apps/desktop) when the page runs inside it (src/files.js).
 */
const files = createFileAccess({ bridge: desktopBridge(window) });
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
  /** Coupon layouts the record step asked for that answered, stored or (stale) dropped. */
  recordLayouts: 0,
  /** Of those, the ones dropped because the coupon settings had changed meanwhile. */
  staleLayouts: 0,
  restarts: 0,
  traps: 0,
  warnings: [],
  error: null,
  downloads: [],
  /** True inside the desktop app, where saving and opening use native dialogs. */
  desktop: files.desktop,
  /** The artwork in use: { sample (its id, null for the user's SVG), asset, notice (shown) }. */
  artwork: null,
  /** The core's artwork size limit (max_asset_bytes()), once loaded. */
  maxAssetBytes: null,
  /**
   * The part being designed: { id, name, source ("bundled" or "file"), file
   * (its file name), presets (names), preset (the one the values match, or
   * ""), unsupported (feature ids this version cannot build), missingArtwork
   * (the drawing it names that the page lacks, or null) }, and `saves`: the
   * part files saved, { name, bytes }.
   */
  design: null,
  saves: [],
  /** The coupon mode's step: print or record. */
  step: "print",
  /**
   * The record step: { cells, labels (in layout order), choices (cell id ->
   * mm), ok, id, fileName, offsets, error ({ field, message } or null),
   * nozzleError, kept }, or null before its map first loads.
   */
  record: null,
  /** The fit select: { selected (example, recorded, mine, none), recordedId, recordedKept }. */
  fit: null,
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

/**
 * Save bytes as `name`: a browser download, or the desktop app's save dialog.
 * Resolves to `{ saved, name, path }`; `saved` is false only when the user
 * cancelled the desktop dialog.
 */
function save(bytes, name, format) {
  return files.save(bytes, name, format);
}

const kb = (n) => `${new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 }).format(n / 1024)} KB`;

async function main() {
  const { backend: wanted, errors: backendErrors } = readBackend(location.search);
  const { mode: firstMode, errors: modeErrors } = readMode(location.search);
  const { step: firstStep, errors: stepErrors } = readStep(location.search);
  page.warnings.push(...backendErrors, ...modeErrors, ...stepErrors);
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

  /**
   * The part being designed: its document as JSON text (what the core gets;
   * saving and adding a preset replace it with the core's answer) and parsed,
   * where it came from, and what the core said when it was opened
   * (open_part_json: presets and features it cannot build yet). The controls
   * (`spec`), the artwork control and the stud grid follow the document.
   */
  let part = null;
  let doc = null;
  let spec = { controls: [], groups: [] };
  let fileControl = null;
  /** The stud grid a sample's tile shape applies to (the tile's size). */
  let gridControl = null;
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
  const state = {
    /** Parameter values by name; set from the part's defaults when it loads. */
    values: {},
    /**
     * The drawing sent for the artwork control: { name, bytes, label, sample },
     * or null when the part has no artwork control or names a drawing the page
     * does not have (then the drop zone asks for it).
     */
    artwork: null,
    /** A note for the status line after the next build (a sample changed the size). */
    statusNote: null,
    process: data.processProfiles.some((p) => p.id === exampleFit.process) ? exampleFit.process : data.processProfiles[0].id,
    fit: "example",
    /**
     * The printer whose bed rule SIZE.BED checks (a bundled printer id), ""
     * for none; the last choice is kept in this browser (src/storage.js).
     */
    printer: loadPrinter((data.printers ?? []).map((p) => p.id)),
    /**
     * The fit profiles the select offers, as JSON text: the example, the one
     * recorded in the record step (kept in this browser), and a loaded file.
     */
    fits: { example: data.exampleFitProfile, recorded: null, mine: null },
    /** Whether this browser's storage kept the recorded profile. */
    recordedKept: false,
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
      /** The printer the bed size came from, "" when it was typed. */
      printer: "",
      key: null,
      request: null,
      result: null,
      error: null,
      /** A bed size field holds something that is not a positive number. */
      invalid: false,
      exporting: false,
      /** The coupon mode's step: print the coupon, or record the fit. */
      step: firstStep,
    },
    /** The record step (src/calibration.js). */
    record: {
      /**
       * The coupon layout the map shows ({ cells, summary }), the coupon key it
       * was laid out for (set only when it is stored, so a map is never used
       * for other settings) and its map; the key of a layout on its way.
       */
      layout: null,
      key: null,
      map: null,
      layoutError: null,
      loadingKey: null,
      /** Settles when the layout on its way answers. */
      loading: null,
      /** Group -> the chosen cell's values ({ stud: mm } or { pocketTubeOD: mm, pocketRib: mm }). */
      choices: {},
      /** Group -> the layout index of the cell chosen last: the group's Tab stop. */
      picked: {},
      /** Fields the person has edited, and whether a save was tried: their errors show. */
      touched: new Set(),
      attempted: false,
      /** The core's answer for the current choices (record_fit), and its request number. */
      result: null,
      callError: null,
      nozzleError: null,
      seq: 0,
      pending: null,
      /** A note on choices the coupon no longer prints. */
      dropped: "",
    },
  };

  // ------------------------------------------------------------------ tile controls

  /** The artwork the page sends for the part's file control, if any. */
  const artworkName = () => state.artwork?.name ?? null;
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
                  value: String(o.value),
                  checked: state.values[c.name] === o.value,
                  // The option's own value, so a number stays a number.
                  onchange: () => {
                    state.values[c.name] = o.value;
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
          // As many decimals as the step has (none for whole studs), at least two for millimetres.
          const places = Math.max(c.unit === "mm" ? 2 : 0, (String(c.step).split(".")[1] ?? "").length);
          out.textContent = `${Number(state.values[c.name]).toFixed(places)} ${c.unit}`.trim();
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
            onclick: () => chooseArtwork(),
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
      case "checkbox":
        return el(
          "div",
          { className: "field check" },
          el(
            "label",
            { for: id },
            el("input", {
              id,
              type: "checkbox",
              checked: state.values[c.name] === true,
              "aria-describedby": c.help ? `${id}-help` : null,
              onchange: (e) => {
                state.values[c.name] = e.target.checked;
                changed();
              },
            }),
            ` ${c.label}`,
          ),
          describe(c, id),
        );
      case "text": {
        const limit = c.maxLength ? `At most ${c.maxLength} characters.` : "";
        return el(
          "div",
          { className: "field" },
          el("label", { for: id, text: c.label }),
          el("input", {
            id,
            type: "text",
            autocomplete: "off",
            value: state.values[c.name] ?? "",
            "aria-describedby": `${id}-help`,
            oninput: (e) => {
              const text = e.target.value;
              if (c.maxLength && [...text].length > c.maxLength) {
                e.target.setAttribute("aria-invalid", "true");
                invalid.set(id, `${c.label} can be at most ${c.maxLength} characters; it has ${[...text].length}.`);
              } else {
                e.target.removeAttribute("aria-invalid");
                invalid.delete(id);
                state.values[c.name] = text;
              }
              changed();
            },
          }),
          el("p", { className: "help", id: `${id}-help`, text: [limit, c.help].filter(Boolean).join(" ") }),
        );
      }
      case "color":
        return el(
          "div",
          { className: "field" },
          el("label", { for: id, text: c.label }),
          el("input", {
            id,
            type: "color",
            value: state.values[c.name],
            "aria-describedby": c.help ? `${id}-help` : null,
            oninput: (e) => {
              state.values[c.name] = e.target.value;
              changed();
            },
          }),
          describe(c, id),
        );
      default:
        // A type the panel cannot edit: say so kindly, and what the part keeps.
        return el(
          "div",
          { className: "field" },
          el("span", { className: "label", text: c.label }),
          el("p", { className: "note", id: `${id}-note`, text: c.note ?? `${c.label} cannot be edited here yet, so the part keeps its default.` }),
        );
    }
  }

  /** Build the panel from the part's parameters, with the current values. */
  function buildForm() {
    for (const id of [...invalid.keys()]) if (id.startsWith("p-")) invalid.delete(id);
    wrappers.clear();
    gridInputs.clear();
    form.replaceChildren();
    for (const g of spec.groups) {
      const fieldset = el("fieldset", {}, el("legend", { text: g.name }));
      for (const c of g.controls) {
        const wrap = el("div", { "data-param": c.name }, control(c));
        wrappers.set(c.name, wrap);
        fieldset.append(wrap);
      }
      form.append(fieldset);
    }
    if (spec.controls.length === 0) form.append(el("p", { className: "note", text: "This part has no parameters to change." }));
  }
  form.addEventListener("submit", (e) => e.preventDefault());

  function showArtNote(text, bad = false) {
    artNote.textContent = text;
    artNote.className = bad ? "note bad" : "note";
  }

  /** The chooser, the notice line and the page summary follow the artwork in use. */
  function renderArtwork() {
    const sample = state.artwork?.sample ?? null;
    sampleSelect.value = sample?.id ?? "own";
    if (sample?.notice) {
      artNotice.replaceChildren(`${sample.notice} `, el("a", { href: sample.noticeUrl, target: "_blank", rel: "noopener" }, "Read the notice"));
      artNotice.hidden = false;
    } else {
      artNotice.replaceChildren();
      artNotice.hidden = true;
    }
    page.artwork = { sample: sample?.id ?? null, asset: artworkName(), notice: !artNotice.hidden };
  }

  /** The invalid-field entry that stands while the part's drawing is missing. */
  const MISSING_ARTWORK = "artwork";
  /**
   * Note the drawing the part names when the page does not have it: the
   * drop zone asks for it and nothing is built until it is given.
   */
  function askForArtwork(wanted) {
    const message = `This part's drawing “${wanted}” is not bundled with the designer: drop that SVG on the drop zone, or choose it or a sample.`;
    invalid.set(MISSING_ARTWORK, message);
    showArtNote(message, true);
  }

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
    invalid.delete(MISSING_ARTWORK);
    renderArtwork();
    showArtNote(`Using ${state.artwork.label}.${note ? ` ${note}` : ""}`);
    changed();
  }

  /**
   * While a sample is in use, send the drawing made for the current tile (the
   * Neuraldeep.net mark has one for 2x4 and one for 4x6). Called on every change.
   */
  function followSample() {
    const sample = state.artwork?.sample;
    if (!sample || !gridControl) return;
    const next = sampleArtwork(sample, state.values[gridControl.name]);
    if (next.name === state.artwork.name) return;
    state.artwork = next;
    renderArtwork();
    showArtNote(`Using ${state.artwork.label}.`);
  }

  async function useFile(file) {
    if (!file) return;
    // A part document dropped or chosen here opens as the part.
    if (isPartFile(file.name)) {
      openPartFile(file);
      return;
    }
    if (!fileControl) {
      showPartNote(`“${file.name}” is a drawing, and ${doc.name} takes none. To open a part, choose a .obpart.json file.`, true);
      return;
    }
    const still = state.artwork ? ` The part keeps ${state.artwork.label}.` : "";
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
    invalid.delete(MISSING_ARTWORK);
    renderArtwork();
    showArtNote(`Using ${state.artwork.label}.`);
    changed();
  }
  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    fileInput.value = "";
    useFile(file);
  });
  /** The drop zone's button: the file input, or the desktop app's open dialog. */
  function chooseArtwork() {
    if (!files.desktop) {
      fileInput.click();
      return;
    }
    files
      .open("svg", data.maxAssetBytes)
      .then(useFile)
      .catch((err) => showArtNote(`The file could not be opened: ${errorText(err)}.${state.artwork ? ` The part keeps ${state.artwork.label}.` : ""}`, true));
  }
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
    // A group whose every parameter is hidden hides its heading too.
    for (const fieldset of form.querySelectorAll("fieldset")) {
      fieldset.hidden = [...fieldset.querySelectorAll(":scope > [data-param]")].every((w) => w.hidden);
    }
  }

  // ------------------------------------------------------------------ part file

  const partSelect = $("part-select");
  const partFile = $("part-file");
  const partNote = $("part-note");
  const presetRow = $("preset-row");
  const presetSelect = $("preset");
  const presetName = $("preset-name");
  const saveNote = $("save-note");
  /** The bundled part documents, the logo tile first. */
  const bundledParts = data.parts;
  const encoderJson = new TextEncoder();

  function showPartNote(text, bad = false) {
    partNote.textContent = text;
    partNote.className = bad ? "note bad" : "note";
  }
  function showSaveNote(text, bad = false) {
    saveNote.textContent = text;
    saveNote.className = bad ? "note bad" : "note";
  }

  /** The part select: the bundled parts, the part opened from a file, and "Open a part file…". */
  function renderPartSelect() {
    const options = bundledParts.map((p) => el("option", { value: `bundled:${p.id}`, text: p.label }));
    if (part?.source === "file") options.push(el("option", { value: "file", text: `“${part.file}” (opened)` }));
    options.push(el("option", { value: "open", text: "Open a part file…" }));
    partSelect.replaceChildren(...options);
    partSelect.value = part?.source === "file" ? "file" : `bundled:${part?.id}`;
  }

  /** The preset picker: "Custom values" and the part's presets; hidden when it has none. */
  function renderPresets() {
    const presets = part.opened.presets ?? [];
    presetRow.hidden = presets.length === 0;
    presetSelect.replaceChildren(
      el("option", { value: "", text: "Custom values" }),
      ...presets.map((p) => el("option", { value: p.name, disabled: p.error ? true : null, text: p.error ? `${p.name} (does not fit this part)` : p.name })),
    );
    syncPreset();
  }

  /** Show the preset the values match, or "Custom values". */
  function syncPreset() {
    if (!part) return;
    const fitting = (part.opened.presets ?? []).filter((p) => !p.error);
    const name = matchingPreset(spec, fitting, state.values, artworkName());
    presetSelect.value = name;
    if (page.design) page.design.preset = name;
  }

  /**
   * Load a part document: its controls with its defaults, its drawing (a
   * bundled sample when the document names one; the logo tile starts with
   * the playground's default, the Neuraldeep.net mark), its presets and a
   * note on the features this version cannot build yet. `opened` is the core's
   * open_part_json answer for `text`.
   */
  function loadPart(text, opened, source) {
    const parsed = JSON.parse(text);
    part = { text, opened, source: source.kind, id: parsed.id, file: source.file };
    // The core's normalised defaults ("0.6mm" is 0.6), so the panel shows and
    // compares numbers; the saved file is written from `text`, as opened.
    doc = withCoreDefaults(parsed, opened.defaults);
    spec = controlsFor(doc, { maxStuds: info.maxStuds });
    fileControl = spec.controls.find((c) => c.kind === "file") ?? null;
    gridControl = spec.controls.find((c) => c.kind === "grid2") ?? null;
    state.values = defaultValues(spec);
    state.result = null;
    state.request = null;
    state.error = null;
    state.reframe = true;
    invalid.delete(MISSING_ARTWORK);
    const size = gridControl ? state.values[gridControl.name] : null;
    let missing = null;
    if (!fileControl) state.artwork = null;
    else if (source.kind === "bundled" && source.index === 0) state.artwork = sampleArtwork(samples[0], size);
    else {
      const found = sampleForAsset(samples, fileControl.default);
      if (found) state.artwork = sampleArtwork(found.sample, size);
      else {
        state.artwork = null;
        missing = String(fileControl.default ?? "");
      }
    }
    $("part-heading").textContent = doc.name;
    buildForm();
    renderArtwork();
    if (missing !== null) askForArtwork(missing);
    else if (state.artwork) showArtNote(`Using ${state.artwork.label}.`);
    renderPartSelect();
    renderPresets();
    const unsupported = unsupportedNote(opened.unsupported, spec);
    showPartNote(source.kind === "file" ? [`Opened “${source.file}”: ${doc.name} ${doc.version}.`, unsupported].filter(Boolean).join(" ") : unsupported, false);
    showSaveNote("");
    page.design = {
      id: doc.id,
      name: doc.name,
      source: source.kind,
      file: source.file,
      presets: (opened.presets ?? []).map((p) => p.name),
      preset: "",
      unsupported: (opened.unsupported ?? []).map((u) => u.feature),
      missingArtwork: missing,
    };
    syncPreset();
  }

  /** Open a part document the user chose or dropped (a browser File or one from the desktop dialog). */
  async function openPartFile(file) {
    if (!file) return;
    const keeps = ` The designer keeps ${doc?.name ?? "the current part"}.`;
    if (file.size > data.maxPartBytes) {
      showPartNote(`“${file.name}” is ${kb(file.size)}, and a part document can be at most ${kb(data.maxPartBytes)}. Choose the part's .obpart.json file.${keeps}`, true);
      return;
    }
    let text;
    try {
      text = await file.text();
    } catch (err) {
      showPartNote(`“${file.name}” could not be read: ${errorText(err)}.${keeps}`, true);
      return;
    }
    let opened;
    try {
      opened = await client.openPart(text, state.process);
    } catch (err) {
      showPartNote(`“${file.name}” could not be checked: ${errorText(err)}.${keeps}`, true);
      return;
    }
    if (!opened.ok) {
      showPartNote(`“${file.name}” could not be opened: ${opened.error}.${keeps}`, true);
      renderPartSelect();
      return;
    }
    loadPart(text, opened, { kind: "file", file: file.name });
    changed();
  }

  /** Open one of the bundled parts. */
  async function openBundled(index) {
    const p = bundledParts[index];
    const opened = await client.openPart(p.text, state.process);
    if (!opened.ok) throw new Error(opened.error);
    loadPart(p.text, opened, { kind: "bundled", file: p.file, index });
  }

  partSelect.addEventListener("change", () => {
    const value = partSelect.value;
    if (value === "open") {
      renderPartSelect(); // shows the part in use until a file is read
      if (files.desktop) {
        files
          .open("part", data.maxPartBytes)
          .then(openPartFile)
          .catch((err) => showPartNote(`The file could not be opened: ${errorText(err)}.`, true));
      } else partFile.click();
      return;
    }
    if (value === "file") return;
    const index = bundledParts.findIndex((p) => `bundled:${p.id}` === value);
    if (index < 0) return;
    openBundled(index)
      .then(() => changed())
      .catch((err) => showPartNote(`The part could not be opened: ${errorText(err)}.`, true));
  });
  partFile.addEventListener("change", () => {
    const file = partFile.files?.[0];
    partFile.value = "";
    openPartFile(file);
  });
  // A part document dropped on the part chooser opens too.
  const partRow = $("part-row");
  partRow.addEventListener("dragover", (e) => e.preventDefault());
  partRow.addEventListener("drop", (e) => {
    e.preventDefault();
    const file = e.dataTransfer.files?.[0];
    if (file && !isPartFile(file.name)) {
      showPartNote(`“${file.name}” is not a part document; choose a .obpart.json file. Drawings go on the drop zone below.`, true);
      return;
    }
    openPartFile(file);
  });

  presetSelect.addEventListener("change", () => {
    const preset = (part.opened.presets ?? []).find((p) => p.name === presetSelect.value);
    if (!preset) {
      syncPreset();
      return;
    }
    const { values, artwork, kept } = applyPreset(spec, state.values, preset);
    state.values = values;
    let note = `Preset “${preset.name}”.`;
    if (artwork !== null && artwork !== artworkName()) {
      const found = sampleForAsset(samples, artwork);
      if (found) state.artwork = sampleArtwork(found.sample, gridControl ? state.values[gridControl.name] : null);
      else note += ` Its drawing “${artwork}” is not bundled, so the part keeps ${state.artwork?.label ?? "no drawing"}.`;
    }
    if (kept.length) note += ` ${kept.join(", ")} cannot be changed here, so ${kept.length === 1 ? "it keeps its value" : "they keep their values"}.`;
    buildForm();
    renderArtwork();
    state.statusNote = note;
    changed();
    presetSelect.focus();
  });

  /** The values the core gets for the current controls (and the drawing's name). */
  function currentParams() {
    const { visible } = visibility(spec, state.values);
    const params = paramOverrides(spec, state.values, visible);
    if (fileControl && state.artwork) params[fileControl.name] = state.artwork.name;
    return params;
  }

  /** Why the part file cannot be saved now, or null. */
  function saveBlocked() {
    const fields = [...invalid.entries()].filter(([id]) => id !== MISSING_ARTWORK);
    if (fields.length > 0) return `Saving is off while a field is invalid: ${fields[0][1]}`;
    return null;
  }

  $("add-preset").addEventListener("click", async () => {
    const name = presetName.value.trim();
    if (!name) {
      showSaveNote("Give the preset a name first, for example “My tile”.", true);
      presetName.focus();
      return;
    }
    const why = saveBlocked();
    if (why) {
      showSaveNote(why, true);
      return;
    }
    try {
      const { text } = await client.addPreset({ doc: part.text, name, params: currentParams(), process: state.process });
      const opened = await client.openPart(text, state.process);
      if (!opened.ok) throw new Error(opened.error);
      const replaced = (part.opened.presets ?? []).some((p) => p.name === name);
      part.text = text;
      part.opened = opened;
      doc = JSON.parse(text);
      page.design.presets = opened.presets.map((p) => p.name);
      renderPresets();
      presetName.value = "";
      showSaveNote(`${replaced ? "Replaced" : "Added"} the preset “${name}” with the current values. Save the part file to keep it.`);
      changed();
    } catch (err) {
      showSaveNote(`The preset could not be added: ${errorText(err)}`, true);
    }
  });

  $("save-part").addEventListener("click", async () => {
    const why = saveBlocked();
    if (why) {
      showSaveNote(why, true);
      return;
    }
    const button = $("save-part");
    button.disabled = true;
    try {
      const params = currentParams();
      const { text } = await client.savePart({ doc: part.text, params, process: state.process });
      const bytes = encoderJson.encode(text);
      const name = partFileName(doc.id);
      const saved = await save(bytes, name, "json");
      if (!saved.saved) {
        showSaveNote(savedText(saved, "").join(" "));
        return;
      }
      // The saved document is the one designed from now on, so the content
      // hash shown is the one the CLI gives for the saved file.
      part.text = text;
      doc = JSON.parse(text);
      page.saves.push({ name: saved.name, bytes: bytes.byteLength });
      const own = fileControl && state.artwork && !state.artwork.sample ? ` The drawing is saved by name: keep “${state.artwork.name}” next to the part file.` : "";
      showSaveNote(`${savedText(saved, kb(bytes.byteLength)).join(", ")}. Its values are the part's defaults now.${own}`);
      changed();
    } catch (err) {
      showSaveNote(`The part could not be saved: ${errorText(err)}`, true);
    } finally {
      button.disabled = false;
    }
  });

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

  // The printer whose bed SIZE.BED checks; the same choice lays the coupon out.
  const printers = data.printers ?? [];
  const printerById = (id) => printers.find((p) => p.id === id) ?? null;
  const volumeText = (p) => p.buildVolumeMm.join(" x ") + " mm";
  const bedPrinter = $("printer");
  const couponPrinter = $("coupon-printer");
  bedPrinter.append(el("option", { value: "", text: "None: the bed size is not checked" }));
  couponPrinter.append(el("option", { value: "", text: "Other: type the bed size below" }));
  for (const select of [bedPrinter, couponPrinter]) {
    for (const p of printers) select.append(el("option", { value: p.id, text: `${p.name} (${volumeText(p)})` }));
    select.disabled = printers.length === 0;
  }
  function showPrinterNote() {
    const p = printerById(state.printer);
    $("printer-note").textContent = p
      ? `SIZE.BED checks that the tile fits the ${volumeText(p)} build volume, as ${p.maker} publishes it.`
      : "Choose your printer to check that the tile fits its bed (rule SIZE.BED); until then the report lists it as not checked.";
  }
  bedPrinter.value = state.printer;
  showPrinterNote();
  /** Choose a printer for both modes and keep the choice in this browser. */
  function choosePrinter(id) {
    state.printer = id;
    savePrinter(id);
    bedPrinter.value = id;
    showPrinterNote();
  }
  bedPrinter.addEventListener("change", () => {
    const id = bedPrinter.value;
    choosePrinter(id);
    if (id) {
      // The coupon is laid out for the same bed.
      usePrinterBed(id);
      couponChanged();
    }
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

  /** "id: printer, material, calibrated for process on date." for a fit profile. */
  const fitSummary = (p) => {
    const what = [p.printer, p.material?.type ?? p.material].filter((x) => typeof x === "string" && x).join(", ");
    const date = p.calibration?.date ? ` on ${p.calibration.date}` : "";
    return `${p.id}: ${what}, calibrated for ${p.process}${date}.`;
  };

  /** The note under the fit select; `before` is said first (what just changed). */
  function showFitNote(before = "") {
    fitNote.className = "note";
    const lead = before ? `${before} ` : "";
    if (state.fit === "example") {
      fitNote.textContent = `${lead}${exampleFit.notes} Calibrated for ${exampleFit.process}.`;
    } else if (state.fit === "recorded") {
      const kept = state.recordedKept
        ? "Kept in this browser only."
        : "This browser's storage is off, so it is gone when the page reloads: download it in the record step to keep it.";
      fitNote.textContent = `${lead}${fitSummary(JSON.parse(state.fits.recorded))} ${kept}`;
    } else if (state.fit === "mine") {
      fitNote.textContent = `${lead}${fitSummary(JSON.parse(state.fits.mine))}`;
    } else {
      fitNote.replaceChildren(
        `${lead}Without a fit profile the tile's tubes are not compensated for your printer, so downloads stay off. `,
        el("button", { type: "button", className: "link", onclick: () => setMode("coupon", { focus: true }) }, "Print the calibration coupon"),
        ", then ",
        el("button", { type: "button", className: "link", onclick: () => openRecordStep() }, "record your fit"),
        ".",
      );
    }
    page.fit = { selected: state.fit, recordedId: state.fits.recorded ? JSON.parse(state.fits.recorded).id : null, recordedKept: state.recordedKept };
  }

  const recordedOption = () => fitSelect.querySelector('option[value="recorded"]');
  /** Offer the recorded profile in the fit select (replacing an earlier one). */
  function adoptRecorded(text, profile, kept) {
    state.fits.recorded = text;
    state.recordedKept = kept;
    let option = recordedOption();
    if (!option) {
      option = el("option", { value: "recorded" });
      fitSelect.insertBefore(option, fitSelect.querySelector('option[value="load"]'));
    }
    option.textContent = fitLabel("Recorded in this browser", profile);
    $("fit-actions").hidden = false;
  }
  /**
   * Select the recorded profile. It was calibrated for one process and rule
   * FIT.PROCESS blocks any other, so the tile's process follows it; returns a
   * sentence saying so, or "".
   */
  function selectRecorded(profile) {
    fitSelect.value = "recorded";
    state.fit = "recorded";
    if (state.process !== profile.process && data.processProfiles.some((p) => p.id === profile.process)) {
      state.process = profile.process;
      processSelect.value = profile.process;
      return `The process is now ${profile.process}, the one it was calibrated for.`;
    }
    return "";
  }

  // A profile recorded earlier in this browser is the one to use; a page
  // without storage simply starts without it.
  const stored = loadRecordedFit();
  if (stored.problem) page.warnings.push(`ignored the stored fit profile: ${stored.problem}`);
  // selectRecorded may move the tile's process to the profile's: say so.
  let movedAtLoad = "";
  if (stored.fit) {
    adoptRecorded(stored.fit.text, stored.fit.profile, true);
    movedAtLoad = selectRecorded(stored.fit.profile);
  }
  showFitNote(movedAtLoad);

  $("forget-fit").addEventListener("click", () => {
    const gone = forgetRecordedFit();
    recordedOption()?.remove();
    state.fits.recorded = null;
    state.recordedKept = false;
    $("fit-actions").hidden = true;
    if (state.fit === "recorded") {
      state.fit = "example";
      fitSelect.value = "example";
    }
    showFitNote(gone ? "Forgot the recorded fit profile: this browser no longer keeps it." : "Removed the recorded fit profile from this page; this browser's storage could not be changed.");
    fitSelect.focus();
    changed();
  });

  function fitError(text) {
    fitNote.className = "note bad";
    fitNote.textContent = text;
  }
  fitSelect.addEventListener("change", () => {
    if (fitSelect.value === "load") {
      fitSelect.value = state.fit; // until a file is read
      if (files.desktop) {
        files
          .open("fit", MAX_FIT_BYTES)
          .then(useFitFile)
          .catch((err) => fitError(`The file could not be opened: ${errorText(err)}.`));
      } else fitFile.click();
      return;
    }
    state.fit = fitSelect.value;
    showFitNote();
    changed();
  });
  fitFile.addEventListener("change", () => {
    const file = fitFile.files?.[0];
    fitFile.value = "";
    useFitFile(file);
  });
  /** Read a fit profile the user chose (a browser File, or one from the desktop app's dialog). */
  async function useFitFile(file) {
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
  }

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
    syncPreset();
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
    return {
      doc: part.text,
      params: currentParams(),
      process: state.process,
      fit: state.fit === "none" ? "" : state.fits[state.fit] ?? "",
      assets: state.artwork ? [{ name: state.artwork.name, bytes: state.artwork.bytes }] : [],
      printer: state.printer,
    };
  }

  async function evaluate() {
    timer = null;
    if (invalid.size > 0) return; // changed() shows why; nothing to build
    const request = currentRequest();
    const artwork = state.artwork;
    const started = edits;
    setState("evaluating");
    showBuilding(`Building “${doc.name}”`, performance.now());
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
    // A part just opened is framed afresh; later builds keep the camera.
    const keepView = page.evaluations > 0 && !state.reframe;
    state.reframe = false;
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
      unchecked: (result.report.unchecked ?? []).map((u) => u.rule),
      printer: request.printer ?? "",
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
      `${doc.name}${gridControl && Array.isArray(v[gridControl.name]) ? ` ${v[gridControl.name].join(" × ")}` : ""}${v.relief ? `, ${v.relief}` : ""}`,
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
    showStatus([`The part could not be built: ${buildErrorText(state.error, part.opened.unsupported, spec)}`], "bad");
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
    const box = el("span", { className: "swatch", "aria-hidden": "true" });
    // Through the CSSOM, not a style="" attribute, which the desktop app's
    // Content Security Policy (style-src 'self') refuses.
    box.style.background = colour;
    return el("li", { id }, box, el("span", { text }));
  }
  /** The legend's line when a build failed: why the 3D view is empty. */
  const emptyNote = (what) => el("li", { id: "legend-empty", text: `Nothing to show: the ${what} could not be built (see the status line).` });
  function renderLegend() {
    if (page.mode !== "tile") return;
    const r = state.result;
    if (!r) {
      legend.replaceChildren(...(state.error ? [emptyNote("part")] : []));
      return;
    }
    const relief = r.params.values.relief;
    // What the accent part holds: the face slab of a filament-swap inlay, the
    // logo fill of a multi-material (parts) inlay, the logo otherwise.
    const accent =
      relief === undefined
        ? "Accent"
        : relief !== "inlay"
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
      summary.textContent = "The part could not be built, so it was not checked.";
      findingsList.replaceChildren(
        el(
          "li",
          { className: "finding blocker" },
          el("span", { className: "sev", "aria-hidden": "true", text: "⛔" }),
          el("div", {}, el("p", { className: "head" }, el("strong", { text: "Error" })), el("p", { text: buildErrorText(state.error, part.opened.unsupported, spec) })),
        ),
      );
      notes.hidden = true;
      return;
    }
    const r = state.result;
    if (!r) return;
    summary.textContent = reportSummary(r.report);
    summary.className = reportPasses(r.report) ? "ok" : "";
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
                  el("button", { type: "button", className: "link", onclick: () => openRecordStep() }, "Calibrate"),
                  " · ",
                  el("button", { type: "button", className: "link", onclick: () => openCouponStep("print") }, "Print the calibration coupon"),
                  " · ",
                  el("a", { href: PRINT_TEST_GUIDE_URL, target: "_blank", rel: "noopener" }, "How to print and measure it"),
                )
              : null,
          ),
        );
      }),
    );
    // Rules the core could not check, with its reason: never a silent pass.
    for (const u of r.report.unchecked ?? []) {
      findingsList.append(
        el(
          "li",
          { className: "finding unchecked", "data-rule": u.rule },
          el("span", { className: "sev", "aria-hidden": "true", text: "ℹ️" }),
          el("div", {}, el("p", { className: "head" }, el("strong", { text: "Not checked" }), el("code", { text: u.rule })), el("p", { text: u.reason })),
        ),
      );
    }
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
    else if (state.error) why = "Download is off until the part builds (see Validation).";
    else if (!state.result) why = "Waiting for the first build…";
    else if (state.dirty) why = "Updating the preview…";
    else why = downloadBlockedReason(state.result.report);
    const enabled = why === null;
    download3mf.disabled = !enabled;
    downloadStl.disabled = !enabled;
    reason.textContent = why ?? "";
    reason.className = why && ((state.result && !state.dirty && !state.exporting) || invalid.size > 0) ? "error" : "";
    // FIT.MISSING or FIT.PROCESS: the way out is the calibration wizard.
    $("calibrate-row").hidden = !(state.result && !state.dirty && !state.error && invalid.size === 0 && needsCalibration(state.result.report));
  }
  $("download-calibrate").addEventListener("click", () => openRecordStep());

  async function downloadPart(format) {
    if (!state.result?.exportAllowed || state.dirty || state.exporting) return;
    const suffix = state.resultArtwork?.sample?.suffix ?? null;
    const name = downloadName(doc, state.result.params.values, format, state.result.params.visible, suffix);
    state.exporting = true;
    renderDownloads();
    try {
      const { bytes } = await client.exportPart({ ...state.request, format, colors: { ...state.colors } });
      const saved = await save(bytes, name, format);
      if (saved.saved) page.downloads.push({ name: saved.name, format, bytes: bytes.byteLength });
      $("download-reason").textContent = "";
      showStatus(savedText(saved, kb(bytes.byteLength)));
    } catch (err) {
      showStatus([`The download failed: ${errorText(err)}`], "bad");
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
  /** Lay the coupon out for a printer's bed (x and y of its build volume). */
  function usePrinterBed(id) {
    const p = printerById(id);
    state.coupon.printer = p ? id : "";
    couponPrinter.value = state.coupon.printer;
    if (p) state.coupon.bed = p.buildVolumeMm.slice(0, 2);
    bedInputs.forEach((input, i) => {
      input.value = String(state.coupon.bed[i]);
    });
  }
  usePrinterBed(state.printer);
  bedInputs.forEach((input) => {
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
    if (page.mode === "coupon") {
      buildCoupon();
      if (state.coupon.step === "record") loadRecordMap();
    }
  }
  couponProcess.addEventListener("change", couponChanged);
  pocketsSelect.addEventListener("change", couponChanged);
  for (const input of bedInputs) {
    input.addEventListener("change", () => {
      // A typed size is no longer the printer's bed.
      state.coupon.printer = "";
      couponPrinter.value = "";
      couponChanged();
    });
  }
  couponPrinter.addEventListener("change", () => {
    const id = couponPrinter.value;
    usePrinterBed(id);
    if (id) {
      // The tile checks the same printer from now on.
      choosePrinter(id);
      changed();
    }
    couponChanged();
  });
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
      const saved = await save(bytes, name, format);
      if (saved.saved) page.downloads.push({ name: saved.name, format, bytes: bytes.byteLength });
      showStatus(savedText(saved, kb(bytes.byteLength)));
    } catch (err) {
      showStatus([`The download failed: ${errorText(err)}`], "bad");
    } finally {
      syncCounters();
      c.exporting = false;
      renderCouponDownloads();
    }
  }
  couponButtons[0].addEventListener("click", () => downloadCoupon("3mf"));
  couponButtons[1].addEventListener("click", () => downloadCoupon("stl"));

  // ------------------------------------------------------------------ record step
  //
  // docs/plan/05 section 4 ("tap the cell") and 06 section 11: a map of the
  // printed coupon from the core's layout (src/calibration.js places the
  // core's cells for display), one choice per line, the printer fields, the
  // core's record_fit for the profile, its offsets and its refusals, then
  // Download and Use.

  const rec = state.record;
  const recordDefaults = data.couponChoices.record ?? {};
  const printerSelect = $("record-printer");
  const printerOther = $("record-printer-other");
  const fieldInputs = {
    printer: printerSelect,
    material: $("record-material"),
    nozzleMm: $("record-nozzle"),
    date: $("record-date"),
    slicer: $("record-slicer"),
    notes: $("record-notes"),
  };
  printerSelect.append(...PRINTERS.map((p) => el("option", { value: p, text: p })), el("option", { value: OTHER_PRINTER, text: "Other…" }));
  $("record-materials").append(...MATERIALS.map((m) => el("option", { value: m })));
  fieldInputs.nozzleMm.value = typeof recordDefaults.nozzleMm === "number" ? String(recordDefaults.nozzleMm) : "";
  $("nozzle-default").textContent = fieldInputs.nozzleMm.value || "–";
  fieldInputs.date.value = todayLocal();
  const mapEl = $("coupon-map");
  /** The map's cell buttons by layout index. */
  let cellButtons = [];
  /** The coupon key whose map (or refusal) mapEl shows; null while it shows "Loading". */
  let drawnKey = null;

  const printerName = () => (printerSelect.value === OTHER_PRINTER ? printerOther.value : printerSelect.value);

  /** The map of the coupon as set above, or null while it loads or could not be laid out (a map of other settings is never used). */
  const currentMap = () => (rec.key === couponKey() ? rec.map : null);
  /** The coupon as set above could not be laid out (the core refused it). */
  const layoutFailed = () => rec.key === couponKey() && Boolean(rec.layoutError);

  /** The record_fit input for the coupon the map shows and the fields as they are. */
  function recordRequest() {
    const nozzle = parseNozzle(fieldInputs.nozzleMm.value);
    const summary = currentMap() ? rec.layout.summary : null;
    const input = recordInput({
      process: summary?.process ?? state.coupon.process,
      pocketSet: summary?.pockets ?? state.coupon.pockets,
      printer: printerName(),
      material: fieldInputs.material.value,
      nozzleMm: nozzle.value,
      slicer: fieldInputs.slicer.value,
      notes: fieldInputs.notes.value,
      // Without a date the CLI writes today's; so does the page.
      date: fieldInputs.date.value || todayLocal(),
      choices: rec.choices,
    });
    return { input, nozzleError: nozzle.error ?? null };
  }

  /** Ask the core for the profile of the current choices; the newest answer wins. */
  function updateRecord() {
    const seq = ++rec.seq;
    const { input, nozzleError } = recordRequest();
    rec.nozzleError = nozzleError;
    if (!currentMap() || nozzleError) {
      rec.result = null;
      rec.pending = null;
      renderRecord();
      return null;
    }
    const pending = client
      .recordFit(input)
      .then(
        (answer) => {
          if (seq === rec.seq) Object.assign(rec, { result: answer, callError: null });
        },
        (err) => {
          if (seq === rec.seq) Object.assign(rec, { result: null, callError: err.message });
        },
      )
      .finally(() => {
        syncCounters();
        if (seq === rec.seq) {
          rec.pending = null;
          renderRecord();
        }
      });
    rec.pending = pending;
    return pending;
  }

  /**
   * Load the map of the coupon as set above (the layout, without geometry).
   * rec.key names the settings of the stored layout only: an answer for
   * settings changed meanwhile is dropped and leaves rec.key alone, so coming
   * back to those settings loads them again instead of reusing another map.
   */
  async function loadRecordMap() {
    if (state.coupon.invalid) return;
    const request = couponRequest();
    const key = couponKey();
    if (rec.key === key && (rec.map || rec.layoutError)) {
      // Back to settings whose layout is stored while another was on its way:
      // draw it again where "Loading" was drawn, and ask the core again.
      if (drawnKey !== key) {
        renderMap();
        updateRecord();
      }
      return;
    }
    if (rec.loadingKey === key) return; // on its way
    rec.loadingKey = key;
    // Until it answers, the map of other settings is not shown or used.
    renderMap();
    renderRecord();
    const asked = client.couponLayout(request);
    rec.loading = asked.then(
      () => {},
      () => {},
    );
    let layout;
    try {
      layout = await asked;
    } catch (err) {
      syncCounters();
      page.recordLayouts++;
      if (rec.loadingKey === key) rec.loadingKey = null;
      if (couponKey() !== key) {
        page.staleLayouts++;
        return;
      }
      Object.assign(rec, { key, layout: null, map: null, layoutError: err.message });
      renderMap();
      updateRecord();
      return;
    }
    syncCounters();
    page.recordLayouts++;
    if (rec.loadingKey === key) rec.loadingKey = null;
    // Settings changed meanwhile, or an earlier request for them answered first.
    if (couponKey() !== key || (rec.key === key && rec.map)) {
      page.staleLayouts++;
      return;
    }
    const map = cellMap(layout);
    // A pocket the coupon as set now does not print is no longer offered.
    const { choices, dropped } = keepPrinted(rec.choices, map);
    rec.dropped = dropped.length
      ? `The coupon as set now has no such ${dropped.map((g) => groupName(g).toLowerCase()).join(" or ")} cell, so that choice was cleared: choose again.`
      : "";
    // The cell indices picked on the last map mean nothing on this one.
    rec.picked = {};
    Object.assign(rec, { key, layout, map, layoutError: null, choices });
    renderMap();
    updateRecord();
  }

  const css = (b) => `left:${b.left};top:${b.top};width:${b.width};height:${b.height}`;

  function renderMap() {
    const map = currentMap();
    cellButtons = [];
    drawnKey = map || layoutFailed() ? rec.key : null;
    const summary = map ? rec.layout.summary : null;
    $("record-coupon").textContent = summary
      ? `The map is the ${summary.process} coupon with ${summary.pockets === "crossAndDiagonal" ? "cross and diagonal" : summary.pockets} pockets and ${summary.copies} ${summary.copies === 1 ? "copy" : "copies"}, as set above: set them as you printed it.`
      : "";
    if (!map) {
      mapEl.style.aspectRatio = "";
      mapEl.replaceChildren(
        el("p", {
          className: layoutFailed() ? "map-empty note bad" : "map-empty note",
          text: layoutFailed() ? `No map: the coupon as set above cannot be laid out (${rec.layoutError}).` : "Loading the coupon's cells…",
        }),
      );
      $("map-caption-top").textContent = "";
      $("map-caption-bottom").textContent = "";
      return;
    }
    mapEl.style.aspectRatio = `${map.widthMm} / ${map.heightMm}`;
    mapEl.style.setProperty("--map-mm", String(map.widthMm));
    // The coupon (one window, or one per copy with a band naming it) and the
    // copy outlines where a window holds more than one copy; the cells' own
    // names say their copy, so these are not read out.
    const windows = map.windows.flatMap((w) => [
      el("div", { className: "window", "aria-hidden": "true", style: css(boxPercent(map, w)) }),
      ...(w.band ? [el("div", { className: "band", "aria-hidden": "true", style: css(boxPercent(map, w.band)), text: w.band.text })] : []),
    ]);
    const several = new Set(map.windows.filter((w) => w.copies.length > 1).flatMap((w) => w.copies));
    const outlines = map.copies.filter((c) => several.has(c.copy)).map((c) => el("div", { className: "copy", "aria-hidden": "true", style: css(boxPercent(map, c)) }));
    const groups = Object.entries(map.groups).map(([group, order]) => {
      const box = el("div", { role: "radiogroup", "aria-label": `Best ${groupName(group).toLowerCase()} cell`, "data-group": group });
      for (const index of order) {
        const cell = map.cells[index];
        const button = el(
          "button",
          {
            type: "button",
            role: "radio",
            className: `cell ${group}`,
            "aria-checked": "false",
            tabindex: -1,
            "aria-label": cellAccessibleName(map, cell, summary),
            "data-group": group,
            "data-copy": cell.copy + 1,
            "data-line": cell.line,
            "data-label": cell.lines.join("/"),
            "data-index": index,
            style: `${css(boxPercent(map, cell.box))};--font-mm:${cell.fontMm.toFixed(4)}`,
          },
          ...cell.lines.map((text) => el("span", { text })),
        );
        button.addEventListener("click", () => pick(index));
        button.addEventListener("keydown", (e) => {
          const next = moveInGroup(order, index, e.key);
          if (next === null) return;
          e.preventDefault();
          pick(next, { focus: true });
        });
        cellButtons[index] = button;
        box.append(button);
      }
      return box;
    });
    mapEl.replaceChildren(...windows, ...outlines, ...groups);
    $("map-caption-top").textContent = map.captions.top;
    $("map-caption-bottom").textContent = map.captions.bottom;
    renderMapSelection();
  }

  /**
   * One checked cell per kind of cell (aria-checked), its twins with the same
   * value marked as such (class and description, not checked), and one Tab
   * stop per kind (calibration.js mapSelection).
   */
  function renderMapSelection() {
    const map = currentMap();
    if (!map || cellButtons.length === 0) return;
    for (const [group, { checked, twins, stop }] of Object.entries(mapSelection(map, rec.choices, rec.picked))) {
      for (const i of map.groups[group]) {
        const b = cellButtons[i];
        const twin = twins.includes(i);
        b.setAttribute("aria-checked", String(i === checked));
        b.classList.toggle("twin", twin);
        if (twin) b.setAttribute("aria-describedby", "twin-note");
        else b.removeAttribute("aria-describedby");
        b.tabIndex = i === stop ? 0 : -1;
      }
    }
  }

  function pick(index, { focus = false } = {}) {
    const cell = currentMap()?.cells[index];
    if (!cell) return;
    rec.choices = choose(rec.choices, cell);
    rec.picked[cell.group] = index;
    rec.dropped = "";
    renderMapSelection();
    if (focus) cellButtons[index].focus();
    updateRecord();
  }

  /** The refusal to show, if any: map errors once a save was tried (or always but "no cells"), field errors once touched. */
  function shownError() {
    const e = rec.result && !rec.result.ok ? rec.result.error : null;
    if (!e) return null;
    const place = errorPlace(e.field);
    const shown = rec.attempted || (place === "map" ? e.field !== "cells" : rec.touched.has(place));
    return shown ? { ...e, place } : null;
  }

  function renderRecord() {
    renderMapSelection();
    const map = currentMap();
    const error = shownError();
    // The choices under the map, one line per kind of cell, with a refusal that concerns one.
    const groups = map ? Object.keys(map.groups) : [];
    $("choices").replaceChildren(
      ...groups.map((group) => {
        const text = choiceText(map, rec.choices, group);
        const own = error?.place === "map" && error.field === group ? error.message : "";
        return el(
          "li",
          { "data-group": group },
          el("span", { text: groupName(group) }),
          el("span", { className: text ? "value" : "value missing", text: text ? `${text} mm` : "not chosen" }),
          el("p", { className: "field-error", text: own }),
        );
      }),
    );
    const mapMessage = error?.place === "map" && !groups.includes(error.field) ? error.message : "";
    $("record-error-map").textContent = [rec.dropped, mapMessage, rec.callError ? `The core could not record: ${rec.callError}` : ""].filter(Boolean).join(" ");
    for (const field of FORM_FIELDS) {
      let message = error?.place === field ? error.message : "";
      if (field === "nozzleMm" && rec.nozzleError && (rec.attempted || rec.touched.has("nozzleMm"))) message = rec.nozzleError;
      $(`record-error-${field}`).textContent = message;
      const input = field === "printer" && printerSelect.value === OTHER_PRINTER ? printerOther : fieldInputs[field];
      if (message) input.setAttribute("aria-invalid", "true");
      else input.removeAttribute("aria-invalid");
      if (field === "printer") (input === printerOther ? printerSelect : printerOther).removeAttribute("aria-invalid");
    }
    const r = rec.result?.ok ? rec.result : null;
    $("record-summary").textContent = r
      ? `Fit profile ${r.fitProfile.id} for ${r.fitProfile.process}${r.fitProfile.calibration?.pocketSet ? `, ${r.fitProfile.calibration.pocketSet} pockets` : ""}, ${r.fitProfile.nozzleMm} mm nozzle: ${r.fileName}. Offsets are the chosen cell minus the standard nominal of its line.`
      : map
        ? "Choose your cells and fill in the printer and material: the offsets show here before you save."
        : "";
    $("record-offsets").replaceChildren(
      ...offsetRows(r?.offsets).map((row) =>
        el(
          "tr",
          { "data-cell": row.cell },
          el("td", { text: row.name }),
          el("td", { text: row.chosen }),
          el("td", { text: row.nominal }),
          el("td", {}, el("strong", { text: row.offset }), el("span", { className: "help", text: ` ${row.field}` })),
        ),
      ),
    );
    const marks = map ? mapSelection(map, rec.choices, rec.picked) : {};
    page.record = {
      step: state.coupon.step,
      cells: map?.cells.length ?? 0,
      labels: map?.cells.map((c) => c.label) ?? [],
      shown: map?.shown ?? null,
      checked: Object.fromEntries(Object.entries(marks).map(([g, m]) => [g, m.checked])),
      choices: chosenCells(rec.choices),
      ok: Boolean(r),
      id: r?.fitProfile.id ?? null,
      fileName: r?.fileName ?? null,
      offsets: r?.offsets ?? null,
      error: rec.result && !rec.result.ok ? rec.result.error : null,
      shownError: error ? error.message : null,
      nozzleError: rec.nozzleError,
      kept: state.recordedKept,
    };
  }

  for (const [field, input] of Object.entries(fieldInputs)) {
    const edited = () => {
      // Choosing "Other…" is not yet a printer name: no message until one is typed.
      if (!(field === "printer" && printerSelect.value === OTHER_PRINTER)) rec.touched.add(field);
      updateRecord();
    };
    input.addEventListener("input", edited);
    input.addEventListener("change", edited);
  }
  printerSelect.addEventListener("change", () => {
    const other = printerSelect.value === OTHER_PRINTER;
    printerOther.hidden = !other;
    if (other) printerOther.focus();
  });
  printerOther.addEventListener("input", () => {
    rec.touched.add("printer");
    updateRecord();
  });
  fieldInputs.date.addEventListener("change", () => {
    // An emptied date means today, as on the command line.
    if (!fieldInputs.date.value) fieldInputs.date.value = todayLocal();
  });

  /** The profile of the current choices once the core answered, or null after showing what to fix. */
  async function recordReady() {
    rec.attempted = true;
    // The map of the coupon as set, and the newest answer, if on their way.
    while (rec.loadingKey || rec.pending) await (rec.pending ?? rec.loading);
    renderRecord();
    if (rec.result?.ok) return rec.result;
    const error = shownError();
    const target =
      error && error.place !== "map"
        ? error.field === "printer" && printerSelect.value === OTHER_PRINTER
          ? printerOther
          : fieldInputs[error.place]
        : rec.nozzleError
          ? fieldInputs.nozzleMm
          : (cellButtons.find((b) => b?.tabIndex === 0) ?? $("record-heading"));
    target?.focus();
    showStatus([`Not recorded yet: ${error?.message ?? rec.nozzleError ?? rec.callError ?? "choose your cells first."}`], "bad");
    return null;
  }

  $("download-fit").addEventListener("click", async () => {
    const r = await recordReady();
    if (!r) return;
    const bytes = new TextEncoder().encode(r.text);
    try {
      // A browser download, or the desktop app's save dialog, which the user
      // may cancel (then nothing was saved).
      const saved = await save(bytes, r.fileName, "json");
      if (saved.saved) page.downloads.push({ name: saved.name, format: "json", bytes: bytes.byteLength });
      showStatus(savedText(saved, kb(bytes.byteLength)));
    } catch (err) {
      showStatus([`The download failed: ${errorText(err)}`], "bad");
    }
  });

  $("use-fit").addEventListener("click", async () => {
    const r = await recordReady();
    if (!r) return;
    const kept = saveRecordedFit(r.text);
    adoptRecorded(r.text, r.fitProfile, kept);
    const moved = selectRecorded(r.fitProfile);
    showFitNote(`Using the fit profile you recorded.${moved ? ` ${moved}` : ""}`);
    changed();
    const where = kept ? "This browser keeps it (and only this browser)." : "This browser's storage is off, so it is gone when the page reloads: download it to keep it.";
    $("record-note").replaceChildren(
      `The logo tile now uses ${r.fitProfile.id}. ${where} `,
      el("button", { type: "button", className: "link", id: "record-to-tile", onclick: () => setMode("tile", { focus: true }) }, "Design a tile with it"),
    );
    showStatus([`Using fit profile ${r.fitProfile.id} in the logo tile`]);
    renderRecord();
  });

  // ------------------------------------------------------------------ coupon steps

  const stepButtons = { print: $("step-print"), record: $("step-record") };
  const stepPanels = { print: $("coupon-print"), record: $("coupon-record") };
  /** The URL for the mode and step shown (?mode=coupon&step=record). */
  const pageUrl = () => `${location.pathname}${writeStep(writeMode(location.search, page.mode), page.mode, state.coupon.step)}${location.hash}`;

  function setStep(step, { focus = false } = {}) {
    state.coupon.step = step;
    page.step = step;
    for (const [s, button] of Object.entries(stepButtons)) {
      if (s === step) button.setAttribute("aria-current", "step");
      else button.removeAttribute("aria-current");
      stepPanels[s].hidden = s !== step;
    }
    if (page.mode === "coupon") {
      history.replaceState(null, "", pageUrl());
      if (step === "record") {
        renderMap();
        renderRecord();
        loadRecordMap();
      }
    }
    if (focus) (step === "record" ? $("record-heading") : stepButtons.print).focus();
  }
  for (const [step, button] of Object.entries(stepButtons)) button.addEventListener("click", () => setStep(step));
  $("howto-record").addEventListener("click", () => setStep("record", { focus: true }));

  /** The coupon mode at `step`. */
  function openCouponStep(step) {
    state.coupon.step = step;
    setMode("coupon", { focus: step === "print" });
    if (step === "record") $("record-heading").focus();
  }
  /** The calibration wizard's record step: the "Calibrate" of FIT.MISSING and FIT.PROCESS. */
  const openRecordStep = () => openCouponStep("record");

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
    history.replaceState(null, "", pageUrl());
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
      // Shows the step's panel (and loads the record step's map).
      setStep(state.coupon.step);
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

  // The first bundled part (the logo tile) is the one designed first.
  await openBundled(0);
  setMode(firstMode);
}

main().catch(fail);
