//! Prints build time, triangle count and volume for nominal 2xN bricks, as input to
//! the M0 kernel spike report (docs/spikes/q01-kernel-conformance.md).
//! Run with: cargo run --release -p openbrick-geom --example kernel_timing

use std::time::Instant;

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::{CrossSection, Kernel, Polygon, Solid};
use openbrick_units::STANDARD;

fn main() {
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
    println!(
        "{:>6} {:>10} {:>10} {:>12}  fingerprint",
        "size", "ms", "triangles", "volume mm3"
    );
    for n in [2u32, 4, 8, 16, 32] {
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
        let mesh = brick.to_mesh();
        println!(
            "{:>6} {:>10.1} {:>10} {:>12.3}  0x{:016x}",
            format!("2x{n}"),
            t0.elapsed().as_secs_f64() * 1e3,
            mesh.triangle_count(),
            brick.volume_mm3(),
            mesh.fingerprint()
        );
    }
}
