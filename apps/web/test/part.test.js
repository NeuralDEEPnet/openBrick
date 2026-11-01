// The part being designed (src/part.js): which files open as parts, the name a
// part saves under, the drawing an opened part starts with, and the sentences
// about features this version cannot build yet. Pure functions, no geometry.

import assert from "node:assert/strict";
import { test } from "node:test";
import { buildErrorText, isPartFile, partFileName, sampleForAsset, unsupportedNote } from "../src/part.js";

/** What open_part_json answers for parts/ndn.nameplate-keychain.obpart.json (crates/openbrick-wasm src/doc.rs). */
const KEYCHAIN_UNSUPPORTED = [
  { feature: "label", op: "relief.text", condition: "!$studsOnTop" },
  { feature: "ring", op: "loop.keyring", condition: "$loop != 'none'" },
];

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

test("features this version cannot build are named with what turns them off", () => {
  assert.equal(unsupportedNote([]), "");
  assert.equal(unsupportedNote(undefined), "");
  const note = unsupportedNote(KEYCHAIN_UNSUPPORTED);
  assert.match(note, /^This part uses 2 features this version cannot build yet: “label” \(relief\.text\) and “ring” \(loop\.keyring\)\./);
  assert.match(note, /“label” is built only while !\$studsOnTop is true and “ring” is built only while \$loop != 'none' is true: change the values so they are off, and the preview builds\./);
  assert.match(note, /You can still edit the values and save the part\.$/);
  const always = unsupportedNote([{ feature: "pin", op: "pin", condition: null }]);
  assert.match(always, /^This part uses a feature this version cannot build yet: “pin” \(pin\)\. “pin” \(pin\) is always built, so the part cannot be previewed or downloaded here yet\./);
});

test("an evaluation that stops at such a feature gets a sentence, other errors stay as the core wrote them", () => {
  const core = "feature `label` uses `relief.text`, which this evaluator does not build yet; remove the feature, turn it off with its `if`, or evaluate with skip_unsupported for a preview without it";
  const text = buildErrorText(core, KEYCHAIN_UNSUPPORTED);
  assert.equal(
    text,
    "“label” uses relief.text, which this version cannot build yet. It is built only while !$studsOnTop is true: change the values so it is off, and the preview builds. You can still edit the values and save the part.",
  );
  assert.doesNotMatch(text, /skip_unsupported/, "no evaluator jargon");
  const detail = "feature `studs` uses `stud.field` with studType hollow, which this evaluator does not build yet; remove the feature";
  assert.match(buildErrorText(detail, []), /^“studs” uses stud\.field with studType hollow, which this version cannot build yet\. Nothing in the panel turns it off/);
  assert.equal(buildErrorText("asset `a.svg`: no asset named a.svg", KEYCHAIN_UNSUPPORTED), "asset `a.svg`: no asset named a.svg");
});
