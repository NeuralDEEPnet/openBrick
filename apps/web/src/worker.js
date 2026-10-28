// The core worker (docs/plan/02 section 3): loads the WebAssembly build of
// crates/openbrick-wasm and answers requests from the page with src/protocol.js.
// Geometry is computed here, by the Rust core, and nowhere else in JavaScript.
//
// The page cancels a long build by terminating this worker and starting another
// (src/client.js), because a running WebAssembly call cannot be interrupted.

import init, * as core from "./wasm/openbrick_wasm.js";
import { dispatch, errorMessage, info } from "./protocol.js";

/** Resolves once the core is instantiated; set by the first init request. */
let loaded = null;

self.onmessage = async ({ data: request }) => {
  if (request?.type === "init") {
    try {
      // The page passes the module it compiled once; without it the glue fetches
      // openbrick_wasm_bg.wasm next to itself.
      loaded ??= init(request.module ? { module_or_path: request.module } : undefined);
      await loaded;
      self.postMessage({ id: request.id, ok: true, result: info(core) });
    } catch (err) {
      loaded = null;
      self.postMessage({ id: request.id, ok: false, error: `could not load the core: ${errorMessage(err)}` });
    }
    return;
  }
  if (!loaded) {
    self.postMessage({ id: request?.id, ok: false, error: "the core is not loaded yet (send init first)" });
    return;
  }
  try {
    await loaded;
  } catch {
    self.postMessage({ id: request?.id, ok: false, error: "the core failed to load" });
    return;
  }
  const { reply, transfer } = dispatch(core, request);
  self.postMessage(reply, transfer);
};
