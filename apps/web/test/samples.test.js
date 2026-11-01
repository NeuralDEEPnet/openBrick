// The artwork samples (src/samples.js): which drawing the page sends for a
// tile and which tile a sample asks for. The fixtures have the shape of the
// core's artwork_samples_json() (crates/openbrick-wasm/src/data.rs);
// test/core.test.js checks the real bundle when the core is built.

import assert from "node:assert/strict";
import { test } from "node:test";
import { fitsShape, sampleFile, sampleLabel, sizeForSample, sizeNote, tooSmall } from "../src/samples.js";

const MARK = {
  id: "neuraldeep-wordmark",
  title: "the NEURALDEEP.NET wordmark",
  shape: "portrait",
  size: [2, 6],
  files: [
    { asset: "assets/brand/neuraldeep-wordmark-2x6.svg", tile: [2, 6], svg: "<svg/>" },
    { asset: "assets/brand/neuraldeep-wordmark-2x8.svg", tile: [2, 8], svg: "<svg/>" },
    { asset: "assets/brand/neuraldeep-wordmark-2x10.svg", tile: [2, 10], svg: "<svg/>" },
  ],
};
const BADGE = {
  id: "neuraldeep-square",
  title: "the square NEURALDEEP.NET wordmark",
  shape: "square",
  size: [4, 4],
  files: [{ asset: "assets/brand/neuraldeep-wordmark-4x4.svg", tile: [4, 4], svg: "<svg/>" }],
};
const CC0 = { id: "sample", title: "the sample mark (CC0)", shape: null, size: null, files: [{ asset: "assets/mark.svg", tile: null, svg: "<svg/>" }] };

const asset = (sample, size) => sampleFile(sample, size).asset.split("/").pop();

test("a sample sends the drawing made for the largest tile that fits", () => {
  assert.equal(asset(MARK, [2, 6]), "neuraldeep-wordmark-2x6.svg");
  assert.equal(asset(MARK, [3, 7]), "neuraldeep-wordmark-2x6.svg");
  assert.equal(asset(MARK, [4, 6]), "neuraldeep-wordmark-2x6.svg");
  assert.equal(asset(MARK, [2, 8]), "neuraldeep-wordmark-2x8.svg");
  assert.equal(asset(MARK, [4, 9]), "neuraldeep-wordmark-2x8.svg");
  assert.equal(asset(MARK, [2, 10]), "neuraldeep-wordmark-2x10.svg");
  assert.equal(asset(MARK, [8, 16]), "neuraldeep-wordmark-2x10.svg");
  // The drawing is never turned: a wide tile gets a drawing that fits.
  assert.equal(asset(MARK, [8, 6]), "neuraldeep-wordmark-2x6.svg");
  // Nothing fits a 2x4 or a 1x1: the smallest drawing (the validator then
  // says what does not print).
  assert.equal(asset(MARK, [2, 4]), "neuraldeep-wordmark-2x6.svg");
  assert.equal(asset(MARK, [1, 1]), "neuraldeep-wordmark-2x6.svg");
  assert.equal(asset(BADGE, [2, 2]), "neuraldeep-wordmark-4x4.svg");
  assert.equal(asset(BADGE, [4, 4]), "neuraldeep-wordmark-4x4.svg");
  assert.equal(asset(BADGE, [6, 6]), "neuraldeep-wordmark-4x4.svg");
  assert.equal(asset(CC0, [1, 1]), "mark.svg");
  assert.equal(asset(CC0, [16, 16]), "mark.svg");
  assert.throws(() => sampleFile({ id: "x", files: [] }, [2, 4]), /no drawing/);
});

test("choosing a sample on a tile of another shape, or a smaller one, sets the sample's size", () => {
  assert.equal(fitsShape("square", [4, 4]), true);
  assert.equal(fitsShape("square", [2, 4]), false);
  assert.equal(fitsShape("portrait", [2, 4]), true);
  assert.equal(fitsShape("portrait", [4, 4]), false);
  assert.equal(fitsShape("portrait", [4, 2]), false);
  assert.equal(fitsShape(null, [7, 3]), true);
  assert.equal(tooSmall(MARK, [2, 4]), true);
  assert.equal(tooSmall(MARK, [2, 6]), false);
  assert.equal(tooSmall(CC0, [1, 1]), false);
  // The square wordmark on a non-square tile, or a square smaller than 4x4,
  // asks for 4x4; on a larger square it stays.
  assert.deepEqual(sizeForSample(BADGE, [2, 4]), [4, 4]);
  assert.deepEqual(sizeForSample(BADGE, [6, 4]), [4, 4]);
  assert.deepEqual(sizeForSample(BADGE, [2, 2]), [4, 4]);
  assert.equal(sizeForSample(BADGE, [4, 4]), null);
  assert.equal(sizeForSample(BADGE, [8, 8]), null);
  // The wordmark on a square or wide tile, or on 2x4 (the logo tile's
  // default, where the whole text does not print), asks for 2x6.
  assert.deepEqual(sizeForSample(MARK, [4, 4]), [2, 6]);
  assert.deepEqual(sizeForSample(MARK, [6, 4]), [2, 6]);
  assert.deepEqual(sizeForSample(MARK, [2, 4]), [2, 6]);
  assert.deepEqual(sizeForSample(MARK, [1, 3]), [2, 6]);
  assert.equal(sizeForSample(MARK, [2, 6]), null);
  assert.equal(sizeForSample(MARK, [4, 6]), null);
  // The CC0 sample takes any tile.
  assert.equal(sizeForSample(CC0, [3, 3]), null);
  // A copy, so the caller may change it.
  const size = sizeForSample(BADGE, [2, 4]);
  size[0] = 9;
  assert.deepEqual(BADGE.size, [4, 4]);
});

test("labels and notes name the sample, its drawing and the new size", () => {
  assert.equal(sampleLabel(MARK, MARK.files[1]), "the NEURALDEEP.NET wordmark (drawn for 2x8)");
  assert.equal(sampleLabel(CC0, CC0.files[0]), "the sample mark (CC0)");
  assert.equal(sizeNote(BADGE, [4, 4], [2, 6]), "The square NEURALDEEP.NET wordmark is drawn for square tiles, so the size is now 4 × 4.");
  assert.equal(sizeNote(MARK, [2, 6], [4, 4]), "The NEURALDEEP.NET wordmark is drawn for tiles longer than they are wide, so the size is now 2 × 6.");
  assert.equal(sizeNote(MARK, [2, 6], [2, 4]), "The NEURALDEEP.NET wordmark needs a tile of at least 2 × 6 to print, so the size is now 2 × 6.");
  assert.equal(sizeNote(BADGE, [4, 4], [2, 2]), "The square NEURALDEEP.NET wordmark needs a tile of at least 4 × 4 to print, so the size is now 4 × 4.");
});
