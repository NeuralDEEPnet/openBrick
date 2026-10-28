//! WebAssembly probe for the M0 kernel spike: builds a nominal 2x4 brick inside a
//! wasm32 module and returns its volume, proving the pure-Rust kernel runs (not just
//! compiles) in a browser-class runtime. Build and run:
//!   cargo build --release --target wasm32-unknown-unknown -p openbrick-geom --example wasm_probe
//!   node scripts/wasm-probe.mjs target/wasm32-unknown-unknown/release/examples/wasm_probe.wasm

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::{CrossSection, Kernel, Polygon, Solid};
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

fn build() -> Option<openbrick_geom::manifold_rust::MrSolid> {
    let s = STANDARD.get();
    let k = ManifoldRust;
    let (pitch, gap, wall, roof, height) = (
        s.mm("module"),
        s.mm("undersize_per_side"),
        s.mm("side_wall"),
        s.mm("roof"),
        s.mm("brick_height"),
    );
    let cavity = height - roof;
    let (w, d) = (2.0 * pitch - 2.0 * gap, 4.0 * pitch - 2.0 * gap);
    let outer = Polygon::rect([0.0, 0.0], [w, d]);
    let inner = Polygon::rect([wall, wall], [w - 2.0 * wall, d - 2.0 * wall]).reversed();
    let stud = k
        .extrude(
            &CrossSection::from(vec![Polygon::circle(
                [0.0, 0.0],
                s.mm("stud_diameter") / 2.0,
                64,
            )]),
            s.mm("stud_height"),
            1.0,
        )
        .ok()?;
    let tube = k
        .extrude(
            &CrossSection::from(vec![
                Polygon::circle([0.0, 0.0], s.mm("tube_outer_diameter") / 2.0, 64),
                Polygon::circle([0.0, 0.0], s.mm("tube_inner_diameter") / 2.0, 64).reversed(),
            ]),
            cavity,
            1.0,
        )
        .ok()?;
    let mut parts = vec![
        k.extrude(&CrossSection::from(vec![outer.clone(), inner]), cavity, 1.0)
            .ok()?,
        k.translate(
            &k.extrude(&CrossSection::from(vec![outer]), roof, 1.0)
                .ok()?,
            [0.0, 0.0, cavity],
        ),
    ];
    for ix in 0..2 {
        for iy in 0..4 {
            parts.push(k.translate(
                &stud,
                [
                    ix as f64 * pitch + pitch / 2.0 - gap,
                    iy as f64 * pitch + pitch / 2.0 - gap,
                    height,
                ],
            ));
        }
    }
    for iy in 1..4 {
        parts.push(k.translate(&tube, [pitch - gap, iy as f64 * pitch - gap, 0.0]));
    }
    let b = k.union_all(&parts).ok()?;
    b.is_manifold().then_some(b)
}
