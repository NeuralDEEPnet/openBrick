import { readFileSync } from "node:fs";
import { defineConfig } from "vite";

// HTTPS is optional and only needed to test the WebGPU backend from another device:
// browsers expose WebGPU in secure contexts only, and http://<lan-ip> is not one.
// Point both variables at a certificate and key (for example made with mkcert);
// see bench/devices/README.md.
const cert = process.env.HARNESS_TLS_CERT;
const key = process.env.HARNESS_TLS_KEY;
const https = cert && key ? { cert: readFileSync(cert), key: readFileSync(key) } : undefined;

export default defineConfig({
  // Relative asset URLs so dist/ works from any path or static host.
  base: "./",
  build: {
    target: "es2022",
    // three.js with the WebGPU and WebGL2 backends is about 0.8 MB minified.
    chunkSizeWarningLimit: 2048,
  },
  server: { https },
  preview: { https },
});
