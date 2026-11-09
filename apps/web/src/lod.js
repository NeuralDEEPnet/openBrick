// Level of detail, chunk keys, adaptive resolution and the performance overlay's text for Build
// mode (docs/plan/07 section 9). Pure functions and small state machines, no DOM and no three.js, so
// node --test checks them. What is drawn at which level never feeds back to the core: collision,
// snapping and connectivity come from the core's boxes and connectors, never from a render level
// ("never derive collision from the render LOD").

/** The edge of a chunk of the model in LDU: 32 studs of 20 LDU (docs/plan/07 section 9). */
export const CHUNK_LDU = 640;

/** Render levels, nearest first: full studs, cheap studs and tubes, cheap studs on a solid body, the hull. */
export const LEVELS = 4;

/** Models of at most this many parts are one chunk per part type (one draw call each). */
export const GRID_ABOVE = 3000;
export const SINGLE_BELOW = 1500;

/** Whether to chunk the model over the lattice: above [`GRID_ABOVE`] parts, back below [`SINGLE_BELOW`]. */
export function wantGrid(count, current = false) {
  return current ? count >= SINGLE_BELOW : count > GRID_ABOVE;
}

/** The chunk a part at `pos` (LDU) belongs to: "x,y,z" of its cell, or "all" when the model is not chunked. */
export function chunkCell(pos, grid) {
  if (!grid) return "all";
  return `${Math.floor(pos[0] / CHUNK_LDU)},${Math.floor(pos[1] / CHUNK_LDU)},${Math.floor(pos[2] / CHUNK_LDU)}`;
}

/** The cell's name for a chunk key "part|cell". */
export const chunkKey = (part, cell) => `${part}|${cell}`;

/**
 * How many pixels a stud is wide on screen at `distance` (same unit as `studSize`), for a camera with
 * vertical field of view `fovDeg` and a viewport `heightPx` tall.
 */
export function studPixels(studSize, distance, fovDeg, heightPx) {
  const d = Math.max(distance, 1e-6);
  return (studSize / (2 * d * Math.tan((fovDeg * Math.PI) / 360))) * heightPx;
}

/**
 * Stud widths, in pixels, at which a chunk drops a level: below the first it is drawn at level 1, below
 * the second at level 2, below the third as the hull. `bias` (at least 1) raises all three when the
 * frame is over its triangle budget. A model of few parts (`small`) keeps full detail until a stud is
 * a speck.
 */
export function thresholds({ small, bias = 1 }) {
  return small ? [0, 0, 0, 1.2 * bias] : [28 * bias, 8 * bias, 3 * bias];
}

/**
 * The render level for a chunk whose studs are `px` wide, given the level it had (`current`, or -1) so it
 * does not flicker at a border: a chunk keeps its level until the stud size passes the border by a margin
 * (15 percent), in either direction. `limits` is the output of [`thresholds`] (3 or 4 numbers; a fourth
 * is the hull's border for small models, which is applied to level 0 only).
 */
export function pickLevel(px, limits, current = -1, margin = 0.15) {
  const level = (scale) => {
    // The border between level k and k + 1, scaled.
    const fine = limits.length === 4 ? limits.slice(0, 3) : limits;
    let k = 0;
    while (k < fine.length && px < fine[k] * scale) k++;
    // fine has 3 borders: 0 | 1 | 2 | hull; small models have [0, 0, 0] and a hull border apart.
    if (limits.length === 4 && px < limits[3] * scale) return 3;
    return k;
  };
  const fresh = level(1);
  if (current < 0 || fresh === current) return fresh;
  // Moving to a nearer level needs the stud to be a margin wider than the border, to a farther one a margin narrower.
  return fresh < current ? level(1 + margin) : level(1 - margin);
}

/** Triangles the frame would draw: `chunks` is [{ level, count }] and `tris[level]` the triangles of one part at that level. */
export function frameTriangles(chunks, trisOf) {
  let n = 0;
  for (const c of chunks) n += c.count * trisOf(c, c.level);
  return n;
}

/**
 * The factor that raises the level borders while the frame is over its triangle `budget`, with a dead band:
 * it grows by `step` above the budget and shrinks back only when the frame is under 60 percent of it. Returns
 * the new bias (at least 1, at most `max`).
 */
export function nextBias(bias, triangles, budget, { step = 1.2, max = 16 } = {}) {
  if (triangles > budget) return Math.min(max, bias * step);
  if (triangles < budget * 0.6 && bias > 1) return Math.max(1, bias / step);
  return bias;
}

/** The default triangle budget of a frame: docs/plan/07 section 9 gives 10 to 20 M on a laptop, 3 to 6 M on a phone; this takes the low half. */
export function triangleBudget({ phone }) {
  return phone ? 2_500_000 : 8_000_000;
}

/**
 * Adaptive resolution: the device pixel ratio to render at, lowered while frames are over budget and raised
 * again when they are comfortably under, with a dead band between so it does not oscillate. Feed it the time
 * of each frame drawn while the camera moves (`frame(ms)`); it answers the ratio to use. `settle()` gives the
 * full ratio, for the still picture after the camera stops.
 */
export function createAdaptiveResolution({ max, min = 0.5, targetMs = 1000 / 60, window = 8, down = 0.85, up = 1.1, high = 1.15, low = 0.7 }) {
  let ratio = max;
  const samples = [];
  return {
    get ratio() {
      return ratio;
    },
    /** Record one frame's time; returns the ratio to use for the next. */
    frame(ms) {
      samples.push(ms);
      if (samples.length < window) return ratio;
      const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
      samples.length = 0;
      if (mean > targetMs * high) ratio = Math.max(min, ratio * down);
      else if (mean < targetMs * low && ratio < max) ratio = Math.min(max, ratio * up);
      return ratio;
    },
    /** The camera stopped: the full ratio. */
    settle() {
      samples.length = 0;
      ratio = max;
      return ratio;
    },
    /** Another cap (the window moved to a screen with another density). */
    setMax(next) {
      max = next;
      ratio = Math.min(ratio, max);
    },
  };
}

/** The pixel ratio the page starts at: the device's, capped (2 on a laptop, 1.5 on a phone). */
export function basePixelRatio({ devicePixelRatio = 1, phone }) {
  return Math.max(0.5, Math.min(devicePixelRatio, phone ? 1.5 : 2));
}

/** True for a phone or tablet: a coarse primary pointer or a narrow screen. */
export function isPhone({ coarse, width }) {
  return coarse === true || width < 600;
}

/** The lines of the performance overlay for a snapshot { drawCalls, triangles, frameMs, parts, chunks, visibleChunks, lod: [n0, n1, n2, n3], pixelRatio, bias }. */
export function hudLines(s) {
  const fps = s.frameMs > 0 ? 1000 / s.frameMs : 0;
  return [
    `${s.drawCalls} draw calls, ${s.triangles.toLocaleString("en-US")} triangles`,
    `${s.frameMs > 0 ? s.frameMs.toFixed(1) : "-"} ms a frame (${fps > 0 ? fps.toFixed(0) : "-"} fps while moving)`,
    `${s.parts.toLocaleString("en-US")} parts in ${s.chunks} chunks (${s.visibleChunks} in view)`,
    `levels in view: full ${s.lod[0]}, cheap ${s.lod[1]}, solid ${s.lod[2]}, hull ${s.lod[3]}`,
    `pixel ratio ${s.pixelRatio.toFixed(2)}, level bias ${s.bias.toFixed(2)}`,
  ];
}
