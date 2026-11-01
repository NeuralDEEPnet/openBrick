// The desktop app's frontend is the playground in apps/web, unchanged: the same
// source and the same Vite configuration. This script builds it into
// apps/desktop/dist/ (tauri.conf.json `frontendDist`) or serves it for
// `tauri dev`, always under the base path "/", whatever OPENBRICK_BASE says in
// the environment (the GitHub Pages build sets "/openBrick/"). It is a Node
// script rather than a shell line so that it runs the same on Windows.
//
//   node scripts/frontend.mjs build   # tauri build: vite build into ../dist
//   node scripts/frontend.mjs dev     # tauri dev: vite on http://localhost:1420

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(dirname(fileURLToPath(import.meta.url)));
const web = join(here, "..", "web");
const dist = join(here, "dist");
/** The dev server port; tauri.conf.json `devUrl` names the same one. */
const DEV_PORT = 1420;

const mode = process.argv[2];
if (mode !== "build" && mode !== "dev") {
  console.error("usage: node scripts/frontend.mjs build|dev");
  process.exit(2);
}
if (!existsSync(join(web, "src", "wasm", "openbrick_wasm.js"))) {
  console.error("apps/web/src/wasm/ is missing: build the core first with `pnpm --dir apps/desktop wasm` (runs `cargo xtask wasm`)");
  process.exit(1);
}

const args =
  mode === "build"
    ? ["exec", "vite", "build", "--outDir", relative(web, dist), "--emptyOutDir"]
    : ["exec", "vite", "--port", String(DEV_PORT), "--strictPort"];
const child = spawn("pnpm", args, {
  cwd: web,
  env: { ...process.env, OPENBRICK_BASE: "/" },
  stdio: "inherit",
  // pnpm is a .cmd script on Windows, which only a shell runs.
  shell: process.platform === "win32",
});
child.on("exit", (code, signal) => process.exit(signal ? 1 : (code ?? 1)));
