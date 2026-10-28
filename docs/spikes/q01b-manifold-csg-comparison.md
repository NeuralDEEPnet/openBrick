# Spike Q01b: `manifold-csg` against `manifold-rust`

**Question** ([13, Q01](../plan/13-risks-and-open-questions.md#2-open-questions-with-spikes), the open half; decision D02 in [03](../plan/03-tech-stack-decisions.md)): does the C++ Manifold library, reached through `manifold-csg`, agree with the pure-Rust port `manifold-rust` on the brick corpus? Do their hashes match? What does the C++ backend cost to build and ship?

**Answer: they agree on geometry but not on every mesh.** On every corpus shape the volumes are exactly equal and the genus and component counts match. Five of the six solids are bit-identical meshes. The 2x4 brick is not: the C++ kernel keeps two extra collinear vertices and triangulates two large faces differently (3,624 triangles against 3,620). Content hashes therefore must never be shared between the two backends, which is already the rule in D02. The C++ kernel is 2 to 10 times faster and scales linearly. It also clones Manifold's C++ sources from GitHub at build time by tag, without a hash check. **Recommendation: D02 holds.** Keep `manifold-rust` as the default and `manifold-csg` as an opt-in development and CI cross-check. Use the speed gap as the trigger to revisit the default on desktop (see the end of this report).

**Date:** 2026-10-28. **Kernels:** `manifold-rust` 0.19.1 (a port of Manifold 3.5.0); `manifold-csg` 0.4.1 with `manifold-csg-sys` 3.5.105, which builds C++ Manifold v3.5.3 (commit `0edd9d54`). **Host:** Linux x86-64 cloud container with 4 vCPUs shared with other builds, Rust 1.97, g++ 13.3, cmake 3.28.

## What was built

- `crates/openbrick-geom/src/manifold_csg.rs` implements the `Kernel` and `Solid` traits on `manifold-csg` behind the cargo feature `manifold-csg`, which is off by default. Each operation maps one to one onto the `manifold-rust` backend. Extrude goes through a Positive-fill-rule cross-section. Batch union uses `batch_boolean(Add)`. 2D offsets use round joins, miter limit 2 and default circular segments. Meshes come from the f32 `MeshGL`, so fingerprints compare the same representation. `manifold_csg::Manifold` is `Send + Sync`, so the trait bounds did not need relaxing.
- The dependency uses `default-features = false`, which turns off the crate's `parallel` feature (oneTBB). See [the `parallel` feature](#the-parallel-feature) below.
- `tests/conformance.rs` now holds each check as a generic function over the kernel. A macro instantiates every check as a `#[test]` for each enabled backend (`manifold_rust::*`, `manifold_csg::*`). Every existing assertion and tolerance is unchanged. One check is new: a stud made with `revolve` must match the extruded prism, because `revolve` was in the trait but not in the corpus.
- `backends_agree_on_volume_and_topology` runs when both features are on. It asserts that volumes agree within 1e-6 relative and that genus and component counts are equal. It prints triangle counts and fingerprints without asserting them.
- `Mesh::canonical_fingerprint` is a new hash that does not depend on vertex numbering or triangle order (see [Fingerprints depend on construction order](#fingerprints-depend-on-construction-order)). It has a unit test.
- `examples/kernel_timing.rs` times either backend: `-- manifold-csg`, or `OPENBRICK_KERNEL=manifold-csg`. `OPENBRICK_TIMING_RUNS=N` reports the best of N runs. The default backend is unchanged, and so is the fingerprint in the fifth column, which CI compares with the wasm probe. A canonical-fingerprint column is appended.

## Results

### Conformance corpus

All eight checks pass on both backends: stud, tapered stud, tube, revolved stud, 2x4 brick, determinism, morphological opening, and the horizontal Technic hole. In a debug build the `manifold-csg` corpus runs in 0.1 s against about 1 s for `manifold-rust`, because the C++ library is always compiled in Release mode.

### Cross-backend comparison

Output of `cargo test -p openbrick-geom --features manifold-csg --test conformance backends_agree -- --nocapture`. A is `manifold-rust`, B is `manifold-csg`.

| Shape | Volume mm³ (A and B) | Rel. diff | Genus | Components | Triangles A / B | Fingerprint A | Fingerprint B | Same mesh |
|-------|---------------------|-----------|-------|------------|-----------------|---------------|---------------|-----------|
| stud | 30.713083 | 0 | 0 | 1 | 252 / 252 | `0x01ab58ee6d124d70` | `0x01ab58ee6d124d70` | yes |
| tapered stud | 27.744151 | 0 | 0 | 1 | 252 / 252 | `0x7cfb69765a779e34` | `0x7cfb69765a779e34` | yes |
| tube | 130.772810 | 0 | 1 | 1 | 512 / 512 | `0x9aa8d4b5abf4a4f5` | `0x9aa8d4b5abf4a4f5` | yes |
| revolved stud | 30.713083 | 0 | 0 | 1 | 256 / 256 | `0xceb2d7e76efc76c7` | `0xceb2d7e76efc76c7` | yes |
| 2x4 brick | 2073.391093 | 0 | 0 | 1 | 3,620 / 3,624 | `0xa684ee51bd00ffa3` | `0xeb037f58b0342a20` | **no** |
| drilled block | 897.652995 | 0 | 1 | 1 | 272 / 272 | `0x4d72b3e98083ef17` | `0x4d72b3e98083ef17` | yes |

The 2D opening of the hairline artwork is identical on both backends: area 99.699657286 mm² and 12 vertices, coordinate for coordinate. The C++ Clipper2 and its Rust port agree here.

### Where the 2x4 brick differs

A comparison of the two meshes as sets of triangles (a throwaway diagnostic, not committed) found two differences:

1. The C++ kernel keeps two extra vertices, at (1.2, 29.385, 8.6) and (14.6, 2.415, 8.6). They lie on the edges where the inner side walls meet the underside of the roof (x = `side_wall` or width − `side_wall`, z = `brick_height` − `roof`), and the pair is point-symmetric about the brick centre. They are collinear with their edge, so they add 4 triangles without changing the volume: two on the walls and two on the cavity ceiling.
2. Two large planar faces with holes are triangulated differently. On the top face (z = `brick_height`, eight stud footprints), 459 of 530 triangles differ. Many of the differing `manifold-rust` triangles share the corner at (0, 31.8); many of the C++ ones share the corner at (0, 0). On the cavity ceiling (z = `brick_height` − `roof`, three tube rings), 297 of 386 triangles differ. The bottom face, the stud tops and every wall, stud and tube side are identical apart from the two wall triangles next to the extra vertices.

The 2x2 brick shows the second effect without the first. Both backends give 1,572 triangles (so, for a closed genus-0 mesh, the same vertex count), but the canonical fingerprints differ. Upstream changes between Manifold v3.5.0 (the version `manifold-rust` ports) and v3.5.3 (built here) touch only timing instrumentation, `ExecutionContext` plumbing and a mutex on CSG nodes. `polygon.cpp`, `face_op.cpp` and `edge_op.cpp` are unchanged. So the divergence most likely comes from the port, not from the version gap. This is an inference from `git diff v3.5.0 v3.5.3 -- src include`; a C++ v3.5.0 build was not tested.

### Fingerprints depend on construction order

The `brick_2x4` in the test and the 2x4 in `kernel_timing` are the same solid, with the same parts in the same union order. The only difference is that the timing example creates the stud and tube before the shell. Their `Mesh::fingerprint` values still differ (`manifold-rust`: `0xa684ee51bd00ffa3` in the test, `0xfa969a6c0bf657e7` in the example). Their triangle sets are identical: `canonical_fingerprint` is `0xf3ee26e7e06f4b52` for both. `manifold-csg` behaves the same way (`0xeb037f58b0342a20` and `0x7e5886f802179694`, canonical `0xe4780e236a42ece5` for both). Manifold numbers vertices and groups triangles by input mesh ID, and those IDs come from a counter that increases as solids are created. A refactor that only reorders feature construction would therefore change every golden fingerprint while the geometry stays the same.

### Timing

`kernel_timing` in release (`lto = "thin"`, one codegen unit), one thread, best of 5. Each backend was run in at least two series and the fastest value is shown. The host load average was 3 to 5.5 on 4 vCPUs, so treat the numbers as ratios rather than absolutes.

| Brick | Triangles A / B | `manifold-rust` ms | `manifold-csg` ms | Speed-up |
|-------|-----------------|-------------------|-------------------|----------|
| 2x2 | 1,572 / 1,572 | 14.4 | 6.9 | 2.1x |
| 2x4 | 3,620 / 3,624 | 53.5 | 15.3 | 3.5x |
| 2x8 | 7,716 / 7,720 | 171.3 | 32.9 | 5.2x |
| 2x16 | 15,908 / 15,912 | 507.5 | 72.5 | 7.0x |
| 2x32 | 32,292 / 32,296 | 1,612.0 | 157.6 | 10.2x |

From 2x4 to 2x32 the triangle count grows 8.9 times. `manifold-rust` gets 30 times slower; `manifold-csg` gets 10 times slower. The superlinear cost noted in [Q01 finding 2](q01-kernel-conformance.md#findings-that-change-the-plan) therefore comes from the Rust port (most likely its batch union, which dominates the timed work), not from Manifold's algorithm. The `manifold-rust` numbers are within 10 percent of the Q01 report. Fingerprints are identical between debug and release builds on both backends.

### Build cost and binary size

`manifold-csg-sys` is built with cargo `--timings` in a fresh target directory:

| Build | `manifold-csg-sys` build script | Whole cold build | Load average during build |
|-------|-------------------------------|------------------|---------------------------|
| debug, no TBB | 55.8 s | 60.9 s | 0.3 rising to 6.4 |
| release, no TBB | 54.5 s | 77 s | 0.2 rising to 5.5 |
| release, `parallel` (TBB) | 170.4 s | 200 s | about 20 (inconclusive: `manifold-rust` itself took 2.9 times longer in this build) |

The C++ library is compiled once per cargo profile and per target directory, because it lives in the build script's `OUT_DIR`. A job that runs debug tests and a release example therefore clones and compiles twice. After that, warm builds do not rerun the build script. The fetched sources take 127 MB on disk (77 MB of that is git history) plus 25 MB of build output, all of which ends up in the CI cache.

Release `kernel_timing` binary, stripped:

| Backends | Size | Change against the default |
|----------|------|----------------------------|
| `manifold-rust` only (default) | 1.29 MB | none |
| `manifold-csg` only | 1.87 MB | +0.57 MB |
| both | 2.68 MB | +1.38 MB |
| both, with `parallel` (TBB) | 4.43 MB | +3.14 MB |

With `manifold-csg`, the binary also links `libstdc++.so.6` dynamically (symbols up to `GLIBCXX_3.4.29`, GCC 11). Its glibc floor rises from 2.34 to 2.38, because Ubuntu 24.04 headers turn `strtol` into `__isoc23_strtol`. A CLI built this way on `ubuntu-24.04` would not start on Debian 12 or Ubuntu 22.04. That matters for the `dist` release builds in D15.

### Supply chain

- The crate does **not** vendor C++ sources. `manifold-csg-sys` 3.5.105 is a 244 KB crate with hand-written FFI declarations and a build script.
- On the first build, the build script runs `git clone https://github.com/elalish/manifold.git` (full history) and then `git checkout v3.5.3`. It checks out a **tag** and does not verify the commit; today the tag resolves to `0edd9d54876f3135e431575214dd6d8a72866fee`. If the tag were moved upstream, the C++ we compile would change while `Cargo.lock` stayed the same.
- Manifold's cmake then uses `FetchContent` to clone Clipper2 from GitHub. That one is pinned by commit hash (`46f639177fe418f9689e8ddb74f08a870c71f5b4`) and gets an in-tree patch. With `parallel` it also clones oneTBB by tag `v2022.3.0` (`f1862f38f83568d96e814e469ab61f88336cc595` at build time).
- None of this C++ appears in `Cargo.lock`, `cargo deny` or a cargo-generated SBOM. The licences are on our allowlist (Manifold Apache-2.0, Clipper2 BSL-1.0, oneTBB Apache-2.0), but they have to be added to third-party notices by hand.
- A build needs git, cmake, a C++17 compiler and access to github.com. Offline or hermetic builds can set `MANIFOLD_CSG_LIB_DIR` to a prebuilt Manifold (`libmanifold`, `libmanifoldc`, `libClipper2`). That variable is the hook for a hash-checked, vendored build.
- `manifold-csg` 0.4.1, `manifold-csg-sys` 3.5.105, `cmake` 0.1.58, `cc`, `shlex`, `find-msvc-tools` and `log` are all MIT or Apache-2.0.

### The `parallel` feature

The crate turns on `parallel` by default. That sets `MANIFOLD_PAR=ON` and `MANIFOLD_USE_BUILTIN_TBB=ON`, so cmake clones and statically links oneTBB (81 MB of source) and booleans run on a TBB thread pool. Here it gave the **same fingerprints** as the single-threaded build, so it is deterministic on this corpus. It gave no measurable speed-up (2x4 in 14.4 ms, 2x32 in 171.5 ms), but the host's load average was about 13 on 4 vCPUs during that run, so there were no idle cores to use. It added 1.76 MB to the stripped binary and a second unverified download. This backend keeps `default-features = false`. The single-threaded comparison is also the fair one, since `manifold-rust` runs on one thread.

## Recommendation for D02

1. **Keep `manifold-rust` as the default.** It agrees with C++ Manifold on every volume and topology in the corpus. It is still the only backend that builds for wasm32 without a C++ toolchain. Its meshes are bit-identical across native and wasm32 (Q01).
2. **Keep `manifold-csg` as an opt-in feature for development and CI, not for release builds yet.** It is valuable as an oracle. Run the cross-backend test in CI so that a port regression in `manifold-rust` shows up as a volume or topology mismatch. Do not ship it in CLI or desktop releases until its C++ sources are pinned by hash (a vendored, checksummed tarball fed through `MANIFOLD_CSG_LIB_DIR`) and the glibc floor fits the release targets.
3. **Hashes: divergence confirmed and documented.** Never mix content hashes from the two backends in one registry. The kernel id (`manifold-rust 0.19` and `manifold-csg 0.4.1 (Manifold v3.5.3)`) stays part of every content hash. Bit-identical cross-kernel hashes are not achievable on real bricks, even after canonicalisation, because the two kernels triangulate large faces with holes differently.
4. **Use `Mesh::canonical_fingerprint` for golden meshes and content hashes.** The plain fingerprint changes with the order in which inputs are created, even when the geometry is the same. The part evaluator (backlog items 6 to 8) should compare canonical fingerprints in its golden-mesh tests.
5. **Treat the speed gap as the trigger to revisit the default on desktop.** C++ is 3.5 times faster at 2x4 and 10 times faster at 2x32. Next steps: report the superlinear batch union to `manifold-rust` upstream with `kernel_timing` as the benchmark, and re-measure after the compose-then-union change from Q01. If `manifold-rust` then still misses the desktop budget in [02](../plan/02-architecture.md#8-performance-budgets) for large parts, consider `manifold-csg` as the desktop and CLI default. The cost is separate hash namespaces for desktop and web.
6. **Correct the D02 text.** `manifold-csg-sys` 3.5.105 builds Manifold v3.5.3, not 3.5.4.

## Still open

- `manifold-3d` WASM in the worker, the third backend D02 names.
- Testing a C++ v3.5.0 build, to confirm that the 2x4 divergence is in the port and not in the version gap.
- Measuring the `parallel` feature on an idle multi-core machine.

## Reproduce

```
cargo test -p openbrick-geom --features manifold-csg
cargo test -p openbrick-geom --features manifold-csg --test conformance backends_agree -- --nocapture
cargo build --release -p openbrick-geom --features manifold-csg --example kernel_timing
OPENBRICK_TIMING_RUNS=5 target/release/examples/kernel_timing manifold-rust
OPENBRICK_TIMING_RUNS=5 target/release/examples/kernel_timing manifold-csg
cargo build -p openbrick-geom --features manifold-csg --timings   # in a fresh target dir, for build cost
```
