# Repository layout and development tooling

**Purpose.** The monorepo tree with responsibilities, the build system, the test strategy, linting and formatting, the CI matrix, release automation, the documentation site, the contribution workflow, and the conventions for AI-assisted development.

## Contents

1. Monorepo tree
2. Build system
3. Test strategy
4. Lint, format, licence policy
5. CI matrix
6. Release automation
7. Documentation site
8. Contribution workflow and review culture
9. AI-assisted development conventions (CLAUDE.md outline)
10. Versioning

## 1. Monorepo tree

```
openBrick/
  README.md                     # product README with disclaimer and plan link
  LICENSE                       # MIT (code)
  LICENSE-POLICY.md             # code MIT; parts/profiles CC0; docs CC BY 4.0; LDraw tree CC BY
  TRADEMARK.md                  # OpenBrick3D name and logo policy
  GOVERNANCE.md                 # maintainers, decisions by ADR, key custodians
  SECURITY.md                   # disclosure policy (90 days), contact
  CONTRIBUTING.md               # DCO, conventions, review culture
  CODE_OF_CONDUCT.md
  PACKAGING.md                  # which channels are maintainer-signed vs community
  CLAUDE.md                     # conventions for AI-assisted work (section 9)
  Cargo.toml                    # workspace
  rust-toolchain.toml
  package.json / pnpm-workspace.yaml
  crates/
    openbrick-units/            # LDU, Rot24, standard constants
    openbrick-geom/             # Kernel trait, manifold-rust default, manifold-csg feature
    openbrick-part/             # openbrick/part@1 parser, evaluator, connectors
    openbrick-print/            # profiles, validator, coupon, nesting
    openbrick-engine/           # instances, hashes, snapping, collision, graph, commands
    openbrick-catalog/          # LDraw pack, parser, primitive scanning, LOD baking
    openbrick-io/               # .obrick, LDraw, 3MF, STL, glTF, mesh import
    openbrick/                  # the `openbrick` CLI crate and binary (crates.io: openbrick)
    openbrick-wasm/             # wasm-bindgen facade over the core
    xtask/                      # cargo xtask: check, dist, bench, corpus
  packages/
    core-wasm/                  # built artefact + TS types + worker wrapper
    viewport/                   # three.js scene, chunks, LOD, picking, edges (today: the kernel-to-three.js frame)
    ui/                         # panels, wizards, designer, assembler tools
    state/                      # document store mirroring the command log
    part-api/                   # @openbrick/part TS API for scripted parts (M5)
  apps/
    desktop/                    # Tauri 2 project (desktop + Android + iOS); src-tauri/ is the openbrick-desktop crate, whose "app" feature pulls in Tauri, so core builds never need the webview libraries
    web/                        # PWA shell; today the M1 browser pipeline (core in a Web Worker, three.js viewer)
  standards/
    dimensions-v1.json          # Dimensional Standard v1 (CC0)
    measurements/               # raw measurement CSVs (CC0)
  parts/                        # openBrick-native part documents (CC0 unless stated)
  profiles/                     # process profiles and seed fit profiles
  fonts/                        # bundled OFL fonts, unmodified, with licences
  schemas/                      # JSON Schemas for every document kind
  corpus/                       # golden meshes, LDraw round-trip files, 3MF fixtures (with licences)
  bench/                        # criterion benches, web harness, devices/ CSV
  docs/
    plan/                       # this plan
    site/                       # documentation site source
  .github/
    workflows/                  # ci.yml, release.yml, bench.yml, corpus.yml
    ISSUE_TEMPLATE/, PULL_REQUEST_TEMPLATE.md
```

## 2. Build system

- Rust: Cargo workspace; `cargo xtask check` runs fmt, clippy, tests and schema validation; `cargo xtask dist --unsigned` builds every artifact locally; `cargo xtask corpus` regenerates golden meshes (reviewed diffs).
- TypeScript: pnpm workspace, Vite, `wasm-bindgen` output consumed by `packages/core-wasm`. Until that package exists, `cargo xtask wasm` writes it into `apps/web/src/wasm/`, with the wasm-bindgen CLI pinned to the crate version in `Cargo.lock`. Playwright for browser tests.
- Tauri: `pnpm --dir apps/desktop dev`, `pnpm --dir apps/desktop build` (and `build:debug`), later `pnpm --dir apps/desktop tauri android build` and `tauri ios build`; the Tauri CLI is the pinned npm devDependency `@tauri-apps/cli`.
- Caching: sccache in CI; pnpm store cache; Cargo registry cache.

## 3. Test strategy

| Layer | Tests | Tooling |
|-------|-------|---------|
| Units and lattice | derived relations (tube OD, pin OD, 5 plates = 2 studs), rotation group closure and composition table | cargo test |
| Geometry | golden meshes: hash equality on the default kernel; volume, AABB, feature counts within 0.01 mm across kernels (manifold-rust, manifold-csg, manifold-3d oracle) | cargo test, corpus/ |
| Part evaluator | every feature op; param bounds; expression language; SVG and text pipelines with removed-area assertions; connector derivation matches expected SNAP lines | cargo test, insta snapshots |
| Print | validator rules each with a passing and failing fixture; coupon sweep steps; nesting determinism (same input, same bytes) | cargo test |
| Engine | property tests: snapping never produces a colliding pose; union-find consistency after random edit sequences; undo of any sequence restores the document hash | proptest |
| IO | LDraw round-trip on a corpus (bit-identical); 3MF validated with lib3mf and opened by OrcaSlicer CLI; fuzzing of all readers | cargo test, cargo-fuzz nightly job |
| Viewport | visual snapshots of fixed scenes on WebGL2 and WebGPU backends; instance count and LOD selection assertions | Playwright |
| App | smoke: launch, open sample, export; calibration wizard flow | Playwright with Tauri driver; manual on mobile |
| Print-fit | physical: coupon prints on two reference printers per release candidate, pull-test recorded in `fit-profiles/` | humans |
| Performance | criterion benches for snap, collision, evaluate; web harness for 10k/100k instances, `bench/scale` (core in node, Build mode in headless Chromium, `?bench=1` on devices); fail on 20 percent regression (not yet automated for `bench/scale`) | bench.yml |

## 4. Lint, format, licence policy

rustfmt; clippy with `-D warnings`; biome for TypeScript; prettier and markdownlint for Markdown; `cargo deny` with an allowlist (MIT, Apache-2.0, BSD-2/3, CC0, Zlib, ISC, MPL-2.0 with care, Unicode) and a denylist (GPL, AGPL, LGPL in client crates); `pnpm licenses` check in CI; REUSE-style headers optional.

## 5. CI matrix

| Job | Runner | What |
|-----|--------|------|
| check | ubuntu-24.04 | fmt, clippy, cargo test, schema validation, biome, markdownlint |
| test-cross | ubuntu-24.04, ubuntu-24.04-arm, windows-2025, windows-11-arm, macos-26 | cargo test per target; CLI smoke |
| wasm, web | ubuntu-24.04 | wasm build, size budget (core under 2.5 MB gzip, `cargo xtask wasm`), Playwright smoke of the `apps/web` playground on Chromium (WebGL2 and WebGPU, phone layout, the `/openBrick/` Pages build) against the native values of the same Rust functions (downloads byte-identical to the native exports, every part 3MF in the part-designer layout), lib3mf strict validation of the downloaded 3MF files; WebKit to follow |
| cli, part-exports, io-3mf | ubuntu-24.04 | the `openbrick` CLI end to end (installed from the clone; logo tile in every relief and inlay style, a parts inlay without colours, coupon, recorded fit, a blocked build) and the part crate's and writer's exports, every 3MF read by lib3mf 2.5.0 in strict mode against its description (`scripts/validate-3mf.py --self-test`) |
| pages | ubuntu-24.04 (push to main, manual) | builds the core and `apps/web` with base `/openBrick/` and the documentation site (`cargo xtask docs`), and deploys both to GitHub Pages, the site under `docs/` (`.github/workflows/pages.yml`) |
| docs | ubuntu-24.04 | plan links; `cargo xtask docs --check`: the CLI reference is current, the credits name the pinned mdBook, the mdBook site builds, every link and anchor resolves, every page ends with its source link and the disclaimer |
| slicer-smoke | ubuntu-24.04 (weekly, manual; never required) | the CLI's logo tiles, brand tile and coupon sliced by a pinned, checksum-verified OrcaSlicer 2.4.2 AppImage; objects, parts and filaments per file for Q18 (`.github/workflows/slicer-smoke.yml`, `scripts/slicer-smoke.sh`) |
| desktop | ubuntu-24.04 | the desktop app alpha (`apps/desktop`): clippy and tests of `openbrick-desktop` with its Tauri feature, `node --test` configuration checks (CSP, permissions, window, pins), the debug app, and a WebDriver smoke test under Xvfb (tauri-driver, WebKitWebDriver): the first tile equals native and is shown, the CSP is in force, saves through the app's save command are byte-identical to the native exports, also over Tauri's postMessage IPC fallback; an unsigned .deb artifact on main (no AppImage until the licence decision in [09](09-platforms-and-distribution.md) section 3) |
| desktop-cross | windows-2025, macos-26 | clippy and tests of `openbrick-desktop` with its Tauri feature, so the app compiles on every desktop OS on every pull request (a dev build embeds no frontend, so no wasm or web build); a first step towards desktop-bundles below |
| corpus | ubuntu-24.04 | golden meshes, LDraw round-trip, 3MF conformance (lib3mf, OrcaSlicer CLI) |
| bench | ubuntu-24.04 (scheduled) | criterion and web harness; regression gate |
| fuzz | ubuntu-24.04 (nightly) | cargo-fuzz on readers |
| desktop-bundles | three OSes | tauri-action unsigned builds on PRs; signed on tags |
| mobile | ubuntu-24.04 (Android NDK), macos-26 (iOS) | archives on tags |
| release | all | dist + tauri-action + manifests (section 6) |

Fork pull requests never receive secrets; signed artifacts only from tags on the main repository.

## 6. Release automation

`dist init` generates the CLI release workflow (installers, npm, Homebrew tap, MSI, SBOM, attestations, Artifact Signing). `tauri-action` builds desktop and mobile bundles in the same `release.yml`. Post-release jobs push the scoop manifest, Homebrew cask (once eligible), winget manifest via komac, and the Flathub manifest pull request (human-written). Release notes are generated from conventional commits and edited by a human.

## 7. Documentation site

Static site from `docs/site/`, built with mdBook (chosen 2026-10-31 over Astro Starlight because it is a Rust tool, so the site needs no JavaScript toolchain of its own; MPL-2.0; it runs as a program and is never linked; version pinned by `cargo xtask docs`). The published site contains mdBook's own MPL-2.0 front-end files and the libraries, fonts and icons it bundles (highlight.js BSD-3-Clause; elasticlunr.js, mark.js and clipboard.js MIT; Open Sans Apache-2.0; Source Code Pro OFL-1.1; Font Awesome icons CC BY 4.0), all unmodified and listed with their licences on the site's credits page. Its contents:

- the user guide (designer, assembler, printing, calibration);
- the Dimensional Standard;
- the part document reference generated from the JSON Schemas;
- the CLI reference generated from clap (done: `docs/site/src/cli.md`, the `--help` of every `openbrick` command);
- the fit-profile dataset browser;
- this plan (done, with the spike reports).

`cargo xtask docs` stages `docs/guides`, `docs/plan` and `docs/spikes` beside `docs/site/src` without copying them into the repository, and ends every page with a link to its source and the disclaimer (plan 12 section 4). It builds the site into `target/docs` and checks every link, anchor and page footer ([docs/site/README.md](../site/README.md)). The site is hosted with the playground on GitHub Pages at https://neuraldeepnet.github.io/openBrick/docs/ until `docs.openbrick3d.org` is live. Docs are CC BY 4.0.

## 8. Contribution workflow and review culture

- Issues use templates: bug (with renderer backend and GPU string from About), feature, part request, fit-profile submission.
- Pull requests: DCO sign-off (`git commit -s`), conventional commit title, tests for geometry changes (golden meshes), a screenshot for UI changes, and a line in the changelog.
- Review culture, as the company asked: name the rule and link the document; suggest, do not demand style; approve with nits rather than block on them; a newcomer's first pull request gets a walkthrough; mistakes become assumptions logged in [13](13-risks-and-open-questions.md), not blame. Wrong ideas argued well are welcome and get an answer, not a close.
- Community-owned packaging (AUR, Nix, Snap) and fit profiles are credited in the README.
- Good first issues: adding a feature op with a golden test; a fit profile for a printer; a part document; a translation.

## 9. AI-assisted development conventions (CLAUDE.md outline)

```
# OpenBrick3D conventions for AI-assisted work
- Positions are integer LDU; never floating mm in the engine or part layout.
- The core has no three.js, DOM or Tauri types. UI never computes geometry.
- Every validator message has a rule id, value, threshold and fix hint.
- Geometry changes need golden-mesh tests; snapping changes need property tests.
- Numbers come from standards/dimensions-v1.json; do not hard-code dimensions.
- Never use the LEGO word mark as a descriptor; say "brick-compatible". Keep the disclaimer verbatim.
- No GPL/LGPL code or data in client crates; LDCad shadow data is off limits; cite sources for geometry.
- Commits: conventional, signed off (DCO). Keep PRs small; include a changelog line.
- Run: cargo xtask check; pnpm test. Benchmarks: cargo xtask bench.
- Be kind in reviews and in generated messages; explain, do not scold.
```

## 10. Versioning

Semver per crate and package, released together from one tag; schema versions evolve independently with migrations ([08](08-file-formats-and-interop.md)); the kernel version is pinned per schema version so content hashes stay reproducible.
