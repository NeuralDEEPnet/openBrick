// The desktop app's configuration keeps its promises (README.md, docs/plan/09):
// a strict Content Security Policy with no remote origin, the least
// permissions (the app's three file commands and nothing else), the window
// the layout needs, the pinned Tauri versions, the disclaimer in the package
// description, and the icon set the bundle names. Pure file checks: no build,
// no webview; `pnpm test` runs them everywhere.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(dirname(fileURLToPath(import.meta.url)));
const repo = join(here, "..", "..");
const read = (...p) => readFileSync(join(...p), "utf8");
const conf = JSON.parse(read(here, "src-tauri", "tauri.conf.json"));
const capability = JSON.parse(read(here, "src-tauri", "capabilities", "main.json"));
const cargo = read(here, "src-tauri", "Cargo.toml");
const buildRs = read(here, "src-tauri", "build.rs");
const appRs = read(here, "src-tauri", "src", "app.rs");
const pkg = JSON.parse(read(here, "package.json"));

/** The app's commands, as build.rs declares them to tauri-build. */
const declared = [...buildRs.match(/commands\(&\[([^\]]*)\]\)/)[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);

test("the Content Security Policy is strict and names no remote origin", () => {
  const csp = conf.app.security.csp;
  assert.equal(typeof csp, "object");
  assert.equal(csp["default-src"], "'self'");
  assert.equal(csp["script-src"], "'self' 'wasm-unsafe-eval'", "WebAssembly needs wasm-unsafe-eval and nothing more");
  assert.equal(csp["style-src"], "'self'", "Tauri adds the hash of the page's <style>; no unsafe-inline");
  assert.equal(csp["object-src"], "'none'");
  for (const [directive, sources] of Object.entries(csp)) {
    for (const source of sources.split(/\s+/)) {
      assert.ok(!["'unsafe-inline'", "'unsafe-eval'", "'unsafe-hashes'"].includes(source) && !source.includes("*"), `${directive} allows ${source}`);
      // http://ipc.localhost is Tauri's own IPC origin on Windows, inside the app.
      if (/^https?:/.test(source)) assert.equal(source, "http://ipc.localhost", `${directive} names ${source}`);
    }
  }
  assert.equal(conf.app.security.dangerousDisableAssetCspModification, undefined);
  assert.equal(conf.app.security.devCsp, undefined, "dev uses the same policy (or none, from the Vite dev server)");
});

test("the webview may call only the app's three file commands", () => {
  assert.deepEqual(conf.app.security.capabilities, ["main"]);
  assert.equal(capability.identifier, "main");
  assert.deepEqual(capability.windows, ["main"]);
  assert.equal(capability.remote, undefined, "no remote origin gets any permission");
  assert.deepEqual(capability.permissions, declared.map((c) => `allow-${c.replaceAll("_", "-")}`));
  for (const p of capability.permissions) assert.doesNotMatch(p, /:/, `${p} is a plugin permission`);
  // The same three are the commands app.rs registers.
  const handler = appRs.match(/generate_handler!\[([^\]]*)\]/)[1].split(",").map((s) => s.trim()).filter(Boolean);
  assert.deepEqual(handler, declared);
  assert.deepEqual(declared, ["pick_file", "read_picked_file", "save_file"]);
});

test("the window: titled OpenBrick3D, created by the app, big enough for the sidebar layout", () => {
  const [main, ...others] = conf.app.windows;
  assert.equal(others.length, 0);
  assert.equal(main.label, "main");
  assert.equal(main.title, "OpenBrick3D");
  assert.equal(main.create, false, "app.rs builds it, to attach the navigation guard");
  // apps/web/index.html switches to the sidebar layout at 900 px.
  assert.ok(main.minWidth >= 900, `minWidth ${main.minWidth}`);
  assert.ok(main.minHeight >= 600, `minHeight ${main.minHeight}`);
  assert.equal(main.dragDropEnabled, false, "SVG files dropped on the page reach the page's own drop zone");
  assert.equal(conf.app.withGlobalTauri, true, "the adapter in apps/web/src/files.js looks for window.__TAURI__");
  assert.equal(conf.productName, "OpenBrick3D");
});

test("the build runs the playground's own frontend with the app feature", () => {
  assert.deepEqual(conf.build.features, ["app"]);
  assert.equal(conf.build.frontendDist, "../dist");
  assert.equal(conf.build.beforeBuildCommand, "pnpm run frontend");
  assert.equal(conf.build.beforeDevCommand, "pnpm run frontend:dev");
  const port = read(here, "scripts", "frontend.mjs").match(/const DEV_PORT = (\d+);/)[1];
  assert.equal(conf.build.devUrl, `http://localhost:${port}`);
  assert.match(cargo, /^app = \[[^\]]*"dep:tauri"/m);
  assert.match(cargo, /required-features = \["app"\]/);
});

test("Tauri is pinned exactly, and the CLI too", () => {
  for (const dep of ["tauri", "tauri-build", "tauri-plugin-dialog", "opener"]) {
    assert.match(cargo, new RegExp(`^${dep} = \\{ version = "=\\d+\\.\\d+\\.\\d+"`, "m"), `${dep} is not pinned exactly`);
  }
  assert.match(pkg.devDependencies["@tauri-apps/cli"], /^\d+\.\d+\.\d+$/);
  // The CLI and the tauri crate share the minor version (2.12).
  const crate = cargo.match(/^tauri = \{ version = "=(\d+\.\d+)\./m)[1];
  assert.ok(pkg.devDependencies["@tauri-apps/cli"].startsWith(`${crate}.`));
});

test("the package description carries the disclaimer word for word", () => {
  const disclaimer = read(repo, "crates", "openbrick-units", "disclaimer.txt").trim();
  assert.ok(conf.bundle.longDescription.endsWith(disclaimer));
  assert.equal(conf.bundle.license, "MIT");
  assert.ok(existsSync(join(here, "src-tauri", conf.bundle.licenseFile)));
});

test("Linux packages are called openbrick3d and their menu entry OpenBrick3D", () => {
  // The Debian package name is the product name in kebab case, which would
  // make "open-brick3-d"; the Linux override names the packages openbrick3d,
  // and the desktop entry template keeps the menu name.
  const linux = JSON.parse(read(here, "src-tauri", "tauri.linux.conf.json"));
  assert.equal(linux.productName, "openbrick3d");
  for (const kind of ["deb", "rpm"]) {
    const template = read(here, "src-tauri", linux.bundle.linux[kind].desktopTemplate);
    assert.match(template, /^Name=OpenBrick3D$/m);
    assert.match(template, /^Exec=\{\{exec\}\}$/m);
  }
  assert.deepEqual(Object.keys(linux).sort(), ["$schema", "bundle", "productName"], "the override changes nothing else");
});

test("every icon the bundle names exists, and so does its source", () => {
  for (const icon of conf.bundle.icon) assert.ok(existsSync(join(here, "src-tauri", icon)), icon);
  assert.ok(existsSync(join(here, "src-tauri", "icons", "icon.png")), "the Linux window icon");
  const svg = read(here, "icon.svg");
  assert.doesNotMatch(svg, /<text|<image|href=/, "the icon is plain shapes: no text, no embedded images");
});

test("the app carries the workspace version, and macOS bundles are ad-hoc signed only", () => {
  // No version of its own: the Tauri CLI then reads the crate's, which is the
  // workspace's (scripts/desktop-version.py checks the same in CI).
  assert.equal(conf.version, undefined, "tauri.conf.json must not set a version");
  assert.match(cargo, /^version\.workspace = true$/m);
  const workspace = read(repo, "Cargo.toml").match(/^\[workspace\.package\]\nversion = "([^"]+)"$/m)[1];
  assert.equal(pkg.version, workspace, "apps/desktop/package.json repeats the workspace version");
  // "-" is Tauri's ad-hoc signature (no identity, no notarisation): Apple
  // silicon runs no unsigned program. Real signing comes in M4.
  assert.equal(conf.bundle.macOS.signingIdentity, "-");
  for (const key of ["providerShortName", "entitlements"]) assert.equal(conf.bundle.macOS[key], undefined, key);
});

test("the release workflow builds no AppImage and uploads only from tags", () => {
  const wf = read(repo, ".github", "workflows", "desktop-release.yml");
  const bundles = [...wf.matchAll(/^\s+bundles: (\S+)$/gm)].map((m) => m[1]);
  assert.deepEqual(bundles, ["deb,rpm", "nsis,msi", "app,dmg"]);
  assert.match(wf, /if: github\.event_name == 'push' && startsWith\(github\.ref, 'refs\/tags\/v'\)/);
  assert.equal(conf.bundle.targets, "all", "the workflow picks the bundles; local builds may make all of them");
});

test("release bundles carry the licence notices, and the Linux packages declare their glibc", () => {
  // .github/workflows/desktop-release.yml builds with --config
  // src-tauri/tauri.release.conf.json after scripts/third-party-licenses.py
  // --desktop has written target/desktop-notices/THIRD-PARTY-LICENSES.txt.
  const release = JSON.parse(read(here, "src-tauri", "tauri.release.conf.json"));
  assert.deepEqual(Object.keys(release), ["$schema", "bundle"]);
  assert.deepEqual(Object.values(release.bundle.resources).sort(), ["LICENSE", "THIRD-PARTY-LICENSES.txt"]);
  assert.equal(conf.bundle.resources, undefined, "local builds need no generated file");
  // The binary built on ubuntu-24.04 needs glibc 2.39; deb and rpm say so, so
  // older systems refuse the package instead of the app failing at its start.
  const linux = JSON.parse(read(here, "src-tauri", "tauri.linux.conf.json")).bundle.linux;
  const deb = linux.deb.depends.map((d) => d.match(/^libc6 \(>= ([\d.]+)\)$/)?.[1]).filter(Boolean);
  const rpm = linux.rpm.depends.map((d) => d.match(/^libc\.so\.6\(GLIBC_([\d.]+)\)\(64bit\)$/)?.[1]).filter(Boolean);
  assert.equal(deb.length, 1);
  assert.deepEqual(rpm, deb, "deb and rpm declare the same glibc");
  const wf = read(repo, ".github", "workflows", "desktop-release.yml");
  assert.match(wf, /third-party-licenses\.py --desktop target\/desktop-notices\/THIRD-PARTY-LICENSES\.txt/);
  assert.match(wf, /--config src-tauri\/tauri\.release\.conf\.json/);
  assert.match(wf, /os: ubuntu-24\.04\n/, "glibc 2.39 is Ubuntu 24.04's; another build OS needs another version");
});
