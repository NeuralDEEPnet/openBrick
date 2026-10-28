import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// The core is built by `cargo xtask wasm` (or `pnpm --dir apps/web wasm`) into
// src/wasm/, which is generated and gitignored. Say so plainly instead of failing
// on an unresolved import.
const glue = fileURLToPath(new URL("./src/wasm/openbrick_wasm.js", import.meta.url));
const requireCore = {
  name: "openbrick-require-core",
  buildStart() {
    if (!existsSync(glue)) {
      throw new Error("apps/web/src/wasm/ is missing: build the core first with `pnpm --dir apps/web wasm` (runs `cargo xtask wasm`)");
    }
  },
};

// HTTPS is optional. Browsers expose WebGPU only in secure contexts, and
// http://<lan-ip> is not one, so a phone on the LAN gets the WebGL2 backend unless
// both variables point at a certificate and key (for example made with mkcert);
// see README.md.
const cert = process.env.WEB_TLS_CERT;
const key = process.env.WEB_TLS_KEY;
const https = cert && key ? { cert: readFileSync(cert), key: readFileSync(key) } : undefined;

export default defineConfig({
  // Relative asset URLs so dist/ works from any path or static host.
  base: "./",
  plugins: [requireCore],
  worker: { format: "es" },
  build: {
    target: "es2022",
    // three.js with the WebGPU and WebGL2 backends is about 0.8 MB minified.
    chunkSizeWarningLimit: 2048,
  },
  server: { https },
  preview: { https },
});
