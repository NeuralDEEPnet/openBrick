// The calibration wizard's record step (docs/plan/05 section 4, docs/plan/06
// section 11): a map of the printed coupon to tap the best cells on, the
// choices, and the record the page sends to the core's record_fit, which
// writes the fit profile exactly as `openbrick calibrate record` does.
//
// Pure functions over the core's coupon layout (coupon_layout_json():
// { cells, summary }, crates/openbrick-part/src/coupon.rs), so node --test
// checks them. Every cell, value, label and position comes from the core. The
// map only places the core's cells for display, as the viewer places meshes:
// it keeps the coupon's millimetres and turns the y axis down, so the back of
// the coupon (line 1, largest y) is at the top and the labels read upright, as
// printed. Copies printed side by side are shown one under the other, each
// copy as printed (see cellMap). JavaScript computes no geometry and no
// offsets (CLAUDE.md): the offsets shown are the core's.

/** Printer suggestions in the record step; "Other" lets the person type one. */
export const PRINTERS = Object.freeze(["Prusa MINI+", "Prusa XL", "Bambu Lab A1"]);
/** The printer select's value for a printer typed by the person. */
export const OTHER_PRINTER = "other";
/** Material suggestions (a datalist: any text is accepted). */
export const MATERIALS = Object.freeze(["PLA", "PETG", "ABS", "ASA"]);

/** How much of the room between neighbouring cells a cell's tap target takes. */
const FILL = 0.9;

/**
 * The band above each copy that names it, when copies printed side by side
 * are shown one under the other: a share of the map's width.
 */
const BAND = 0.08;

/**
 * A cell's label in ems, as index.html draws it (keep the two in step): the
 * symbol above the label (a dot, a ring, or a larger square for a pocket),
 * the gap under it, each label line, the room a tabular digit or point takes
 * across, and the slack kept free around them so nothing is clipped.
 */
export const LABEL_EM = Object.freeze({ symbol: 1, pocketSymbol: 1.2, gap: 0.15, line: 1.1, char: 0.62, slack: 0.35 });

/**
 * The group a coupon cell belongs to: `pocket` for a cell that carries a
 * crossed second value (a 2x2 pocket: tube OD and rib depth, one choice),
 * otherwise its cell id (`stud`, `tube`, ...). One choice per group.
 */
export function groupOf(cell) {
  return cell?.cross ? "pocket" : cell.cellId;
}

const GROUP_NAMES = { stud: "Stud", tube: "Tube bore", pocket: "2x2 pocket" };
const CELL_NAMES = { stud: "Stud", tube: "Tube bore", pocketTubeOD: "Pocket tube OD", pocketRib: "Pocket rib depth" };

/** A group's name for people: Stud, Tube bore, 2x2 pocket (the cell id otherwise). */
export function groupName(group) {
  return GROUP_NAMES[group] ?? group;
}

/** A cell id's name for people, for the offsets summary. */
export function cellName(cellId) {
  return CELL_NAMES[cellId] ?? cellId;
}

/**
 * What choosing `cell` records: its value by cell id, and for a pocket both
 * values together ({ pocketTubeOD, pocketRib }), never one without the other.
 */
export function cellValues(cell) {
  const values = { [cell.cellId]: cell.valueMm };
  if (cell.cross) values[cell.cross.cellId] = cell.cross.valueMm;
  return values;
}

/** The label lines as embossed on the cell (the core's label, split at line breaks). */
export function labelLines(cell) {
  return String(cell.label).split("\n");
}

/** A cell's label on one line, as the cell table and the CLI show it: "tube OD / rib depth" for a pocket. */
export function labelText(cell) {
  return labelLines(cell).join(" / ");
}

/**
 * Where copy `copy` (0-based) lies with two copies: back or front (stacked),
 * left or right (side by side); null otherwise. As `openbrick calibrate
 * coupon` prints it.
 */
export function copyPlace(arrangement, copy, copies) {
  if (copies !== 2) return null;
  if (arrangement === "sideBySide") return copy === 0 ? "left" : "right";
  return copy === 0 ? "back" : "front";
}

/** "Copy 1 (back)", "Copy 2 (right)" or "Copy 3". */
export function copyName(summary, copy) {
  const place = copyPlace(summary?.arrangement, copy, summary?.copies);
  return place ? `Copy ${copy + 1} (${place})` : `Copy ${copy + 1}`;
}

/**
 * The captions above and below the map: which edge is the back and where the
 * copies are, so the map is read the way the coupon lies with its labels
 * upright. Copies side by side are shown one under the other (cellMap), and
 * the caption says so.
 */
export function mapCaptions(summary) {
  const copies = summary?.copies ?? 1;
  if (copies <= 1) return { top: "Back edge (labels read upright)", bottom: "Front edge" };
  if (summary.arrangement === "sideBySide") {
    const names = copies === 2 ? `${copyName(summary, 0)} and ${copyName(summary, 1)}` : `Copies 1 to ${copies}`;
    return {
      top: `${names} lie side by side${copies > 2 ? ", left to right" : ""}; here each is shown under the one before, back edge at the top (labels read upright).`,
      bottom: "Front edge",
    };
  }
  return { top: `Back edge: ${copyName(summary, 0)}`, bottom: `Front edge: ${copyName(summary, copies - 1)}` };
}

/**
 * The font size a cell's label fits its tap target with, in map millimetres
 * (index.html turns it into pixels with the map's width): the symbol, the
 * gap and every label line fit the box's height, the longest line its width,
 * each with LABEL_EM's slack.
 */
export function labelFontMm(cell) {
  const lines = cell.lines ?? labelLines(cell);
  const symbol = (cell.group ?? groupOf(cell)) === "pocket" ? LABEL_EM.pocketSymbol : LABEL_EM.symbol;
  const tall = symbol + LABEL_EM.gap + lines.length * LABEL_EM.line + LABEL_EM.slack;
  const wide = Math.max(...lines.map((l) => l.length)) * LABEL_EM.char + LABEL_EM.slack;
  return Math.min(cell.box.height / tall, cell.box.width / wide);
}

/**
 * Where each copy is drawn. Stacked copies are drawn as printed: one window,
 * the whole coupon. Copies side by side would make the map about as wide as
 * two copies, too small to tap or read in a side panel or on a phone, so each
 * copy's part of the coupon (cut halfway between the copies) gets a window of
 * its own, one under the other, each under a band that names the copy; inside
 * a window every cell is where it is printed.
 */
function windowsOf(summary, outlines, widthMm, heightMm) {
  if (summary.arrangement !== "sideBySide" || outlines.length < 2) {
    const whole = { copies: outlines.map((o) => o.copy), x0: 0, left: 0, top: 0, width: widthMm, height: heightMm, band: null };
    return { shown: "asPrinted", windows: [whole], width: widthMm, height: heightMm };
  }
  const sorted = [...outlines].sort((a, b) => a.left - b.left);
  const cuts = sorted.slice(1).map((o, i) => (sorted[i].right + o.left) / 2);
  const spans = sorted.map((o, i) => ({ copy: o.copy, x0: i > 0 ? cuts[i - 1] : 0, x1: i < cuts.length ? cuts[i] : widthMm }));
  const width = Math.max(...spans.map((s) => s.x1 - s.x0));
  const band = BAND * width;
  let top = 0;
  const windows = spans.map((s) => {
    const w = s.x1 - s.x0;
    const left = (width - w) / 2;
    const win = { copies: [s.copy], x0: s.x0, left, top: top + band, width: w, height: heightMm, band: { left, top, width: w, height: band, text: copyName(summary, s.copy) } };
    top += band + heightMm;
    return win;
  });
  return { shown: "oneUnderTheOther", windows, width, height: top };
}

/** The smallest gap between neighbouring values, or null for fewer than two. */
function smallestGap(values) {
  const sorted = [...values].sort((a, b) => a - b);
  let gap = null;
  for (let i = 1; i < sorted.length; i++) {
    const d = sorted[i] - sorted[i - 1];
    if (d > 1e-9 && (gap === null || d < gap)) gap = d;
  }
  return gap;
}

const EPS = 1e-9;

/**
 * The map of the printed coupon from the core's layout: { widthMm, heightMm,
 * shown, windows, cells, copies, captions, groups }. Each cell keeps the
 * core's data (index in the layout, row, copy, cellId, valueMm, cross, label)
 * and adds its group, its label lines, the values choosing it records, where
 * it is printed (`printed`: x from the left edge, y down from the back edge,
 * mm), and its place on the map in map millimetres: centre (x, y), a tap
 * target `box` (left, top, width, height), and `fontMm`, the size its label
 * fits the box with (the same for every cell of a group).
 *
 * The boxes split the coupon between the cells, so no two overlap: across, a
 * cell reaches halfway to its neighbours on the same line (and to a twin line
 * of another copy beside it), the ends of a line half its pitch further;
 * down, a line reaches halfway to the nearest line above and below that
 * shares some of its width (the coupon's edge for the first and last). Each
 * box then takes FILL of its share, so neighbours keep a visible gap.
 * `windows` say where each copy is drawn (windowsOf: the whole coupon as
 * printed, or copies side by side one under the other, `shown` says which);
 * `copies` are the outlines of the copies; `groups` the cell indices of each
 * group in reading order (copy, line, left to right), the order the arrow keys
 * follow.
 */
export function cellMap(layout) {
  const cells = layout?.cells;
  const summary = layout?.summary;
  if (!Array.isArray(cells) || !Array.isArray(summary?.footprintMm)) throw new TypeError("cellMap needs the core's coupon layout: { cells, summary }");
  const [widthMm, heightMm] = summary.footprintMm;
  const yOf = (c) => heightMm - c.centerMm[1];
  const byRow = new Map();
  for (const c of cells) {
    if (!byRow.has(c.row)) byRow.set(c.row, []);
    byRow.get(c.row).push(c.centerMm[0]);
  }
  const rows = new Map(
    [...byRow].map(([row, xs]) => {
      const sorted = [...xs].sort((a, b) => a - b);
      const y = yOf(cells.find((c) => c.row === row));
      return [row, { row, y, xs: sorted, pitch: smallestGap(sorted) }];
    }),
  );
  const pitches = [...rows.values()].map((r) => r.pitch).filter((p) => p !== null);
  const fallbackPitch = pitches.length ? Math.min(...pitches) : Math.min(widthMm, heightMm);
  // Down: halfway to the nearest line above and below that shares some width.
  for (const r of rows.values()) {
    const shares = [...rows.values()].filter((o) => o !== r && o.xs[0] <= r.xs.at(-1) + EPS && r.xs[0] <= o.xs.at(-1) + EPS);
    const above = shares.filter((o) => o.y < r.y - EPS).map((o) => o.y);
    const below = shares.filter((o) => o.y > r.y + EPS).map((o) => o.y);
    r.top = above.length ? (r.y + Math.max(...above)) / 2 : 0;
    r.bottom = below.length ? (r.y + Math.min(...below)) / 2 : heightMm;
    // Twin lines beside it (copies side by side) stand at the same depth.
    r.beside = [...rows.values()].filter((o) => o !== r && Math.abs(o.y - r.y) <= EPS).flatMap((o) => o.xs);
  }
  const out = cells.map((c, index) => {
    const r = rows.get(c.row);
    const x = c.centerMm[0];
    const y = yOf(c);
    const half = (r.pitch ?? fallbackPitch) / 2;
    const at = r.xs.indexOf(x);
    let left = at > 0 ? (r.xs[at - 1] + x) / 2 : x - half;
    let right = at < r.xs.length - 1 ? (x + r.xs[at + 1]) / 2 : x + half;
    for (const other of r.beside) {
      if (other < x) left = Math.max(left, (other + x) / 2);
      else if (other > x) right = Math.min(right, (x + other) / 2);
    }
    left = Math.max(0, left);
    right = Math.min(widthMm, right);
    const [w, h] = [right - left, r.bottom - r.top];
    return {
      index,
      row: c.row,
      line: summary.rowsPerCopy ? c.row - c.copy * summary.rowsPerCopy + 1 : c.row + 1,
      copy: c.copy,
      cellId: c.cellId,
      valueMm: c.valueMm,
      cross: c.cross ?? null,
      label: c.label,
      lines: labelLines(c),
      group: groupOf(c),
      values: cellValues(c),
      x,
      y,
      box: {
        left: left + (w * (1 - FILL)) / 2,
        top: r.top + (h * (1 - FILL)) / 2,
        width: w * FILL,
        height: h * FILL,
      },
    };
  });
  // Copy outlines: around the copy's tap targets, a quarter of the smallest gap
  // between two targets further out, so the outline runs in the gaps.
  const gap = Math.min(...out.map((c) => (Math.min(c.box.width, c.box.height) * (1 - FILL)) / FILL));
  const pad = gap / 4;
  const outlines = [];
  for (const cell of out) {
    const b = cell.box;
    const o = (outlines[cell.copy] ??= { copy: cell.copy, name: copyName(summary, cell.copy), left: b.left, top: b.top, right: b.left + b.width, bottom: b.top + b.height });
    o.left = Math.min(o.left, b.left);
    o.top = Math.min(o.top, b.top);
    o.right = Math.max(o.right, b.left + b.width);
    o.bottom = Math.max(o.bottom, b.top + b.height);
  }
  const copies = outlines.filter(Boolean);
  for (const o of copies) {
    o.left = Math.max(0, o.left - pad);
    o.top = Math.max(0, o.top - pad);
    o.right = Math.min(widthMm, o.right + pad);
    o.bottom = Math.min(heightMm, o.bottom + pad);
  }
  // Onto the map: each copy moves with its window (a translation only, so
  // inside a copy every cell keeps its printed place).
  const shown = windowsOf(summary, copies, widthMm, heightMm);
  const move = new Map(shown.windows.flatMap((w) => w.copies.map((copy) => [copy, { dx: w.left - w.x0, dy: w.top }])));
  for (const cell of out) {
    const { dx, dy } = move.get(cell.copy);
    cell.printed = { x: cell.x, y: cell.y };
    cell.x += dx;
    cell.y += dy;
    cell.box = { ...cell.box, left: cell.box.left + dx, top: cell.box.top + dy };
  }
  for (const o of copies) {
    const { dx, dy } = move.get(o.copy);
    Object.assign(o, { left: o.left + dx, right: o.right + dx, top: o.top + dy, bottom: o.bottom + dy });
  }
  const groups = {};
  const order = [...out].sort((a, b) => a.copy - b.copy || a.row - b.row || a.printed.x - b.printed.x);
  for (const cell of order) (groups[cell.group] ??= []).push(cell.index);
  // One label size per group, the largest every cell of the group fits.
  for (const indices of Object.values(groups)) {
    const fontMm = Math.min(...indices.map((i) => labelFontMm(out[i])));
    for (const i of indices) out[i].fontMm = fontMm;
  }
  return {
    widthMm: shown.width,
    heightMm: shown.height,
    shown: shown.shown,
    windows: shown.windows.map(({ copies: inside, left, top, width, height, band }) => ({ copies: inside, left, top, width, height, band })),
    cells: out,
    copies: copies.map(({ copy, name, left, top, right, bottom }) => ({ copy, name, left, top, width: right - left, height: bottom - top })),
    captions: mapCaptions(summary),
    groups,
  };
}

/** A cell's box as CSS percentages of the map: { left, top, width, height } with a % unit. */
export function boxPercent(map, box) {
  const pct = (v, of) => `${((100 * v) / of).toFixed(4)}%`;
  return {
    left: pct(box.left, map.widthMm),
    top: pct(box.top, map.heightMm),
    width: pct(box.width, map.widthMm),
    height: pct(box.height, map.heightMm),
  };
}

/** The name a screen reader gives a cell: "Stud <label> mm, copy 1 (back), line 1". */
export function cellAccessibleName(map, cell, summary) {
  const value = cell.cross ? `${cell.lines.join(" / ")} mm, tube OD / rib depth` : `${cell.lines.join(" ")} mm`;
  return `${groupName(cell.group)} ${value}, ${copyName(summary, cell.copy).toLowerCase()}, line ${cell.line}`;
}

/**
 * The cell the arrow keys move to in a group (radio group keys): Right and
 * Down go on, Left and Up go back, both wrapping; Home and End go to the
 * first and last. Returns the cell index, or null for another key.
 */
export function moveInGroup(order, current, key) {
  if (!Array.isArray(order) || order.length === 0) return null;
  const at = Math.max(0, order.indexOf(current));
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return order[(at + 1) % order.length];
    case "ArrowLeft":
    case "ArrowUp":
      return order[(at + order.length - 1) % order.length];
    case "Home":
      return order[0];
    case "End":
      return order.at(-1);
    default:
      return null;
  }
}

const same = (a, b) => typeof a === "number" && typeof b === "number" && Math.abs(a - b) <= 1e-9;

/** Whether two value objects ({ cellId: mm }) are the same choice. */
function sameValues(a, b) {
  const ka = Object.keys(a ?? {});
  return ka.length === Object.keys(b ?? {}).length && ka.every((k) => same(a[k], b[k]));
}

/**
 * Choose `cell`: its group's choice becomes the cell's values (both pocket
 * values together). Returns new choices; the old ones are not changed.
 */
export function choose(choices, cell) {
  return { ...choices, [cell.group ?? groupOf(cell)]: { ...(cell.values ?? cellValues(cell)) } };
}

/** Whether `cell` carries its group's choice (every cell with the same values does: both copies of a cell). */
export function isChosen(choices, cell) {
  const chosen = choices?.[cell.group ?? groupOf(cell)];
  return Boolean(chosen) && sameValues(chosen, cell.values ?? cellValues(cell));
}

/**
 * What the map marks, by group: `checked`, the one cell shown as the group's
 * choice (aria-checked; a radio group has one), which is the cell picked last
 * (`picked`: group -> cell index) when it carries the choice, else the first
 * that does in reading order, or null without a choice; `twins`, the other
 * cells with the same values (the cell in the other copy, the nominal pocket
 * in both pocket lines), marked as the same value but not as checked; and
 * `stop`, the group's one Tab stop (the checked cell, else the first).
 */
export function mapSelection(map, choices, picked = {}) {
  const out = {};
  for (const [group, order] of Object.entries(map?.groups ?? {})) {
    const same = order.filter((i) => isChosen(choices, map.cells[i]));
    const checked = same.includes(picked?.[group]) ? picked[group] : (same[0] ?? null);
    out[group] = { checked, twins: same.filter((i) => i !== checked), stop: checked ?? order[0] };
  }
  return out;
}

/** The chosen cells as record_fit takes them: { stud, tube, pocketTubeOD, pocketRib, ... }. */
export function chosenCells(choices) {
  return Object.assign({}, ...Object.values(choices ?? {}));
}

/**
 * Keep only the choices some cell of `map` carries: after the coupon's
 * process or pocket set changes, a pocket the new coupon does not print is no
 * longer offered, so it is dropped (record_fit would refuse it). Returns {
 * choices, dropped: [group] }.
 */
export function keepPrinted(choices, map) {
  const kept = {};
  const dropped = [];
  for (const [group, values] of Object.entries(choices ?? {})) {
    if (map.cells.some((c) => c.group === group && sameValues(c.values, values))) kept[group] = values;
    else dropped.push(group);
  }
  return { choices: kept, dropped };
}

/** A choice as its cell's label shows it (labelText); null when the group has none. */
export function choiceText(map, choices, group) {
  const cell = map?.cells.find((c) => c.group === group && isChosen(choices, c));
  return cell ? labelText(cell) : null;
}

/**
 * The nozzle field as a number of millimetres: { value } for a finite number
 * (zero or less included: the core refuses those with its own message), or {
 * error } for text that is not a number.
 */
export function parseNozzle(text) {
  const t = String(text ?? "").trim();
  const n = t === "" ? NaN : Number(t);
  if (Number.isFinite(n)) return { value: n };
  return { error: "Nozzle diameter must be a number of millimetres, such as 0.4." };
}

/** Today's date in the browser's time zone, YYYY-MM-DD (the date field's default). */
export function todayLocal(now = new Date()) {
  const pad = (n, w = 2) => String(n).padStart(w, "0");
  return `${pad(now.getFullYear(), 4)}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/**
 * The record_fit input for the wizard's fields (crates/openbrick-wasm/src/
 * record.rs, RecordFitInput): process and pocket set of the coupon as printed,
 * printer, material, nozzle, date and the chosen cells; slicer and notes only
 * when they hold text (the CLI's defaults otherwise: no slicer, the note
 * naming the coupon version). Text is sent as typed; the core trims what the
 * CLI trims.
 */
export function recordInput({ process, pocketSet, printer, material, nozzleMm, slicer = "", notes = "", date = "", choices }) {
  const input = { process, pocketSet, printer: printer ?? "", material: material ?? "", cells: chosenCells(choices) };
  if (typeof nozzleMm === "number" && Number.isFinite(nozzleMm)) input.nozzleMm = nozzleMm;
  if (String(slicer).trim() !== "") input.slicer = slicer;
  if (String(notes).trim() !== "") input.notes = notes;
  if (String(date).trim() !== "") input.date = String(date).trim();
  return input;
}

/** The record step's form fields, by the record_fit field each one sets. */
export const FORM_FIELDS = Object.freeze(["printer", "material", "nozzleMm", "slicer", "date", "notes"]);

/**
 * Where a refusal of record_fit is shown: next to its form field, or with the
 * map's choices for everything about the cells (`cells`, `stud`, `tube`,
 * `pocket`) and the coupon as printed (`process`, `pocketSet`).
 */
export function errorPlace(field) {
  return FORM_FIELDS.includes(field) ? field : "map";
}

const fixed = (v, digits) => (typeof v === "number" ? v.toFixed(digits) : "–");

/** An offset with its sign, three decimals, as the CLI prints it: +0.150, -0.014, +0.000. */
export function signedMm(v) {
  if (typeof v !== "number" || !Number.isFinite(v)) return "–";
  return `${v < 0 ? "-" : "+"}${Math.abs(v).toFixed(3)}`;
}

/**
 * The offsets summary rows from the core's offsets (record_fit's `offsets`:
 * cell, chosenMm, nominalMm, offsetField, offsetMm): the cell's name, the
 * chosen cell and the nominal in mm, the offset with its sign, and the offset
 * field it sets.
 */
export function offsetRows(offsets) {
  return (offsets ?? []).map((o) => ({
    cell: o.cell,
    name: cellName(o.cell),
    chosen: fixed(o.chosenMm, 2),
    nominal: fixed(o.nominalMm, 3),
    offset: signedMm(o.offsetMm),
    field: o.offsetField,
  }));
}
