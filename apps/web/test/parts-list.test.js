// The parts palette's logic for hundreds of parts: search, category filter, pages, notes,
// attribution, missing-pack text and the caches. Node only, no DOM.

import assert from "node:assert/strict";
import test from "node:test";

import {
  MESH_CACHE_SIZE,
  MeshCache,
  PAGE_SIZE,
  attributionLine,
  categoriesOf,
  creditLine,
  filterParts,
  gpuPlan,
  isPackFile,
  isPackPart,
  missingPackText,
  packFileLabel,
  pageCount,
  pageHolding,
  pageOf,
  partBadge,
  partNote,
  rangeText,
  searchTokens,
} from "../src/parts-list.js";

const parts = [
  { id: "ob.brick-2x4", name: "Brick 2 x 4", category: "Brick", origin: "builtin", connectivity: "derived", joints: [] },
  { id: "ob.plate-1x2", name: "Plate 1 x 2", category: "Plate", origin: "builtin", connectivity: "derived", joints: [] },
  { id: "ld.3001", name: "Brick 2 x 4", category: "Brick", origin: "ldraw", connectivity: "derived", joints: [] },
  { id: "ld.3040b", name: "Slope Brick 45 2 x 1", category: "Slope", origin: "ldraw", connectivity: "derived", joints: [] },
  { id: "ld.4081b", name: "Plate 1 x 1 with Clip Light", category: "Plate", origin: "ldraw", connectivity: "derived", joints: ["clip"] },
  { id: "ld.9", name: "Odd thing", category: "Brick", origin: "ldraw", connectivity: "none", joints: [] },
];

test("a search is words that all have to match, and a size is read as a size", () => {
  assert.deepEqual(searchTokens("  Brick   2X4 "), ["brick", "2 x 4"]);
  assert.deepEqual(searchTokens("2 × 4"), ["2 x 4"]);
  assert.deepEqual(searchTokens("slope 45"), ["slope", "45"]);
  assert.deepEqual(filterParts(parts, { query: "brick 2x4" }), [0, 2]);
  assert.deepEqual(filterParts(parts, { query: "slope" }), [3]);
  assert.deepEqual(filterParts(parts, { query: "ld.3040" }), [3], "the id is searched too");
  assert.deepEqual(filterParts(parts, { query: "bric" }), [0, 2, 3, 5], "a word matches by its start");
  assert.deepEqual(filterParts(parts, { query: "ick" }), [], "but not from the middle");
  assert.deepEqual(filterParts(parts, { query: "clip plate" }), [4], "order of words does not matter");
  assert.deepEqual(filterParts(parts, { query: "zzz" }), []);
  assert.deepEqual(filterParts(parts, { query: "" }), [0, 1, 2, 3, 4, 5]);
});

test("the category and connectivity filters combine with the search", () => {
  assert.deepEqual(filterParts(parts, { category: "Plate" }), [1, 4]);
  assert.deepEqual(filterParts(parts, { category: "Plate", query: "clip" }), [4]);
  assert.deepEqual(filterParts(parts, { connectivity: "none" }), [5]);
  assert.deepEqual(filterParts(parts, { category: "Nope" }), []);
});

test("categories are counted and sorted", () => {
  assert.deepEqual(categoriesOf(parts), [
    { name: "Brick", count: 3 },
    { name: "Plate", count: 2 },
    { name: "Slope", count: 1 },
  ]);
  assert.deepEqual(categoriesOf([]), []);
});

test("pages", () => {
  assert.equal(pageCount(0), 1);
  assert.equal(pageCount(PAGE_SIZE), 1);
  assert.equal(pageCount(PAGE_SIZE + 1), 2);
  const list = Array.from({ length: 60 }, (_, i) => i);
  assert.deepEqual(pageOf(list, 0).items.length, PAGE_SIZE);
  const last = pageOf(list, 2);
  assert.deepEqual([last.page, last.pages, last.items.length], [2, 3, 60 - 2 * PAGE_SIZE]);
  assert.equal(pageOf(list, 99).page, 2, "clamped to the last page");
  assert.equal(pageOf(list, -3).page, 0);
  assert.equal(pageOf(list, Number.NaN).page, 0);
  assert.equal(pageOf([], 0).pages, 1);
  assert.equal(pageHolding(0), 0);
  assert.equal(pageHolding(PAGE_SIZE), 1);
  assert.equal(pageHolding(-1), 0);
});

test("the range text says what is shown", () => {
  assert.equal(rangeText(1371, 0), "Showing 1 to 24 of 1,371 parts");
  assert.equal(rangeText(30, 1), "Showing 25 to 30 of 30 parts");
  assert.equal(rangeText(1, 0), "Showing 1 to 1 of 1 part");
  assert.match(rangeText(0, 0), /No part matches/);
});

test("parts that cannot connect or have joints say so", () => {
  assert.equal(partNote(parts[0]), null);
  assert.match(partNote(parts[5]), /Free placement only/);
  assert.match(partNote(parts[4]), /clip joint is not modelled/);
  assert.equal(partBadge(parts[0]), "");
  assert.equal(partBadge(parts[5]), "ground only");
  assert.equal(partBadge(parts[4]), "joint not modelled");
});

test("the attribution names the library and keeps the credit line", () => {
  assert.equal(attributionLine(null), "");
  const line = attributionLine({ attribution: "This software uses the LDraw Parts Library (https://www.ldraw.org).", library: "2026-10", parts: 400 });
  assert.match(line, /LDraw Parts Library/);
  assert.match(line, /2026-10/);
  assert.match(line, /CC BY/);
  assert.equal(creditLine({ author: "A B", license: "Licensed under CC BY 4.0" }), "Author: A B · Licensed under CC BY 4.0");
  assert.equal(creditLine({}), "");
});

test("a model that needs a pack gets a text that says which and what to do", () => {
  assert.equal(missingPackText({ requires: [], missing: [], missingCount: 0 }, false), null);
  const needs = { requires: [{ pack: "ldraw", library: "2026-10", fingerprint: "f".repeat(64) }], missing: ["ld.3001", "ld.3003"], missingCount: 5 };
  const text = missingPackText(needs, false);
  assert.match(text, /5 parts/);
  assert.match(text, /ldraw pack \(library 2026-10\)/);
  assert.match(text, /Add parts from an LDraw library/);
  assert.match(missingPackText(needs, true), /does not have/);
  assert.match(missingPackText({ requires: [], missing: ["ld.1"], missingCount: 1 }, false), /1 part from an LDraw parts pack/);
});

test("pack files and part ids", () => {
  assert.ok(isPackFile("ldraw.obpack") && isPackFile("X.OBPACK"));
  assert.ok(!isPackFile("model.obrick.json") && !isPackFile(""));
  assert.equal(packFileLabel("ldraw.obpack"), "ldraw");
  assert.ok(isPackPart("ld.3001") && !isPackPart("ob.brick-2x4"));
});

test("the mesh cache drops the oldest spare entries and never the ones in use", () => {
  const cache = new MeshCache(3);
  const used = new Set([1]);
  const keep = (k) => used.has(k);
  for (let i = 1; i <= 8; i++) cache.set(i, { i }, keep);
  assert.ok(cache.has(1), "an entry in use stays");
  assert.deepEqual(cache.keys().filter((k) => k !== 1), [6, 7, 8], "the three newest spare ones remain");
  cache.get(6);
  cache.set(9, { i: 9 }, keep);
  assert.ok(cache.has(6) && !cache.has(7), "a read refreshes an entry");
  assert.equal(new MeshCache().size, 0);
  assert.equal(MESH_CACHE_SIZE > 0, true);
  cache.clear();
  assert.equal(cache.size, 0);
});

test("the GPU plan keeps placed parts and the ghost, and drops the oldest spare ones", () => {
  const held = Array.from({ length: 50 }, (_, i) => i);
  const { need, drop } = gpuPlan({ placed: [0, 1, 2], ghost: 3, held, spare: 10 });
  assert.deepEqual([...need], [0, 1, 2, 3]);
  // 50 held, 4 needed: 46 spare, ten allowed, so the 36 oldest spare ones go.
  assert.equal(drop.length, 36);
  assert.deepEqual(drop.slice(0, 3), [4, 5, 6]);
  assert.ok(!drop.includes(0) && !drop.includes(3));
  assert.deepEqual(gpuPlan({ placed: [], held: [1, 2], spare: 5 }).drop, []);
});
