// The artwork samples (src/samples.js): which drawing the page sends for a
// tile and which tile a sample asks for. The fixtures have the shape of the
// core's artwork_samples_json() (crates/openbrick-wasm/src/data.rs);
// test/core.test.js checks the real bundle when the core is built.

import assert from "node:assert/strict";
import { test } from "node:test";
import { fitsShape, sampleFile, sampleLabel, sizeForSample, sizeNote } from "../src/samples.js";

const MARK = {
  id: "neuraldeep-mark",
  title: "the Neuraldeep.net mark",
  shape: "portrait",
  size: [2, 4],
  files: [
    { asset: "assets/brand/neuraldeep-mark-2x4.svg", tile: [2, 4], svg: "<svg/>" },
    { asset: "assets/brand/neuraldeep-mark-4x6.svg", tile: [4, 6], svg: "<svg/>" },
  ],
};
const BADGE = {
  id: "neuraldeep-badge",
  title: "the Neuraldeep.net badge",
  shape: "square",
  size: [4, 4],
  files: [
    { asset: "assets/brand/neuraldeep-badge-2x2.svg", tile: [2, 2], svg: "<svg/>" },
    { asset: "assets/brand/neuraldeep-badge-4x4.svg", tile: [4, 4], svg: "<svg/>" },
  ],
};
const CC0 = { id: "sample", title: "the sample mark (CC0)", shape: null, size: null, files: [{ asset: "assets/mark.svg", tile: null, svg: "<svg/>" }] };

const asset = (sample, size) => sampleFile(sample, size).asset.split("/").pop();

test("a sample sends the drawing made for the largest tile that fits", () => {
  assert.equal(asset(MARK, [2, 4]), "neuraldeep-mark-2x4.svg");
  assert.equal(asset(MARK, [3, 5]), "neuraldeep-mark-2x4.svg");
  assert.equal(asset(MARK, [4, 6]), "neuraldeep-mark-4x6.svg");
  assert.equal(asset(MARK, [8, 16]), "neuraldeep-mark-4x6.svg");
  // The drawing is never turned: a wide tile gets the 2x4 drawing, which fits.
  assert.equal(asset(MARK, [6, 4]), "neuraldeep-mark-2x4.svg");
  // Nothing fits a 1x1: the smallest drawing (the validator then says what
  // does not print).
  assert.equal(asset(MARK, [1, 1]), "neuraldeep-mark-2x4.svg");
  assert.equal(asset(BADGE, [2, 2]), "neuraldeep-badge-2x2.svg");
  assert.equal(asset(BADGE, [3, 3]), "neuraldeep-badge-2x2.svg");
  assert.equal(asset(BADGE, [4, 4]), "neuraldeep-badge-4x4.svg");
  assert.equal(asset(BADGE, [6, 6]), "neuraldeep-badge-4x4.svg");
  assert.equal(asset(CC0, [1, 1]), "mark.svg");
  assert.equal(asset(CC0, [16, 16]), "mark.svg");
  assert.throws(() => sampleFile({ id: "x", files: [] }, [2, 4]), /no drawing/);
});

test("choosing a sample on a tile of another shape sets the sample's size", () => {
  assert.equal(fitsShape("square", [4, 4]), true);
  assert.equal(fitsShape("square", [2, 4]), false);
  assert.equal(fitsShape("portrait", [2, 4]), true);
  assert.equal(fitsShape("portrait", [4, 4]), false);
  assert.equal(fitsShape("portrait", [4, 2]), false);
  assert.equal(fitsShape(null, [7, 3]), true);
  // The badge on a non-square tile asks for 4x4; on any square it stays.
  assert.deepEqual(sizeForSample(BADGE, [2, 4]), [4, 4]);
  assert.deepEqual(sizeForSample(BADGE, [6, 4]), [4, 4]);
  assert.equal(sizeForSample(BADGE, [2, 2]), null);
  assert.equal(sizeForSample(BADGE, [8, 8]), null);
  // The portrait mark on a square (or wide) tile asks for 2x4.
  assert.deepEqual(sizeForSample(MARK, [4, 4]), [2, 4]);
  assert.deepEqual(sizeForSample(MARK, [6, 4]), [2, 4]);
  assert.equal(sizeForSample(MARK, [4, 6]), null);
  assert.equal(sizeForSample(MARK, [1, 3]), null);
  // The CC0 sample takes any tile.
  assert.equal(sizeForSample(CC0, [3, 3]), null);
  // A copy, so the caller may change it.
  const size = sizeForSample(BADGE, [2, 4]);
  size[0] = 9;
  assert.deepEqual(BADGE.size, [4, 4]);
});

test("labels and notes name the sample, its drawing and the new size", () => {
  assert.equal(sampleLabel(MARK, MARK.files[1]), "the Neuraldeep.net mark (drawn for 4x6)");
  assert.equal(sampleLabel(CC0, CC0.files[0]), "the sample mark (CC0)");
  assert.equal(sizeNote(BADGE, [4, 4]), "The Neuraldeep.net badge is drawn for square tiles, so the size is now 4 × 4.");
  assert.equal(sizeNote(MARK, [2, 4]), "The Neuraldeep.net mark is drawn for tiles longer than they are wide, so the size is now 2 × 4.");
});
