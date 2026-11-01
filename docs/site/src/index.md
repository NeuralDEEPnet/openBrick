# OpenBrick3D

<p class="ob-lead"><strong>OpenBrick3D</strong> is an open-source, brick-compatible part designer, assembler and 3D-print pipeline for desktop, mobile and the command line. Design parts that snap onto the bricks you already own, print them with real clutch, and build with them on a PC, a phone or from a terminal.</p>

The part designer, the print pipeline and the `openbrick` command line work today (milestone M1); the desktop app and the assembler follow the [roadmap](plan/11-roadmap.md). Everything is computed by one Rust core: the same code builds your part in the browser, in the CLI and in the tests, so a tile you preview is the tile you print.

<div class="ob-cards">
  <a class="ob-card" href="https://neuraldeepnet.github.io/openBrick/"><strong>Try the playground</strong><span>Design a logo tile in the browser, check it and download a 3MF or STL. Nothing to install.</span></a>
  <a class="ob-card" href="guides/install.html"><strong>Install</strong><span>Get the <code>openbrick</code> command line on your computer.</span></a>
  <a class="ob-card" href="guides/print-test.html"><strong>First print test</strong><span>Calibration coupon, fit profile, your own logo tile in one or two colours, pull test.</span></a>
  <a class="ob-card" href="cli.html"><strong>CLI reference</strong><span>Every <code>openbrick</code> command and option, generated from the program itself.</span></a>
  <a class="ob-card" href="plan/README.html"><strong>The plan</strong><span>Architecture, geometry spec, print pipeline, file formats, roadmap and risks.</span></a>
  <a class="ob-card" href="spikes.html"><strong>Spike reports</strong><span>What we measured before deciding: kernels, LDraw connectivity, slicer import.</span></a>
</div>

## How it fits together

| Pillar | What it does |
|--------|--------------|
| **Part designer** | Parametric, print-first parts that no catalogue has: logo tiles, nameplates, keychains, adapters. Artwork comes from SVG; connectivity is derived from the features, so designed parts snap. |
| **Print pipeline** | A calibration coupon writes a fit profile for your printer and material, the compensation is baked into the geometry, and a validator blocks parts that would not print or clutch. Exports are plain 3MF with colours, or STL. |
| **Assembler** | Snap bricks on an integer lattice with the 24 lattice rotations, from 10k to 100k instances, with lossless LDraw round-trip (from milestone M3). |

Read the [executive summary](plan/00-executive-summary.md) for the whole picture on one page, or the [architecture](plan/02-architecture.md) for how the Rust core, the WebAssembly worker and the apps divide the work.

## Help and contributing

The source, issues and discussions are on [GitHub](https://github.com/NeuralDEEPnet/openBrick). Contributions are welcome and reviewed kindly: start with [CONTRIBUTING.md](https://github.com/NeuralDEEPnet/openBrick/blob/main/CONTRIBUTING.md). This site is built from the repository by `cargo xtask docs`; the foot of every page links to the file it comes from.

The documentation is licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) (credit "OpenBrick3D contributors" with a link to the repository), the code MIT, and part documents, profiles and the Dimensional Standard CC0 ([licence policy](https://github.com/NeuralDEEPnet/openBrick/blob/main/LICENSE-POLICY.md)). The OpenBrick3D name is covered by the [trademark policy](https://github.com/NeuralDEEPnet/openBrick/blob/main/TRADEMARK.md). The site is built with mdBook and includes mdBook's page scripts, style sheets and fonts; [Credits and licences](credits.md) names them and their licences.
