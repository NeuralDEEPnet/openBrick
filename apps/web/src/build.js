// Build mode's pure logic (docs/plan/07 section 10): reading what the core
// sends, turning keys into actions, the texts of the panel and the status line,
// and the thumbnails of the parts palette. No DOM and no three.js, so node --test
// checks it. Snapping, collision and connectivity are never computed here: the
// page sends pointer rays and commands to the core (CLAUDE.md), and everything
// below only reads the answers.

/** Numbers per placed part in the core's instance array. */
export const INSTANCE_STRIDE = 7;

/**
 * Above this many parts the page switches to the cheap mesh level (8 segments per
 * stud and tube instead of 24: about 45 percent of the triangles), and back below
 * [`DETAIL_BELOW`] (a gap, so a model near the limit does not flicker between the
 * two). A 2x4 brick is 2,320 triangles detailed and 976 cheap, so 250 detailed
 * bricks are about 580,000 triangles per frame, which an integrated GPU draws at
 * 60 fps (docs/plan/07 section 9 budgets 10 to 20 million). The real LOD pipeline
 * (LOD2 hulls, chunked culling) is the rest of M3a.
 */
export const LOW_DETAIL_ABOVE = 250;
export const DETAIL_BELOW = 150;

/** The mesh level for a model of `count` parts, given the level shown now: true for the detailed one. */
export function wantDetail(count, current = true) {
  if (current) return count <= LOW_DETAIL_ABOVE;
  return count < DETAIL_BELOW;
}

/** The placed parts of the core's instance array: [{ id, part, pos: [x, y, z], rot, colour }]. */
export function decodeInstances(flat) {
  if (!flat || flat.length % INSTANCE_STRIDE !== 0) throw new RangeError(`the instance array has ${flat?.length} numbers, not a multiple of ${INSTANCE_STRIDE}`);
  const out = [];
  for (let i = 0; i < flat.length; i += INSTANCE_STRIDE) {
    out.push({ id: flat[i], part: flat[i + 1], pos: [flat[i + 2], flat[i + 3], flat[i + 4]], rot: flat[i + 5], colour: flat[i + 6] });
  }
  return out;
}

/**
 * Placed parts by catalog part number, in placement order: Map part -> [instance].
 * One InstancedMesh per part, with the colour per instance.
 */
export function groupByPart(instances) {
  const groups = new Map();
  for (const inst of instances) {
    let g = groups.get(inst.part);
    if (!g) groups.set(inst.part, (g = []));
    g.push(inst);
  }
  return groups;
}

/**
 * A column-major 4x4 for an instance, from the core's rotation matrix (row
 * major 3x3 of integers, `catalog.rotations[rot]`) and the position: what
 * three.js `Matrix4.fromArray` takes. It only arranges numbers the core gave.
 */
export function instanceMatrix(rotations, rot, pos) {
  const m = rotations[rot];
  if (!m) throw new RangeError(`rotation ${rot} is not one of the ${rotations.length} lattice rotations`);
  return [m[0][0], m[1][0], m[2][0], 0, m[0][1], m[1][1], m[2][1], 0, m[0][2], m[1][2], m[2][2], 0, pos[0], pos[1], pos[2], 1];
}

/** The colour of a palette code as "#rrggbb"; unknown codes give grey. */
export function colourHex(palette, code) {
  return palette?.colours?.find((c) => c.code === code)?.rgb ?? "#808080";
}

/** The name of a palette code, for labels. */
export function colourName(palette, code) {
  return palette?.colours?.find((c) => c.code === code)?.name ?? `colour ${code}`;
}

/** What a key press does in Build mode, or null. `event` is { key, ctrlKey, metaKey, shiftKey }. */
export function keyAction(event) {
  const key = String(event.key ?? "");
  const mod = event.ctrlKey || event.metaKey;
  const lower = key.toLowerCase();
  if (mod && lower === "z") return event.shiftKey ? "redo" : "undo";
  if (mod && lower === "y") return "redo";
  if (mod) return null;
  if (lower === "r") return event.shiftKey ? "rotateBack" : "rotate";
  if (key === "Delete" || key === "Backspace") return "delete";
  if (key === "Escape") return "deselect";
  return null;
}

/**
 * What a key does while the 3D view itself has the focus (Build mode's keyboard path,
 * for people who cannot aim a pointer): arrow keys move the aim by one stud, Enter or
 * Space places at the aim, Shift+arrows orbit the camera, plus and minus zoom, Home frames
 * the model. Returns { type: "aim", key } | { type: "orbit", azimuth, polar } |
 * { type: "zoom", factor } | { type: "place" } | { type: "reframe" } | null, with angles
 * in degrees. Ctrl and Cmd combinations are never taken (undo and redo are keyAction's).
 */
export function stageKeyAction(event) {
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  const key = String(event.key ?? "");
  const arrow = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[key];
  if (arrow) {
    if (event.shiftKey) return { type: "orbit", azimuth: -arrow[0] * ORBIT_STEP_DEG, polar: -arrow[1] * ORBIT_STEP_DEG };
    return { type: "aim", key };
  }
  if (key === "Enter" || key === " ") return { type: "place" };
  if (key === "+" || key === "=") return { type: "zoom", factor: 1 / ZOOM_STEP };
  if (key === "-" || key === "_") return { type: "zoom", factor: ZOOM_STEP };
  if (key === "Home") return { type: "reframe" };
  return null;
}

/** Degrees one Shift+arrow press turns the camera, and the factor one zoom key press scales the distance by. */
export const ORBIT_STEP_DEG = 15;
export const ZOOM_STEP = 1.25;

/**
 * The aim's step for an arrow key, in studs [dx, dz] of the model frame, relative to how
 * the camera looks: Up goes away from you, Right to your right, along the nearer of the
 * two ground axes. `forward` is the camera's horizontal view direction in the scene's
 * frame ([x, z], Y up; the model frame has the same X and the opposite Z). The page only
 * moves a cursor here: where the part then goes is the core's answer to a ray at the aim.
 */
export function aimStep(key, forward) {
  const [fx, fz] = forward;
  // Nearest axis of the view direction, in the scene frame.
  const f = Math.abs(fx) >= Math.abs(fz) ? [Math.sign(fx) || 1, 0] : [0, Math.sign(fz) || -1];
  const right = [-f[1], f[0]];
  const sceneStep = { ArrowUp: f, ArrowDown: [-f[0], -f[1]], ArrowRight: right, ArrowLeft: [-right[0], -right[1]] }[key];
  if (!sceneStep) return null;
  // Scene to model frame: X stays, Z flips. (Avoid -0.)
  return [sceneStep[0] || 0, -sceneStep[1] || 0];
}

/** A ray straight down onto the ground point (x, z), from high above, in LDU of the model frame. */
export function aimRay(x, z, height = 1_000_000) {
  return { origin: [x, -height, z], dir: [0, 1, 0] };
}

/**
 * Whether replacing the model (Open, Samples) needs the person's say first: it does when
 * edits since the last save or open would be lost.
 */
export function replaceNeedsConfirm({ dirty }) {
  return dirty === true;
}

/**
 * Whether Clear has anything to remove: any part but a baseplate standing on the ground
 * (the engine's `ClearBuilt` keeps exactly those). `parts` is the catalog's part list
 * (`base`, `heightLdu`); a baseplate is on the ground when its bottom (position plus height)
 * is at Y = 0, whichever way it is turned about the vertical axis.
 */
export function clearable(instances, parts) {
  return instances.some((i) => !(parts[i.part].base && i.pos[1] + parts[i.part].heightLdu === 0));
}

/** True where typing must not trigger a shortcut: text fields, selects and the like. */
export function typingTarget(el) {
  const tag = String(el?.tagName ?? "").toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || el?.isContentEditable === true;
}

/**
 * A pointer gesture as a click: it moved under `slopPx` and lasted under
 * `maxMs`. Longer or wider is an orbit or pan, which the camera controls own.
 */
export function isClick(down, up, { slopPx = 6, maxMs = 450 } = {}) {
  return Math.hypot(up.x - down.x, up.y - down.y) <= slopPx && up.t - down.t <= maxMs;
}

/** The ghost's state from the engine's snap: "valid", "invalid" or "none". */
export function ghostState(snap) {
  if (!snap) return "none";
  return snap.valid ? "valid" : "invalid";
}

/**
 * The status line for a snap: where the part would go, or why it cannot. The
 * reasons are the engine's findings in words (the snap's `collides`, `mates`
 * and `grounded`), each with what to do.
 */
export function snapText(snap, partName, names = {}) {
  if (!snap) return [`Point at the baseplate or a part to place ${partName}`];
  if (snap.valid) {
    const how = snap.grounded ? "on the ground" : `${snap.mates} stud${snap.mates === 1 ? "" : "s"} connected`;
    return [`${partName}: click or tap to place (${how})`];
  }
  if (snap.collides != null) {
    const other = names[snap.collides];
    return [`${partName} would overlap ${other ? `the ${other}` : `part ${snap.collides}`}`, "Move so the two only touch"];
  }
  if (snap.mates === 0 && !snap.grounded) return [`${partName} would not connect to anything`, "Press it onto studs, or place it on the baseplate"];
  return [`${partName} cannot go here`, "Move it onto the baseplate or another part"];
}

/** The summary of the model for the stats panel, as [label, value] rows. */
export function statsRows(stats) {
  const floating = stats.floating?.length ?? 0;
  return [
    ["Parts", String(stats.parts)],
    ["Connections", String(stats.connections)],
    ["Connected groups", String(stats.components)],
    ["On the ground", String(stats.grounded)],
    ["Floating", floating === 0 ? "none" : `${floating} (no chain of connections to the ground)`],
  ];
}

/** One sentence for the live region after an edit. */
export function stateSentence(stats) {
  const floating = stats.floating?.length ?? 0;
  const parts = `${stats.parts} part${stats.parts === 1 ? "" : "s"}`;
  return floating === 0 ? `${parts}, all connected to the ground` : `${parts}, ${floating} floating`;
}

/** Rows of the bill of materials for display: [{ count, part, colour }], and the total. */
export function bomRows(bom) {
  return { rows: bom.map((l) => ({ count: l.count, part: l.name, colour: l.colourName || `colour ${l.colour}`, code: l.colour })), total: bom.reduce((n, l) => n + l.count, 0) };
}

/** A safe file name for a saved model: `<name>.obrick.json` (the alpha saves the model.json text of docs/plan/08). */
export function modelFileName(name) {
  const stem =
    String(name ?? "")
      .trim()
      .replace(/\.obrick(\.json)?$/i, "")
      .replace(/[^\p{L}\p{N}._ -]+/gu, "")
      .trim()
      .replace(/\s+/g, "-")
      .slice(0, 60) || "model";
  return `${stem}.obrick.json`;
}

/** The model's name from a file name: "my-house.obrick.json" gives "my-house". */
export function modelNameFromFile(fileName) {
  return String(fileName ?? "").replace(/\.json$/i, "").replace(/\.obrick$/i, "").replace(/-/g, " ").trim() || "Model";
}

/** Whether a file looks like a model: .obrick or .json. */
export function isModelFile(name) {
  return /\.(obrick|json)$/i.test(String(name ?? ""));
}

/** Frames-per-second summary of frame times in ms: { fps, p95Ms, frames }. */
export function frameSummary(times) {
  if (!times.length) return { fps: 0, p95Ms: 0, frames: 0 };
  const sorted = [...times].sort((a, b) => a - b);
  const mean = times.reduce((a, b) => a + b, 0) / times.length;
  return { fps: 1000 / mean, p95Ms: sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))], frames: times.length };
}

// ------------------------------------------------------------- thumbnails

/**
 * Triangles of a part mesh projected for a small isometric thumbnail:
 * [{ depth, shade, ny (the face normal's Y in the part frame: negative faces up), points: [x0, y0, x1, y1, x2, y2] }] sorted far to near (`depth` grows towards the viewer),
 * and the box they fit in. The LDraw frame has Y down, like the screen, so a
 * part stands upright; the view turns it 45 degrees and looks down at about
 * 35 degrees. This is drawing, not model geometry: nothing here feeds back to
 * the core.
 */
export function thumbnailTriangles(positions, indices) {
  const c = Math.cos(Math.PI / 4);
  const s = Math.sin(Math.PI / 4);
  const tilt = Math.sin(Math.atan(1 / Math.SQRT2)); // 0.577: isometric elevation
  const cos = Math.sqrt(1 - tilt * tilt);
  const project = (i) => {
    const x = positions[i * 3];
    const y = positions[i * 3 + 1];
    const z = positions[i * 3 + 2];
    const rx = x * c - z * s;
    const rz = x * s + z * c;
    return [rx, y * cos + rz * tilt, rz * cos - y * tilt];
  };
  const light = [-0.4, -0.8, -0.45];
  const len = Math.hypot(...light);
  const tris = [];
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (let t = 0; t < indices.length; t += 3) {
    const a = project(indices[t]);
    const b = project(indices[t + 1]);
    const d = project(indices[t + 2]);
    const area = (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]);
    // Counter-clockwise from outside, seen on a Y-down screen, has a positive area here:
    // keep the faces that look at the viewer, drop the ones that show a part's inside.
    if (area <= 0) continue;
    const u = [positions[indices[t + 1] * 3] - positions[indices[t] * 3], positions[indices[t + 1] * 3 + 1] - positions[indices[t] * 3 + 1], positions[indices[t + 1] * 3 + 2] - positions[indices[t] * 3 + 2]];
    const v = [positions[indices[t + 2] * 3] - positions[indices[t] * 3], positions[indices[t + 2] * 3 + 1] - positions[indices[t] * 3 + 1], positions[indices[t + 2] * 3 + 2] - positions[indices[t] * 3 + 2]];
    const n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    const nl = Math.hypot(...n) || 1;
    const lambert = Math.abs((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / (nl * len));
    for (const p of [a, b, d]) {
      minX = Math.min(minX, p[0]);
      maxX = Math.max(maxX, p[0]);
      minY = Math.min(minY, p[1]);
      maxY = Math.max(maxY, p[1]);
    }
    tris.push({ depth: (a[2] + b[2] + d[2]) / 3, shade: 0.45 + 0.55 * lambert, ny: n[1] / nl, points: [a[0], a[1], b[0], b[1], d[0], d[1]] });
  }
  // Larger depth is nearer the viewer (higher and towards the camera), so paint the smallest first.
  tris.sort((p, q) => p.depth - q.depth);
  return { tris, box: { minX, minY, maxX, maxY } };
}

/**
 * Fit projected triangles into a `size` x `size` square with `pad` pixels
 * around: { scale, offsetX, offsetY } so that x * scale + offsetX is the pixel.
 */
export function thumbnailFit(box, size, pad = 3) {
  const w = Math.max(1e-6, box.maxX - box.minX);
  const h = Math.max(1e-6, box.maxY - box.minY);
  const scale = Math.min((size - 2 * pad) / w, (size - 2 * pad) / h);
  return { scale, offsetX: (size - w * scale) / 2 - box.minX * scale, offsetY: (size - h * scale) / 2 - box.minY * scale };
}

/** A shaded colour: "#rrggbb" times `shade` (0 to 1). */
export function shadeHex(hex, shade) {
  const n = Number.parseInt(hex.slice(1), 16);
  const f = (v) => Math.max(0, Math.min(255, Math.round(v * shade)));
  const r = f((n >> 16) & 255);
  const g = f((n >> 8) & 255);
  const b = f(n & 255);
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}
