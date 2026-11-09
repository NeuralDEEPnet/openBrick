// Build mode's controller (docs/plan/07 section 10): the parts and colour
// palettes, the pointer and keyboard handling, undo and redo, saving and
// opening models, the samples menu and the stats panel. It sends pointer rays
// and commands to the core through the client and shows the answers with the
// assembly view; where a part goes, whether it collides and what is connected
// are the engine's answers (CLAUDE.md).

import { createAssemblyView } from "./assembly-view.js";
import { ModelKeeper } from "./model-keeper.js";
import { ModelTable, ROW } from "./model-table.js";
import { basePixelRatio, createAdaptiveResolution, hudLines, isPhone, triangleBudget } from "./lod.js";
import { benchWarnings, frameStats } from "./bench.js";
import {
  MeshCache,
  PAGE_SIZE,
  attributionLine,
  categoriesOf,
  creditLine,
  filterParts,
  gpuPlan,
  isPackFile,
  missingPackText,
  packFileLabel,
  pageHolding,
  pageOf,
  partBadge,
  partNote,
  rangeText,
} from "./parts-list.js";
import {
  aimRay,
  aimStep,
  bomRows,
  colourHex,
  colourName,
  frameSummary,
  ghostState,
  isClick,
  isModelFile,
  keyAction,
  replaceNeedsConfirm,
  usesPackParts,
  paletteName,
  createTaskQueue,
  removalTarget,
  stageKeyAction,
  clearable,
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
  // Every request to the core goes through `asm`, which first puts the model back if the
  // client replaced the worker (a cancelled Part or coupon build, or a trap).
  // The 3D view's accessible name outside Build mode, put back when Build is left.
  const stageLabel = $("stage").getAttribute("aria-label");
  const keeper = new ModelKeeper({
    client,
    onRestored: async (restored) => {
      await applyState(restored);
      info.restores += 1;
      message("The core was restarted, so your model was put back from the page's copy. The undo history is gone.", { alert: true });
    },
  });
  const asm = keeper.guarded();
  let ready = null;
  let catalog = null;
  let view = null;
  let detail = true;
  /** CPU copies of part meshes (for thumbnails and uploads), least recently used out first. */
  const cpuMeshes = new MeshCache();
  /** Thumbnail canvases by part number, drawn once when first listed. */
  const thumbs = new Map();
  /** The LDraw pack that is loaded: { info, bytes, name } or null. */
  let pack = null;
  /** The palette's search, category and page. */
  const list = { query: "", category: "", page: 0, indexes: [] };
  /** A model that needs a pack, kept until one is added: { text, name } or null. */
  let pendingModel = null;
  let partIndex = new Map();

  // What is chosen and shown.
  const choice = { part: null, colour: null, rot: 0, tool: "place" };
  /** The page's copy of the model (src/model-table.js), patched with the core's deltas. */
  const table = new ModelTable();
  /** Parts placed, by catalog part number. */
  let partCounts = new Map();
  /** What the core last said besides the parts: { canUndo, canRedo, revision, parts, stats, bom }. */
  let state = null;
  /** True while the model is small enough for full detail everywhere (src/build.js wantDetail). */
  let small = true;
  const hud = { on: false, timer: null };
  let hovered = null;
  let lastPointer = null;
  /** The keyboard's aim: a ground point of the model frame in LDU, or null when the pointer aims. */
  let aim = null;
  /** Edits since the last save or open: replacing the model asks first. */
  let dirty = false;
  let lastReason = "";
  let snapBusy = false;
  let snapAgain = false;
  let lastSnap = null;
  let active = false;
  let modelName = "My build";

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
    /** How many times the model was put back into a replaced worker. */
    restores: 0,
    /** Notices of the last model opened (another build of its pack was loaded). */
    notices: [],
    saveCount: 0,
    lastSave: null,
    hover: "",
    aim: null,
    dirty: false,
    canRestore: false,
    confirming: false,
    thumbnails: 0,
    /** True while a model is being opened in steps, and how far it is. */
    loading: false,
    progress: null,
    hud: false,
    pixelRatio: 1,
    /** The revision the statistics shown are for (the model's own, `revision`, when they are up to date). */
    statsFor: -1,
    /** The palette: parts listed, the page shown, the pack loaded, mesh bookkeeping. */
    list: { total: 0, shown: 0, page: 0, pages: 1, query: "", category: "" },
    pack: null,
    gpuParts: 0,
    cpuMeshes: 0,
    needs: null,
    /** Client coordinates of a point in the model frame (LDU), for tests. */
    project: (pos) => view?.project(pos),
    measure: (n) => viewer.measureFrames(n),
    /**
     * Place up to `n` different parts of the catalog, one at its own spot on the ground: each pose is the
     * core's answer to a ray straight down (the page picks the spots, never the poses). For the smoke test
     * and the numbers on draw calls and uploaded meshes with many part types.
     */
    async spread(n) {
      const candidates = catalog.parts.map((p, i) => ({ p, i })).filter(({ p }) => !p.base && p.connectivity !== "none");
      let placed = 0;
      for (const { p } of candidates) {
        if (placed >= n) break;
        const x = (placed % 16) * 100 - 750;
        const z = Math.floor(placed / 16) * 100 - 400;
        const snap = await asm.assemblerSnap(p.id, 0, aimRay(x, z));
        if (!snap?.valid) continue;
        await asm.assemblerApply({ op: "add", part: p.id, pos: snap.pos, rot: snap.rot, colour: choice.colour });
        placed += 1;
      }
      await refresh();
      return { placed, counts: info.counts, gpuParts: info.gpuParts, cpuMeshes: info.cpuMeshes };
    },
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
      const snapMs = await time(() => asm.assemblerSnap(id, choice.rot, ray()));
      const pickMs = await time(() => asm.assemblerPick(ray()));
      const snap = await asm.assemblerSnap(id, choice.rot, ray());
      let placeUndoMs = null;
      if (snap?.valid) {
        placeUndoMs = await time(async () => {
          await asm.assemblerApply({ op: "add", part: id, pos: snap.pos, rot: snap.rot, colour: choice.colour });
          await asm.assemblerUndo();
        });
      }
      await refresh();
      return { n, snapMs, pickMs, placeUndoMs, parts: table.size };
    },
  });

  // The catalog ids of the placed parts in id order (for the smoke test; empty for a big model), and what the view draws now.
  Object.defineProperty(info, "placed", { enumerable: true, get: () => (catalog ? table.placed(catalog.parts) : []) });
  Object.defineProperty(info, "counts", { enumerable: true, get: () => view?.counts() ?? null });

  const enqueue = createTaskQueue();

  const partOf = (id) => partIndex.get(id) ?? -1;
  /** Part names by id, looked up when asked (a hover line names one part, not all of them). */
  const names = new Proxy({}, { get: (_, id) => (table.has(Number(id)) ? catalog.parts[table.get(Number(id)).part].name : undefined) });
  /** The status sentence: the statistics when they are for this model, else just the count. */
  function sentence(s = state) {
    const stats = s?.stats ?? state?.stats;
    if (stats && stats.parts === table.size) return stateSentence(stats);
    return `${table.size} part${table.size === 1 ? "" : "s"}`;
  }

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

  /** Whether Clear has anything to remove (src/build.js clearable, over the table): any part but a baseplate on the ground. */
  function clearableNow() {
    for (const [part, n] of partCounts) if (n > 0 && !catalog.parts[part].base) return true;
    let any = false;
    table.forEachSlot((slot) => {
      const p = catalog.parts[table.partAt(slot)];
      if (!(p.base && table.posAt(slot, 1) + p.heightLdu === 0)) any = true;
    });
    return any;
  }

  /**
   * Take what the core says: a whole table (`instances`, after an open, a pack change or a restore) or what an
   * edit changed (`delta`), and the summary beside it. Only the chunks an edit touched are redrawn.
   */
  async function applyState(next) {
    let full = false;
    let ids = [];
    if (next.instances) {
      table.replaceAll(next.instances);
      partCounts = table.countsByPart();
      full = true;
    } else if (next.delta) {
      const rows = next.delta;
      const before = new Map();
      for (let at = 1; at < rows.length; at += ROW) before.set(rows[at], table.get(rows[at])?.part);
      const r = table.applyDelta(rows);
      if (r.full) {
        partCounts = table.countsByPart();
        full = true;
      } else {
        ids = r.ids;
        for (const id of ids) {
          const was = before.get(id);
          const now = table.get(id)?.part;
          if (was === now) continue;
          if (was !== undefined) partCounts.set(was, (partCounts.get(was) ?? 1) - 1);
          if (now !== undefined) partCounts.set(now, (partCounts.get(now) ?? 0) + 1);
        }
        for (const [part, n] of partCounts) if (n <= 0) partCounts.delete(part);
      }
    } else {
      throw new TypeError("the core's reply has neither a table nor a delta");
    }
    // Stats and the bill of materials come with the edit for a small model, later for a big one.
    state = {
      canUndo: next.canUndo,
      canRedo: next.canRedo,
      revision: next.revision,
      parts: next.parts ?? table.size,
      stats: next.stats ?? state?.stats ?? null,
      bom: next.bom ?? state?.bom ?? [],
    };
    small = wantDetail(table.size, small);
    info.detail = small;
    view.setPolicy({ small });
    await uploadParts([...partCounts.keys()]);
    if (full) view.setModel(table);
    else view.update(ids);
    releaseSpare();
    info.parts = table.size;
    info.stats = state.stats;
    info.revision = next.revision;
    renderStats();
    if (next.stats) info.statsFor = next.revision;
    else wantStats();
    $("build-undo").disabled = !next.canUndo;
    $("build-redo").disabled = !next.canRedo;
    $("build-clear").disabled = !clearableNow();
    $("build-restore").disabled = keeper.previous === null;
    info.canRestore = keeper.previous !== null;
    const sizeNote = table.size > 0 && !small ? " (simple studs: the model is large)" : "";
    status([sentence(next) + sizeNote]);
  }

  let statsTimer = null;
  /**
   * The statistics of a big model are asked for when editing pauses: the core counts connections again after a
   * removal (30 ms or more for 100,000 parts), so doing it after every edit would queue the pointer's snap behind it.
   */
  function wantStats() {
    clearTimeout(statsTimer);
    statsTimer = setTimeout(async () => {
      try {
        const r = await asm.assemblerStats();
        if (r.revision !== info.revision) return wantStats(); // edited since: a later one will ask
        state = { ...state, stats: r.stats, bom: r.bom };
        info.statsFor = r.revision;
        info.stats = r.stats;
        renderStats();
        if (active) status([sentence(state)]);
      } catch {
        // the core was replaced or the page left: the next edit asks again
      }
    }, 250);
  }

  // ----------------------------------------------------------- meshes of hundreds of parts

  /** Part numbers whose mesh must stay: the placed ones and the one chosen. */
  const keepMesh = (index) => index === choice.part || (partCounts.get(index) ?? 0) > 0;

  /** The CPU meshes of these part numbers, fetched in one request for the ones not held. */
  async function meshesOf(indices) {
    const missing = [...new Set(indices)].filter((i) => !cpuMeshes.has(i));
    for (let at = 0; at < missing.length; at += 128) {
      const { meshes } = await asm.assemblerMeshes(detail, missing.slice(at, at + 128));
      for (const m of meshes) cpuMeshes.set(m.index, m, (k) => keepMesh(k) || indices.includes(k));
    }
    return new Map(indices.map((i) => [i, cpuMeshes.get(i)]));
  }

  /** Make sure the view has the meshes of every render level for each of these part numbers. */
  async function uploadParts(indices) {
    const need = [...new Set(indices)].filter((i) => !view.has(i));
    if (need.length === 0) return;
    // A part of a pack has one mesh and its hull; the built-in parts have four levels (src/assembly-view.js).
    for (const [group, lods] of [
      [need.filter((i) => catalog.parts[i].origin !== "ldraw"), [0, 1, 2, 3]],
      [need.filter((i) => catalog.parts[i].origin === "ldraw"), [0, 3]],
    ]) {
      for (let at = 0; at < group.length; at += 32) {
        const { meshes } = await asm.assemblerLodMeshes(group.slice(at, at + 32), lods);
        view.addMeshes(meshes);
      }
    }
  }

  /** Drop GPU meshes of parts that are neither placed nor chosen, beyond a small spare. */
  function releaseSpare() {
    const plan = gpuPlan({ placed: [...partCounts.keys()], ghost: choice.part, held: view.resident() });
    if (plan.drop.length) view.release(plan.drop);
    info.gpuParts = view.resident().length;
    info.cpuMeshes = cpuMeshes.size;
  }

  function renderStats() {
    const stats = $("build-stats");
    stats.replaceChildren();
    if (state?.stats) {
      for (const [k, v] of statsRows(state.stats)) stats.append(h("dt", { text: k }), h("dd", { text: v }));
      if (state.stats.parts !== table.size) stats.append(h("dt", { text: "Counting" }), h("dd", { text: "connections are being counted again" }));
    } else {
      stats.append(h("dt", { text: "Parts" }), h("dd", { text: String(table.size) }), h("dt", { text: "Counting" }), h("dd", { text: "connections are being counted" }));
    }
    const { rows, total } = bomRows(state?.bom ?? []);
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
    await applyState(await asm.assemblerState());
  }

  // ----------------------------------------------------------- commands

  async function command(cmd, done) {
    try {
      const next = await asm.assemblerApply(cmd);
      await applyState(next);
      info.error = null;
      setDirty(true);
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

  const partName = () => displayName(catalog.parts[choice.part]);

  function setDirty(on) {
    dirty = on;
    info.dirty = on;
  }

  /** Place the chosen part where `ray` points (a pointer ray or the keyboard's aim). */
  async function placeWith(ray) {
    const snap = await asm.assemblerSnap(catalog.parts[choice.part].id, choice.rot, ray);
    showSnap(snap);
    if (!snap) return message("Point at the baseplate or a part first");
    if (!snap.valid) return message(snapText(snap, partName(), names).join(". "), { alert: true });
    await command(
      { op: "add", part: catalog.parts[choice.part].id, pos: snap.pos, rot: snap.rot, colour: choice.colour },
      (s) => `Placed ${partName()}, ${colourName(catalog.palette, choice.colour)}. ${sentence(s)}`,
    );
  }

  async function removeWith(ray) {
    const inst = await removalTarget((r) => asm.assemblerPick(r), ray, table);
    if (!inst) return message("Nothing to remove there");
    const name = catalog.parts[inst.part].name;
    await command({ op: "remove", id: inst.id }, (s) => `Removed ${name}. ${sentence(s)}`);
  }

  /**
   * Delete: remove the part at the aim or the pointer. Asked of the core now, inside the queue, so it
   * is the part there after every earlier edit; `hovered` is only what the last hover saw, which is
   * from before a part that was just placed (it still names the baseplate under it).
   */
  async function removeAtAim() {
    const ray = currentRay();
    if (!ray) return message("Point at a part to remove it");
    await removeWith(ray);
  }

  async function undo() {
    if (!state?.canUndo) return;
    try {
      await applyState(await asm.assemblerUndo());
      setDirty(true);
      message(`Undone. ${sentence(state)}`);
    } catch (err) {
      message(err.message, { alert: true });
    }
    updateHover();
  }

  async function redo() {
    if (!state?.canRedo) return;
    try {
      await applyState(await asm.assemblerRedo());
      setDirty(true);
      message(`Redone. ${sentence(state)}`);
    } catch (err) {
      message(err.message, { alert: true });
    }
    updateHover();
  }

  async function rotate(quarterTurns) {
    const { rot } = await asm.assemblerTurn(choice.rot, quarterTurns);
    choice.rot = rot;
    info.rot = rot;
    message(`Rotated: ${quarterTurns > 0 ? "a quarter turn" : "a quarter turn back"}`);
    updateHover();
  }

  // ----------------------------------------------------------- hover and ghost

  /** The hover line: not a live region, so moving the pointer is silent for a screen reader. */
  function setHover(text) {
    $("build-hover").textContent = text;
    info.hover = text;
  }

  /** The ray the keyboard's aim or the pointer gives, or null when neither points anywhere. */
  function currentRay() {
    if (aim) return aimRay(aim.x, aim.z);
    if (lastPointer) return view.pointerRay(lastPointer.x, lastPointer.y);
    return null;
  }

  function showSnap(snap) {
    lastSnap = snap;
    const g = ghostState(snap);
    info.ghost = g;
    info.ghostPos = snap ? snap.pos : null;
    hovered = snap?.target ?? null;
    const target = hovered && table.get(hovered);
    view.setOutline(target ? target : null);
    if (choice.tool === "place" && snap) view.setGhost({ part: choice.part, rot: snap.rot, pos: snap.pos, state: g });
    else view.setGhost(null);
    const text = snapText(snap, partName(), names);
    setHover(text.join(". "));
    // Only a change to a different problem is announced, never each pointer move.
    const reason = g === "invalid" ? `${snap.collides ?? ""}|${snap.mates === 0 && !snap.grounded}` : "";
    if (reason !== lastReason) {
      lastReason = reason;
      if (g === "invalid") message(text.join(". "));
    }
  }

  async function hoverNow() {
    const ray = currentRay();
    if (!ray || !active) return;
    if (choice.tool === "place") {
      const snap = await asm.assemblerSnap(catalog.parts[choice.part].id, choice.rot, ray);
      // The answer may arrive after the person left Build mode or moved the pointer away.
      if (!active || !currentRay()) return;
      showSnap(snap);
    } else {
      const hit = await asm.assemblerPick(ray);
      if (!active || !currentRay()) return;
      hovered = hit?.instance ?? null;
      const target = hovered && table.get(hovered);
      view.setGhost(null);
      view.setOutline(target ? target : null);
      info.ghost = "none";
      setHover(target ? `Click to remove ${catalog.parts[target.part].name}` : "Point at a part to remove it");
    }
  }

  /** Ask for the hover of the latest pointer position; one request at a time. */
  function updateHover() {
    if (!currentRay() || !active) return;
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
      setAim(null);
      updateHover();
    });
    canvas.addEventListener("pointerleave", (e) => {
      if (!active || e.pointerType === "touch") return;
      lastPointer = null;
      if (aim) return; // the keyboard's aim goes on
      hovered = null;
      view.setGhost(null);
      view.setOutline(null);
      info.ghost = "none";
      setHover("");
    });
    canvas.addEventListener("pointerup", (e) => {
      if (!active || !down || down.id !== e.pointerId) return;
      const d = down;
      down = null;
      if (d.button !== 0 || !isClick(d, { x: e.clientX, y: e.clientY, t: performance.now() })) return;
      lastPointer = { x: e.clientX, y: e.clientY };
      setAim(null);
      const ray = view.pointerRay(e.clientX, e.clientY);
      enqueue(() => (choice.tool === "place" ? placeWith(ray) : removeWith(ray)));
    });
    canvas.addEventListener("pointercancel", () => {
      down = null;
    });
    canvas.addEventListener("contextmenu", (e) => active && e.preventDefault());
  }

  /** Set or clear the keyboard's aim ({ x, z } in LDU); the pointer takes over by clearing it. */
  function setAim(next) {
    aim = next;
    info.aim = next ? [next.x, next.z] : null;
  }

  /** What an arrow, Enter, Shift+arrow or +/- does while the 3D view has the focus. */
  async function stageAction(a) {
    const { internals } = viewer;
    switch (a.type) {
      case "orbit":
        return internals.orbitBy(a.azimuth, a.polar);
      case "zoom":
        return internals.zoomBy(a.factor);
      case "reframe":
        return view.frameLdu(...modelFrame());
      case "aim": {
        const [dx, dz] = aimStep(a.key, internals.cameraForward());
        const pitch = catalog.studPitchLdu;
        setAim({ x: (aim?.x ?? 0) + dx * pitch, z: (aim?.z ?? 0) + dz * pitch });
        lastPointer = null;
        await hoverNow();
        const where = `stud ${aim.x / pitch}, ${aim.z / pitch}`;
        message(`Aim at ${where}. ${info.hover}`);
        return;
      }
      case "place": {
        if (!aim) {
          // The first press only shows where the aim is; nothing is placed blind.
          setAim({ x: 0, z: 0 });
          lastPointer = null;
          await hoverNow();
          return message(`Aim at the middle of the baseplate. ${info.hover}. Arrow keys move it; Enter places.`);
        }
        const ray = aimRay(aim.x, aim.z);
        await (choice.tool === "place" ? placeWith(ray) : removeWith(ray));
        return;
      }
    }
  }

  function wireKeys() {
    const stage = $("stage");
    document.addEventListener("keydown", (e) => {
      if (!active || typingTarget(e.target) || e.altKey) return;
      if (document.activeElement === stage) {
        const a = stageKeyAction(e);
        if (a) {
          e.preventDefault();
          enqueue(() => stageAction(a));
          return;
        }
      }
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
            return removeAtAim();
          case "deselect":
            setAim(null);
            return setTool("place");
        }
      });
    });
  }

  // ----------------------------------------------------------- files

  /** Save the model as .obrick.json text; true when a file was written. */
  async function save() {
    const name = $("build-name").value.trim() || modelName;
    modelName = name;
    try {
      const { text } = await asm.assemblerSave(name);
      const bytes = new TextEncoder().encode(text);
      const fileName = modelFileName(name);
      const saved = await files.save(bytes, fileName, "json");
      if (!saved.saved) {
        message("Not saved: the save dialog was closed without choosing a file");
        return false;
      }
      // Only the name and size are kept, never the bytes.
      info.saveCount += 1;
      info.lastSave = { name: fileName, size: bytes.length };
      setDirty(false);
      message(`Saved ${saved.path ?? saved.name}`);
      status([`Saved ${saved.path ?? saved.name}`, `${(bytes.length / 1024).toFixed(1)} KB`]);
      return true;
    } catch (err) {
      message(err.message ?? String(err), { alert: true });
      return false;
    }
  }

  /** Show how far a big document is read (only above a few thousand parts, so a small one does not flash it). */
  function showProgress(p) {
    if (!p || p.total < 3000 || p.done >= p.total) {
      $("build-progress").hidden = true;
      info.progress = null;
      return;
    }
    $("build-progress").hidden = false;
    $("build-progress-bar").max = p.total;
    $("build-progress-bar").value = p.done;
    $("build-progress-text").textContent = `Opening: ${p.done.toLocaleString("en-US")} of ${p.total.toLocaleString("en-US")} parts checked`;
    info.progress = { done: p.done, total: p.total };
  }

  /**
   * Make the model what `produce()` reads: a document in steps (with progress and a Cancel button) or the
   * table of a model set aside. Resolves true when it opened.
   */
  async function openModel(name, produce) {
    info.loading = true;
    const t0 = performance.now();
    try {
      const next = await produce();
      const t1 = performance.now();
      // The model opened, with another build of the pack it was made with: say so.
      const notices = next.notices ?? [];
      info.notices = notices;
      modelName = name;
      $("build-name").value = name;
      await applyState(next);
      /** Where the time of an open went, in ms: the core reading the document, and the page taking it in. */
      info.openTiming = { coreMs: Math.round(t1 - t0), pageMs: Math.round(performance.now() - t1) };
      setDirty(false);
      view.frameLdu(...modelFrame());
      choice.rot = 0;
      info.rot = 0;
      message(`Opened ${name}: ${sentence(next)}${notices.length ? ` Notice: ${notices.join(" ")}` : ""}`);
      if (notices.length) status([`Opened ${name}`, ...notices]);
      return true;
    } catch (err) {
      const cancelled = /was cancelled/.test(err.message);
      message(cancelled ? `Opening ${name} was cancelled; your model is as it was` : `Could not open ${name}: ${err.message}`, { alert: !cancelled });
      status([cancelled ? `Opening ${name} was cancelled` : `Could not open ${name}`, ...(cancelled ? [] : [err.message])], cancelled ? "" : "bad");
      info.error = cancelled ? null : err.message;
      return false;
    } finally {
      info.loading = false;
      showProgress(null);
      updateHover();
    }
  }

  async function loadText(text, name) {
    // A model made with an LDraw pack says so: if the parts are not here, offer to add the pack
    // instead of failing, and keep the model for when it is added.
    try {
      const needs = await asm.assemblerNeeds(text);
      const why = missingPackText(needs, pack !== null);
      if (why) {
        pendingModel = { text, name };
        showNeeds(why);
        message(`${name} needs parts that are not loaded. ${why}`, { alert: true });
        status([`${name} needs an LDraw pack`, why], "bad");
        return false;
      }
    } catch (err) {
      message(`Could not open ${name}: ${err.message}`, { alert: true });
      status([`Could not open ${name}`, err.message], "bad");
      info.error = err.message;
      return false;
    }
    showNeeds(null);
    pendingModel = null;
    return openModel(name, () => asm.assemblerLoad(text, showProgress));
  }

  // The in-page confirmation (a dialog box would block the desktop webview and a test
  // driver): Save first, Replace, or Cancel, with the focus on Cancel.
  let answer = null;
  function confirmReplace(label, question) {
    $("build-confirm-text").textContent = question ?? `Replace your unsaved model with ${label}? The changes you made since the last save would be lost.`;
    $("build-confirm").hidden = false;
    info.confirming = true;
    $("build-confirm-cancel").focus();
    return new Promise((resolve) => {
      answer = (value) => {
        answer = null;
        $("build-confirm").hidden = true;
        info.confirming = false;
        resolve(value);
      };
    });
  }

  /**
   * Replace the model with what `produce()` gives ({ text, name }, or null for nothing),
   * after asking when there are unsaved edits, and set the old one aside for "Restore
   * previous model". Resolves true when the model was replaced.
   */
  async function replaceModel(label, produce) {
    if (replaceNeedsConfirm({ dirty })) {
      const reply = await confirmReplace(label);
      if (reply === "cancel") {
        message("Kept your model");
        return false;
      }
      if (reply === "save" && !(await save())) return false;
    }
    const prior = { rows: table.toRows(), name: modelName, dirty };
    const made = await produce();
    if (!made) return false;
    if (!(await loadText(made.text, made.name))) return false;
    setAside(prior);
    message(`Opened ${made.name}. "Restore previous model" brings back the one it replaced.${info.notices.length ? ` Notice: ${info.notices.join(" ")}` : ""}`);
    return true;
  }

  /** Bring back the model that Open or a sample replaced; the one on screen becomes the previous one. */
  async function restorePrevious() {
    const prev = keeper.previous;
    if (!prev) return;
    const current = { rows: table.toRows(), name: modelName, dirty };
    if (await openModel(prev.name, () => asm.assemblerLoadRows(prev.rows))) {
      setDirty(prev.dirty);
      keeper.setPrevious(current);
      message(`Restored ${prev.name}.${info.notices.length ? ` Notice: ${info.notices.join(" ")}` : ""}`);
    }
  }

  /** Keep `prior` ({ rows, name, dirty }) for "Restore previous model". */
  function setAside(prior) {
    keeper.setPrevious(prior);
    $("build-restore").disabled = false;
    info.canRestore = true;
  }

  /**
   * Before a change of pack that may take away parts the model on screen uses: when the model
   * has unsaved edits, ask (Save first, Replace, Cancel). Resolves true to go on. The model is
   * set aside afterwards if the core had to clear it, so nothing is lost silently.
   */
  async function confirmPackChange(what) {
    if (![...partCounts.keys()].some((i) => catalog.parts[i]?.origin === "ldraw") || !replaceNeedsConfirm({ dirty })) return true;
    const reply = await confirmReplace(
      what,
      `${what} may take away parts your unsaved model uses, and the model would be set aside (Restore previous model brings it back). The changes you made since the last save could be lost. Continue?`,
    );
    if (reply === "cancel") {
      message("Kept your model and the parts you have");
      return false;
    }
    if (reply === "save" && !(await save())) return false;
    return true;
  }

  async function openFile(file) {
    if (!file) return;
    if (!isModelFile(file.name)) return message(`${file.name} is not a model: choose a .obrick.json file`, { alert: true });
    if (file.size > catalog.maxModelBytes) {
      return message(`${file.name} is ${file.size} bytes; a model file can be at most ${catalog.maxModelBytes} bytes`, { alert: true });
    }
    await enqueue(() => replaceModel(file.name, async () => ({ text: await file.text(), name: modelNameFromFile(file.name) })));
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
    await enqueue(() =>
      replaceModel(`the ${sample.label} sample`, async () => {
        const { text } = await asm.assemblerSample(id);
        return { text, name: sample.label };
      }),
    );
  }

  // ----------------------------------------------------------- the camera

  /** The sphere (centre and radius in LDU) around the baseplate and what stands on it. */
  function modelFrame() {
    let lo = [-160, -120, -160];
    let hi = [160, 0, 160];
    table.forEachSlot((slot) => {
      const b = catalog.parts[table.partAt(slot)].pickBox;
      for (const k of [0, 2]) {
        lo[k] = Math.min(lo[k], table.posAt(slot, k) + b.min[k]);
        hi[k] = Math.max(hi[k], table.posAt(slot, k) + b.max[k]);
      }
      lo[1] = Math.min(lo[1], table.posAt(slot, 1) + b.min[1]);
    });
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
    status([tool === "remove" ? "Remove mode: click or tap a part to remove it" : `Placing ${choice.part === null ? "" : partName()}: point at the ground or a part`]);
    updateHover();
  }

  async function selectPart(index) {
    choice.part = index;
    info.selected = catalog.parts[index].id;
    partGroup?.setChecked(index);
    $("build-part-note").textContent = partNote(catalog.parts[index]) ?? "";
    if (choice.tool !== "place") setTool("place");
    else updateHover();
    message(`${partName()} selected`);
    // Its mesh may not be uploaded yet (the palette lists parts it has not drawn).
    try {
      await uploadParts([index]);
      releaseSpare();
    } catch (err) {
      message(err.message ?? String(err), { alert: true });
    }
    updateHover();
  }

  /** A thumbnail slot for part `index`: a placeholder now, the picture when its mesh arrives. */
  function thumbSlot(index) {
    // The canvas itself moves from page to page (a clone would not carry the picture); a part is listed once.
    const cached = thumbs.get(index);
    if (cached) return cached;
    return h("span", { className: "thumb pending", "aria-hidden": "true", "data-part": String(index) });
  }

  let thumbChain = Promise.resolve();
  /** Draw the thumbnails of parts on the page that have none yet, one request for all of them. */
  function drawThumbs(indices) {
    const todo = indices.filter((i) => !thumbs.has(i));
    if (todo.length === 0) return thumbChain;
    thumbChain = thumbChain
      .then(async () => {
        const got = await meshesOf(todo);
        for (const i of todo) {
          const m = got.get(i);
          if (!m) continue;
          const canvas = thumbnail(m, colourHex(catalog.palette, catalog.palette.defaultPart));
          thumbs.set(i, canvas);
          document.querySelector(`#build-parts [data-part="${i}"]`)?.replaceWith(canvas);
        }
        info.thumbnails = thumbs.size;
        releaseSpare();
      })
      .catch(() => {});
    return thumbChain;
  }

  /** The palette: the parts that match the search and category, one page of them, and its pager. */
  function renderPartList({ keepPage = false } = {}) {
    list.indexes = filterParts(catalog.parts, { query: list.query, category: list.category });
    if (!keepPage) list.page = 0;
    const { page, pages, items } = pageOf(list.indexes, list.page, PAGE_SIZE);
    list.page = page;
    const buttons = items.map((index) => {
      const p = catalog.parts[index];
      const badge = partBadge(p);
      const label = displayName(p);
      const node = h(
        "span",
        { className: "choice-body" },
        thumbSlot(index),
        h("span", { className: "choice-label", text: label.replace(/ x /g, "×") }),
        badge ? h("span", { className: "choice-badge", text: badge }) : "",
      );
      return { value: index, label: badge ? `${label} (${badge})` : label, node };
    });
    partGroup = radioGroup($("build-parts"), buttons, (i) => selectPart(i));
    if (choice.part !== null) partGroup.setChecked(choice.part);
    $("build-range").textContent = rangeText(list.indexes.length, page, PAGE_SIZE);
    $("build-prev").disabled = page === 0;
    $("build-next").disabled = page >= pages - 1;
    Object.assign(info.list, { total: list.indexes.length, shown: items.length, page, pages, query: list.query, category: list.category });
    drawThumbs(items);
  }

  function fillCategories() {
    const select = $("build-category");
    const keep = list.category;
    select.replaceChildren(h("option", { value: "", text: `All categories (${catalog.parts.length})` }), ...categoriesOf(catalog.parts).map((c) => h("option", { value: c.name, text: `${c.name} (${c.count})` })));
    select.value = [...select.options].some((o) => o.value === keep) ? keep : "";
    list.category = select.value;
    // The category filter is only worth showing when there is more than a handful of parts.
    $("build-part-tools").hidden = catalog.parts.length <= PAGE_SIZE;
  }

  const displayName = (p) => paletteName(p.name.replace(/\s+/g, " ").trim());

  function buildPalettes() {
    const base = catalog.palette.defaultPart;
    choice.colour = base;
    info.colour = base;
    partIndex = new Map(catalog.parts.map((p, i) => [p.id, i]));
    fillCategories();
    renderPartList();
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
    // The baseplate is the model's floor: it is listed last among the built-ins, so the first
    // part chosen is a brick.
    const first = partIndex.get("ob.brick-2x4");
    // Show the page that holds it, and choose it.
    list.page = pageHolding(list.indexes.indexOf(first), PAGE_SIZE);
    renderPartList({ keepPage: true });
    selectPart(first);
  }

  // ----------------------------------------------------------- LDraw packs

  /** Show or hide the attribution and licence text for the loaded pack. */
  function renderPackInfo() {
    const box = $("build-pack-info");
    box.hidden = !pack;
    info.pack = pack ? { library: pack.info.library, parts: pack.info.parts, fingerprint: pack.info.fingerprint, name: pack.name } : null;
    if (!pack) return;
    $("build-attribution").textContent = attributionLine(pack.info);
    $("build-licences").textContent = pack.info.licences.map((l) => `${l.name}\n\n${l.text}`).join("\n\n--------\n\n");
    $("build-pack-counts").textContent = `Pack ${packFileLabel(pack.name)}: ${pack.info.counts.imported} parts from ${pack.info.counts.scanned.toLocaleString("en-US")} files of LDraw library ${pack.info.library}; ${pack.info.counts.freePlacement} free placement only; ${pack.info.counts.withJoints} with joints that are not modelled yet.`;
  }

  /** Show the credit of the chosen part (its author and licence line) when it comes from a pack. */
  function renderCredit() {
    const p = catalog.parts[choice.part];
    $("build-credit").textContent = p && p.origin === "ldraw" ? creditLine(p) : "";
  }

  /** Take an LDraw pack file: the core checks it and joins its parts to the catalog. */
  async function loadPackBytes(bytes, name) {
    try {
      if (!(await confirmPackChange(`Adding ${name}`))) return false;
      // What the model was, to set aside if the new pack lacks parts it uses and the core clears it.
      const prior = { rows: table.toRows(), name: modelName, dirty };
      message(`Reading ${name}…`);
      const r = await asm.assemblerLoadPack(bytes);
      pack = { info: r.pack, bytes, name };
      keeper.rememberPack(bytes);
      catalog = r.catalog;
      view.setCatalog(catalog);
      thumbs.clear();
      cpuMeshes.clear();
      info.error = null;
      renderPackInfo();
      buildPartsAgain();
      await applyState(r);
      if (!r.kept) {
        setAside(prior);
        setDirty(false);
      }
      message(`Added ${r.pack.parts} parts from the LDraw library ${r.pack.library}.${r.kept ? "" : ' The model on screen uses parts the pack lacks, so it was set aside: "Restore previous model" brings it back when a pack that has them is loaded.'}`);
      status([`${name}: ${r.pack.parts} parts added`, packFileLabel(name)]);
      if (pendingModel) {
        const m = pendingModel;
        pendingModel = null;
        showNeeds(null);
        await replaceModel(m.name, async () => m);
      }
      return true;
    } catch (err) {
      const text = err.message ?? String(err);
      message(`Could not add ${name}: ${text}`, { alert: true });
      status([`Could not add ${name}`, text], "bad");
      info.error = text;
      return false;
    }
  }

  /** Rebuild the part list after the catalog changed, keeping the chosen part when it still exists. */
  function buildPartsAgain() {
    const keepId = choice.part !== null ? catalog.parts[choice.part]?.id : null;
    partIndex = new Map(catalog.parts.map((p, i) => [p.id, i]));
    fillCategories();
    const stay = keepId && partIndex.has(keepId) ? partIndex.get(keepId) : partIndex.get("ob.brick-2x4");
    choice.part = stay;
    list.page = 0;
    renderPartList();
    selectPart(stay);
  }

  async function removePack() {
    try {
      if (!(await confirmPackChange("Removing the LDraw pack"))) return;
      const prior = { rows: table.toRows(), name: modelName, dirty };
      const r = await asm.assemblerUnloadPack();
      pack = null;
      keeper.rememberPack(null);
      catalog = r.catalog;
      view.setCatalog(catalog);
      thumbs.clear();
      cpuMeshes.clear();
      renderPackInfo();
      buildPartsAgain();
      await applyState(r);
      if (!r.kept) {
        setAside(prior);
        setDirty(false);
      }
      message(r.kept ? "Removed the LDraw pack" : 'Removed the LDraw pack. The model on screen used its parts, so it was set aside: "Restore previous model" brings it back (it asks for the pack again).');
    } catch (err) {
      message(err.message ?? String(err), { alert: true });
    }
  }

  async function openPackFile(file) {
    if (!file) return;
    if (!isPackFile(file.name)) return message(`${file.name} is not a parts pack: choose an .obpack made by "openbrick catalog import" (see docs/guides/ldraw-parts.md)`, { alert: true });
    const limit = catalog.maxPackBytes;
    if (file.size > limit) return message(`${file.name} is ${file.size} bytes; a pack can be at most ${limit} bytes`, { alert: true });
    await loadPackBytes(new Uint8Array(await file.arrayBuffer()), file.name);
  }

  async function choosePack() {
    if (files.desktop) {
      try {
        await openPackFile(await files.open("pack", catalog.maxPackBytes));
      } catch (err) {
        message(String(err?.message ?? err), { alert: true });
      }
    } else {
      $("build-pack-file").click();
    }
  }

  /** The notice for a model that needs a pack: what it needs, and a button to add one. */
  function showNeeds(text) {
    info.needs = text;
    $("build-needs").hidden = text === null;
    $("build-needs-text").textContent = text ?? "";
    if (text !== null) $("build-needs-add").focus();
  }

  function wireControls() {
    $("build-rotate").addEventListener("click", () => enqueue(() => rotate(1)));
    $("build-place-centre").addEventListener("click", () => {
      const r = viewer.internals.canvas.getBoundingClientRect();
      enqueue(() => (choice.tool === "place" ? placeWith(view.pointerRay(r.left + r.width / 2, r.top + r.height / 2)) : removeWith(view.pointerRay(r.left + r.width / 2, r.top + r.height / 2))));
    });
    $("build-remove-mode").addEventListener("click", () => setTool(choice.tool === "remove" ? "place" : "remove"));
    $("build-undo").addEventListener("click", () => enqueue(undo));
    $("build-redo").addEventListener("click", () => enqueue(redo));
    $("build-clear").addEventListener("click", () => enqueue(() => command({ op: "clearBuilt" }, (s) => `Cleared. ${sentence(s)}`)));
    $("build-save").addEventListener("click", () => enqueue(save));
    $("build-restore").addEventListener("click", () => enqueue(restorePrevious));
    $("build-confirm-save").addEventListener("click", () => answer?.("save"));
    $("build-confirm-replace").addEventListener("click", () => answer?.("replace"));
    $("build-confirm-cancel").addEventListener("click", () => answer?.("cancel"));
    $("build-confirm").addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        answer?.("cancel");
      }
    });
    $("build-progress-cancel").addEventListener("click", () => {
      // Not queued: the open itself is the task being waited for.
      asm.assemblerCancel().catch(() => {});
    });
    $("build-open").addEventListener("click", () => chooseOpen());
    $("build-pack-open").addEventListener("click", () => choosePack());
    $("build-needs-add").addEventListener("click", () => choosePack());
    $("build-needs-dismiss").addEventListener("click", () => {
      pendingModel = null;
      showNeeds(null);
    });
    $("build-pack-remove").addEventListener("click", () => enqueue(removePack));
    $("build-pack-file").addEventListener("change", () => {
      const file = $("build-pack-file").files?.[0];
      $("build-pack-file").value = "";
      enqueue(() => openPackFile(file));
    });
    let searchTimer = null;
    $("build-search").addEventListener("input", () => {
      list.query = $("build-search").value;
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => renderPartList(), 120);
    });
    $("build-category").addEventListener("change", () => {
      list.category = $("build-category").value;
      renderPartList();
    });
    $("build-prev").addEventListener("click", () => {
      list.page = Math.max(0, list.page - 1);
      renderPartList({ keepPage: true });
    });
    $("build-next").addEventListener("click", () => {
      list.page += 1;
      renderPartList({ keepPage: true });
    });
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
    catalog = await asm.assemblerCatalog();
    const first = await asm.assemblerState();
    view = createAssemblyView(viewer, { catalog, lduMm: catalog.lduMm });
    // The page's table is its copy of the model: a replaced core is given it back (src/model-keeper.js), no text to write or read.
    keeper.remember(() => table.toRows());
    wireDisplay();
    buildPalettes();
    wireControls();
    wirePointer();
    wireKeys();
    if (first.parts === 0) {
      // A new model starts on a baseplate, as one undoable command.
      const base = catalog.parts.find((p) => p.base);
      const next = await asm.assemblerApply({ op: "add", part: base.id, pos: [0, -base.heightLdu, 0], rot: 0, colour: catalog.palette.defaultBase });
      await applyState(next);
      // The baseplate is part of the start, not an edit to undo: the core takes the table as it is, with no history.
      await applyState(await asm.assemblerLoadRows(table.toRows()));
    } else await applyState(first);
    info.ready = true;
    info.parts = table.size;
  }

  // ----------------------------------------------------------- the display: pixel ratio, overlay

  /**
   * Adaptive resolution and the performance overlay (docs/plan/07 section 9): the pixel ratio starts at the
   * device's, capped at 1.5 on a phone, and drops while frames of a moving camera are over budget, with a dead
   * band so it does not hunt; a still picture is drawn at the full ratio. The overlay is off until asked for.
   */
  function wireDisplay() {
    const phone = isPhone({ coarse: matchMedia?.("(pointer: coarse)").matches ?? false, width: window.innerWidth });
    const base = basePixelRatio({ devicePixelRatio: window.devicePixelRatio, phone });
    const params = new URLSearchParams(location.search);
    const adaptive = createAdaptiveResolution({ max: base, targetMs: phone ? 1000 / 30 : 1000 / 60 });
    view.setPolicy({ trianglesBudget: triangleBudget({ phone }) });
    let settle = null;
    viewer.internals.onFrameTime((ms) => {
      if (!active || params.get("adaptive") === "0") return;
      viewer.internals.setPixelRatio(adaptive.frame(ms));
      clearTimeout(settle);
      settle = setTimeout(() => viewer.internals.setPixelRatio(adaptive.settle()), 400);
    });
    info.pixelRatioBase = base;
    displayBase = base;
    const toggle = $("build-hud-toggle");
    const show = (on) => {
      hud.on = on;
      info.hud = on;
      toggle.checked = on;
      $("build-hud").hidden = !on;
      clearInterval(hud.timer);
      if (on) {
        const draw = () => {
          const c = view.counts();
          $("build-hud").textContent = hudLines({
            drawCalls: c.drawCalls,
            triangles: c.triangles,
            frameMs: viewer.internals.frameMs,
            parts: table.size,
            chunks: c.chunks,
            visibleChunks: c.visibleChunks,
            lod: c.lod,
            pixelRatio: viewer.internals.pixelRatio,
            bias: c.bias,
          }).join("\n");
          info.pixelRatio = viewer.internals.pixelRatio;
        };
        draw();
        hud.timer = setInterval(draw, 250);
      }
    };
    toggle.addEventListener("change", () => show(toggle.checked));
    if (params.get("hud") === "1") show(true);
  }
  let displayBase = 1;

  // ----------------------------------------------------------- the scale benchmark (?bench=1, src/bench.js)

  /** The GPU's name as the browser reports it, or "" when it hides it. */
  function gpuName() {
    try {
      const gl = document.createElement("canvas").getContext("webgl2");
      const ext = gl?.getExtension("WEBGL_debug_renderer_info");
      return ext ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)) : "";
    } catch {
      return "";
    }
  }

  /**
   * Open generated models of increasing size and measure each on this device: how long the core and the page took
   * to take it in, what a frame draws, frame times while the camera orbits (real animation frames: only a number
   * of a device when this is a device), and the core round trips of a pointer. One row per size, in the columns of
   * src/bench.js CSV_COLUMNS.
   */
  async function runScale({ sizes, layout, seconds, device }, onRow, onStatus) {
    await ready; // Build mode's first start (the catalog, the palettes)
    const { layoutText } = await import("../../../bench/scale/layouts.mjs");
    const gpu = gpuName();
    const rows = [];
    for (const bricks of sizes) {
      onStatus(`${layout} ${bricks.toLocaleString("en-US")}: opening…`);
      const text = layoutText(layout, bricks);
      const ok = await enqueue(() => openModel(`${layout} ${bricks}`, () => asm.assemblerLoad(text, (p) => onStatus(`${layout} ${bricks.toLocaleString("en-US")}: ${p.done.toLocaleString("en-US")} of ${p.total.toLocaleString("en-US")} parts checked`))));
      const row = {
        date: new Date().toISOString(),
        device,
        user_agent: navigator.userAgent,
        gpu,
        backend: viewer.backend,
        device_pixel_ratio: window.devicePixelRatio,
        viewport: `${window.innerWidth}x${window.innerHeight}`,
        layout,
        bricks,
      };
      let error = null;
      let stats = { frames: 0, p50: 0, p95: 0, fps: 0 };
      if (!ok) error = info.error ?? "the model did not open";
      else {
        row.parts = table.size;
        row.open_core_ms = info.openTiming?.coreMs;
        row.open_page_ms = info.openTiming?.pageMs;
        onStatus(`${layout} ${bricks.toLocaleString("en-US")}: orbiting for ${seconds} s…`);
        // Two frames to settle the first draw, then the orbit.
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        const hiddenBefore = document.hidden;
        stats = frameStats(await viewer.measureFor(seconds * 1000));
        row.hiddenDuring = hiddenBefore || document.hidden;
        const c = view.counts();
        Object.assign(row, {
          pixel_ratio_end: viewer.internals.pixelRatio,
          chunks: c.chunks,
          draw_calls: c.drawCalls,
          triangles: c.triangles,
          chunks_full: c.lod[0],
          chunks_cheap: c.lod[1],
          chunks_solid: c.lod[2],
          chunks_hull: c.lod[3],
          frames: stats.frames,
          frame_ms_p50: +stats.p50.toFixed(1),
          frame_ms_p95: +stats.p95.toFixed(1),
          fps_mean: +stats.fps.toFixed(1),
        });
        try {
          const r = await info.bench(20);
          Object.assign(row, { snap_ms: +r.snapMs.toFixed(2), pick_ms: +r.pickMs.toFixed(2), place_undo_ms: r.placeUndoMs === null ? "" : +r.placeUndoMs.toFixed(2) });
        } catch (err) {
          error = err.message;
        }
        row.heap_mib = performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : "";
      }
      row.warnings = benchWarnings({ hidden: row.hiddenDuring, gpu, backend: viewer.backend, fpsMean: stats.fps, frames: stats.frames, error }).join("; ");
      rows.push(row);
      onRow(row);
    }
    return rows;
  }

  return {
    async enter() {
      active = true;
      ready ??= start();
      await ready.catch((err) => {
        info.error = err.message;
        throw err;
      });
      // The worker may have been replaced while Part or the coupon was showing: this goes
      // through the keeper, which puts the model back before answering.
      await applyState(await asm.assemblerState());
      view.show(true);
      view.frameLdu(...modelFrame());
      $("build-hint").textContent = "Click or tap to place · drag to orbit · R rotates · Delete removes · Ctrl+Z undoes · Tab to this view for the keyboard";
      const stage = $("stage");
      stage.tabIndex = 0;
      stage.setAttribute("aria-label", "3D view of the model. Arrow keys move the aim by one stud, Enter places a part at the aim, Shift and arrow keys orbit, plus and minus zoom, Home frames the model.");
      setTool(choice.tool);
      status([sentence(state)]);
      viewer.internals.setPixelRatio(displayBase);
    },
    leave() {
      active = false;
      lastPointer = null;
      setAim(null);
      setHover("");
      view?.setGhost(null);
      view?.setOutline(null);
      view?.show(false);
      viewer.internals.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      const stage = $("stage");
      stage.removeAttribute("tabindex");
      stage.setAttribute("aria-label", stageLabel);
    },
    summary: frameSummary,
    /** The scale benchmark of `?bench=1` (src/bench.js): resolves with one row per size. */
    runScale,
  };
}
