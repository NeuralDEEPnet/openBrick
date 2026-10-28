// The core worker (docs/plan/02 section 3): loads the WebAssembly build of
// crates/openbrick-wasm and answers requests from the page with src/protocol.js.
// Geometry is computed here, by the Rust core, and nowhere else in JavaScript.
//
// The page cancels a long build by terminating this worker and starting another
// (src/client.js), because a running WebAssembly call cannot be interrupted. It
// does the same after a trap, which createHandler() reports as a fatal reply.

import init, * as core from "./wasm/openbrick_wasm.js";
import { createHandler } from "./protocol.js";

const handle = createHandler(
  async (module) => {
    // The page passes the module it compiled once; without it the glue fetches
    // openbrick_wasm_bg.wasm next to itself.
    await init(module ? { module_or_path: module } : undefined);
    return core;
  },
  (reply, transfer) => self.postMessage(reply, transfer),
);

self.onmessage = ({ data }) => handle(data);
