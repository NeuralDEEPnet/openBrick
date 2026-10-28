# Licence policy

OpenBrick3D uses a three-tier licence layout so that code, data and documentation each carry the licence that suits them. See [docs/plan/12](docs/plan/12-legal-and-naming.md) for the reasoning.

| Tree | Licence | Notes |
|------|---------|-------|
| `crates/`, `packages/`, `apps/`, build scripts | MIT ([LICENSE](LICENSE)) | contributions under DCO 1.1, inbound = outbound |
| `parts/`, `profiles/`, `standards/`, `schemas/` | CC0 1.0 Universal (public domain dedication) | every part and profile file carries a `credit` field; keeping it is a community norm, not a legal condition. A contributor may instead mark an individual file CC BY 4.0 in its `license` field. |
| `docs/` | CC BY 4.0 | credit "OpenBrick3D contributors" with a link to the repository |
| LDraw Parts Library (downloaded at first run into the user's cache; never vendored here) | CC BY 2.0 or CC BY 4.0 per file | headers and `!LICENSE` lines are preserved; derived caches carry the same attribution; the app shows "This software uses the LDraw Parts Library" |
| `fonts/` | SIL Open Font License 1.1 | complete, unmodified font files with their licence text; never subset |

## What may not enter the repository

- Code under GPL, AGPL or LGPL in any client crate or package (`cargo deny` enforces an allowlist: MIT, Apache-2.0, BSD-2/3-Clause, CC0-1.0, ISC, Zlib, Unicode, BSL-1.0, MPL-2.0 with review; plus a per-crate MIT-0 exception for `borrow-or-share`, a dependency of the schema validator in the `xtask` dev tool). GPL tools may be invoked as separate processes only.
- LDCad's connectivity shadow library or any of its data (all rights reserved). The SNAP meta syntax is a public specification and may be implemented.
- Geometry or parameter files copied from CC BY-SA or CC BY-NC projects (PELA Blocks, MachineBlocks, cwesson/openBrick). Methods may be learned; files may not be copied.
- BrickLink catalogue data. Rebrickable bulk data may be used with attribution and never for AI training.
- Minifigure-shaped geometry of any kind.

## Provenance

Every part, profile and standard file carries a `provenance` block (`source`, `author`, `license` SPDX id, `url`, `modified`, `history`) so attribution travels with the data through conversion, export and printing. `CREDITS.txt` and the in-app attribution screen are generated from these fields.
