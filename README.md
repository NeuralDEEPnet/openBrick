# OpenBrick3D

**OpenBrick3D** (repository `openBrick`) is an open-source, brick-compatible part designer, assembler and 3D-print pipeline for desktop, mobile and the command line. Design parts that snap onto the bricks you already own, print them with real clutch, and build with them on a PC, a phone, or from a terminal.

> **Disclaimer.** OpenBrick3D is an independent, community-driven open-source project published by Neural Deep Network Ltd (Malta). It is not affiliated with, sponsored by, authorized by, or endorsed by the LEGO Group. LEGO®, the LEGO® logo and the Minifigure are trademarks of the LEGO Group of companies, which does not sponsor, authorize or endorse this project. Works with LEGO®-compatible bricks. LDraw™ is a trademark of the Estate of James Jessiman. This software uses the LDraw Parts Library (https://www.ldraw.org).

**Status:** milestone M1, the CLI alpha (0.1.0). The geometry kernel works (pure Rust, bit-identical on native and WebAssembly), data documents have JSON Schemas, and the M0 spikes are reported in [`docs/spikes/`](docs/spikes/). The `openbrick` command line installs with one command from the first release, v0.1.0, on (see [Install](#install)); the browser playground runs at https://neuraldeepnet.github.io/openBrick/ once GitHub Pages is switched on. There you design a logo tile, starting from the Neuraldeep.net demo tile (the playground's default artwork; its sample chooser also has a square badge and a CC0 sample mark) or from your own SVG, check it and download a 3MF or STL, or print the calibration coupon; see [apps/web/README.md](apps/web/README.md). The same playground runs as a desktop app alpha too (Tauri 2, native open and save dialogs, unsigned; build it from source with [apps/desktop/README.md](apps/desktop/README.md), and CI keeps an unsigned Linux .deb from main). The plan and its milestones are in [`docs/plan/`](docs/plan/README.md); start with the [executive summary](docs/plan/00-executive-summary.md).

## Three pillars, one core

| Pillar | What it does | Where it lives |
|--------|--------------|----------------|
| **Part designer** | Parametric, print-first parts that do not exist in any catalogue: logo tiles, nameplates, keychains, mosaics, adapters. Artwork from SVG, raster or text. Connectivity is derived from features, so designed parts snap in the assembler and in LDCad. | `crates/openbrick-part`, `parts/` |
| **Assembler** | Snap bricks on an integer-LDU lattice with the 24 lattice rotations; 10k to 100k instances; open text formats with lossless LDraw round-trip. | `crates/openbrick-engine`, `crates/openbrick-catalog` |
| **Print pipeline** | Per-printer fit profiles written by a calibration coupon, compensation baked into geometry, a validator that blocks unprintable fit-critical parts, vanilla 3MF with colours. | `crates/openbrick-print`, `profiles/`, `standards/` |

## Features (planned, see the roadmap for milestones)

- Stud-and-tube snapping with Technic, bar, clip, hinge and ball connectors.
- Dynamic colour palettes keyed to LDraw colour codes.
- Custom part import (`.stl`, `.obj`, `.gltf`, `.3mf`) with sanitising and connector tagging.
- Thousands of instances with chunked GPU instancing and three LOD tiers.
- Open formats: `.obrick` (JSON in a zip), `.obpart.json`, `.obprofile.json`, `.obrun.json`, and `.ldr`/`.mpd` compatibility.
- Vanilla 3MF, STL, glTF export; STEP via an optional service later.

## Repository layout

```
crates/        Rust workspace: units, geom, part, print, engine, catalog, io, openbrick (CLI), openbrick-wasm, xtask
packages/      TypeScript packages: viewport (kernel-to-three.js frame so far); core-wasm, ui, state from M2
apps/          web (the browser playground: logo tile designer and calibration coupon; Rust core as WebAssembly in a Web Worker, three.js viewer; PWA in M4) and desktop (the desktop app alpha: the same playground in a Tauri 2 window with native open and save dialogs)
standards/     Dimensional Standard v1 (CC0) and measurement data
profiles/      process profiles (fdm-0.4, fdm-0.2, resin-msla) and example fit profiles
parts/         openBrick-native part documents (CC0; the Neuraldeep.net marks in parts/assets/brand/ are all rights reserved, see its NOTICE.md)
schemas/       JSON Schemas for every document kind
bench/         device render harness (bench/harness) and device results (bench/devices)
docs/plan/     the full planning record
docs/spikes/   M0 spike reports (kernels, LDraw connectivity)
docs/site/     the documentation site (mdBook; cargo xtask docs)
```

## Building (M0)

```
cargo xtask check      # fmt, clippy, tests, data validation
cargo xtask wasm       # WebAssembly core into apps/web/src/wasm/ (pinned wasm-bindgen CLI, size budget)
cargo xtask docs       # documentation site into target/docs/ (mdBook 0.5.4: cargo install mdbook --locked --version 0.5.4)
cargo run -p openbrick -- standard   # print the Dimensional Standard
cargo install --path crates/openbrick --locked                      # the openbrick CLI from this clone
openbrick calibrate coupon --out coupon.3mf --manifest coupon.json  # print it, then openbrick calibrate record ...
openbrick build parts/ndn.logo-tile.obpart.json --fit my-fit.json --out tile.3mf
```

Requires Rust 1.90 or newer (`rust-toolchain.toml` selects stable). Node 22 and pnpm are needed for the device render harness (`pnpm --dir bench/harness dev`) and the web app (`pnpm --dir apps/web start`, which first runs `cargo xtask wasm`; that needs the `wasm32-unknown-unknown` target and `cargo install wasm-bindgen-cli --version =0.2.129 --locked`). `pnpm test` runs their unit tests. See [apps/web/README.md](apps/web/README.md). The desktop app: `pnpm --dir apps/desktop dev` after `cargo xtask wasm`, with the Tauri prerequisites of your OS ([apps/desktop/README.md](apps/desktop/README.md)).

First print test (coupon, fit profile, logo tile in one or two colours, pull test): [docs/guides/print-test.md](docs/guides/print-test.md).

Prefer tapping to typing? The playground's Calibration coupon mode records your fit too (2. Record my fit): tap the best cells on a map of the printed coupon, and the logo tile uses the fit profile at once, kept in that browser only.

The documentation site (install, the print-test guide, the CLI reference generated from the `openbrick` command, the plan and the spike reports) is published with the playground at https://neuraldeepnet.github.io/openBrick/docs/ once GitHub Pages is switched on. Its source and how to build it are in [docs/site/README.md](docs/site/README.md).

## Install

From the first release, v0.1.0, on ([every way to install, and how to verify a download](docs/guides/install.md)):

| System | Command |
|--------|---------|
| macOS, Linux | `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/NeuralDEEPnet/openBrick/releases/latest/download/openbrick-installer.sh \| sh` |
| Windows (PowerShell) | `powershell -ExecutionPolicy Bypass -c "irm https://github.com/NeuralDEEPnet/openBrick/releases/latest/download/openbrick-installer.ps1 \| iex"` |
| cargo-binstall | `cargo binstall --git https://github.com/NeuralDEEPnet/openBrick openbrick` (`cargo binstall openbrick` once the crate is on crates.io, M2) |
| From a clone | `cargo install --path crates/openbrick --locked` (works today) |

Every release file has a GitHub artifact attestation; check one with `gh attestation verify <file> --repo NeuralDEEPnet/openBrick`. Each release also carries a CycloneDX SBOM (`openbrick.cdx.xml`). The example parts of the [print-test guide](docs/guides/print-test.md) are in `parts/`; clone the release tag that matches your `openbrick --version` (for example `git clone --branch v0.1.0 https://github.com/NeuralDEEPnet/openBrick`).

Planned channels:

| Channel | Command | Milestone |
|---------|---------|-----------|
| npm, uv, Homebrew tap, scoop | see [docs/plan/09](docs/plan/09-platforms-and-distribution.md) | M2 |
| Desktop app alpha (unsigned) | from source: `pnpm --dir apps/desktop build`; a Linux .deb as a CI artifact from main ([apps/desktop](apps/desktop/README.md)) | M2 |
| Desktop stores, Flathub, winget | | M4 |
| PWA, Android | `openbrick serve --qr` and scan | M4 |
| iOS | TestFlight, then App Store | M5, M6 |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) (DCO sign-off, conventions, review culture), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and [GOVERNANCE.md](GOVERNANCE.md). Good first issues: a feature op with a golden test, a fit profile for your printer, a part document, a translation. We review kindly and explain the rule we are applying.

## Licences

Code is MIT ([LICENSE](LICENSE)). Part definitions, profiles and the Dimensional Standard are CC0 1.0 with a credit norm; documentation is CC BY 4.0; imported LDraw parts keep their own CC BY headers. The one exception is the Neuraldeep.net marks in `parts/assets/brand/`, the demo tiles' artwork, which the WebAssembly core and the playground also bundle. They are Neural Deep Network Ltd's trademarks, all rights reserved, and not under these licences ([NOTICE](parts/assets/brand/NOTICE.md)). See [LICENSE-POLICY.md](LICENSE-POLICY.md) and [TRADEMARK.md](TRADEMARK.md).
