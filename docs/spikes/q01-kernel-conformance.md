# Spike Q01: kernel conformance on `manifold-rust`

**Question** ([13, Q01](../plan/13-risks-and-open-questions.md#2-open-questions-with-spikes)): is `manifold-rust`, the pure-Rust port of Manifold 3.5.0 chosen as the default kernel in decision D02, correct, deterministic and fast enough on brick geometry, and does it really run in WebAssembly?

**Answer so far: yes for `manifold-rust` on the M0 corpus.** The comparison against `manifold-csg` (backlog item 2 in [14](../plan/14-lean-ai-agent-development.md#9-first-agent-ready-backlog-m0-and-m1)) is now in [Q01b](q01b-manifold-csg-comparison.md): volumes and topology agree exactly, the 2x4 brick mesh differs, and C++ Manifold is 3.5 to 10 times faster. On-device phone timings are still open.

**Date:** 2026-10-28. **Kernel:** `manifold-rust` 0.19.1 (Apache-2.0). **Host:** Linux x86-64 cloud container, Rust 1.97, Node 22.22.

## What was built

- `openbrick-geom` implements the `Kernel` trait on `manifold-rust` (feature `manifold-rust`, on by default): extrude with `scaleTop` taper, revolve, union, difference, intersection, batch union, translate, and 2D round offsets through Manifold's Clipper2 cross-sections.
- A conformance corpus in `crates/openbrick-geom/tests/conformance.rs`. Every brick number comes from `standards/dimensions-v1.json`; none are hard-coded.
- A timing example (`examples/kernel_timing.rs`) and a WebAssembly probe (`examples/wasm_probe.rs`, run by `scripts/wasm-probe.mjs`). CI now builds the probe for wasm32 and runs it under Node on every push.

## Results

| Check | Result |
|-------|--------|
| Stud (Ø4.8 x 1.7 mm, 64 segments) | watertight, genus 0, volume within 0.1 % of the analytic polygon volume |
| Tapered stud (`scaleTop` 0.9, the chamfer route in D03) | watertight, smaller than the plain stud |
| Tube (Ø6.514 / Ø4.8) | watertight, genus 1, volume within 0.1 % |
| Nominal 2x4 brick (shell, roof, 8 studs, 3 tubes) | one body, genus 0 (tube bores are blind holes under the roof), volume within 0.1 % of analytic, bounding box exact to 0.1 µm |
| Horizontal Technic hole through a block at the standard axis height | watertight, genus 1 |
| Morphological opening at 0.8 mm (two 0.4 mm lines) | a 0.3 mm hairline 6 mm long is removed; a geometric nub of about 0.03 mm remains at the concave junction, which is the mathematically correct opening and far below the printable minimum |
| Determinism | identical mesh fingerprint on repeated builds |
| Cross-target determinism | 2x4 fingerprint `0xfa969a6c0bf657e7` identical in native release, native debug and wasm32 builds |
| Runs in WebAssembly | yes; 2x4 brick volume 2073.391 mm³ and 3,620 triangles, identical to native |
| WASM size (probe: kernel + standard loader + std) | 929 KB raw, 301 KB gzip (manifold-3d's C++ build is 204 KB gzip) |

Build time for nominal 2xN bricks, release build, one thread:

| Brick | ms (native) | Triangles | Volume mm³ |
|-------|-------------|-----------|------------|
| 2x2 | 16 | 1,572 | 1105.953 |
| 2x4 | 57 | 3,620 | 2073.391 |
| 2x8 | 173 | 7,716 | 4008.267 |
| 2x16 | 521 | 15,908 | 7878.019 |
| 2x32 | 1,705 | 32,292 | 15617.522 |

The 2x4 brick builds in 107 to 119 ms on first call inside WebAssembly under Node, about twice native, which is inside the phone budget of 2 s in [02](../plan/02-architecture.md#8-performance-budgets) on desktop-class hardware. Phone timings still need a device.

## Findings that change the plan

1. **D02 holds.** The pure-Rust kernel is correct on the corpus, runs in wasm32 without a C++ toolchain, and is bit-identical across targets, so content hashes can be shared between CLI, desktop and web as planned.
2. **Cost grows faster than linearly with part size** (2x32 is 30 times the 2x4 for 9 times the triangles). The batch union of all studs and tubes against the body is the expensive step. The part evaluator (backlog item 6 onwards) should build repeated features as one compound first (`Manifold::compose` of disjoint studs is free) and union it with the body once, and should cache identical sub-solids by content hash. Re-measure after that change before M1 closes.
3. **Debug builds are about 8 times slower** than release (456 ms for the 2x4). Tests run in debug; keep the corpus small and run timing only in release.
4. **The opening test needed an honest tolerance.** Test expectations for 2D operations must allow the true geometric result, not an idealised one; the report above records the arithmetic.
5. **Mesh fingerprints depend on the order in which the inputs were created.** Building the 2x4 brick with the tube extruded after the shell and roof, instead of before them, gives the same solid (3,620 triangles, 2073.391 mm³) with another fingerprint (`0x5cb8bad1e4f5e44f` instead of `0xfa969a6c0bf657e7`): the vertex order of the batch union follows the creation order of its inputs. Only the relative order matters; `kernel_timing` builds the 2x4 after the 2x2 and still matches the fresh build in the WebAssembly probe. Content hashes therefore need a canonical evaluation order in the part evaluator (backlog item 6 onwards), or a hash over a canonicalised mesh. Found while writing `examples/export_lods.rs` for the device render harness (backlog item 3).

## Still open

- ~~Backlog item 2: the same corpus on `manifold-csg` (C++ via FFI) behind a feature flag, to confirm hashes match or document divergences.~~ Done; the divergences are documented in [Q01b](q01b-manifold-csg-comparison.md).
- Timing on a real mid-range phone in a WebView (needs the device harness, backlog item 3).
- Ribs, chamfers and fit offsets are not in the corpus yet; they arrive with the part evaluator in M1 and extend this corpus.

## Reproduce

```
cargo test -p openbrick-geom
cargo run --release -p openbrick-geom --example kernel_timing
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown -p openbrick-geom --example wasm_probe
node scripts/wasm-probe.mjs target/wasm32-unknown-unknown/release/examples/wasm_probe.wasm
```
