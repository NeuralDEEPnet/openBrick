# Packaging

This file states which channels are maintainer-signed and which are community-owned, so users know what to trust and packagers know they are welcome.

| Channel | Owner | Signed by | Status |
|---------|-------|-----------|--------|
| GitHub Releases (CLI installers, cargo-binstall, MSI, DMG, NSIS, AppImage, deb, rpm, APK) | maintainers | maintainers (attestations, SBOM) | from M1 (CLI), M2 (desktop) |
| npm `openbrick3d`, PyPI `openbrick3d`, crates.io `openbrick` | maintainers | registry | M2 |
| Homebrew tap `openbrick3d/tap`, scoop bucket | maintainers | maintainers | M2 |
| winget, Microsoft Store, Flathub | maintainers | Microsoft Store and Flathub re-sign | M4 |
| Google Play, F-Droid, TestFlight, App Store | maintainers | stores | M4 to M6 |
| AUR, Nix, Snap, Chocolatey, distribution packages | community | the packager | whenever you like; tell us and we will list you here |

Community packagers: please build from a tagged release, keep the disclaimer in the package description, and open an issue so we can credit you in the README.

One folder is not under the repository's licences: `parts/assets/brand/` holds the Neuraldeep.net marks, Neural Deep Network Ltd's trademarks, all rights reserved, which may be used to build, print and show the OpenBrick3D demo tiles ([NOTICE](parts/assets/brand/NOTICE.md), [LICENSE-POLICY.md](LICENSE-POLICY.md)). The WebAssembly core (`crates/openbrick-wasm`) bundles them as the playground's sample artwork, so its module and a build of `apps/web` contain them even though both are otherwise MIT. A package that ships the playground or the module as the OpenBrick3D demo may keep them; a package or fork that may not ship them removes the four brand files from `artwork_samples_json` in `crates/openbrick-wasm/src/data.rs`, which leaves the CC0 sample mark as the default. The CLI does not bundle them: it reads them from `parts/` only when you build the demo tiles.
