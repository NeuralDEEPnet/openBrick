// The part being designed (src/part.js): which files open as parts, the name a
// part saves under, the drawing an opened part starts with, and the sentences
// about features this version cannot build yet. Pure functions, no geometry.

import assert from "node:assert/strict";
import { test } from "node:test";
import { controlsFor, defaultValues, applyPreset, matchingPreset } from "../src/params.js";
import { buildErrorText, conditionInWords, isPartFile, partFileName, sampleForAsset, unsupportedNote, withCoreDefaults } from "../src/part.js";

/** What open_part_json answers for parts/ndn.nameplate-keychain.obpart.json (crates/openbrick-wasm src/doc.rs). */
const KEYCHAIN_UNSUPPORTED = [
  { feature: "label", op: "relief.text", condition: "!$studsOnTop" },
  { feature: "ring", op: "loop.keyring", condition: "$loop != 'none'" },
];

/** The keychain's panel: the parameters its conditions name (parts/ndn.nameplate-keychain.obpart.json). */
const KEYCHAIN_SPEC = controlsFor({
  params: {
    studsOnTop: { type: "bool", default: false },
    loop: { type: "enum", default: "left", options: ["none", "left", "right"] },
  },
});

test("part documents are told from drawings by their extension", () => {
  for (const name of ["ndn.brand-tile.obpart.json", "MY.JSON", "a.json"]) assert.equal(isPartFile(name), true, name);
  for (const name of ["logo.svg", "fit.json.svg", "", undefined, "obpart"]) assert.equal(isPartFile(name), false, String(name));
});

test("a part saves as {id}.obpart.json, made safe for a file name", () => {
  assert.equal(partFileName("ndn.brand-tile"), "ndn.brand-tile.obpart.json");
  assert.equal(partFileName("my part/1"), "my-part-1.obpart.json");
  assert.equal(partFileName(""), "part.obpart.json");
  assert.equal(partFileName(undefined), "part.obpart.json");
});

test("an opened part starts with the bundled drawing it names", () => {
  const samples = [
    { id: "mark", files: [{ asset: "assets/brand/mark-2x4.svg" }, { asset: "assets/brand/mark-4x6.svg" }] },
    { id: "cc0", files: [{ asset: "assets/mark.svg" }] },
  ];
  const found = sampleForAsset(samples, "assets/brand/mark-4x6.svg");
  assert.equal(found.sample.id, "mark");
  assert.equal(found.file.asset, "assets/brand/mark-4x6.svg");
  assert.equal(sampleForAsset(samples, "assets/mark.svg").sample.id, "cc0");
  assert.equal(sampleForAsset(samples, "assets/other.svg"), null);
  assert.equal(sampleForAsset(undefined, "x"), null);
});

test("features this version cannot build are named with the settings that turn them off", () => {
  assert.equal(unsupportedNote([]), "");
  assert.equal(unsupportedNote(undefined), "");
  const note = unsupportedNote(KEYCHAIN_UNSUPPORTED, KEYCHAIN_SPEC);
  assert.match(note, /^This part uses 2 features this version cannot build yet: “label” \(relief\.text\) and “ring” \(loop\.keyring\)\./);
  assert.match(note, /“label” is built only while Studs on top is off and “ring” is built only while Loop is not None\. To preview the part, turn on Studs on top and set Loop to None\./);
  assert.match(note, /\(In the part file: “label” has if !\$studsOnTop; “ring” has if \$loop != 'none'\.\)/, "the expression stays as detail");
  assert.match(note, /You can still edit the values and save the part\.$/);
  // An expression this page does not put into words is shown as written.
  const odd = unsupportedNote([{ feature: "x", op: "relief.text", condition: "$a > 2 && $b" }], KEYCHAIN_SPEC);
  assert.match(odd, /“x” is built only while \$a > 2 && \$b is true\. To preview the part, change the values so it is off\./);
  const always = unsupportedNote([{ feature: "pin", op: "pin", condition: null }]);
  assert.match(always, /^This part uses a feature this version cannot build yet: “pin” \(pin\)\. “pin” \(pin\) is always built, so the part cannot be previewed or downloaded here yet\./);
});

test("simple conditions are put in the panel's words", () => {
  assert.deepEqual(conditionInWords("!$studsOnTop", KEYCHAIN_SPEC), { while: "Studs on top is off", off: "turn on Studs on top" });
  assert.deepEqual(conditionInWords("$studsOnTop", KEYCHAIN_SPEC), { while: "Studs on top is on", off: "turn off Studs on top" });
  assert.deepEqual(conditionInWords("$loop != 'none'", KEYCHAIN_SPEC), { while: "Loop is not None", off: "set Loop to None" });
  assert.deepEqual(conditionInWords('loop == "left"', KEYCHAIN_SPEC), { while: "Loop is Left", off: "set Loop to something other than Left" });
  assert.equal(conditionInWords("$a > 2", KEYCHAIN_SPEC), null);
  assert.equal(conditionInWords("!$loop == 'none'", KEYCHAIN_SPEC), null);
});

test("an opened part's values written with units read as the core's numbers", () => {
  // As open_part_json answers for a length default "0.6mm" and a preset "0.8mm".
  const doc = {
    params: { h: { type: "length", unit: "mm", default: "0.6mm", min: 0.2, max: 1.2, step: 0.1 } },
    presets: [{ name: "High", params: { h: "0.8mm" } }],
  };
  const opened = { defaults: { h: 0.6 }, presets: [{ name: "High", params: { h: 0.8 }, error: null }] };
  const spec = controlsFor(withCoreDefaults(doc, opened.defaults));
  const values = defaultValues(spec);
  assert.equal(values.h, 0.6);
  assert.equal(Number(values.h).toFixed(2), "0.60", "the slider shows a number");
  const applied = applyPreset(spec, values, opened.presets[0]).values;
  assert.equal(applied.h, 0.8);
  assert.equal(matchingPreset(spec, opened.presets, { h: 0.8 }, null), "High");
  assert.equal(doc.params.h.default, "0.6mm", "the document itself is not changed");
  assert.equal(withCoreDefaults(doc, null), doc, "no answer keeps the document");
});

test("an evaluation that stops at such a feature gets a sentence, other errors stay as the core wrote them", () => {
  const core = "feature `label` uses `relief.text`, which this evaluator does not build yet; remove the feature, turn it off with its `if`, or evaluate with skip_unsupported for a preview without it";
  const text = buildErrorText(core, KEYCHAIN_UNSUPPORTED, KEYCHAIN_SPEC);
  assert.equal(
    text,
    "“label” uses relief.text, which this version cannot build yet. It is built only while Studs on top is off: turn on Studs on top, and the preview builds. You can still edit the values and save the part.",
  );
  assert.match(buildErrorText(core, [{ feature: "label", op: "relief.text", condition: "$a > 2" }]), /built only while \$a > 2 is true: change the values so it is off/);
  assert.doesNotMatch(text, /skip_unsupported/, "no evaluator jargon");
  const detail = "feature `studs` uses `stud.field` with studType hollow, which this evaluator does not build yet; remove the feature";
  assert.match(buildErrorText(detail, []), /^“studs” uses stud\.field with studType hollow, which this version cannot build yet\. Nothing in the panel turns it off/);
  assert.equal(buildErrorText("asset `a.svg`: no asset named a.svg", KEYCHAIN_UNSUPPORTED), "asset `a.svg`: no asset named a.svg");
});
