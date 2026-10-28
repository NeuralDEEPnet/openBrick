//! Prints build time, triangle count and volume for nominal 2xN bricks, as input to
//! the kernel spike reports (docs/spikes/q01-kernel-conformance.md and
//! docs/spikes/q01b-manifold-csg-comparison.md).
//!
//! Run with: cargo run --release -p openbrick-geom --example kernel_timing
//! The backend is the first argument or the OPENBRICK_KERNEL environment variable,
//! `manifold-rust` (default) or `manifold-csg`; the latter needs the feature:
//!   cargo run --release -p openbrick-geom --features manifold-csg \
//!     --example kernel_timing -- manifold-csg
//! OPENBRICK_TIMING_RUNS=N builds each brick N times and reports the fastest run
//! (default 1), which steadies the numbers on a busy machine.

use std::time::Instant;

use openbrick_geom::{CrossSection, Kernel, Polygon, Solid};
use openbrick_units::STANDARD;

fn main() {
    let name = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("OPENBRICK_KERNEL").ok())
        .unwrap_or_else(|| "manifold-rust".to_string());
    let runs = std::env::var("OPENBRICK_TIMING_RUNS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1)
        .max(1);
    match name.as_str() {
        #[cfg(feature = "manifold-rust")]
        "manifold-rust" => time_bricks(&openbrick_geom::manifold_rust::ManifoldRust, runs),
        #[cfg(feature = "manifold-csg")]
        "manifold-csg" => time_bricks(&openbrick_geom::manifold_csg::ManifoldCsg, runs),
        other => {
            eprintln!(
                "unknown or disabled kernel {other:?}; this build has [{}] \
                 (manifold-rust is the default feature; manifold-csg needs --features manifold-csg)",
                enabled().join(", ")
            );
            std::process::exit(2);
        }
    }
}

fn enabled() -> Vec<&'static str> {
    let mut v = Vec::new();
    if cfg!(feature = "manifold-rust") {
        v.push("manifold-rust");
    }
    if cfg!(feature = "manifold-csg") {
        v.push("manifold-csg");
    }
    v
}

fn time_bricks<K: Kernel>(k: &K, runs: u32) {
    let s = STANDARD.get();
    let (pitch, gap, wall, roof, height) = (
        s.mm("module"),
        s.mm("undersize_per_side"),
        s.mm("side_wall"),
        s.mm("roof"),
        s.mm("brick_height"),
    );
    let cavity = height - roof;
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
        .unwrap();
    let tube = k
        .extrude(
            &CrossSection::from(vec![
                Polygon::circle([0.0, 0.0], s.mm("tube_outer_diameter") / 2.0, 64),
                Polygon::circle([0.0, 0.0], s.mm("tube_inner_diameter") / 2.0, 64).reversed(),
            ]),
            cavity,
            1.0,
        )
        .unwrap();
    println!("kernel: {}", k.id());
    // Keep the fingerprint in the fifth column: CI compares it with the wasm probe.
    println!(
        "{:>6} {:>10} {:>10} {:>12}  {:<18}  canonical",
        "size", "ms", "triangles", "volume mm3", "fingerprint"
    );
    for n in [2u32, 4, 8, 16, 32] {
        let mut best = f64::INFINITY;
        let mut result = None;
        for _ in 0..runs {
            let t0 = Instant::now();
            let (w, d) = (2.0 * pitch - 2.0 * gap, n as f64 * pitch - 2.0 * gap);
            let outer = Polygon::rect([0.0, 0.0], [w, d]);
            let inner = Polygon::rect([wall, wall], [w - 2.0 * wall, d - 2.0 * wall]).reversed();
            let mut parts = vec![
                k.extrude(&CrossSection::from(vec![outer.clone(), inner]), cavity, 1.0)
                    .unwrap(),
                k.translate(
                    &k.extrude(&CrossSection::from(vec![outer]), roof, 1.0)
                        .unwrap(),
                    [0.0, 0.0, cavity],
                ),
            ];
            for ix in 0..2 {
                for iy in 0..n {
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
            for iy in 1..n {
                parts.push(k.translate(&tube, [pitch - gap, iy as f64 * pitch - gap, 0.0]));
            }
            let brick = k.union_all(&parts).unwrap();
            // Extracting the mesh forces evaluation in lazy kernels, so it is timed.
            let mesh = brick.to_mesh();
            best = best.min(t0.elapsed().as_secs_f64() * 1e3);
            result = Some((brick, mesh));
        }
        let (brick, mesh) = result.expect("at least one run");
        println!(
            "{:>6} {:>10.1} {:>10} {:>12.3}  0x{:016x}  0x{:016x}",
            format!("2x{n}"),
            best,
            mesh.triangle_count(),
            brick.volume_mm3(),
            mesh.fingerprint(),
            mesh.canonical_fingerprint()
        );
    }
}
