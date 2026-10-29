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

// The non-affiliation disclaimer has one source, crates/openbrick-units/
// disclaimer.txt, which the CLI and the core also read (docs/plan/12 section 4).
// It goes into the page's HTML when the page is served or built, so the footer
// shows it even before (or without) JavaScript.
const disclaimerFile = fileURLToPath(new URL("../../crates/openbrick-units/disclaimer.txt", import.meta.url));
const escapeHtml = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const disclaimer = {
  name: "openbrick-disclaimer",
  transformIndexHtml(html) {
    const text = escapeHtml(readFileSync(disclaimerFile, "utf8").trim());
    if (!html.includes("<!--openbrick:disclaimer-->")) throw new Error("index.html lost its disclaimer placeholder");
    return html.replace("<!--openbrick:disclaimer-->", text);
  },
};

// HTTPS is optional. Browsers expose WebGPU only in secure contexts, and
// http://<lan-ip> is not one, so a phone on the LAN gets the WebGL2 backend unless
// both variables point at a certificate and key (for example made with mkcert);
// see README.md.
const cert = process.env.WEB_TLS_CERT;
const key = process.env.WEB_TLS_KEY;
const https = cert && key ? { cert: readFileSync(cert), key: readFileSync(key) } : undefined;

// The path the app is served under: "/" locally, "/openBrick/" on GitHub Pages
// (https://neuraldeepnet.github.io/openBrick/). The page, the worker, the
// WebAssembly module and every asset resolve under it.
const base = process.env.OPENBRICK_BASE || "/";
if (!base.startsWith("/") || !base.endsWith("/")) {
  throw new Error(`OPENBRICK_BASE must start and end with "/", got ${JSON.stringify(base)}`);
}

export default defineConfig({
  base,
  plugins: [requireCore, disclaimer],
  worker: { format: "es" },
  build: {
    target: "es2022",
    // three.js with the WebGPU and WebGL2 backends is about 0.8 MB minified.
    chunkSizeWarningLimit: 2048,
  },
  server: { https },
  preview: { https },
});
