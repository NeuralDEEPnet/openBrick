// The fit profile recorded in the playground, kept in this browser only
// (localStorage), so the tile designed next, and after a reload, uses it.
// Storage may be missing or refuse (a private window, blocked site data, a full
// quota, a sandboxed frame): every access is wrapped, and the page works
// without it, keeping the profile for the open page only. Nothing here leaves
// the browser. Pure functions over an injected storage getter, so node --test
// checks the fallbacks.

/** The storage key; the version suffix lets a later page change the format. */
export const RECORDED_FIT_KEY = "openbrick.playground.recordedFit.v1";

/** The browser's localStorage; reading the property itself can throw. */
export const browserStorage = () => globalThis.localStorage;

/**
 * The recorded fit profile kept in storage: { fit: { text, profile } | null,
 * available, problem }. `available` is false when storage cannot be read;
 * `problem` names why a stored value was ignored (not JSON, not a fit
 * profile), and the value is then removed so it is not read again.
 */
export function loadRecordedFit(getStorage = browserStorage) {
  let storage;
  let text;
  try {
    storage = getStorage();
    text = storage ? storage.getItem(RECORDED_FIT_KEY) : null;
  } catch {
    return { fit: null, available: false, problem: null };
  }
  if (!storage) return { fit: null, available: false, problem: null };
  if (text === null || text === undefined || text === "") return { fit: null, available: true, problem: null };
  let profile;
  try {
    profile = JSON.parse(text);
  } catch {
    profile = null;
  }
  if (profile?.kind !== "fit" || typeof profile.id !== "string" || typeof profile.process !== "string") {
    try {
      storage.removeItem(RECORDED_FIT_KEY);
    } catch {
      // Nothing more to do: the value is ignored either way.
    }
    return { fit: null, available: true, problem: "the stored value is not a fit profile" };
  }
  return { fit: { text, profile }, available: true, problem: null };
}

/** Keep `text` (a fit profile file) in storage. True when the browser kept it. */
export function saveRecordedFit(text, getStorage = browserStorage) {
  try {
    const storage = getStorage();
    if (!storage) return false;
    storage.setItem(RECORDED_FIT_KEY, text);
    return storage.getItem(RECORDED_FIT_KEY) === text;
  } catch {
    return false;
  }
}

/** Remove the kept profile. True when storage no longer holds one. */
export function forgetRecordedFit(getStorage = browserStorage) {
  try {
    const storage = getStorage();
    if (!storage) return true;
    storage.removeItem(RECORDED_FIT_KEY);
    return storage.getItem(RECORDED_FIT_KEY) === null;
  } catch {
    return false;
  }
}
