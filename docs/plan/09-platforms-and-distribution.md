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

"Install from the command line" on a phone therefore means: `adb install` or Obtainium on Android; TestFlight links, `cargo tauri ios dev` or `devicectl` on iOS; and a QR code to the PWA on both. There is no CLI install for iOS App Store apps or for PWAs, and we say so in the README.

## 3. Desktop packaging, signing, notarisation

- **macOS:** Apple Developer Program ($99 per year, company account). Developer ID signing and notarisation are mandatory: Gatekeeper on Sequoia and later has no Control-click bypass, and Homebrew 5.0 removes un-notarised casks. Enrol now; identity verification and reputation take weeks.
- **Windows:** do not buy an EV certificate (Microsoft states EV stopped bypassing SmartScreen in 2024). Order of preference: SignPath Foundation (free for open source, keys in HSM, builds verified from the repo), Azure Artifact Signing Basic ($9.99 per month; Neural Deep Network LTD is registered in Malta, an EU member, so the organisation is eligible), Microsoft Store MSIX (free re-signing, no SmartScreen). `dist` 0.33 supports Artifact Signing for x86_64 natively. Sign every release with the same publisher identity so SmartScreen reputation accrues.
- **Linux:** no CA needed. Flathub and Snap sign for us; AppImage carries GPG plus Tauri updater signatures; deb/rpm attached to releases.
- **Tauri bundler outputs:** `.app`/DMG, NSIS and MSI, AppImage, deb, rpm, MSIX; documented paths to Flathub, Snap, AUR, Mac App Store, Microsoft Store, Google Play and iOS App Store.
- **Linux rendering caveat:** WebKitGTK has no WebGPU and WebGL can silently fall back to software rendering on NVIDIA; the app ships a "safe rendering" setting that sets `WEBKIT_DISABLE_DMABUF_RENDERER` and related env vars before the webview starts, and the renderer backend is shown in About. If this becomes a support burden, an Electron build of the same bundle for Linux is the escape hatch (D13).

## 4. Updater

Tauri updater plugin: desktop only (AppImage, `.app`, NSIS, MSI). Channels `stable` and `beta`; signed manifests; the updater key lives only in CI secrets (loss means no further updates, so it is backed up offline). Store builds (Microsoft Store, Flathub, Play, App Store) update through the stores. The PWA updates through the service worker.

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

## 8. Supply chain and reproducibility

Commit `Cargo.lock` and `rust-toolchain.toml`; set `SOURCE_DATE_EPOCH` from the commit; `--remap-path-prefix` or `trim-paths`; `cargo-auditable`; rebuild the CLI twice in CI and diff; CycloneDX SBOMs and GitHub artifact attestations on from the first tag, with the `gh attestation verify` one-liner in the README; aim for F-Droid reproducible builds on Android; `cargo xtask dist --unsigned` lets any contributor produce every artifact locally without secrets; fork pull requests never see signing secrets.

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
