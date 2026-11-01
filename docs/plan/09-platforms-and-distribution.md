# Platforms and distribution

**Purpose.** The support matrix, the install matrix with the exact commands per operating system and channel, desktop packaging with signing and notarisation costs, the updater, the honest mobile strategy, the PWA, and the release process. Prices and versions as of 2026-10-27 from the distribution research.

## Contents

1. Support matrix
2. Install matrix
3. Desktop packaging, signing, notarisation
4. Updater
5. Mobile strategy, honestly
6. PWA
7. Release process and cadence
8. Supply chain and reproducibility
9. Yearly costs

## 1. Support matrix

| Platform | Tier | Shell | Renderer path | Notes |
|----------|------|-------|---------------|-------|
| Windows 10/11 x64, arm64 | 1 | Tauri (WebView2) | WebGPU (Chrome 113+ core) | NSIS, MSI, MSIX |
| macOS 13+ arm64, x64 | 1 | Tauri (WKWebView) | WebGPU (Safari 26 engine) | DMG, notarised |
| Linux x64, arm64 | 1 | Tauri (WebKitGTK 2.54) | WebGL2 only; "safe rendering" flag | AppImage, deb, rpm, Flathub |
| CLI on all three | 1 | native | n/a | dist |
| Web (PWA) | 2 | browser | WebGPU where present, WebGL2 floor | Chrome 113+, Safari 26, Firefox 141+ on Windows and 147+ on macOS (WebGL2 on Linux Firefox) |
| Android 7.0+ (minSdk 24) | 2 | Tauri (System WebView) | WebGPU on Chrome 121+ Android 12+ (System WebView: device test); WebGL2 floor | APK, Play |
| iOS 15+ (16+ recommended) | 2 | Tauri (WKWebView) | WebGPU status in WKWebView: device test | TestFlight, App Store |
| iPadOS | 2 | as iOS | | |

Tier 1 means CI-built, signed, tested on every release. Tier 2 means CI-built and smoke-tested, store review cadence.

Status 2026-11-01: the desktop app loads the same playground code, so it is expected to show the printer menu too (rule SIZE.BED), with the bundled printers of `profiles/printer/`, and to keep the choice in the webview's storage. The web smoke test checks the menu; the desktop smoke test does not step through it.

Status 2026-10-31 (M2 desktop alpha, `apps/desktop`): the playground runs in Tauri 2.12.5 (dialog plugin 2.8.4, CLI 2.12.4) with native open and save dialogs and builds from source on all three desktop OSes. CI compiles, lints and tests the Tauri app on Linux x64, Windows x64 and macOS arm64 on every pull request, but runs it on Linux only: a WebDriver smoke test under Xvfb (tauri-driver 2.1.0, WebKitWebDriver from webkit2gtk-driver) drives the debug app, and main keeps an unsigned .deb. Status 2026-11-01: `.github/workflows/desktop-release.yml` builds unsigned installers on every release tag (.deb and .rpm on Linux x64, NSIS and MSI on Windows x64, a universal DMG on macOS) and adds them to the CLI's release (section 7); the arm64 Linux and Windows rows are not there yet, and the first tag run is still to come. Until then macOS and Windows are tested by hand. Nothing is signed yet, so no platform is tier 1 in the sense above.

## 2. Install matrix

| OS | Channel | Command or action | Available from |
|----|---------|-------------------|----------------|
| macOS, Linux | shell installer (dist) | `curl -fsSL https://openbrick3d.org/install.sh \| sh` | M1 |
| Windows | PowerShell installer (dist) | `irm https://openbrick3d.org/install.ps1 \| iex` | M1 |
| any with Rust | cargo-binstall | `cargo binstall openbrick` (prebuilt) or `cargo install openbrick` (build) | M1 |
| any with Node | npm | `npx openbrick3d --help` or `npm i -g openbrick3d` | M2 |
| any with Python | uv / pipx | `uv tool install openbrick3d` or `pipx install openbrick3d` | M2 |
| macOS, Linux | Homebrew tap | `brew install openbrick3d/tap/openbrick` | M2; core formula and cask once the repo is 30 days old, notarised, and has 225 stars for self-submission (75 if a third party submits) |
| Windows | scoop | `scoop bucket add openbrick3d https://github.com/openbrick3d/scoop-bucket; scoop install openbrick` | M2 |
| Windows | winget | `winget install OpenBrick3D.OpenBrick3D` (komac in CI) | M4 |
| Windows | Microsoft Store | MSIX, free re-signing, no SmartScreen prompt | M4 |
| macOS | DMG | download, drag; unsigned in M2, notarised from M4 | M2 |
| Linux | Flathub | `flatpak install flathub net.neuraldeep.OpenBrick3D` | M4 (offline-source manifest and human review) |
| Linux | AppImage / deb / rpm | GitHub Releases assets; AppImage has the updater | M2 |
| Linux | AUR, Nix, Snap | community-maintained; listed in PACKAGING.md | community |
| Web | PWA | open `https://app.openbrick3d.org`, "Add to Home Screen"; or `openbrick serve --qr` on a laptop and scan | M4 |
| Android | GitHub APK | `adb install openbrick3d-<ver>.apk`; Obtainium tracks releases | M4 |
| Android | Play | closed test, then production; organisation account | M4 |
| Android | F-Droid | Play-Services-free flavour, reproducible build | M5 |
| iOS | TestFlight | public link; up to 10,000 external testers; 90-day builds | M5 |
| iOS | App Store | at 1.0 | M6 |
| iOS (own device) | developer tooling | `cargo tauri ios dev` or `xcrun devicectl` with a developer certificate | M5 |
| iOS (EU) | alternative marketplace | evaluate an AltStore PAL listing after App Store launch; free OSS app pays nothing under the 5 percent Core Technology Commission | post-1.0 |

Until openbrick3d.org is live, the M1 commands use the GitHub release URLs (`https://github.com/NeuralDEEPnet/openBrick/releases/latest/download/openbrick-installer.sh` and `.ps1`); [docs/guides/install.md](../guides/install.md) has them. `cargo binstall` needs `--git https://github.com/NeuralDEEPnet/openBrick` until the crate is on crates.io.

"Install from the command line" on a phone therefore means: `adb install` or Obtainium on Android; TestFlight links, `cargo tauri ios dev` or `devicectl` on iOS; and a QR code to the PWA on both. There is no CLI install for iOS App Store apps or for PWAs, and we say so in the README.

## 3. Desktop packaging, signing, notarisation

- **macOS:** Apple Developer Program ($99 per year, company account). Developer ID signing and notarisation are mandatory: Gatekeeper on Sequoia and later has no Control-click bypass, and Homebrew 5.0 removes un-notarised casks. Enrol now; identity verification and reputation take weeks.
- **Windows:** do not buy an EV certificate (Microsoft states EV stopped bypassing SmartScreen in 2024). Order of preference: SignPath Foundation (free for open source, keys in HSM, builds verified from the repo), Azure Artifact Signing Basic ($9.99 per month; Neural Deep Network LTD is registered in Malta, an EU member, so the organisation is eligible), Microsoft Store MSIX (free re-signing, no SmartScreen). `dist` 0.33 supports Artifact Signing for x86_64 natively. Sign every release with the same publisher identity so SmartScreen reputation accrues.
- **Linux:** no CA needed. Flathub and Snap sign for us; AppImage carries GPG plus Tauri updater signatures; deb/rpm attached to releases.
- **Tauri bundler outputs:** `.app`/DMG, NSIS and MSI, AppImage, deb, rpm, MSIX; documented paths to Flathub, Snap, AUR, Mac App Store, Microsoft Store, Google Play and iOS App Store.
- **Linux rendering caveat:** WebKitGTK has no WebGPU and WebGL can silently fall back to software rendering on NVIDIA; the app ships a "safe rendering" setting that sets `WEBKIT_DISABLE_DMABUF_RENDERER` and related env vars before the webview starts, and the renderer backend is shown in About. If this becomes a support burden, an Electron build of the same bundle for Linux is the escape hatch (D13).
- **Status (M2 alpha, 2026-10-31):** bundles come from `apps/desktop/src-tauri/tauri.conf.json` (`targets: all`, identifier `net.neuraldeep.OpenBrick3D`; Linux packages named `openbrick3d`, binary `openbrick-desktop`). Signing and notarisation are deferred to M4: M2 builds are unsigned (macOS needs the Privacy and Security override; SmartScreen warns on Windows). The AppImage waits for a licence decision. Unlike the .deb, it copies the system libraries the app runs on into the file, among them WebKitGTK, GTK and GLib (LGPL). That needs their notices and a source offer, and LICENSE-POLICY.md keeps LGPL code out of client packages. Until a maintainer decides, CI keeps only the .deb and the release workflow builds .deb and .rpm. The app's version is the workspace version (tauri.conf.json sets none; `scripts/desktop-version.py` checks it in CI and against each tag); macOS bundles are ad-hoc signed only (`signingIdentity: "-"`), and docs/guides/install.md shows how to open an unsigned app on macOS and past SmartScreen. Safe rendering exists as `--safe-rendering` or `OPENBRICK_SAFE_RENDERING=1` and sets only `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Under Xvfb, WebKitGTK 2.52 shows no WebGL with it. It also shows none with `WEBKIT_DISABLE_COMPOSITING_MODE=1`, which the app never sets because WebKit shows WebGL only with accelerated compositing. Safe rendering is therefore opt-in and still needs a test on an NVIDIA machine. The setting in the app and the renderer in About come later in M2.

## 4. Updater

Tauri updater plugin: desktop only (AppImage, `.app`, NSIS, MSI). Channels `stable` and `beta`; signed manifests; the updater key lives only in CI secrets (loss means no further updates, so it is backed up offline). Store builds (Microsoft Store, Flathub, Play, App Store) update through the stores. The PWA updates through the service worker. Deferred: the M2 desktop alpha has no updater plugin and no updater key; both come with signed builds in M4. The AppImage, the only Linux format the updater serves, also waits for the licence decision in section 3.

## 5. Mobile strategy, honestly

- **Sequence:** PWA (M4) first, Android (M4) second, iOS (M5) third. Mobile is the most expensive and slowest channel; the desktop and CLI must be earning trust first.
- **Android:** register the Play developer account as the organisation now (D-U-N-S number takes up to 28 days) to avoid the personal-account rule of a 12-tester, 14-day closed test. Decide before the first upload whether to enrol in Play App Signing (irreversible) or keep one developer-held key so GitHub, F-Droid and Play APKs stay signature-compatible for Obtainium users; we lean to one key. Google's developer verification ($25) is enforced in several countries from 2026-09-30 and globally in 2027; it applies to sideloaded APKs on certified devices but `adb install` stays exempt.
- **iOS:** App Store and TestFlight only outside the EU. Apple's EU web-distribution entitlement needs one million first annual installs or financial-standing proof, which does not fit; AltStore PAL is the realistic EU alternative after launch. Builds run on free `macos-26` GitHub runners in the public repo, so no team Mac is strictly required (one is advisable for debugging).
- **Tauri mobile gaps:** the updater is desktop-only; fs, shell, process and global-shortcut plugins are untested on mobile; budget small Swift and Kotlin plugins for file import/export and share sheets. Android minSdk 24; iOS minimum 15.
- **WebGPU on mobile:** Chrome 121+ on Android 12+ has it; whether Android System WebView and WKWebView expose it by default is a device test in M0. WebGL2 is the floor either way.

## 6. PWA

- Same bundle as desktop; service worker for offline; OPFS with `navigator.storage.persist()` for documents (Home Screen web apps get browser-level quotas: Safari 60 percent of disk per origin in browsers, 15 percent in other apps; Chrome 60 percent).
- Export through File System Access (`showSaveFilePicker` on Chromium) and the File System WritableStream in Safari 26; share sheet on mobile.
- `openbrick serve --qr` serves the web build on the LAN with a random token in the URL and prints a QR code, which is the "install on your phone from the terminal" story.
- **Your data is yours:** "Export all documents" writes every `.obrick`, part and profile to a folder or zip; the desktop app also keeps an automatic rolling backup of `.obrick` files in a user folder, because PWA storage is best-effort unless `persist()` is granted.
- Hosted at `app.openbrick3d.org` with COOP/COEP headers so WASM threads can be enabled later.

## 7. Release process and cadence

- Trunk-based; tags `vX.Y.Z` trigger `release.yml`: `dist` jobs for the CLI (all targets), `tauri-action` jobs for desktop bundles, Android and iOS archives; manifests pushed to the scoop bucket, Homebrew tap and winget (komac).
- Cadence: CLI and desktop monthly; nightly prereleases from main for contributors; mobile stores take every second desktop release to amortise review.
- Every release note lists which channels are maintainer-signed and which are community-owned (`PACKAGING.md`).
- Flathub manifest, store listings and winget manifests are written by hand (Flathub bans AI-generated manifest content) and credit packagers in the README.
- Every store listing carries the non-affiliation disclaimer from [12](12-legal-and-naming.md) to avoid trademark rejections.

Status 2026-11-01: release.yml (dist) releases the CLI; desktop bundles come from a separate workflow, `.github/workflows/desktop-release.yml`, on the same tags (plain `vX.Y.Z`; an MSI allows no pre-release suffix), because release.yml is dist's output and `scripts/release-workflow.py --check` keeps it so. It builds the Tauri bundles itself with the pinned Tauri CLI instead of tauri-action, so no third-party action holds the release token. Rows: ubuntu-24.04 (deb, rpm), windows-2025 (NSIS, MSI), macos-26 (one universal DMG and `.app.tar.gz`); no AppImage (section 3). Each file gets a SHA-256 file and a GitHub artifact attestation; the publish job, the only one with `contents: write` and only on tag pushes, waits for dist's release and uploads to it, and a manual run is a dry run. The Linux leg was built locally (deb and rpm, 3.6 MiB each); Windows and macOS run first on the next tag. Not yet: arm64 rows, nightly prereleases, store and package-manager manifests.

## 8. Supply chain and reproducibility

Commit `Cargo.lock` and `rust-toolchain.toml`; set `SOURCE_DATE_EPOCH` from the commit; `--remap-path-prefix` or `trim-paths`; `cargo-auditable`; rebuild the CLI twice in CI and diff; CycloneDX SBOMs and GitHub artifact attestations on from the first tag, with the `gh attestation verify` one-liner in the README; aim for F-Droid reproducible builds on Android; `cargo xtask dist --unsigned` lets any contributor produce every artifact locally without secrets; fork pull requests never see signing secrets.

Status at M1 (backlog item 15):

- Enabled: `--remap-path-prefix` for registry sources (`trim-paths` is still unstable in Rust 1.99); a CycloneDX SBOM (cargo-cyclonedx 0.5.5 as dist 0.33.1 pins it: CycloneDX 1.3, no hashes, features resolved across the workspace, so it errs towards listing more); GitHub attestations for every release file; release actions pinned by commit; the licence texts of the CLI's 71 dependency crates in every archive.
- Deferred: `cargo-auditable` (dist 0.33.1 installs it from an unpinned `releases/latest` URL); `SOURCE_DATE_EPOCH` (rustc embeds no timestamps, and dist ignores it for archive mtimes, so archives differ while binaries are identical); the rebuild-twice CI diff; `cargo xtask dist --unsigned`.

## 9. Yearly costs

| Item | Cost | Required |
|------|------|----------|
| Apple Developer Program | $99 / year | yes (macOS notarisation, iOS) |
| Azure Artifact Signing Basic | about $120 / year | fallback if SignPath declines |
| SignPath Foundation | $0 | preferred |
| Microsoft Store account | $0 | yes |
| Google Play | $25 once | yes |
| Google developer verification | $25 once | yes |
| GitHub Actions (public repo) | $0 incl. macos-26 arm64, windows-11-arm, ubuntu-24.04-arm | yes |
| Domains (openbrick3d.com/.org, openbrick.dev) | about $60 / year | yes |
| Total recurring | about $160 to $280 / year | |
| One-time hardware for M0: six harness devices (Iris Xe laptop, Radeon 780M laptop, base M-series Mac, Pixel 8a or 9a, Galaxy A5x, iPhone 16e) and two reference printers (one Prusa-class, one Bambu-class) | about $4,000 to $6,000, partly already owned | yes for the benchmarks and the print-fit tests |
