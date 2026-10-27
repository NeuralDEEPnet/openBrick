# Risks, open questions and assumptions

**Purpose.** The risk register with likelihood, impact, mitigation, owner and trigger; the open questions with the spike that resolves each, its success criterion and the milestone by which it must be decided; the assumptions log; and the sources appendix for the whole plan.

## 1. Risk register

Likelihood and impact: L = low, M = medium, H = high.

| ID | Risk | L | I | Mitigation | Owner | Trigger to act |
|----|------|---|---|------------|-------|----------------|
| R01 | manifold-rust (0.19.1, 33 stars) diverges from C++ Manifold or stalls | M | H | Kernel trait; manifold-csg hedge for CLI and desktop; manifold-3d WASM hedge for web and mobile; conformance corpus (brick primitives and a Thingi10K subset at M0, merch parts added as they are designed) in CI comparing all three; 2-week contingency on M2 | geometry lead | corpus failure or 3 months without upstream commits |
| R02 | Webview rendering on Linux (WebKitGTK, no WebGPU, software fallback on NVIDIA) and Android System WebView is too weak | M | M | WebGL2 floor; "safe rendering" flag; week-1 device harness; Electron build for Linux as escape hatch; wgpu renderer on the watch list | shells lead | harness shows under 30 fps at 10k on a reference device |
| R03 | Name conflict (cwesson/openBrick; Open Bricks® retail brand; OpenBrick fintech) | H | M | OpenBrick3D everywhere; register handles now; paid clearance; separate coined merch brand | founder | clearance search result |
| R04 | Users skip calibration and get poor clutch, then blame the tool | H | H | first-run wizard; `FIT.MISSING` blocker for fit-critical features; crowd-sourced fit-profile dataset as a starting point; pull-test protocol in docs | print lead | support tickets about clutch |
| R05 | Scope exceeds a three-person team | M | H | MVP is one journey and M1 and M2 hold only what it uses; milestones have demo criteria and a definition of done; checkpoints at M2 and M4 reprioritise; community contributions from M1 | founder | M2 slips more than 3 weeks |
| R06 | Tauri mobile plugin gaps (fs, share sheets) and store review delays | M | M | small Swift and Kotlin plugins budgeted in M4 and M5; PWA first; TestFlight before App Store | shells lead | plugin untested on target OS |
| R07 | LDraw connectivity cannot be derived for enough parts | M | M | M0 spike on 200 parts; hand-author the top 500 in the SNAP vocabulary with contributors; ask for a shadow-library licence | engine lead | spike under 70 percent |
| R08 | 3MF vendor drift (Bambu/Orca metadata, PrusaSlicer 3.0) breaks "opens with colours" | M | L | vanilla 3MF canonical; vendor flavours as version-pinned plugins; CI opens fixtures in OrcaSlicer CLI | io lead | a slicer release changes behaviour |
| R09 | GPL or NC contamination from the ecosystem (LeoCAD code, PELA files, MachineBlocks, LDCad data) | L | H | cargo deny; licence policy; clean-room rules in CLAUDE.md and CONTRIBUTING.md; review checklist item | maintainers | any PR touching geometry generators |
| R10 | Toy-safety exposure for merchandise sold to families | M | H | per-SKU route decision; EN 71 and ASTM F963 on a curated line; adult-only marking for B2B; DPP fields in the data model now | founder | first consumer sale |
| R11 | Performance targets (100k at 60 fps) not met on integrated GPUs | M | M | LOD budget enforced; chunk culling; benchmark gate at 20 percent regression; honest public targets (30 fps on phones) | viewport lead | bench.yml regression |
| R12 | Single points of failure in dependencies (lib3mf-core single maintainer; cadrum single maintainer; vtracer alpha) | M | L | thin abstractions; fallbacks named (threemf2, own writer; occt-wasm; potrace is GPL, so it is not a fallback); vendoring allowed | maintainers | a dependency goes 6 months quiet |
| R13 | Updater key loss or CI secret leak | L | H | key only in CI secrets with an offline backup; fork PRs get no secrets; attestations | maintainers | any secret rotation |
| R14 | Community uploads bring moderation and DSA obligations | M | M | registry starts as a Git repository with PR review; hosting decision deferred; blocklists and takedown process ready | founder | decision to host |
| R15 | WASM memory ceilings on iOS WebKit for large mosaics or Minkowski | M | M | size limits per feature on mobile; tier 3 rounding desktop-only; measure in M4 | geometry lead | crash reports on iOS |

## 2. Open questions with spikes

| ID | Question | Spike or experiment | Success criterion | Decide by |
|----|----------|---------------------|-------------------|-----------|
| Q01 | Is manifold-rust close enough to C++ Manifold on brick geometry? | Run the conformance corpus on manifold-rust, manifold-csg and manifold-3d; compare hashes, volumes, watertightness | 100 percent watertight, volumes within 0.1 percent, hashes stable across runs | M0 |
| Q02 | React or Svelte for the UI? | One-week spike: param panel plus viewport bridge in each | faster to working panel with manipulators; contributor familiarity tie-break to React | M0 |
| Q03 | Does Android System WebView and WKWebView on iOS 26 expose WebGPU by default, and does Tauri mobile pass basic file import and export? | Device test in the Tauri shell on a Pixel 8a, Galaxy A5x, iPhone 16e | `navigator.gpu` present and renders the harness; file round-trip works (else Capacitor per D14) | M0 |
| Q04 | How many LDraw parts get usable connectors from primitive scanning? | Scan 200 common parts; compare against hand-authored expectations | at least 70 percent fully correct, 90 percent partially | M0 |
| Q05 | Default fit intent: "fits genuine bricks" or "fits itself"? | Dog-food the coupon on three printers and three materials; collect pull tests | choose the intent that passes 9 of 10 with the fewest recalibrations | M1 |
| Q06 | Which 3MF colour representation survives Orca, Bambu Studio, PrusaSlicer and Cura round-trips? | Export fixtures; open in each; record what is kept | per-object colours kept in all four | M1 |
| Q07 | Rust-to-webview IPC cost for large meshes in Tauri? | Measure custom protocol vs channel vs WASM-side generation at 100k triangles | interactive (under 100 ms transfer) | M3a |
| Q08 | Is STEP needed before 1.0? | Ask the first five merch clients; count requests | fewer than 2 of 5 ask: keep v2 | M2 |
| Q09 | CRDT: Yjs, Loro or Automerge, and is 100k entities viable on a phone? | Load a 100k-instance document in each; measure load, memory, update size on a Pixel 8a | load under 3 s, memory under 300 MB | M3b |
| Q10 | Native wgpu renderer: needed? | Compare webview harness results against a minimal wgpu instancing demo on Linux and Android | webview within 30 percent of native at 100k with LOD: no | M3b |
| Q11 | Will Roland Melkert license LDCad shadow data? Will the LDraw SteerCo confirm aggregate credit for caches? | Write to both; post the connectivity proposal on forums.ldraw.org | written answers | M1 |
| Q12 | Resin and SLS: can we promise fit? | Run the coupon sweep on an MSLA printer with tough resin and via an MJF bureau; collect data | a repeatable fit profile exists | M5 |
| Q13 | Horizontal Technic holes: polyhole or ellipse by default? | Print both on three printers; measure roundness and pin fit | the strategy with the smaller spread | M1 |
| Q14 | Exact Technic dimensions (axle arm, counterbore 6.2 vs 6.4, hole axis 5.8 vs 5.6, pin shaft)? | Measurement program (20 to 30 samples each; CMM session if available) | published CSV; spec rows upgraded from L or I to V | M1 |
| Q15 | Should mirrored placements be allowed in the assembler? | Survey LDraw corpus for mirrored type-1 lines; test physical buildability on symmetric parts | allow with a warning for asymmetric parts | M3a |
| Q16 | How is the trademark guard matched, and what is its false-positive budget? | Spike: perceptual hash and silhouette matching on a labelled set of 500 logos including the blocked marks | recall over 95 percent on the blocked set with under 1 percent false positives; otherwise the guard is a warning, not a blocker | M5 |

## 3. Assumptions log

| ID | Assumption | Where used | Revisit |
|----|------------|------------|---------|
| A01 | three engineers with AI assistance (two adds 30 percent to every milestone); community from M1 | roadmap | M2 checkpoint |
| A02 | FDM with a 0.4 mm nozzle is the primary process | print pipeline | when resin data exists |
| A03 | Merchandise value is in parts that do not exist, not catalogue clones | vision, roadmap | if customers ask for catalogue reproduction |
| A04 | The company is registered in Malta (C114166) | legal, signing eligibility | registry confirmation |
| A05 | Desktop users accept a 140 MiB first-run library download | catalog | user feedback |
| A06 | Performance numbers for laptops and phones are FLOPS-scaled inferences from one RTX 5070 Ti benchmark | engine, budgets | week-1 harness |
| A07 | Tauri 2.12 mobile is stable enough for Android in M4 and iOS in M5; Capacitor only if the M0 device tests fail | platforms | M0 device tests |
| A08 | Stud nominal 4.8 mm (measured 4.87 to 4.90); sources disagree (LUGNET says 5.0 nominal) | geometry | measurement program |
| A09 | Hosting of community uploads is not needed before 1.0 | legal, roadmap | founder decision |
| A10 | Rebrickable's no-AI-training clause does not affect planned features (none train on their data) | ecosystem | if AI-assisted design features are planned |

## 4. Sources

The plan draws on nine research passes run on 2026-10-27, each with web-verified findings and URLs. Their JSON handoff files (topic, summary, findings with URLs and confidence, options compared, key numbers, recommendations, open questions, sources) are the audit trail and live under [research/](research/) in this folder. Topics: brick-geometry, printing, kernels, frameworks, ecosystem, legal, distribution, part-designer, assembler.

Primary references cited across the documents: US patent 3,005,282; CJEU C-48/09 P; General Court T-297/22, T-515/19, T-537/22; LDraw.org file format 1.0.2, library licence pages and update 2026-10; LDCad SNAP meta documentation; Robert Cailliau's dimension page; LUGNET / Munafo stud-pitch measurement; Lemeš 2019 SN Applied Sciences 1:193; Air and Wodehouse, DESIGN 2024 (Strathclyde); Kollsker and Malaguti 2021; Brickset 2026 printed-brick test; cfinke/LEGO.scad; PELA Blocks; printablebricks / ppBricks; Calibrix; 3MF Consortium specifications (Core 1.4.0, Materials 1.2.1, Production 1.1.2, Toolpath 1.0.0) and lib3mf 2.5.0; Manifold releases 3.4.0 to 3.5.4, manifold-rust 0.19.1, manifold-csg 0.4.1; OCCT 8.0.1, build123d 0.13.0, cadrum 0.9.1, occt-wasm 5.1.0; three.js r186 release notes and the 2026 instancing benchmark; Tauri 2.12 release notes and mobile plugin matrix; Electron 45; Godot 4.7.2; Bevy 0.20; Flutter 3.47; Capacitor 8.5.3; dist 0.33.1; cargo-binstall 1.26.1; Microsoft Learn on code signing and SmartScreen; Azure Artifact Signing pricing; SignPath Foundation; Apple Developer Program and EU alternative distribution terms (2026-10-01); Google Play developer verification timeline; Homebrew 5.0 cask policy; WebKit and Chrome WebGPU status pages; Safari 26 release notes; Teschner et al. 2003 spatial hashing; Legolization 2015; StableLego 2024; Agrawala et al. 2003; Rapier 0.36.1; Loro 1.17.1; Automerge 3.5.0; Yjs 13.6.33; Extism 1.41.0; vtracer 1.0.0-alpha.4; SIL OFL FAQ; Rebrickable terms (July 2026); EU GPSR 2023/988; Directive 2009/48/EC; Regulation (EU) 2025/2509; 16 CFR 1250, 1501; ASTM F963-23.
