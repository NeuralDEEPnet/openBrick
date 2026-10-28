//! Golden tests of the nominal brick construction (`openbrick_geom::nominal`), which
//! the device harness meshes and the browser pipeline (`openbrick-wasm`, `apps/web`)
//! share.
//!
//! For each footprint: one watertight genus-0 body, the analytic volume (cylinders
//! as the regular polygons the kernel extrudes), the bounding box from the
//! standard, the triangle count and both mesh fingerprints. Fingerprints are exact
//! on x86-64 Linux, where the Q01 spike verified bit-identical kernel output
//! (native and wasm32); elsewhere the platform's sin and cos may round the circle
//! vertices differently in the last bit, so only the structural checks run.
//!
//! To print the table below after an intended geometry change:
//!   cargo test -p openbrick-geom --test nominal golden_table -- --ignored --nocapture

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::nominal::{
    nominal_brick, nominal_volume, underside_centres, Detail, NominalDims, Underside,
};
use openbrick_geom::{GeomError, Kernel, Solid, Tessellation};
use openbrick_units::STANDARD;

/// Allowed relative error of the kernel volume against the analytic one. The kernel
/// works in f64 from the same polygon vertices; the largest error measured on these
/// footprints is 1.04e-9 (1x1 with 16-segment studs, from boolean rounding). 1e-6
/// still catches any missing or extra part: the smallest one, a 16-segment stud, is
/// about 8 percent of a 1x1 brick.
const VOL_TOL: f64 = 1e-6;
/// Bounds are compared after the f32 narrowing of mesh positions.
const BOUNDS_TOL: f64 = 1e-5;

/// One recorded footprint at full detail.
struct Golden {
    studs: [u32; 2],
    triangles: usize,
    fingerprint: u64,
    canonical: u64,
}

/// Recorded with `golden_table` on x86-64 Linux (manifold-rust 0.19.1, dimensions-v1
/// 1.0.0). The 2x4 row equals the 2x4 row of `examples/kernel_timing.rs` and LOD0 in
/// `bench/harness/public/meshes/manifest.json` (fingerprint 0xfa969a6c0bf657e7,
/// canonical 0xf3ee26e7e06f4b52 in docs/spikes/q01b-manifold-csg-comparison.md).
/// The largest footprint the browser offers, 16x16, is left out because it takes
/// minutes in a debug build (180,772 triangles; Q01 finding 2, superlinear batch
/// union); 3x5 and 1x16 cover tubes in both directions and a long pin row.
const GOLDEN: &[Golden] = &[
    Golden {
        studs: [1, 1],
        triangles: 292,
        fingerprint: 0xdacc_655f_07ff_e715,
        canonical: 0x45fe_4b8f_c3b4_ce43,
    },
    Golden {
        studs: [1, 4],
        triangles: 1828,
        fingerprint: 0x8bed_f3ec_6279_e28f,
        canonical: 0x6b66_27f7_ee3f_97f9,
    },
    Golden {
        studs: [4, 1],
        triangles: 1828,
        fingerprint: 0x7475_b983_2c10_9a51,
        canonical: 0x00af_8ba2_1bce_2a8f,
    },
    Golden {
        studs: [2, 2],
        triangles: 1572,
        fingerprint: 0xfcd2_a45a_29aa_05d0,
        canonical: 0xa1ad_3110_8801_1ec2,
    },
    Golden {
        studs: [2, 4],
        triangles: 3620,
        fingerprint: 0xfa96_9a6c_0bf6_57e7,
        canonical: 0xf3ee_26e7_e06f_4b52,
    },
    Golden {
        studs: [3, 5],
        triangles: 7972,
        fingerprint: 0x0b69_b274_bace_537c,
        canonical: 0x177c_2a1b_93b0_42ba,
    },
    Golden {
        studs: [1, 16],
        triangles: 7972,
        fingerprint: 0x8d32_30d5_4654_7fd3,
        canonical: 0xd334_5d1b_5090_b57a,
    },
];

fn dims() -> NominalDims {
    NominalDims::from_standard(STANDARD.get())
}

fn build(studs: [u32; 2]) -> openbrick_geom::manifold_rust::MrSolid {
    nominal_brick(&ManifoldRust, &dims(), studs, &Detail::full())
        .unwrap_or_else(|e| panic!("{studs:?}: {e}"))
}

#[test]
fn full_detail_uses_the_default_tessellation() {
    let full = Detail::full();
    assert_eq!(full.segments, Some(Tessellation::default().stud_segments));
    assert!(full.underside);
}

#[test]
fn dimensions_come_from_the_standard() {
    let s = STANDARD.get();
    let d = dims();
    assert_eq!(d.pitch, s.mm("module"));
    assert_eq!(d.pin_diameter, s.mm("pin_1wide_diameter"));
    assert_eq!(d.cavity(), s.mm("brick_height") - s.mm("roof"));
    // n studs are n pitches minus the undersize on both sides (docs/plan/04).
    let [x, y] = d.body([3, 7]);
    assert!((x - (3.0 * d.pitch - 2.0 * d.undersize)).abs() < 1e-12);
    assert!((y - (7.0 * d.pitch - 2.0 * d.undersize)).abs() < 1e-12);
}

#[test]
fn underside_follows_the_footprint() {
    let d = dims();
    assert_eq!(Underside::of([1, 1]), Underside::None);
    assert_eq!(Underside::of([1, 6]), Underside::Pins);
    assert_eq!(Underside::of([6, 1]), Underside::Pins);
    assert_eq!(Underside::of([2, 2]), Underside::Tubes);
    assert!(underside_centres(&d, [1, 1]).is_empty());
    // One pin between each pair of neighbouring studs, on the row's centre line.
    let pins = underside_centres(&d, [1, 4]);
    assert_eq!(pins.len(), 3);
    for (i, [x, y]) in pins.iter().enumerate() {
        assert!((x - (d.pitch / 2.0 - d.undersize)).abs() < 1e-12);
        assert!((y - ((i + 1) as f64 * d.pitch - d.undersize)).abs() < 1e-12);
    }
    assert_eq!(
        underside_centres(&d, [4, 1])
            .iter()
            .map(|[x, y]| [*y, *x])
            .collect::<Vec<_>>(),
        pins,
        "a 4x1 is a 1x4 with x and y swapped"
    );
    // One tube at each centre between four studs: (n - 1) x (m - 1).
    assert_eq!(underside_centres(&d, [2, 4]).len(), 3);
    assert_eq!(underside_centres(&d, [3, 5]).len(), 8);
    assert_eq!(underside_centres(&d, [16, 16]).len(), 225);
}

#[test]
fn zero_studs_are_rejected_with_a_readable_error() {
    for studs in [[0, 4], [4, 0], [0, 0]] {
        match nominal_brick(&ManifoldRust, &dims(), studs, &Detail::full()) {
            Err(GeomError::InvalidInput(message)) => {
                assert!(message.contains("at least one stud"), "{message}")
            }
            Err(e) => panic!("{studs:?}: unexpected error {e}"),
            Ok(_) => panic!("{studs:?}: built a brick without studs"),
        }
    }
}

#[test]
fn every_golden_footprint_is_one_closed_genus_zero_body_with_analytic_volume_and_bounds() {
    let d = dims();
    for g in GOLDEN {
        let name = format!("{}x{}", g.studs[0], g.studs[1]);
        let solid = build(g.studs);
        assert!(solid.is_manifold(), "{name}: not manifold");
        // Tube bores are blind holes closed by the roof, and pins are solid, so
        // every nominal brick is one genus-0 body.
        assert_eq!(solid.genus(), 0, "{name}: genus");
        assert_eq!(solid.component_count(), 1, "{name}: components");
        let expected = nominal_volume(&d, g.studs, &Detail::full());
        let rel = ((solid.volume_mm3() - expected) / expected).abs();
        assert!(
            rel < VOL_TOL,
            "{name}: volume {} vs analytic {expected}",
            solid.volume_mm3()
        );
        let mesh = solid.to_mesh();
        let (lo, hi) = mesh.bounds();
        let [w, depth] = d.body(g.studs);
        let want_hi = [w, depth, d.height + d.stud_height];
        for k in 0..3 {
            assert!(
                (lo[k] as f64).abs() < BOUNDS_TOL,
                "{name}: min[{k}] {}",
                lo[k]
            );
            assert!(
                (hi[k] as f64 - want_hi[k]).abs() < BOUNDS_TOL,
                "{name}: max[{k}] {} vs {}",
                hi[k],
                want_hi[k]
            );
        }
        assert_eq!(mesh.triangle_count(), g.triangles, "{name}: triangles");
    }
}

#[test]
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux")),
    ignore = "bit-exact output is verified on x86-64 Linux only (docs/spikes/q01-kernel-conformance.md)"
)]
fn every_golden_footprint_has_its_recorded_fingerprints() {
    for g in GOLDEN {
        let mesh = build(g.studs).to_mesh();
        let name = format!("{}x{}", g.studs[0], g.studs[1]);
        assert_eq!(
            format!("0x{:016x}", mesh.fingerprint()),
            format!("0x{:016x}", g.fingerprint),
            "{name}: fingerprint"
        );
        assert_eq!(
            format!("0x{:016x}", mesh.canonical_fingerprint()),
            format!("0x{:016x}", g.canonical),
            "{name}: canonical fingerprint"
        );
    }
}

/// A long-lived module (the browser worker) builds many bricks in one process. The
/// kernel numbers solids from a counter, but only the relative creation order within
/// one brick reaches the mesh, so a brick built after others is identical to a fresh
/// one.
#[test]
fn a_brick_built_after_others_is_identical_to_a_fresh_one() {
    let fresh = build([2, 4]).to_mesh();
    for studs in [[1, 1], [1, 4], [3, 5], [2, 2]] {
        build(studs);
    }
    let later = build([2, 4]).to_mesh();
    assert_eq!(fresh, later);
}

/// The lower levels of detail of the render harness: 16-segment studs without the
/// underside, and the stud-less hull box.
#[test]
fn lower_detail_levels_match_their_analytic_volume() {
    let d = dims();
    for detail in [
        Detail {
            segments: Some(16),
            underside: false,
        },
        Detail {
            segments: None,
            underside: false,
        },
    ] {
        for studs in [[1, 1], [1, 3], [2, 4]] {
            let solid = nominal_brick(&ManifoldRust, &d, studs, &detail).expect("brick");
            assert!(solid.is_manifold());
            let expected = nominal_volume(&d, studs, &detail);
            let rel = ((solid.volume_mm3() - expected) / expected).abs();
            assert!(rel < VOL_TOL, "{studs:?} {detail:?}: {rel}");
        }
    }
    let hull = nominal_brick(
        &ManifoldRust,
        &d,
        [2, 4],
        &Detail {
            segments: None,
            underside: true,
        },
    )
    .expect("hull");
    assert_eq!(
        hull.to_mesh().triangle_count(),
        12,
        "a box has 12 triangles"
    );
}

/// Prints the golden table (run with `--ignored --nocapture`).
#[test]
#[ignore = "prints the golden table; run by hand after an intended geometry change"]
fn golden_table() {
    println!("kernel: {}", ManifoldRust.id());
    for g in GOLDEN {
        let solid = build(g.studs);
        let mesh = solid.to_mesh();
        println!(
            "    Golden {{\n        studs: [{}, {}],\n        triangles: {},\n        fingerprint: 0x{:016x},\n        canonical: 0x{:016x},\n    }},  // volume {:.3} mm3",
            g.studs[0],
            g.studs[1],
            mesh.triangle_count(),
            mesh.fingerprint(),
            mesh.canonical_fingerprint(),
            solid.volume_mm3()
        );
    }
}
