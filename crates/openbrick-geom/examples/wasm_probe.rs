//! WebAssembly probe for the M0 kernel spike: builds a nominal 2x4 brick inside a
//! wasm32 module and returns its volume, proving the pure-Rust kernel runs (not just
//! compiles) in a browser-class runtime. Build and run:
//!   cargo build --release --target wasm32-unknown-unknown -p openbrick-geom --example wasm_probe
//!   node scripts/wasm-probe.mjs target/wasm32-unknown-unknown/release/examples/wasm_probe.wasm
//!
//! The brick comes from the shared nominal construction (`openbrick_geom::nominal`);
//! CI compares its fingerprint with the 2x4 row of `kernel_timing.rs`, which builds
//! the same brick independently.

use openbrick_geom::manifold_rust::{ManifoldRust, MrSolid};
use openbrick_geom::nominal::{nominal_brick, Detail, NominalDims};
use openbrick_geom::Solid;
use openbrick_units::STANDARD;

/// Build a nominal 2x4 brick and return its volume in mm3 (negative on failure).
#[no_mangle]
pub extern "C" fn ob_brick_2x4_volume() -> f64 {
    brick().map(|(v, _)| v).unwrap_or(-1.0)
}

/// Triangle count of the same brick (negative on failure).
#[no_mangle]
pub extern "C" fn ob_brick_2x4_triangles() -> f64 {
    brick().map(|(_, t)| t as f64).unwrap_or(-1.0)
}

/// FNV-1a fingerprint of the brick mesh, to compare bit-for-bit with native builds.
#[no_mangle]
pub extern "C" fn ob_brick_2x4_fingerprint() -> u64 {
    brick_mesh().map(|m| m.fingerprint()).unwrap_or(0)
}

fn brick_mesh() -> Option<openbrick_geom::Mesh> {
    build().map(|b| b.to_mesh())
}

fn brick() -> Option<(f64, usize)> {
    let b = build()?;
    Some((b.volume_mm3(), b.to_mesh().triangle_count()))
}

fn build() -> Option<MrSolid> {
    let d = NominalDims::from_standard(STANDARD.get());
    let b = nominal_brick(&ManifoldRust, &d, [2, 4], &Detail::full()).ok()?;
    b.is_manifold().then_some(b)
}
