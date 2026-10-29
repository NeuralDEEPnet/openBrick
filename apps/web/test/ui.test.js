// The playground's text helpers (src/ui.js): validation panel entries, download
// reasons, stats, the coupon table and the record command, the mode in the URL.
// Report findings here follow schemas/report.schema.json.

import assert from "node:assert/strict";
import { test } from "node:test";
import { centredOffset } from "../src/placement.js";
import {
  couponRows,
  couponStats,
  downloadBlockedReason,
  evaluatingParts,
  findingView,
  fitLabel,
  partStats,
  pocketSetFlag,
  PRINT_TEST_GUIDE_URL,
  readMode,
  recordCommand,
  removedText,
  reportSummary,
  sizeText,
  SLOW_BUILD_MS,
  sortedFindings,
  writeMode,
} from "../src/ui.js";

const missing = {
  rule: "FIT.MISSING",
  severity: "blocker",
  message: "fit-critical connector tubeF needs the calibration cell `tube`, but no fit profile was given",
  value: null,
  threshold: null,
  fix: "print the calibration coupon and record it",
  feature: "bottom",
  unit: "",
};
// Each finding as the worker hands it over: the report entry plus the core's unit.
const removed = {
  rule: "FEATURE.MIN",
  severity: "warning",
  message: "the morphological opening removed 1.03 % of the artwork area, more than the 1 % allowed",
  value: 1.0324211814310704,
  threshold: 1,
  fix: "make those lines wider",
  feature: null,
  unit: "%",
};
const wall = { rule: "WALL.MIN", severity: "blocker", message: "the side wall is 0.6 mm", value: 0.6, threshold: 0.8, fix: "thicker", feature: "body", unit: "mm" };

test("a finding shows severity in words and as an icon, its rule, values, threshold and fix", () => {
  const v = findingView(removed);
  assert.deepEqual(
    [v.blocker, v.icon, v.severity, v.rule, v.measured, v.threshold, v.fix, v.calibrate],
    [false, "⚠️", "Warning", "FEATURE.MIN", "1.032 %", "1 %", "make those lines wider", false],
  );
  const b = findingView(missing);
  assert.deepEqual([b.blocker, b.icon, b.severity, b.measured, b.threshold, b.feature, b.calibrate], [true, "⛔", "Blocker", null, null, "bottom", true]);
  const w = findingView(wall);
  assert.deepEqual([w.measured, w.threshold], ["0.6 mm", "0.8 mm"]);
  // A stroke the estimate could not measure keeps its threshold.
  const unmeasured = findingView({ ...wall, rule: "FEATURE.MIN", message: "the thinnest stroke was not measured", value: null, threshold: 0.8 });
  assert.deepEqual([unmeasured.measured, unmeasured.threshold], ["not measured", "0.8 mm"]);
  assert.equal(findingView({ ...wall, rule: "FIT.PROCESS" }).calibrate, true);
});

test("the unit is the core's, never read from the message", () => {
  // The same message with another unit from the core shows that unit: the
  // wording plays no part.
  assert.deepEqual(
    [findingView({ ...removed, unit: "mm" }).measured, findingView({ ...removed, unit: "mm" }).threshold],
    ["1.032 mm", "1 mm"],
  );
  const volume = findingView({ ...wall, rule: "MESH.VOLUME", value: -5, threshold: 0, unit: "mm³" });
  assert.deepEqual([volume.measured, volume.threshold], ["-5 mm³", "0 mm³"]);
  // A count (the components of an empty mesh) has no unit.
  const empty = findingView({ ...wall, rule: "MESH.MANIFOLD", value: 0, threshold: 1, unit: "" });
  assert.deepEqual([empty.measured, empty.threshold], ["0", "1"]);
  // Without a unit from the core the numbers are shown bare.
  const { unit: _, ...bare } = removed;
  assert.deepEqual([findingView(bare).measured, findingView(bare).threshold], ["1.032", "1"]);
});

test("blockers come first, and the download reason names them", () => {
  const report = { findings: [removed, missing, wall] };
  assert.deepEqual(sortedFindings(report).map((f) => f.rule), ["FIT.MISSING", "WALL.MIN", "FEATURE.MIN"]);
  assert.equal(downloadBlockedReason(report), "Download is off until the blockers FIT.MISSING and WALL.MIN are fixed (see Validation).");
  assert.equal(downloadBlockedReason({ findings: [missing, { ...missing, message: "pocketRib" }] }), "Download is off until the blocker FIT.MISSING is fixed (see Validation).");
  assert.equal(downloadBlockedReason({ findings: [removed] }), null, "warnings never disable downloads");
  assert.equal(downloadBlockedReason({ findings: [] }), null);
  assert.equal(reportSummary(report), "2 blockers, 1 warning.");
  assert.equal(reportSummary({ findings: [removed] }), "0 blockers, 1 warning.");
  assert.match(reportSummary({ findings: [] }), /^No findings/);
});

test("stats show triangles, size in mm, mass and time from the core's numbers", () => {
  const part = { triangles: 3016, bounds: [0, 0, 0, 12.5, 25.25, 2.5], massGPla: 1.1098, buildMs: 84.4 };
  assert.equal(sizeText(part.bounds), "12.5 × 25.3 × 2.5 mm");
  assert.deepEqual(partStats(part), [
    ["Triangles", "3,016"],
    ["Size", "12.5 × 25.3 × 2.5 mm"],
    ["Mass", "about 1.1 g of PLA, printed solid"],
    ["Built in", "84 ms"],
  ]);
  const coupon = {
    triangles: 44770,
    cells: new Array(54),
    buildMs: 591,
    summary: { footprintMm: [100.25, 150], heightMm: 2.5, copies: 2 },
    estimate: { massGPla: 32.17, timeS: 6485, flowMm3S: 4 },
  };
  assert.deepEqual(couponStats(coupon).map(([k]) => k), ["Triangles", "Size", "Cells", "Mass", "Print time", "Built in"]);
  assert.equal(couponStats(coupon)[1][1], "100.3 × 150.0 × 2.5 mm");
  assert.equal(couponStats(coupon)[3][1], "about 32 g of PLA");
  assert.match(couponStats(coupon)[4][1], /^about 1\.8 h at 4\.0 mm³\/s/);
});

test("the red overlay's legend entry says how much is removed, in words", () => {
  assert.match(removedText({ measurements: { removedAreaPct: 1.0324 } }), /^Red: 1\.032 % of the artwork does not print as drawn/);
  assert.equal(removedText({ measurements: {} }), null);
});

test("a slow evaluation says how to cancel it", () => {
  assert.deepEqual(evaluatingParts("Building the logo tile", 0), ["Building the logo tile…"]);
  assert.deepEqual(evaluatingParts("Building the logo tile", SLOW_BUILD_MS), [
    "Building the logo tile…",
    "Large parts take a while. Any change cancels this one.",
  ]);
});

test("the mode lives in the URL, the tile without a parameter", () => {
  assert.deepEqual(readMode(""), { mode: "tile", errors: [] });
  assert.deepEqual(readMode("?mode=coupon"), { mode: "coupon", errors: [] });
  assert.equal(readMode("?mode=brick").mode, "tile");
  assert.match(readMode("?mode=brick").errors[0], /mode must be one of tile, coupon/);
  assert.equal(writeMode("?backend=webgl2", "coupon"), "?backend=webgl2&mode=coupon");
  assert.equal(writeMode("?mode=coupon&backend=webgl2", "tile"), "?backend=webgl2");
  assert.equal(writeMode("?mode=coupon", "tile"), "");
});

test("fit profiles are labelled by printer and material", () => {
  assert.equal(fitLabel("Example fit profile", { printer: "Prusa MK4S", material: { type: "PETG" } }), "Example fit profile (Prusa MK4S, PETG)");
  assert.equal(fitLabel("My fit profile", { printer: "Mini", material: "PLA" }), "My fit profile (Mini, PLA)");
  assert.equal(fitLabel("My fit profile", {}), "My fit profile");
  assert.equal(PRINT_TEST_GUIDE_URL, "https://github.com/NeuralDEEPnet/openBrick/blob/main/docs/guides/print-test.md");
});

test("the coupon table and the record command use the command line's spelling", () => {
  assert.equal(pocketSetFlag("crossAndDiagonal"), "cross-and-diagonal");
  assert.equal(pocketSetFlag("cross"), "cross");
  // Sample values come from a fit profile's chosen cells (the page passes the
  // bundled example's); these are made up, not coupon values.
  assert.equal(
    recordCommand("fdm-0.4", "crossAndDiagonal", { stud: 1.25, tube: 2.5, pocketTubeOD: 3.75, pocketRib: 0.5 }),
    'openbrick calibrate record --process fdm-0.4 --pocket-set cross-and-diagonal --printer "Your printer" --material PLA --stud 1.25 --tube 2.50 --pocket 3.75/0.50 --out my-fit.json',
  );
  assert.match(recordCommand("fdm-0.2", "cross"), /--pocket-set cross .*--stud D --tube D --pocket OD\/RIB --out my-fit\.json$/);
  const cells = [
    { row: 0, copy: 0, cellId: "stud", valueMm: 1.25, cross: null, label: "1.25" },
    { row: 1, copy: 0, cellId: "tube", valueMm: 2.5, cross: null, label: "2.50" },
    { row: 2, copy: 0, cellId: "pocketTubeOD", valueMm: 3.75, cross: { cellId: "pocketRib", valueMm: 0.5 }, label: "3.75\n0.50" },
    { row: 0, copy: 1, cellId: "stud", valueMm: 1.25, cross: null, label: "1.25" },
  ];
  assert.deepEqual(couponRows(cells), [
    { line: "Stud", label: "1.25", record: "--stud 1.25" },
    { line: "Tube bore", label: "2.50", record: "--tube 2.50" },
    { line: "2x2 pocket", label: "3.75 / 0.50", record: "--pocket 3.75/0.50" },
  ]);
});

test("a part off the lattice is centred on the origin, standing on the ground", () => {
  assert.deepEqual(centredOffset([0, 0, 0, 100, 150, 2]), [-50, -75, -0]);
  assert.deepEqual(centredOffset([2, 4, 1, 6, 8, 3]), [-4, -6, -1]);
});
