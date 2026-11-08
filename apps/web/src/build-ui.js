// Build mode's controller (docs/plan/07 section 10): the parts and colour
// palettes, the pointer and keyboard handling, undo and redo, saving and
// opening models, the samples menu and the stats panel. It sends pointer rays
// and commands to the core through the client and shows the answers with the
// assembly view; where a part goes, whether it collides and what is connected
// are the engine's answers (CLAUDE.md).

import { createAssemblyView } from "./assembly-view.js";
import {
  bomRows,
  colourHex,
  colourName,
  decodeInstances,
  frameSummary,
  ghostState,
  groupByPart,
  isClick,
  isModelFile,
  keyAction,
  modelFileName,
  modelNameFromFile,
  shadeHex,
  snapText,
  stateSentence,
  statsRows,
  thumbnailFit,
  thumbnailTriangles,
  typingTarget,
  wantDetail,
} from "./build.js";

const $ = (id) => document.getElementById(id);

function h(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "text") node.textContent = v;
    else if (k === "className") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  node.append(...children);
  return node;
}

/**
 * A radio group of buttons with a roving tabindex and arrow keys. `items` is
 * [{ value, label, node }]; `choose(value)` is called on a choice.
 */
function radioGroup(container, items, choose) {
  container.replaceChildren();
  const buttons = new Map();
  const select = (value, focus = false) => {
    for (const [v, b] of buttons) {
      b.setAttribute("aria-checked", String(v === value));
      b.tabIndex = v === value ? 0 : -1;
    }
    if (focus) buttons.get(value).focus();
    choose(value);
  };
  for (const item of items) {
    const b = h("button", { type: "button", role: "radio", "aria-checked": "false", "aria-label": item.label, title: item.label, className: "choice" }, item.node);
    b.addEventListener("click", () => select(item.value));
    b.addEventListener("keydown", (e) => {
      const i = items.findIndex((x) => x.value === item.value);
      const next = { ArrowRight: i + 1, ArrowDown: i + 1, ArrowLeft: i - 1, ArrowUp: i - 1, Home: 0, End: items.length - 1 }[e.key];
      if (next === undefined) return;
      e.preventDefault();
      select(items[(next + items.length) % items.length].value, true);
    });
    buttons.set(item.value, b);
    container.append(b);
  }
  return {
    select: (value) => buttons.has(value) && select(value),
    setChecked(value) {
      for (const [v, b] of buttons) {
        b.setAttribute("aria-checked", String(v === value));
        b.tabIndex = v === value ? 0 : -1;
      }
    },
  };
}

/** A small isometric picture of a part mesh, drawn on a canvas. */
function thumbnail(mesh, colour, size = 48) {
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const canvas = h("canvas", { width: String(size * ratio), height: String(size * ratio), "aria-hidden": "true", className: "thumb" });
  const ctx = canvas.getContext("2d");
  if (!ctx) return canvas;
  const { tris, box } = thumbnailTriangles(mesh.positions, mesh.indices);
  if (tris.length === 0) return canvas;
  const fit = thumbnailFit(box, size * ratio, 3 * ratio);
  for (const t of tris) {
    const p = t.points;
    ctx.beginPath();
    ctx.moveTo(p[0] * fit.scale + fit.offsetX, p[1] * fit.scale + fit.offsetY);
    ctx.lineTo(p[2] * fit.scale + fit.offsetX, p[3] * fit.scale + fit.offsetY);
    ctx.lineTo(p[4] * fit.scale + fit.offsetX, p[5] * fit.scale + fit.offsetY);
    ctx.closePath();
    ctx.fillStyle = shadeHex(colour, t.shade);
    ctx.fill();
    ctx.strokeStyle = ctx.fillStyle;
    ctx.lineWidth = 0.5;
    ctx.stroke();
  }
  return canvas;
}

/**
 * @param options { client, viewer, files, showStatus, page }
 * Returns { enter(), leave() }; `page.build` carries state for the smoke test.
 */
export function createBuildMode({ client, viewer, files, showStatus, page }) {
  let ready = null;
  let catalog = null;
  let view = null;
  let meshes = null;
  let detail = true;

  // What is chosen and shown.
  const choice = { part: null, colour: null, rot: 0, tool: "place" };
  let instances = [];
  const byId = new Map();
  let state = null;
  let hovered = null;
  let lastPointer = null;
  let snapBusy = false;
  let snapAgain = false;
  let lastSnap = null;
  let active = false;
  let modelName = "My build";
  let chain = Promise.resolve();

  const info = (page.build = {
    ready: false,
    parts: 0,
    selected: null,
    colour: null,
    rot: 0,
    tool: "place",
    ghost: "none",
    ghostPos: null,
    stats: null,
    message: "",
    error: null,
    detail: true,
    revision: 0,
    saves: [],
    counts: null,
    thumbnails: 0,
    /** Client coordinates of a point in the model frame (LDU), for tests. */
    project: (pos) => view?.project(pos),
    measure: (n) => viewer.measureFrames(n),
    /**
     * Time `n` of each core round trip a pointer causes, in ms per call, with
     * the model as it is: a snap query and a pick at `at` (client coordinates; the middle of the view by default),
     * and placing a part there and undoing it. For the smoke test and the
     * performance numbers in apps/web/README.md.
     */
    async bench(n = 20, at = null) {
      const r = viewer.internals.canvas.getBoundingClientRect();
      const x = at?.x ?? r.left + r.width / 2;
      const y = at?.y ?? r.top + r.height / 2;
      const ray = () => view.pointerRay(x, y);
      const time = async (fn) => {
        const t0 = performance.now();
        for (let i = 0; i < n; i++) await fn();
        return (performance.now() - t0) / n;
      };
      const id = catalog.parts[choice.part].id;
      const snapMs = await time(() => client.assemblerSnap(id, choice.rot, ray()));
      const pickMs = await time(() => client.assemblerPick(ray()));
      const snap = await client.assemblerSnap(id, choice.rot, ray());
      let placeUndoMs = null;
      if (snap?.valid) {
        placeUndoMs = await time(async () => {
          await client.assemblerApply({ op: "add", part: id, pos: snap.pos, rot: snap.rot, colour: choice.colour });
          await client.assemblerUndo();
        });
      }
      await refresh();
      return { n, snapMs, pickMs, placeUndoMs, parts: instances.length };
    },
  });

  const enqueue = (fn) => {
    chain = chain.then(fn, fn);
    return chain;
  };

  const partOf = (id) => catalog.parts.findIndex((p) => p.id === id);
  const names = () => Object.fromEntries(instances.map((i) => [i.id, catalog.parts[i.part].name]));

  function message(text, { alert = false } = {}) {
    const node = $("build-message");
    node.textContent = text;
    node.setAttribute("role", alert ? "alert" : "status");
    info.message = text;
  }

  function status(parts, kind = "") {
    showStatus(parts, kind);
  }

  // ----------------------------------------------------------- state from the core

  async function applyState(next) {
    state = next;
    await syncDetail(next.stats.parts);
    instances = decodeInstances(next.instances);
    byId.clear();
    for (const i of instances) byId.set(i.id, i);
    view.setInstances(groupByPart(instances));
    info.parts = instances.length;
    info.stats = next.stats;
    info.revision = next.revision;
    info.counts = view.counts();
    renderStats();
    $("build-undo").disabled = !next.canUndo;
    $("build-redo").disabled = !next.canRedo;
    $("build-clear").disabled = !instances.some((i) => !catalog.parts[i.part].base);
    const sizeNote = instances.length > 0 && !detail ? " (simple studs: the model is large)" : "";
    status([stateSentence(next.stats) + sizeNote]);
  }

  /** Switch the meshes to the other level when the model crossed the limit (src/build.js wantDetail). */
  async function syncDetail(count) {
    const want = wantDetail(count, detail);
    if (want === detail) return;
    ({ meshes } = await client.assemblerMeshes(want));
    detail = want;
    info.detail = want;
    view.setMeshes(meshes);
  }

  function renderStats() {
    const stats = $("build-stats");
    stats.replaceChildren();
    for (const [k, v] of statsRows(state.stats)) stats.append(h("dt", { text: k }), h("dd", { text: v }));
    const { rows, total } = bomRows(state.bom);
    const body = $("build-bom");
    body.replaceChildren();
    for (const r of rows) {
      const sw = h("span", { className: "swatch", "aria-hidden": "true" });
      sw.style.backgroundColor = colourHex(catalog.palette, r.code);
      body.append(h("tr", {}, h("td", { text: String(r.count) }), h("td", { text: r.part }), h("td", {}, sw, ` ${r.colour}`)));
    }
    $("build-bom-total").textContent = `${total} part${total === 1 ? "" : "s"} in all`;
  }

  async function refresh() {
    await applyState(await client.assemblerState());
  }

  // ----------------------------------------------------------- commands

  async function command(cmd, done) {
    try {
      const next = await client.assemblerApply(cmd);
      await applyState(next);
      info.error = null;
      if (done) message(done(next));
      return next;
    } catch (err) {
      info.error = err.message;
      message(err.message, { alert: true });
      status([err.message], "bad");
      return null;
    } finally {
      updateHover();
    }
  }

  const partName = () => catalog.parts[choice.part].name;

  async function placeAt(x, y) {
    const snap = await client.assemblerSnap(catalog.parts[choice.part].id, choice.rot, view.pointerRay(x, y));
    showSnap(snap);
    if (!snap) return message("Point at the baseplate or a part first");
    if (!snap.valid) return message(snapText(snap, partName(), names()).join(". "), { alert: true });
    await command(
      { op: "add", part: catalog.parts[choice.part].id, pos: snap.pos, rot: snap.rot, colour: choice.colour },
      (s) => `Placed ${partName()}, ${colourName(catalog.palette, choice.colour)}. ${stateSentence(s.stats)}`,
    );
  }

  async function removeAt(x, y) {
    const hit = await client.assemblerPick(view.pointerRay(x, y));
    const inst = hit?.instance != null ? byId.get(hit.instance) : null;
    if (!inst) return message("Nothing to remove there");
    if (catalog.parts[inst.part].base) return message("The baseplate stays: use Clear to remove the parts on it");
    const name = catalog.parts[inst.part].name;
    await command({ op: "remove", id: inst.id }, (s) => `Removed ${name}. ${stateSentence(s.stats)}`);
  }

  async function removeHovered() {
    const inst = hovered && byId.get(hovered);
    if (!inst || catalog.parts[inst.part].base) return message("Point at a part to remove it");
    const name = catalog.parts[inst.part].name;
    await command({ op: "remove", id: inst.id }, (s) => `Removed ${name}. ${stateSentence(s.stats)}`);
  }

  async function undo() {
    if (!state?.canUndo) return;
    try {
      await applyState(await client.assemblerUndo());
      message(`Undone. ${stateSentence(state.stats)}`);
    } catch (err) {
      message(err.message, { alert: true });
    }
    updateHover();
  }

  async function redo() {
    if (!state?.canRedo) return;
    try {
      await applyState(await client.assemblerRedo());
      message(`Redone. ${stateSentence(state.stats)}`);
    } catch (err) {
      message(err.message, { alert: true });
    }
    updateHover();
  }

  async function rotate(quarterTurns) {
    const { rot } = await client.assemblerTurn(choice.rot, quarterTurns);
    choice.rot = rot;
    info.rot = rot;
    message(`Rotated: ${quarterTurns > 0 ? "a quarter turn" : "a quarter turn back"}`);
    updateHover();
  }

  // ----------------------------------------------------------- hover and ghost

  function showSnap(snap) {
    lastSnap = snap;
    const g = ghostState(snap);
    info.ghost = g;
    info.ghostPos = snap ? snap.pos : null;
    hovered = snap?.target ?? null;
    const target = hovered && byId.get(hovered);
    view.setOutline(target && !catalog.parts[target.part].base ? target : null);
    if (choice.tool === "place" && snap) view.setGhost({ part: choice.part, rot: snap.rot, pos: snap.pos, state: g });
    else view.setGhost(null);
    if (active && choice.tool === "place") status(snapText(snap, partName(), names()));
  }

  async function hoverNow() {
    if (!lastPointer || !active) return;
    const { x, y } = lastPointer;
    const ray = view.pointerRay(x, y);
    if (choice.tool === "place") {
      showSnap(await client.assemblerSnap(catalog.parts[choice.part].id, choice.rot, ray));
    } else {
      const hit = await client.assemblerPick(ray);
      hovered = hit?.instance ?? null;
      const target = hovered && byId.get(hovered);
      view.setGhost(null);
      view.setOutline(target && !catalog.parts[target.part].base ? target : null);
      info.ghost = "none";
      status([target && !catalog.parts[target.part].base ? `Click to remove ${catalog.parts[target.part].name}` : "Point at a part to remove it"]);
    }
  }

  /** Ask for the hover of the latest pointer position; one request at a time. */
  function updateHover() {
    if (!lastPointer || !active) return;
    if (snapBusy) {
      snapAgain = true;
      return;
    }
    snapBusy = true;
    hoverNow()
      .catch(() => {})
      .finally(() => {
        snapBusy = false;
        if (snapAgain) {
          snapAgain = false;
          updateHover();
        }
      });
  }

  // ----------------------------------------------------------- the pointer

  function wirePointer() {
    const canvas = viewer.internals.canvas;
    let down = null;
    canvas.addEventListener("pointerdown", (e) => {
      if (!active) return;
      down = { x: e.clientX, y: e.clientY, t: performance.now(), id: e.pointerId, button: e.button };
    });
    canvas.addEventListener("pointermove", (e) => {
      if (!active || e.buttons !== 0 || e.pointerType === "touch") return;
      lastPointer = { x: e.clientX, y: e.clientY };
      updateHover();
    });
    canvas.addEventListener("pointerleave", (e) => {
      if (!active || e.pointerType === "touch") return;
      lastPointer = null;
      hovered = null;
      view.setGhost(null);
      view.setOutline(null);
      info.ghost = "none";
    });
    canvas.addEventListener("pointerup", (e) => {
      if (!active || !down || down.id !== e.pointerId) return;
      const d = down;
      down = null;
      if (d.button !== 0 || !isClick(d, { x: e.clientX, y: e.clientY, t: performance.now() })) return;
      lastPointer = { x: e.clientX, y: e.clientY };
      enqueue(() => (choice.tool === "place" ? placeAt(e.clientX, e.clientY) : removeAt(e.clientX, e.clientY)));
    });
    canvas.addEventListener("pointercancel", () => {
      down = null;
    });
    canvas.addEventListener("contextmenu", (e) => active && e.preventDefault());
  }

  function wireKeys() {
    document.addEventListener("keydown", (e) => {
      if (!active || typingTarget(e.target) || e.altKey) return;
      // Space and Enter on a focused button belong to the button.
      const action = keyAction(e);
      if (!action) return;
      e.preventDefault();
      enqueue(async () => {
        switch (action) {
          case "undo":
            return undo();
          case "redo":
            return redo();
          case "rotate":
            return rotate(1);
          case "rotateBack":
            return rotate(-1);
          case "delete":
            return removeHovered();
          case "deselect":
            return setTool("place");
        }
      });
    });
  }

  // ----------------------------------------------------------- files

  async function save() {
    const name = $("build-name").value.trim() || modelName;
    modelName = name;
    try {
      const { text } = await client.assemblerSave(name);
      const bytes = new TextEncoder().encode(text);
      const fileName = modelFileName(name);
      const saved = await files.save(bytes, fileName, "json");
      info.saves.push({ name: fileName, bytes });
      if (!saved.saved) return message("Not saved: the save dialog was closed without choosing a file");
      message(`Saved ${saved.path ?? saved.name}`);
      status([`Saved ${saved.path ?? saved.name}`, `${(bytes.length / 1024).toFixed(1)} KB`]);
    } catch (err) {
      message(err.message ?? String(err), { alert: true });
    }
  }

  async function loadText(text, name) {
    try {
      const next = await client.assemblerLoad(text);
      modelName = name;
      $("build-name").value = name;
      await applyState(next);
      view.frameLdu(...modelFrame());
      choice.rot = 0;
      info.rot = 0;
      message(`Opened ${name}: ${stateSentence(next.stats)}`);
      return true;
    } catch (err) {
      message(`Could not open ${name}: ${err.message}`, { alert: true });
      status([`Could not open ${name}`, err.message], "bad");
      info.error = err.message;
      return false;
    } finally {
      updateHover();
    }
  }

  async function openFile(file) {
    if (!file) return;
    if (!isModelFile(file.name)) return message(`${file.name} is not a model: choose a .obrick file`, { alert: true });
    if (file.size > catalog.maxModelBytes) {
      return message(`${file.name} is ${file.size} bytes; a model file can be at most ${catalog.maxModelBytes} bytes`, { alert: true });
    }
    await enqueue(async () => {
      await loadText(await file.text(), modelNameFromFile(file.name));
    });
  }

  async function chooseOpen() {
    if (files.desktop) {
      try {
        await openFile(await files.open("model", catalog.maxModelBytes));
      } catch (err) {
        message(String(err?.message ?? err), { alert: true });
      }
    } else {
      $("build-file").click();
    }
  }

  async function loadSample(id) {
    const sample = catalog.samples.find((s) => s.id === id);
    if (!sample) return;
    await enqueue(async () => {
      const { text } = await client.assemblerSample(id);
      await loadText(text, sample.label);
    });
  }

  // ----------------------------------------------------------- the camera

  /** The sphere (centre and radius in LDU) around the baseplate and what stands on it. */
  function modelFrame() {
    let lo = [-160, -120, -160];
    let hi = [160, 0, 160];
    for (const i of instances) {
      const b = catalog.parts[i.part].pickBox;
      for (const k of [0, 2]) {
        lo[k] = Math.min(lo[k], i.pos[k] + b.min[k]);
        hi[k] = Math.max(hi[k], i.pos[k] + b.max[k]);
      }
      lo[1] = Math.min(lo[1], i.pos[1] + b.min[1]);
    }
    const centre = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
    const radius = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) / 2;
    return [centre, radius];
  }

  // ----------------------------------------------------------- controls

  let partGroup = null;
  let colourGroup = null;

  function setTool(tool) {
    choice.tool = tool;
    info.tool = tool;
    const remove = $("build-remove-mode");
    remove.setAttribute("aria-pressed", String(tool === "remove"));
    document.body.dataset.buildTool = tool;
    view?.setGhost(null);
    view?.setOutline(null);
    status([tool === "remove" ? "Remove mode: click or tap a part to remove it" : `Placing ${choice.part === null ? "" : partName()}: point at the baseplate`]);
    updateHover();
  }

  function selectPart(index) {
    choice.part = index;
    info.selected = catalog.parts[index].id;
    partGroup?.setChecked(index);
    if (choice.tool !== "place") setTool("place");
    else updateHover();
    message(`${partName()} selected`);
  }

  function buildPalettes() {
    const base = catalog.palette.defaultPart;
    choice.colour = base;
    info.colour = base;
    const parts = catalog.parts.map((p, index) => {
      const mesh = meshes.find((m) => m.index === index);
      const node = h("span", { className: "choice-body" }, mesh ? thumbnail(mesh, colourHex(catalog.palette, base)) : "", h("span", { className: "choice-label", text: p.name.replace(/ x /g, "×") }));
      return { value: index, label: p.name, node };
    });
    info.thumbnails = parts.length;
    // The baseplate is the model's floor: it is listed last, so the first
    // part chosen is a brick.
    partGroup = radioGroup($("build-parts"), parts, (i) => selectPart(i));
    colourGroup = radioGroup(
      $("build-colours"),
      catalog.palette.colours.map((c) => {
        const sw = h("span", { className: "swatch big", "aria-hidden": "true" });
        sw.style.backgroundColor = c.rgb;
        return { value: c.code, label: c.name, node: sw };
      }),
      (code) => {
        choice.colour = code;
        info.colour = code;
        $("build-colour-name").textContent = colourName(catalog.palette, code);
      },
    );
    colourGroup.select(base);
    partGroup.select(catalog.parts.findIndex((p) => p.id === "ob.brick-2x4"));
  }

  function wireControls() {
    $("build-rotate").addEventListener("click", () => enqueue(() => rotate(1)));
    $("build-place-centre").addEventListener("click", () => {
      const r = viewer.internals.canvas.getBoundingClientRect();
      enqueue(() => (choice.tool === "place" ? placeAt(r.left + r.width / 2, r.top + r.height / 2) : removeAt(r.left + r.width / 2, r.top + r.height / 2)));
    });
    $("build-remove-mode").addEventListener("click", () => setTool(choice.tool === "remove" ? "place" : "remove"));
    $("build-undo").addEventListener("click", () => enqueue(undo));
    $("build-redo").addEventListener("click", () => enqueue(redo));
    $("build-clear").addEventListener("click", () => enqueue(() => command({ op: "clearBuilt" }, (s) => `Cleared. ${stateSentence(s.stats)}`)));
    $("build-save").addEventListener("click", () => enqueue(save));
    $("build-open").addEventListener("click", () => chooseOpen());
    $("build-file").addEventListener("change", () => {
      const file = $("build-file").files?.[0];
      $("build-file").value = "";
      openFile(file);
    });
    $("build-name").addEventListener("input", () => {
      modelName = $("build-name").value;
    });
    const samples = $("build-samples");
    samples.replaceChildren(h("option", { value: "", text: "Samples…" }), ...catalog.samples.map((s) => h("option", { value: s.id, text: s.label })));
    samples.addEventListener("change", () => {
      const id = samples.value;
      samples.value = "";
      if (id) loadSample(id);
    });
  }

  // ----------------------------------------------------------- start

  async function start() {
    status(["Building the parts with the core…"]);
    catalog = await client.assemblerCatalog();
    const first = await client.assemblerState();
    detail = wantDetail(first.stats.parts, true);
    info.detail = detail;
    ({ meshes } = await client.assemblerMeshes(detail));
    view = createAssemblyView(viewer, { catalog, lduMm: catalog.lduMm });
    view.setMeshes(meshes);
    buildPalettes();
    wireControls();
    wirePointer();
    wireKeys();
    if (first.stats.parts === 0) {
      // A new model starts on a baseplate, as one undoable command.
      const baseId = catalog.parts.find((p) => p.base).id;
      const next = await client.assemblerApply({ op: "add", part: baseId, pos: [0, -catalog.parts.find((p) => p.base).heightLdu, 0], rot: 0, colour: catalog.palette.defaultBase });
      await applyState(next);
      // The baseplate is part of the start, not an edit to undo.
      await client.assemblerLoad((await client.assemblerSave(modelName)).text);
      await refresh();
    } else await applyState(first);
    info.ready = true;
    info.parts = instances.length;
  }

  return {
    async enter() {
      active = true;
      ready ??= start();
      await ready.catch((err) => {
        info.error = err.message;
        throw err;
      });
      view.show(true);
      view.frameLdu(...modelFrame());
      $("build-hint").textContent = "Click or tap to place · drag to orbit · R rotates · Del removes · Ctrl+Z undoes";
      setTool(choice.tool);
      status([stateSentence(state.stats)]);
    },
    leave() {
      active = false;
      lastPointer = null;
      view?.setGhost(null);
      view?.setOutline(null);
      view?.show(false);
    },
    summary: frameSummary,
  };
}
