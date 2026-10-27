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
    openbrick-cli/              # the `openbrick` binary
    openbrick-wasm/             # wasm-bindgen facade over the core
    xtask/                      # cargo xtask: check, dist, bench, corpus
  packages/
    core-wasm/                  # built artefact + TS types + worker wrapper
    viewport/                   # three.js scene, chunks, LOD, picking, edges
    ui/                         # panels, wizards, designer, assembler tools
    state/                      # document store mirroring the command log
    part-api/                   # @openbrick/part TS API for scripted parts (M5)
  apps/
    desktop/                    # Tauri 2 project (desktop + Android + iOS)
    web/                        # PWA shell
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
- TypeScript: pnpm workspace, Vite, `wasm-pack`/`wasm-bindgen` output consumed by `packages/core-wasm`; Playwright for browser tests.
- Tauri: `pnpm tauri dev`, `pnpm tauri build`, `pnpm tauri android build`, `pnpm tauri ios build`.
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
| Performance | criterion benches for snap, collision, evaluate; web harness for 10k/100k instances; fail on 20 percent regression | bench.yml |

## 4. Lint, format, licence policy

rustfmt; clippy with `-D warnings`; biome for TypeScript; prettier and markdownlint for Markdown; `cargo deny` with an allowlist (MIT, Apache-2.0, BSD-2/3, CC0, Zlib, ISC, MPL-2.0 with care, Unicode) and a denylist (GPL, AGPL, LGPL in client crates); `pnpm licenses` check in CI; REUSE-style headers optional.

## 5. CI matrix

| Job | Runner | What |
|-----|--------|------|
| check | ubuntu-24.04 | fmt, clippy, cargo test, schema validation, biome, markdownlint |
| test-cross | ubuntu-24.04, ubuntu-24.04-arm, windows-2025, windows-11-arm, macos-26 | cargo test per target; CLI smoke |
| wasm | ubuntu-24.04 | wasm build, size budget (core under 2.5 MB gzip), Playwright on Chromium and WebKit |
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

Static site from `docs/site/` (Astro Starlight or mdBook; choose in M0): user guide (designer, assembler, printing, calibration), the Dimensional Standard, the part document reference generated from the JSON Schemas, the CLI reference generated from clap, the fit-profile dataset browser, and this plan. Hosted at `docs.openbrick3d.org`. Docs are CC BY 4.0.

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
