// The parameter-to-control mapping (src/params.js) over the shipped logo tile
// document and small hand-made ones: pure functions, no geometry.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  applyPreset,
  assetSizeError,
  controlFor,
  controlsFor,
  couponDownloadName,
  defaultValues,
  downloadName,
  humanLabel,
  isVisible,
  looksLikeSvg,
  matchingPreset,
  PARAM_TYPES,
  paramOverrides,
  sameValue,
  visibility,
} from "../src/params.js";

const LOGO_TILE = JSON.parse(readFileSync(new URL("../../../parts/ndn.logo-tile.obpart.json", import.meta.url), "utf8"));
const BRAND_TILE = JSON.parse(readFileSync(new URL("../../../parts/ndn.brand-tile.obpart.json", import.meta.url), "utf8"));
const KEYCHAIN = JSON.parse(readFileSync(new URL("../../../parts/ndn.nameplate-keychain.obpart.json", import.meta.url), "utf8"));
const P = LOGO_TILE.params;

test("labels are the parameter names in words", () => {
  assert.equal(humanLabel("reliefHeight"), "Relief height");
  assert.equal(humanLabel("size"), "Size");
  assert.equal(humanLabel("ring_hole"), "Ring hole");
  assert.equal(humanLabel("emboss"), "Emboss");
});

test("every logo tile parameter gets a control with the document's limits", () => {
  const spec = controlsFor(LOGO_TILE, { maxStuds: 16 });
  const byName = Object.fromEntries(spec.controls.map((c) => [c.name, c]));
  assert.deepEqual(Object.keys(byName), Object.keys(P), "document order");
  assert.deepEqual(
    spec.groups.map((g) => [g.name, g.controls.map((c) => c.name)]),
    [
      ["Size", ["size", "height"]],
      ["Artwork", ["logo", "relief", "inlayStyle"]],
      // reliefHeight and margin name no group in the document.
      ["Details", ["reliefHeight", "margin"]],
    ],
  );

  const size = byName.size;
  assert.equal(size.kind, "grid2");
  assert.deepEqual(size.default, P.size.default);
  assert.deepEqual(
    size.axes.map((a) => [a.label, a.min, a.max, a.capped]),
    [
      ["Width", P.size.min[0], Math.min(P.size.max[0], 16), P.size.max[0] > 16],
      ["Length", P.size.min[1], Math.min(P.size.max[1], 16), P.size.max[1] > 16],
    ],
    "1 to 16 per side: the document's limits, capped at the core's footprint limit",
  );

  assert.equal(byName.height.kind, "choice");
  assert.deepEqual(byName.height.options, P.height.options, "labels from the document");
  assert.deepEqual(byName.relief.options, [
    { value: "emboss", label: "Emboss" },
    { value: "deboss", label: "Deboss" },
    { value: "inlay", label: "Inlay" },
  ]);
  // The inlay style: a choice labelled in words, its options as the document
  // labels them (one extruder or AMS / tool changer), shown only for an inlay.
  const style = byName.inlayStyle;
  assert.equal(style.kind, "choice");
  assert.equal(style.label, "Inlay style");
  assert.deepEqual(style.options, P.inlayStyle.options);
  assert.deepEqual(
    style.options.map((o) => o.value),
    ["swap", "parts"],
  );
  for (const o of style.options) assert.ok(o.label.length > o.value.length, `option ${o.value} has a label in words`);
  assert.equal(style.visibleWhen, P.inlayStyle.visibleWhen);
  assert.equal(style.help, P.inlayStyle.help);
  assert.equal(style.group, "Artwork");

  const rh = byName.reliefHeight;
  assert.deepEqual([rh.kind, rh.min, rh.max, rh.step, rh.unit], ["range", P.reliefHeight.min, P.reliefHeight.max, P.reliefHeight.step, "mm"]);
  assert.equal(rh.visibleWhen, P.reliefHeight.visibleWhen);
  assert.equal(rh.help, P.reliefHeight.help);
  // No step in the document: a number field, any value between the limits.
  const margin = byName.margin;
  assert.deepEqual([margin.kind, margin.min, margin.max, margin.step], ["number", P.margin.min, P.margin.max, "any"]);

  assert.equal(byName.logo.kind, "file");
  assert.equal(byName.logo.default, "assets/mark.svg");
  assert.match(byName.logo.accept, /\.svg/);

  assert.deepEqual(defaultValues(spec), {
    size: P.size.default,
    height: P.height.default,
    logo: P.logo.default,
    relief: P.relief.default,
    inlayStyle: P.inlayStyle.default,
    reliefHeight: P.reliefHeight.default,
    margin: P.margin.default,
  });
});

test("the inlay style is shown and sent only for an inlay", () => {
  const spec = controlsFor(LOGO_TILE, { maxStuds: 16 });
  for (const relief of ["emboss", "deboss", "inlay"]) {
    const values = { ...defaultValues(spec), relief, inlayStyle: "parts" };
    const { visible, errors } = visibility(spec, values);
    assert.deepEqual(errors, []);
    assert.equal(visible.inlayStyle, relief === "inlay", relief);
    const sent = paramOverrides(spec, values, visible);
    assert.equal(sent.inlayStyle, relief === "inlay" ? "parts" : undefined, relief);
  }
});

test("other parameter types map to plain controls", () => {
  assert.equal(controlFor("on", { type: "bool", default: true }).kind, "checkbox");
  assert.equal(controlFor("c", { type: "color", default: "#fff" }).kind, "color");
  assert.equal(controlFor("t", { type: "string", default: "Hi" }).kind, "text");
  const n = controlFor("count", { type: "int", default: 2, min: 1, max: 9 });
  assert.deepEqual([n.kind, n.step], ["range", 1]);
  // A grid in millimetres is not capped by the stud limit.
  const g = controlFor("box", { type: "grid2", unit: "mm", default: [10, 20], min: [1, 1], max: [100, 200] }, { maxStuds: 16 });
  assert.deepEqual(g.axes.map((a) => a.max), [100, 200]);
  assert.equal(controlFor("turn", { type: "angle", unit: "deg", default: 0 }).kind, "number");
  const keychain = controlsFor(KEYCHAIN, { maxStuds: 16 });
  assert.ok(keychain.controls.length > 0);
  for (const c of keychain.controls) assert.ok(c.kind && c.label, c.name);
});

test("every parameter type of the evaluator gets a control or a kind note", () => {
  const spec = controlsFor(KEYCHAIN, { maxStuds: 16 });
  const kinds = Object.fromEntries(spec.controls.map((c) => [c.name, c.kind]));
  assert.deepEqual(kinds, {
    text: "text",
    font: "unsupported",
    length: "range",
    // An int with options is a choice whose values stay numbers.
    width: "choice",
    studsOnTop: "checkbox",
    loop: "choice",
    ringHole: "range",
  });
  const byName = Object.fromEntries(spec.controls.map((c) => [c.name, c]));
  assert.equal(byName.text.maxLength, KEYCHAIN.params.text.maxLength);
  assert.deepEqual(byName.width.options, [
    { value: 1, label: "1" },
    { value: 2, label: "2" },
  ]);
  // The font is not offered yet: the note says so, and what the part keeps.
  assert.match(byName.font.note, /font is not possible here yet, so the part keeps “Inter:Bold”/);
  // Unsupported parameters are never sent, so the core uses their defaults.
  const sent = paramOverrides(spec, defaultValues(spec));
  assert.equal("font" in sent, false);
  assert.equal(sent.width, 2);
  for (const [type, pattern] of [
    ["image", /pictures cannot be used here yet/],
    ["list", /Lists cannot be edited here yet/],
    ["vector", /“vector” is not a parameter type this designer knows/],
  ]) {
    const c = controlFor("x", { type, default: type === "list" ? [1] : "a" });
    assert.equal(c.kind, "unsupported", type);
    assert.match(c.note, pattern, type);
  }
  assert.equal(controlFor("e", { type: "enum", default: "a" }).kind, "unsupported", "an enum without options");
  for (const type of PARAM_TYPES) assert.ok(controlFor("x", { type, default: null, options: type === "enum" ? ["a"] : undefined }).kind, type);
  // Only one drawing is chosen in the page; a second keeps its default.
  const two = controlsFor({ params: { a: { type: "svg", default: "a.svg" }, b: { type: "svg", default: "b.svg" } } });
  assert.deepEqual(two.controls.map((c) => c.kind), ["file", "unsupported"]);
  assert.match(two.controls[1].note, /keeps “b\.svg”/);
});

test("presets set the values they name and keep the others", () => {
  const spec = controlsFor(BRAND_TILE, { maxStuds: 16 });
  const values = { ...defaultValues(spec), size: [2, 6] };
  const [raised, pressed, inlay] = BRAND_TILE.presets;
  const applied = applyPreset(spec, values, pressed);
  assert.equal(applied.values.relief, "deboss");
  assert.deepEqual(applied.values.size, [2, 6], "the size is not in the preset");
  assert.equal(applied.artwork, null);
  assert.deepEqual(applied.kept, []);
  assert.equal(values.relief, "emboss", "the values given are not changed");
  // The picker shows the preset the values match, else Custom ("").
  const art = BRAND_TILE.params.logo.default;
  assert.equal(matchingPreset(spec, BRAND_TILE.presets, values, art), raised.name);
  assert.equal(matchingPreset(spec, BRAND_TILE.presets, applied.values, art), pressed.name);
  assert.equal(matchingPreset(spec, BRAND_TILE.presets, applyPreset(spec, values, inlay).values, art), inlay.name);
  assert.equal(matchingPreset(spec, BRAND_TILE.presets, { ...values, reliefHeight: 1.2 }, art), "");
  assert.equal(matchingPreset(spec, [], values, art), "");
  // Numbers compare by value, arrays item by item; the artwork by its name.
  const mine = { name: "Mine", params: { size: [2, 6], margin: 1, logo: "mine.svg", font: "x", nope: 1 } };
  const m = applyPreset(spec, values, mine);
  assert.equal(m.artwork, "mine.svg");
  assert.deepEqual(m.kept, ["font", "nope"]);
  assert.equal(matchingPreset(spec, [mine], values, "mine.svg"), "Mine");
  assert.equal(matchingPreset(spec, [mine], values, art), "");
  assert.equal(sameValue(1, 1.0), true);
  assert.equal(sameValue([2, 4], [2, 4]), true);
  assert.equal(sameValue([2, 4], [4, 2]), false);
  assert.equal(sameValue("2", 2), false);
});

test("visibleWhen reads the core's comparisons and logic", () => {
  const v = { relief: "inlay", loop: "none", size: [2, 4], height: 1.2, on: true };
  assert.equal(isVisible(P.reliefHeight.visibleWhen, { relief: "emboss" }), true);
  assert.equal(isVisible(P.reliefHeight.visibleWhen, { relief: "deboss" }), true);
  assert.equal(isVisible(P.reliefHeight.visibleWhen, { relief: "inlay" }), false);
  assert.equal(isVisible("$relief == 'inlay'", v), true);
  assert.equal(isVisible('relief == "inlay" && loop != "none"', v), false);
  assert.equal(isVisible("relief == 'inlay' || loop != 'none'", v), true);
  assert.equal(isVisible("!(relief == 'inlay')", v), false);
  assert.equal(isVisible("height >= 1.2 && on", v), true);
  assert.equal(isVisible("$on", v), true);
  assert.equal(isVisible("false || (height < 1)", v), false);
  assert.equal(isVisible(undefined, v), true);
  assert.equal(isVisible("", v), true);
  // What the page cannot read throws (and the control stays shown).
  for (const bad of ["relief == 1", "nope == 'x'", "height + 1 > 2", "relief ==", "(on", "on on", "'x'"]) {
    assert.throws(() => isVisible(bad, v), Error, bad);
  }
});

test("hidden parameters are left out of the overrides, so they take the document's default", () => {
  const spec = controlsFor(LOGO_TILE, { maxStuds: 16 });
  const values = { ...defaultValues(spec), relief: "inlay", reliefHeight: 1.2 };
  const { visible, errors } = visibility(spec, values);
  assert.deepEqual(errors, []);
  assert.equal(visible.reliefHeight, false);
  const sent = paramOverrides(spec, values);
  assert.equal("reliefHeight" in sent, false);
  assert.deepEqual(sent.size, [2, 4]);
  assert.equal(sent.relief, "inlay");
  const emboss = paramOverrides(spec, { ...values, relief: "emboss" });
  assert.equal(emboss.reliefHeight, 1.2);
  // An expression the page cannot read shows the control and says why.
  const odd = controlsFor({ params: { a: { type: "length", default: 1, visibleWhen: "$b - 1 > 0" }, b: { type: "length", default: 2 } } });
  const r = visibility(odd, defaultValues(odd));
  assert.equal(r.visible.a, true);
  assert.match(r.errors[0], /^a: visibleWhen/);
});

test("download names follow {id}-{w}x{h}-{relief}[-{inlayStyle}].{ext}", () => {
  assert.equal(downloadName(LOGO_TILE, { size: [2, 4], relief: "emboss" }, "3mf"), "ndn.logo-tile-2x4-emboss.3mf");
  assert.equal(downloadName(LOGO_TILE, { size: [16, 1], relief: "inlay" }, "stl"), "ndn.logo-tile-16x1-inlay.stl");
  // The core's resolved values carry every parameter, hidden ones too: the
  // inlay style is named only while it is shown, so an emboss keeps its name
  // and the two inlays never share one.
  const core = (relief, inlayStyle) => ({ size: [2, 4], relief, inlayStyle, height: "plate" });
  const names = [
    ["emboss", "swap"],
    ["deboss", "parts"],
    ["inlay", "swap"],
    ["inlay", "parts"],
  ].map(([relief, style]) => downloadName(LOGO_TILE, core(relief, style), "3mf", { inlayStyle: relief === "inlay" }));
  assert.deepEqual(names, [
    "ndn.logo-tile-2x4-emboss.3mf",
    "ndn.logo-tile-2x4-deboss.3mf",
    "ndn.logo-tile-2x4-inlay-swap.3mf",
    "ndn.logo-tile-2x4-inlay-parts.3mf",
  ]);
  assert.equal(new Set(names).size, names.length, "unique");
  // Without the core's answer the page reads visibleWhen itself.
  assert.equal(downloadName(LOGO_TILE, core("inlay", "parts"), "stl"), "ndn.logo-tile-2x4-inlay-parts.stl");
  assert.equal(downloadName(LOGO_TILE, core("emboss", "parts"), "stl"), "ndn.logo-tile-2x4-emboss.stl");
  assert.equal(downloadName({ id: "my part/1", params: {} }, {}, "3mf"), "my-part-1.3mf", "unsafe characters become -");
  assert.equal(downloadName(null, {}, "stl"), "part.stl");
  assert.equal(couponDownloadName("coupon-fdm-0.4-cross", "3mf"), "coupon-fdm-0.4-cross.3mf");
});

test("download names end with the sample's suffix, and the user's own SVG adds none", () => {
  const visible = { inlayStyle: true };
  const values = { size: [2, 4], relief: "inlay", inlayStyle: "parts" };
  assert.equal(downloadName(LOGO_TILE, values, "3mf", visible, "neuraldeep"), "ndn.logo-tile-2x4-inlay-parts-neuraldeep.3mf");
  assert.equal(downloadName(LOGO_TILE, { size: [4, 4], relief: "emboss" }, "stl", null, "neuraldeep-badge"), "ndn.logo-tile-4x4-emboss-neuraldeep-badge.stl");
  assert.equal(downloadName(LOGO_TILE, values, "3mf", visible, null), "ndn.logo-tile-2x4-inlay-parts.3mf", "an uploaded SVG");
  assert.equal(downloadName(LOGO_TILE, values, "3mf", visible, ""), "ndn.logo-tile-2x4-inlay-parts.3mf");
  const names = ["neuraldeep", "neuraldeep-badge", "sample", null].map((s) => downloadName(LOGO_TILE, { size: [2, 4], relief: "emboss" }, "3mf", null, s));
  assert.equal(new Set(names).size, names.length, `unique: ${names}`);
  assert.equal(downloadName(LOGO_TILE, { size: [2, 4], relief: "emboss" }, "3mf", null, "a b/c"), "ndn.logo-tile-2x4-emboss-a-b-c.3mf", "made safe");
});

test("artwork over the core's limit is refused with a friendly message before it is sent", () => {
  const max = 4 * 1024 * 1024;
  assert.equal(assetSizeError("logo.svg", max, max), null, "exactly the limit passes");
  const message = assetSizeError("big.svg", max + 1, max);
  assert.match(message, /“big\.svg” is 4\.0 MiB \(4,194,305 bytes\), and artwork can be at most 4\.0 MiB \(4,194,304 bytes\)/);
  assert.match(message, /Simplify the drawing/);
  const enc = new TextEncoder();
  assert.equal(looksLikeSvg(enc.encode('<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>')), true);
  assert.equal(looksLikeSvg(enc.encode("<svg viewBox='0 0 1 1'></svg>")), true);
  assert.equal(looksLikeSvg(enc.encode("\u0089PNG\r\n")), false);
});
