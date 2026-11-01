// The recorded fit profile kept in this browser (src/storage.js): it round
// trips, a stored value that is not a fit profile is ignored and removed, and
// storage that is missing or refuses never throws into the page.

import assert from "node:assert/strict";
import { test } from "node:test";
import { forgetRecordedFit, loadPrinter, loadRecordedFit, PRINTER_KEY, RECORDED_FIT_KEY, saveRecordedFit, savePrinter } from "../src/storage.js";

/** A Map-backed Storage; `fail` makes the named methods throw like a browser does. */
function fakeStorage(fail = []) {
  const items = new Map();
  const guard = (name) => {
    if (fail.includes(name)) throw new DOMException(`${name} refused`, name === "setItem" ? "QuotaExceededError" : "SecurityError");
  };
  return {
    items,
    getItem(k) {
      guard("getItem");
      return items.has(k) ? items.get(k) : null;
    },
    setItem(k, v) {
      guard("setItem");
      items.set(k, String(v));
    },
    removeItem(k) {
      guard("removeItem");
      items.delete(k);
    },
  };
}

const profile = { openbrick: "1.1", kind: "fit", id: "prusa-mini-pla-fdm-0.4", process: "fdm-0.4", printer: "Prusa MINI+" };
const text = `${JSON.stringify(profile, null, 2)}\n`;

test("a recorded profile is kept, read back byte for byte and forgotten", () => {
  const s = fakeStorage();
  assert.deepEqual(loadRecordedFit(() => s), { fit: null, available: true, problem: null });
  assert.equal(saveRecordedFit(text, () => s), true);
  assert.equal(s.items.get(RECORDED_FIT_KEY), text);
  const back = loadRecordedFit(() => s);
  assert.equal(back.fit.text, text);
  assert.deepEqual(back.fit.profile, profile);
  assert.equal(forgetRecordedFit(() => s), true);
  assert.equal(s.items.size, 0);
  assert.equal(loadRecordedFit(() => s).fit, null);
});

test("a stored value that is not a fit profile is ignored and removed", () => {
  for (const bad of ["{", "null", '{"kind":"process","id":"fdm-0.4","process":"x"}', '{"kind":"fit"}', '{"kind":"fit","id":3,"process":"fdm-0.4"}']) {
    const s = fakeStorage();
    s.items.set(RECORDED_FIT_KEY, bad);
    const r = loadRecordedFit(() => s);
    assert.equal(r.fit, null, bad);
    assert.equal(r.available, true);
    assert.match(r.problem, /not a fit profile/);
    assert.equal(s.items.has(RECORDED_FIT_KEY), false, `${bad} was not removed`);
  }
  // Even when it cannot be removed, it is ignored without an exception.
  const stuck = fakeStorage(["removeItem"]);
  stuck.items.set(RECORDED_FIT_KEY, "{");
  assert.equal(loadRecordedFit(() => stuck).fit, null);
});

test("without storage the page goes on: nothing throws, nothing is kept", () => {
  const throwing = () => {
    throw new DOMException("The operation is insecure.", "SecurityError");
  };
  for (const getStorage of [throwing, () => null, () => undefined]) {
    assert.deepEqual(loadRecordedFit(getStorage), { fit: null, available: false, problem: null });
    assert.equal(saveRecordedFit(text, getStorage), false);
  }
  assert.equal(forgetRecordedFit(throwing), false);
  assert.equal(forgetRecordedFit(() => null), true);
  // Reading refused, writing refused (a full quota), removing refused.
  assert.equal(loadRecordedFit(() => fakeStorage(["getItem"])).available, false);
  assert.equal(saveRecordedFit(text, () => fakeStorage(["setItem"])), false);
  assert.equal(forgetRecordedFit(() => fakeStorage(["removeItem"])), false);
});

test("in Node, without a localStorage of its own, the defaults do not throw", () => {
  // Node 22 has no localStorage unless --localstorage-file is given: the
  // browser getter then yields undefined or throws, and both are handled.
  assert.doesNotThrow(() => loadRecordedFit());
  assert.doesNotThrow(() => saveRecordedFit(text));
  assert.doesNotThrow(() => forgetRecordedFit());
});

test("the printer chosen last is kept, and only a bundled one is read back", () => {
  const ids = ["bambu-a1", "prusa-mini-plus"];
  const s = fakeStorage();
  assert.equal(loadPrinter(ids, () => s), "");
  assert.equal(savePrinter("prusa-mini-plus", () => s), true);
  assert.equal(s.items.get(PRINTER_KEY), "prusa-mini-plus");
  assert.equal(loadPrinter(ids, () => s), "prusa-mini-plus");
  // A printer this build does not bundle (a newer page wrote it) is ignored.
  s.items.set(PRINTER_KEY, "ender-3");
  assert.equal(loadPrinter(ids, () => s), "");
  // None removes the choice.
  assert.equal(savePrinter("", () => s), true);
  assert.equal(s.items.has(PRINTER_KEY), false);
  // Refusing storage never throws into the page.
  assert.equal(loadPrinter(ids, () => fakeStorage(["getItem"])), "");
  assert.equal(savePrinter("bambu-a1", () => fakeStorage(["setItem"])), false);
  assert.equal(savePrinter("bambu-a1", () => undefined), false);
});
