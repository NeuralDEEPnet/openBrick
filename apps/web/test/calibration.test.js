// The record step's model (src/calibration.js): the map of the printed coupon,
// the choices, and the record_fit input, over the core's own coupon layouts.
// The fixtures are coupon_layout_json() output, which the openbrick-wasm test
// the_web_fixtures_are_the_core_layouts keeps equal to the core: the default
// coupon (fdm-0.4, cross pockets, copies stacked) and the full pocket set on a
// 256 x 256 mm bed (copies side by side).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  boxPercent,
  cellAccessibleName,
  cellMap,
  cellValues,
  choiceText,
  choose,
  chosenCells,
  copyName,
  copyPlace,
  errorPlace,
  groupOf,
  isChosen,
  keepPrinted,
  LABEL_EM,
  labelFontMm,
  labelText,
  mapCaptions,
  mapSelection,
  moveInGroup,
  offsetRows,
  parseNozzle,
  recordInput,
  signedMm,
  todayLocal,
} from "../src/calibration.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8"));
const cross = fixture("coupon-layout-fdm-0.4-cross.json");
const full = fixture("coupon-layout-fdm-0.4-full-256.json");

/** The boxes of two cells overlap (touching edges do not count). */
const overlaps = (a, b) => a.left < b.left + b.width - 1e-9 && b.left < a.left + a.width - 1e-9 && a.top < b.top + b.height - 1e-9 && b.top < a.top + a.height - 1e-9;

test("the map has every cell of the core's layout, with its label, value and copy", () => {
  for (const layout of [cross, full]) {
    const map = cellMap(layout);
    const [width, height] = layout.summary.footprintMm;
    assert.equal(map.cells.length, layout.cells.length);
    if (map.shown === "asPrinted") assert.deepEqual([map.widthMm, map.heightMm], [width, height]);
    map.cells.forEach((c, i) => {
      const core = layout.cells[i];
      assert.equal(c.index, i);
      assert.equal(c.label, core.label);
      assert.equal(c.lines.join("\n"), core.label);
      assert.equal(c.copy, core.copy);
      assert.equal(c.valueMm, core.valueMm);
      assert.equal(c.group, core.cross ? "pocket" : core.cellId);
      // Placed, not computed: where it is printed is the core's centre, the y
      // axis turned down; on the map it moves with its copy's window only.
      assert.equal(c.printed.x, core.centerMm[0]);
      assert.equal(c.printed.y, height - core.centerMm[1]);
      const win = map.windows.find((w) => w.copies.includes(c.copy));
      assert.ok(win, `cell ${i} has no window`);
      assert.ok(c.x >= win.left && c.x <= win.left + win.width && c.y >= win.top && c.y <= win.top + win.height, `cell ${i} outside its window`);
      if (map.shown === "asPrinted") assert.deepEqual([c.x, c.y], [c.printed.x, c.printed.y]);
      // The tap target holds the centre and stays on the map.
      assert.ok(c.box.left <= c.x && c.x <= c.box.left + c.box.width, `cell ${i} x`);
      assert.ok(c.box.top <= c.y && c.y <= c.box.top + c.box.height, `cell ${i} y`);
      assert.ok(c.box.left >= 0 && c.box.top >= 0 && c.box.left + c.box.width <= map.widthMm + 1e-9 && c.box.top + c.box.height <= map.heightMm + 1e-9, `cell ${i} inside`);
      assert.ok(c.line >= 1 && c.line <= layout.summary.rowsPerCopy, `cell ${i} line ${c.line}`);
    });
    // Labels as the core embossed them, two decimals, a pocket over two lines.
    assert.ok(map.cells.every((c) => c.lines.every((l) => /^\d+\.\d\d$/.test(l))));
    assert.ok(map.cells.filter((c) => c.group === "pocket").every((c) => c.lines.length === 2));
  }
});

test("no two tap targets overlap, so every cell can be tapped", () => {
  for (const layout of [cross, full]) {
    const { cells } = cellMap(layout);
    for (let i = 0; i < cells.length; i++) {
      for (let j = i + 1; j < cells.length; j++) {
        assert.ok(!overlaps(cells[i].box, cells[j].box), `${cells[i].label} (copy ${cells[i].copy}) overlaps ${cells[j].label} (copy ${cells[j].copy})`);
      }
    }
  }
});

test("the back of the coupon is at the top: line 1 of copy 1 first, the copies where the CLI says", () => {
  const map = cellMap(cross);
  // Stacked copies are drawn as printed: one window, the whole coupon.
  assert.equal(map.shown, "asPrinted");
  assert.deepEqual(map.windows, [{ copies: [0, 1], left: 0, top: 0, width: map.widthMm, height: map.heightMm, band: null }]);
  const top = map.cells.reduce((a, b) => (b.y < a.y ? b : a));
  assert.equal(top.row, 0);
  assert.equal(top.copy, 0);
  assert.equal(top.group, "stud");
  // Stacked: copy 1 at the back (above), copy 2 at the front.
  const [one, two] = map.copies;
  assert.ok(one.top + one.height <= two.top + 1e-9, "copy 1 above copy 2");
  assert.deepEqual(map.copies.map((c) => c.name), ["Copy 1 (back)", "Copy 2 (front)"]);
  assert.deepEqual(mapCaptions(cross.summary), { top: "Back edge: Copy 1 (back)", bottom: "Front edge: Copy 2 (front)" });
  // Printed side by side (copy 1 on the left, each line with its twin at the
  // same depth), drawn one under the other, each copy in a window of its own
  // under a band naming it, every cell where it is printed in its copy.
  const wide = cellMap(full);
  assert.equal(wide.shown, "oneUnderTheOther");
  assert.deepEqual(wide.copies.map((c) => c.name), ["Copy 1 (left)", "Copy 2 (right)"]);
  assert.match(mapCaptions(full.summary).top, /Copy 1 \(left\) and Copy 2 \(right\) lie side by side; here each is shown under the one before/);
  const [w1, w2] = wide.windows;
  assert.deepEqual([w1.copies, w2.copies], [[0], [1]]);
  assert.deepEqual([w1.band.text, w2.band.text], ["Copy 1 (left)", "Copy 2 (right)"]);
  assert.ok(w1.band.top + w1.band.height <= w1.top + 1e-9 && w1.top + w1.height <= w2.band.top + 1e-9, "band, copy 1, band, copy 2 from the top");
  assert.ok(Math.abs(w2.top + w2.height - wide.heightMm) < 1e-9, "the map ends with copy 2");
  // Each window is the copy's part of the coupon: as deep as the coupon, as
  // wide as the cut halfway between the copies leaves.
  for (const w of wide.windows) assert.equal(w.height, full.summary.footprintMm[1]);
  assert.ok(Math.abs(w1.width + w2.width - full.summary.footprintMm[0]) < 1e-9);
  const firstX = Math.min(...full.cells.filter((c) => c.copy === 1).map((c) => c.centerMm[0]));
  const lastX = Math.max(...full.cells.filter((c) => c.copy === 0).map((c) => c.centerMm[0]));
  assert.ok(lastX < w1.width && w1.width < firstX, `the cut at ${w1.width} mm lies between the copies (${lastX}, ${firstX})`);
  for (const c of wide.cells.filter((c) => c.copy === 0)) {
    const twin = wide.cells.find((t) => t.copy === 1 && t.label === c.label && t.printed.y === c.printed.y && t.row - t.copy * full.summary.rowsPerCopy === c.row);
    assert.ok(twin, `${c.label} has no twin`);
    // Moved as a whole: the same offset from its window as printed.
    assert.ok(Math.abs(twin.x - w2.left - (twin.printed.x - w1.width) - (c.x - w1.left - c.printed.x)) < 1e-9, `${c.label} moved across`);
    assert.ok(Math.abs(twin.y - w2.top - (c.y - w1.top)) < 1e-9, `${c.label} moved down`);
  }
  assert.equal(copyPlace("stacked", 1, 3), null);
  assert.equal(copyName({ arrangement: "stacked", copies: 3 }, 2), "Copy 3");
  assert.equal(mapCaptions({ copies: 1 }).top, "Back edge (labels read upright)");
});

test("each copy's outline holds its cells, stays in its window and leaves the other copy alone", () => {
  for (const layout of [cross, full]) {
    const map = cellMap(layout);
    assert.equal(map.copies.length, layout.summary.copies);
    for (const o of map.copies) {
      const w = map.windows.find((w) => w.copies.includes(o.copy));
      assert.ok(o.left >= w.left - 1e-9 && o.top >= w.top - 1e-9 && o.left + o.width <= w.left + w.width + 1e-9 && o.top + o.height <= w.top + w.height + 1e-9, `${o.name} leaves its window`);
      for (const c of map.cells.filter((c) => c.copy === o.copy)) {
        const b = c.box;
        assert.ok(b.left > o.left && b.top >= o.top && b.left + b.width < o.left + o.width && b.top + b.height <= o.top + o.height, `${c.label} is not inside ${o.name}`);
      }
    }
    assert.ok(!overlaps(map.copies[0], map.copies[1]), `the copies of ${layout.summary.pockets} overlap`);
  }
});

test("boxes become CSS percentages of the map", () => {
  const map = cellMap(cross);
  const p = boxPercent(map, { left: map.widthMm / 4, top: 0, width: map.widthMm / 2, height: map.heightMm });
  assert.deepEqual(p, { left: "25.0000%", top: "0.0000%", width: "50.0000%", height: "100.0000%" });
});

test("each group's reading order: copy, line, left to right; the arrow keys follow it", () => {
  const map = cellMap(cross);
  assert.deepEqual(Object.keys(map.groups).sort(), ["pocket", "stud", "tube"]);
  for (const [group, order] of Object.entries(map.groups)) {
    const cells = order.map((i) => map.cells[i]);
    assert.ok(cells.every((c) => c.group === group));
    for (let k = 1; k < cells.length; k++) {
      const [a, b] = [cells[k - 1], cells[k]];
      assert.ok(a.copy < b.copy || (a.copy === b.copy && (a.row < b.row || (a.row === b.row && a.printed.x < b.printed.x))), `${group} order at ${k}`);
    }
  }
  const studs = map.groups.stud;
  assert.equal(studs.length, cross.cells.filter((c) => c.cellId === "stud").length);
  assert.equal(moveInGroup(studs, studs[0], "ArrowRight"), studs[1]);
  assert.equal(moveInGroup(studs, studs[0], "ArrowDown"), studs[1]);
  assert.equal(moveInGroup(studs, studs[0], "ArrowLeft"), studs.at(-1));
  assert.equal(moveInGroup(studs, studs.at(-1), "ArrowRight"), studs[0]);
  assert.equal(moveInGroup(studs, studs[3], "Home"), studs[0]);
  assert.equal(moveInGroup(studs, studs[3], "End"), studs.at(-1));
  assert.equal(moveInGroup(studs, studs[3], "Enter"), null);
  assert.equal(moveInGroup([], 0, "ArrowRight"), null);
});

test("a pocket is one choice: its tube OD and rib depth together, from the cell's label", () => {
  const map = cellMap(cross);
  const pocket = map.cells.find((c) => c.group === "pocket" && c.copy === 1);
  assert.deepEqual(Object.keys(pocket.values).sort(), ["pocketRib", "pocketTubeOD"]);
  assert.deepEqual(cellValues(cross.cells[pocket.index]), pocket.values);
  let choices = choose({}, pocket);
  assert.deepEqual(chosenCells(choices), { pocketTubeOD: pocket.valueMm, pocketRib: pocket.cross.valueMm });
  // Another pocket replaces both values, never one of them.
  const other = map.cells.find((c) => c.group === "pocket" && c.label !== pocket.label);
  choices = choose(choices, other);
  assert.deepEqual(chosenCells(choices), { pocketTubeOD: other.valueMm, pocketRib: other.cross.valueMm });
  assert.equal(choiceText(map, choices, "pocket"), labelText(other));
  assert.equal(labelText(other), other.lines.join(" / "));
  // A stud and a bore are their own groups.
  const stud = map.cells.find((c) => c.group === "stud");
  const tube = map.cells.find((c) => c.group === "tube");
  choices = choose(choose(choices, stud), tube);
  assert.deepEqual(Object.keys(chosenCells(choices)).sort(), ["pocketRib", "pocketTubeOD", "stud", "tube"]);
  assert.equal(choiceText(map, {}, "stud"), null);
  assert.equal(groupOf(cross.cells[stud.index]), "stud");
});

test("a choice is carried by every cell with its values (both copies), and only those", () => {
  const map = cellMap(cross);
  const stud = map.cells.find((c) => c.group === "stud" && c.copy === 1);
  const choices = choose({}, stud);
  const chosen = map.cells.filter((c) => isChosen(choices, c));
  assert.deepEqual(chosen.map((c) => c.copy).sort(), [0, 1]);
  assert.ok(chosen.every((c) => c.label === stud.label && c.group === "stud"));
  // The nominal pocket is printed in both pocket lines of a copy: all four carry one choice.
  const label = (c) => c.label;
  const counts = new Map();
  for (const c of map.cells.filter((c) => c.group === "pocket")) counts.set(label(c), (counts.get(label(c)) ?? 0) + 1);
  const [nominal] = [...counts].find(([, n]) => n === 4) ?? [];
  assert.ok(nominal, "the nominal pocket is in both lines of both copies");
  const both = choose({}, map.cells.find((c) => c.label === nominal));
  assert.equal(map.cells.filter((c) => isChosen(both, c)).length, 4);
});

test("the map checks one cell per kind (a radio group has one checked radio); the others with its value are twins", () => {
  const map = cellMap(cross);
  const nominal = map.groups.pocket.map((i) => map.cells[i]).find((c, _, all) => all.filter((o) => o.label === c.label).length === 4);
  const tapped = map.groups.pocket.filter((i) => map.cells[i].label === nominal.label)[2];
  const stud = map.cells.find((c) => c.group === "stud" && c.copy === 1);
  const choices = choose(choose({}, stud), map.cells[tapped]);
  const marks = mapSelection(map, choices, { pocket: tapped, stud: stud.index });
  // The cell tapped is the one checked; the other three with its values are twins.
  assert.equal(marks.pocket.checked, tapped);
  assert.equal(marks.pocket.stop, tapped);
  assert.equal(marks.pocket.twins.length, 3);
  assert.ok(marks.pocket.twins.every((i) => i !== tapped && map.cells[i].label === nominal.label));
  assert.equal(marks.stud.checked, stud.index);
  assert.deepEqual(marks.stud.twins.map((i) => map.cells[i].copy), [0]);
  // Nothing chosen: nothing checked, the first cell is the Tab stop.
  assert.deepEqual(marks.tube, { checked: null, twins: [], stop: map.groups.tube[0] });
  // A picked index that does not carry the choice (a new map, a choice set
  // another way) gives way to the first cell that does, in reading order.
  const fallback = mapSelection(map, choices, { pocket: map.groups.pocket.find((i) => map.cells[i].label !== nominal.label) });
  assert.equal(fallback.pocket.checked, map.groups.pocket.find((i) => map.cells[i].label === nominal.label));
  assert.equal(mapSelection(map, choices).stud.checked, map.groups.stud.find((i) => map.cells[i].label === stud.label));
  // Exactly one checked cell in every group that has a choice, on both coupons.
  for (const layout of [cross, full]) {
    const m = cellMap(layout);
    let all = {};
    for (const order of Object.values(m.groups)) all = choose(all, m.cells[order.at(-1)]);
    for (const [group, mark] of Object.entries(mapSelection(m, all))) {
      assert.ok(mark.checked !== null, `${group} has no checked cell`);
      assert.equal(new Set([mark.checked, ...mark.twins]).size, 1 + mark.twins.length);
      assert.equal(1 + mark.twins.length, m.groups[group].filter((i) => isChosen(all, m.cells[i])).length);
    }
  }
  assert.deepEqual(mapSelection(null, {}), {});
});

/** The narrowest map: the desktop side panel at its narrowest (320 px) or a 320 px phone, less the panel's 12 px gutters. */
const NARROWEST_MAP_PX = 320 - 2 * 12;
/** WCAG 2.2 success criterion 2.5.8 (target size, minimum), CSS pixels. */
const MIN_TARGET_PX = 24;

test("every tap target is at least 24 px and every label at least 10 px on the narrowest map, its label fitting its box", () => {
  for (const layout of [cross, full]) {
    const map = cellMap(layout);
    const px = NARROWEST_MAP_PX / map.widthMm;
    const name = `${layout.summary.pockets} (${map.shown})`;
    const smallest = Math.min(...map.cells.map((c) => Math.min(c.box.width, c.box.height)));
    assert.ok(smallest * px >= MIN_TARGET_PX, `${name}: the smallest tap target is ${(smallest * px).toFixed(1)} px`);
    for (const [group, order] of Object.entries(map.groups)) {
      const sizes = new Set(order.map((i) => map.cells[i].fontMm));
      assert.equal(sizes.size, 1, `${name}: ${group} labels differ in size`);
      const [fontMm] = sizes;
      assert.ok(fontMm * px >= 10, `${name}: ${group} labels are ${(fontMm * px).toFixed(1)} px`);
      for (const i of order) {
        const c = map.cells[i];
        assert.ok(fontMm <= labelFontMm(c) + 1e-12, `${name}: ${c.label} is drawn larger than its box takes`);
        // The symbol, the gap and the lines fit the height, the longest line the width.
        const symbol = group === "pocket" ? LABEL_EM.pocketSymbol : LABEL_EM.symbol;
        assert.ok((symbol + LABEL_EM.gap + c.lines.length * LABEL_EM.line) * fontMm < c.box.height, `${name}: ${c.label} is too tall`);
        assert.ok(Math.max(...c.lines.map((l) => l.length)) * LABEL_EM.char * fontMm < c.box.width, `${name}: ${c.label} is too wide`);
      }
    }
  }
  // Printed side by side, the copies would share the width and the targets would be too small: that is why they are drawn one under the other.
  const sideBySide = full.summary.footprintMm[0];
  const smallestFull = Math.min(...cellMap(full).cells.map((c) => Math.min(c.box.width, c.box.height)));
  assert.ok(smallestFull * (NARROWEST_MAP_PX / sideBySide) < MIN_TARGET_PX);
});

test("only pockets the printed pocket set has are offered; a choice the new coupon lacks is dropped", () => {
  const small = cellMap(cross);
  const big = cellMap(full);
  const offered = (map) => new Set(map.cells.filter((c) => c.group === "pocket").map((c) => c.label));
  const crossPockets = offered(small);
  const fullPockets = offered(big);
  assert.ok([...crossPockets].every((p) => fullPockets.has(p)), "the cross is part of the full set");
  const extra = big.cells.find((c) => c.group === "pocket" && !crossPockets.has(c.label));
  assert.ok(extra, "the full set prints pockets the cross does not");
  // Chosen on the full coupon, then the coupon is set back to the cross.
  const stud = big.cells.find((c) => c.group === "stud");
  const { choices, dropped } = keepPrinted(choose(choose({}, stud), extra), small);
  assert.deepEqual(dropped, ["pocket"]);
  assert.deepEqual(Object.keys(choices), ["stud"]);
  // A pocket both coupons print is kept.
  const shared = big.cells.find((c) => c.group === "pocket" && crossPockets.has(c.label));
  assert.deepEqual(keepPrinted(choose({}, shared), small).dropped, []);
});

test("the record input: the coupon as printed, the fields as typed, the CLI's defaults left to the core", () => {
  const map = cellMap(cross);
  const stud = map.cells.find((c) => c.group === "stud");
  const input = recordInput({
    process: "fdm-0.4",
    pocketSet: "cross",
    printer: "Prusa MINI+",
    material: "PLA",
    nozzleMm: 0.4,
    slicer: "  ",
    notes: "",
    date: "2026-11-12",
    choices: choose({}, stud),
  });
  assert.deepEqual(input, {
    process: "fdm-0.4",
    pocketSet: "cross",
    printer: "Prusa MINI+",
    material: "PLA",
    nozzleMm: 0.4,
    date: "2026-11-12",
    cells: { stud: stud.valueMm },
  });
  const typed = recordInput({ process: "fdm-0.2", pocketSet: "full", printer: " X ", material: "", nozzleMm: undefined, slicer: "Cura 5", notes: " 3 walls\n", date: "", choices: {} });
  assert.deepEqual(typed, { process: "fdm-0.2", pocketSet: "full", printer: " X ", material: "", slicer: "Cura 5", notes: " 3 walls\n", cells: {} });
});

test("the nozzle field: numbers go to the core (which refuses zero), text is refused here", () => {
  assert.deepEqual(parseNozzle("0.4"), { value: 0.4 });
  assert.deepEqual(parseNozzle(" 0.25 "), { value: 0.25 });
  assert.deepEqual(parseNozzle("0"), { value: 0 });
  for (const bad of ["", " ", "abc", "0.4mm", "Infinity", null, undefined]) {
    assert.match(parseNozzle(bad).error ?? "", /must be a number of millimetres/, JSON.stringify(bad));
  }
});

test("refusals go next to their field; cell and coupon refusals go with the map", () => {
  for (const f of ["printer", "material", "nozzleMm", "date", "slicer", "notes"]) assert.equal(errorPlace(f), f);
  for (const f of ["cells", "stud", "tube", "pocket", "pocketSet", "process", "id", "intent", "nope"]) assert.equal(errorPlace(f), "map");
});

test("the offsets summary shows the core's numbers with the CLI's signs", () => {
  // Neutral numbers, not brick dimensions (CLAUDE.md): this checks the
  // formatting only; core.test.js checks the rows of the real core's offsets.
  const rows = offsetRows([
    { cell: "stud", chosenMm: 1.25, nominalMm: 1.25, offsetField: "studDiameter", offsetMm: 0 },
    { cell: "pocketTubeOD", chosenMm: 2.5, nominalMm: 2.514, offsetField: "tubeOuterDiameter", offsetMm: -0.014 },
    { cell: "tube", chosenMm: 1.35, nominalMm: 1.2, offsetField: "tubeInnerDiameter", offsetMm: 0.15 },
  ]);
  assert.deepEqual(
    rows.map((r) => [r.name, r.chosen, r.nominal, r.offset, r.field]),
    [
      ["Stud", "1.25", "1.250", "+0.000", "studDiameter"],
      ["Pocket tube OD", "2.50", "2.514", "-0.014", "tubeOuterDiameter"],
      ["Tube bore", "1.35", "1.200", "+0.150", "tubeInnerDiameter"],
    ],
  );
  assert.deepEqual(offsetRows(null), []);
  assert.equal(signedMm(-0.0004), "-0.000");
  assert.equal(signedMm(NaN), "–");
});

test("today's date in the browser's time zone, as the date field takes it", () => {
  assert.equal(todayLocal(new Date(2026, 10, 2, 23, 59)), "2026-11-02");
  assert.equal(todayLocal(new Date(2027, 0, 31, 0, 1)), "2027-01-31");
  assert.match(todayLocal(), /^\d{4}-\d\d-\d\d$/);
});

test("a cell's accessible name says what, which copy and which line", () => {
  const map = cellMap(cross);
  const pocket = map.cells.find((c) => c.group === "pocket" && c.copy === 1);
  const name = cellAccessibleName(map, pocket, cross.summary);
  assert.match(name, /^2x2 pocket \d\.\d\d \/ \d\.\d\d mm, tube OD \/ rib depth, copy 2 \(front\), line [34]$/);
  const stud = map.cells.find((c) => c.group === "stud");
  assert.equal(cellAccessibleName(map, stud, cross.summary), `Stud ${stud.label} mm, copy 1 (back), line 1`);
});

test("cellMap refuses something that is not a layout", () => {
  assert.throws(() => cellMap(null), /coupon layout/);
  assert.throws(() => cellMap({ cells: [] }), /coupon layout/);
});
