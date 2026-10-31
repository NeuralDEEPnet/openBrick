# Desktop app (alpha)

The M2 desktop surface ([09 sections 1 to 4](../../docs/plan/09-platforms-and-distribution.md#1-support-matrix), decision D13 in [03](../../docs/plan/03-tech-stack-decisions.md)): the browser playground ([`apps/web`](../web/README.md)) in a Tauri 2 window on Linux, macOS and Windows, with native open and save dialogs instead of browser downloads and file inputs.

It is the same page, not a port. Through M2 the desktop app computes nothing natively ([02 section 3](../../docs/plan/02-architecture.md#3-the-wasm-boundary-and-threading)): the Rust core runs as WebAssembly in a Web Worker exactly as in the browser, so the desktop app, the browser and the CLI give the same meshes, content hashes and files. The only desktop code in the page is the adapter in [`apps/web/src/files.js`](../web/src/files.js): when `window.__TAURI__` exists it sends saves and opens to the app's commands; otherwise the page downloads and uses file inputs as before, so the web build behaves exactly as it did.

| Path | What it is |
|------|------------|
| `src-tauri/` | the `openbrick-desktop` crate (a workspace member): the window and three commands |
| `src-tauri/src/files.rs` | the file rules: formats, suggested names, bounded reads, atomic writes (no Tauri types; `cargo test --workspace` runs them on every OS) |
| `src-tauri/src/shell.rs` | which links open in the system browser; Linux safe rendering |
| `src-tauri/src/app.rs` | the Tauri application (feature `app`): `pick_file`, `read_picked_file`, `save_file`, the navigation guard |
| `src-tauri/tauri.conf.json` | window, Content Security Policy, bundle settings |
| `src-tauri/tauri.linux.conf.json`, `src-tauri/linux/` | the Linux package name and menu entry |
| `src-tauri/capabilities/main.json` | the only permissions the webview has |
| `icon.svg` | the app icon's source; `src-tauri/icons/` is generated from it |
| `scripts/frontend.mjs` | builds or serves `apps/web` for the app, always under the base path `/` |
| `test/config.test.js` | `node --test`: the CSP, the permissions, the window and the pins stay as described here |
| `smoke.mjs` | the debug app under tauri-driver and WebDriver (Linux, Xvfb) |

## What the app may do

- **Save** (Download 3MF, Download STL, the coupon files, and a JSON file such as a fit profile): the page hands the bytes from the core to `save_file` with the suggested name; the app shows the native save dialog, starting in Downloads (then in the last folder used), with a filter for the format. The name loses any path and any character a file system refuses; if the user types a name without the extension, the extension is added, and an existing file under that new name is never replaced without the dialog having asked (save again and choose it). The file is written to a temporary file in the same folder and renamed over the target, so a failed save never leaves half a file. Cancelling saves nothing and the status line says so.
- **Open** (the drop zone's "choose a file", and "Load my fit profile…"): `pick_file` shows the native open dialog filtered to SVG or JSON and tells the page only the file's name and size. The page checks the size against the core's limit (4 MiB for SVG, `max_asset_bytes()`; 1 MiB for a fit profile) before it asks `read_picked_file` for the bytes, which reads that one file once, never more than asked (and never more than 16 MiB). The core then checks the content as in the browser. Dropping an SVG on the drop zone or the 3D view works as in the browser (`dragDropEnabled` is off, so the page gets the drop).
- **Links**: the project's own links (the source, the print test guide, the notice of the Neuraldeep.net marks) open in the system browser; any other link is ignored, and the window never navigates away from the app.

The webview gets nothing else. [`capabilities/main.json`](src-tauri/capabilities/main.json) grants only the three commands (`build.rs` declares them to tauri-build, which makes every command denied unless granted): no dialog, file system, shell, opener or window permission, and no remote origin. The dialog plugin is used from Rust only. The Content Security Policy in [`tauri.conf.json`](src-tauri/tauri.conf.json) allows only the app's own files: `script-src 'self' 'wasm-unsafe-eval'` (WebAssembly needs the second), `style-src 'self'` plus the hash Tauri adds for the page's `<style>` element, and `connect-src` for the app's own files and Tauri's IPC. Because `style=""` attributes are refused, the page sets inline colours through the CSSOM.

## Versions and licences

| Component | Version (pinned exactly) | Licence |
|-----------|--------------------------|---------|
| `tauri` | 2.12.5 | Apache-2.0 OR MIT |
| `tauri-build` | 2.7.4 | Apache-2.0 OR MIT |
| `tauri-plugin-dialog` | 2.8.4 | Apache-2.0 OR MIT |
| `opener` (system browser for project links) | 0.9.0 | MIT OR Apache-2.0 |
| `@tauri-apps/cli` (npm, dev dependency) | 2.12.4 | Apache-2.0 OR MIT |
| `tauri-driver` (smoke test only, `cargo install`) | 2.1.0 | Apache-2.0 OR MIT |

`cargo deny --all-features check` covers the whole Tauri tree (the `app` feature included). `tauri-plugin-fs` is not used: the commands read and write the chosen file themselves, so the webview needs no file system scope at all.

## Build and run

You need everything [`apps/web`](../web/README.md#run-it) needs (Rust with the `wasm32-unknown-unknown` target, Node 22, pnpm 10, the pinned wasm-bindgen CLI) plus the Tauri prerequisites of your OS (<https://v2.tauri.app/start/prerequisites/>):

- **Linux** (Debian, Ubuntu): `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libsoup-3.0-dev build-essential file`
- **macOS** 13 or later: the Xcode command line tools.
- **Windows** 10 or 11: the Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11).

```sh
pnpm install                          # once, from the repository root
pnpm --dir apps/desktop wasm          # build the core into apps/web/src/wasm/ (cargo xtask wasm)
pnpm --dir apps/desktop dev           # the app with the Vite dev server on port 1420; reloads on change
pnpm --dir apps/desktop build:debug   # a debug app: target/debug/openbrick-desktop
pnpm --dir apps/desktop build         # release app and installers in target/release/bundle/
pnpm --dir apps/desktop test          # node --test: configuration checks (no build)
cargo test -p openbrick-desktop       # the file rules (no webview libraries needed)
cargo test -p openbrick-desktop --features app   # also the navigation guard (needs them)
```

`build` makes every installer the platform supports (`bundle.targets` is `all`): deb, rpm and AppImage on Linux, `.app` and DMG on macOS, NSIS and MSI on Windows. Pick some with `pnpm --dir apps/desktop tauri build --bundles deb,appimage` (add `--debug` for a quick debug bundle). On Linux the packages are called `openbrick3d` (`tauri.linux.conf.json`; Tauri would name the Debian package `open-brick3-d` after the product name), the binary is `openbrick-desktop`, and the menu entry is OpenBrick3D (`src-tauri/linux/openbrick3d.desktop`). Nothing is signed or notarised yet, and there is no updater: both come with the release work in M4 (09 sections 3 and 4). An unsigned macOS app has to be allowed in System Settings, Privacy and Security; Windows SmartScreen warns about an unsigned installer.

`dev` and `build` run `scripts/frontend.mjs`, which serves or builds `apps/web` with `OPENBRICK_BASE=/` into `apps/desktop/dist/` (gitignored); `apps/web/dist/` and the GitHub Pages build are untouched.

### Linux: safe rendering

WebKitGTK has no WebGPU, so the app renders with WebGL2 on Linux, and on some NVIDIA setups its DMA-BUF renderer shows a blank or flickering window ([09 section 3](../../docs/plan/09-platforms-and-distribution.md#3-desktop-packaging-signing-notarisation)). Start the app with `--safe-rendering` (or `OPENBRICK_SAFE_RENDERING=1`) to set `WEBKIT_DISABLE_DMABUF_RENDERER=1` before the webview starts. It is off by default because it is no general cure: under Xvfb (no GPU), WebKitGTK 2.52 draws the 3D view with its defaults but leaves it empty with this variable set, and also with `WEBKIT_DISABLE_COMPOSITING_MODE=1`, which older advice adds and which the app therefore never sets (WebKit shows WebGL only with accelerated compositing). Whether it fixes a blank window on NVIDIA still needs a test on such a machine. A setting in the app and the renderer in an About panel come later in M2.

## The icon

`icon.svg` is an original drawing: a generic 2x2 brick in isometric view, flat colours, no text and no logo (CC0, like the parts). The icon set in `src-tauri/icons/` is generated from it with the Tauri CLI; only the desktop sizes are kept (the Android, iOS and Microsoft Store sizes come with those channels):

```sh
cd apps/desktop
pnpm tauri icon icon.svg -o src-tauri/icons
rm -r src-tauri/icons/android src-tauri/icons/ios src-tauri/icons/Square*Logo.png src-tauri/icons/StoreLogo.png
```

## Smoke test

`smoke.mjs` starts the debug app with [tauri-driver](https://v2.tauri.app/develop/tests/webdriver/) and drives it over WebDriver. It runs on Linux (WebKitGTK's WebKitWebDriver; tauri-driver has no macOS support, and on Windows it needs msedgedriver):

```sh
sudo apt install webkit2gtk-driver xvfb
cargo install tauri-driver --version =2.1.0 --locked
xvfb-run -a -s "-screen 0 1600x1000x24" pnpm --dir apps/desktop smoke   # builds the core and the debug app first
xvfb-run -a -s "-screen 0 1600x1000x24" pnpm --dir apps/desktop smoke --no-build --screenshots smoke-screenshots
```

It checks that the playground runs in the app and its first tile equals `native_eval` (content hash, triangles, per-object fingerprints), and that the webview shows it: the frame drew the part, and in WebDriver's screenshot the tile's blue body covers part of the 3D view (8 percent; a WebGL canvas the webview does not show reads as 0); that the CSP is in force (no `eval`, no `style=""`) and the page needs neither; that Download 3MF, Download STL and the coupon 3MF each write a file byte-identical to what `native_eval` exports, under the page's name, and that the status line names the saved path; that an SVG and a fit profile open from the open dialog; that an SVG over the core's limit is refused before it is read; that a cancelled open changes nothing; and that a project link leaves the window on the app.

WebDriver cannot reach a native dialog, so the smoke test starts the app with `OPENBRICK_DESKTOP_TEST_DIALOGS=<folder>`: a save then writes `<folder>/<suggested name>` and an open picks `<folder>/open.svg` or `<folder>/open.json` (a cancel when it is missing). Only debug builds contain this; release builds ignore the variable. The real dialogs are checked by hand on each OS before a release: save a 3MF, cancel a save, type a name without the extension, open an SVG and a fit profile, cancel an open, drop an SVG from the file manager on the drop zone (WebDriver cannot drag files from outside the window either), and click "Read the notice". None of these hand checks has been done yet on macOS or Windows.
