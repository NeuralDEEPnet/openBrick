//! Golden parts for the evaluator (backlog items 6 to 8 in docs/plan/14): a 2x4
//! tile, 2x4 plate, 2x4 brick, 1x4 plate and 2x2 brick on the nominal fdm-0.4
//! process (no fit profile).
//!
//! Each is checked for: a watertight, manifold, genus-0 single body; the exact
//! polyhedral volume from the independent model in `common::analytic_volume`
//! (relative tolerance 1e-6: the model is exact, so only floating-point error
//! remains, about 1e-12 here); the bounding box (absolute tolerance 1e-4 mm, the
//! mesh stores f32, whose rounding below 64 mm is under 4e-6 mm); connector
//! counts and corner positions; and the canonical mesh fingerprint, recorded
//! below for `manifold-rust 0.19.1`. A fingerprint change means the mesh changed:
//! if that was intended, bump `EVALUATOR_REVISION` and record the new value.

mod common;

use common::*;
use openbrick_part::eval::EVALUATOR_REVISION;
use openbrick_part::Built;
use openbrick_print::Offsets;
use openbrick_units::{BRICK_LDU, HALF_STUD_LDU, PLATE_LDU, STUD_PITCH_LDU};

const VOLUME_TOL: f64 = 1e-6;
const BOUNDS_TOL: f64 = 1e-4;

struct Golden {
    name: &'static str,
    op: &'static str,
    size: [u32; 2],
    plates: u32,
    studs: bool,
    tubes: bool,
    canonical: u64,
}

fn check(g: &Golden) -> Built {
    let p = process("fdm-0.4");
    let doc = part(g.op, g.size, None, g.studs, g.tubes);
    let b = build(&doc, &p, None);
    let mesh = &b.objects[0].mesh;
    let shape = Shape {
        size: g.size,
        plates: g.plates,
        studs: g.studs,
        tubes: g.tubes,
        top_chamfer: 0.0,
    };
    let expected = analytic_volume(shape, &p, &Offsets::default());
    eprintln!(
        "{}: canonical 0x{:016x}, {} triangles, volume {:.6} mm3, analytic {:.6} mm3",
        g.name,
        mesh.canonical_fingerprint(),
        mesh.triangle_count(),
        b.measurements.volume_mm3,
        expected
    );
    assert!(b.warnings.is_empty(), "{}: {:?}", g.name, b.warnings);
    assert_eq!(b.objects.len(), 1, "{}: one object", g.name);
    assert_eq!(b.objects[0].role, "body");
    let m = &b.measurements;
    assert!(m.is_manifold, "{}: manifold", g.name);
    assert_eq!(m.components, 1, "{}: one component", g.name);
    assert_eq!(m.genus, 0, "{}: tube bores are blind, so genus 0", g.name);

    let rel = ((m.volume_mm3 - expected) / expected).abs();
    assert!(
        rel < VOLUME_TOL,
        "{}: volume {} vs analytic {expected} (relative error {rel:e})",
        g.name,
        m.volume_mm3
    );
    // The union never exceeds the sum of its parts; ribs overlap the wall-to-roof
    // chamfer slightly, so the sum is a little larger.
    assert!(m.feature_volume_sum_mm3 >= m.volume_mm3 - 1e-6);
    assert!((m.feature_volume_sum_mm3 - m.volume_mm3) / m.volume_mm3 < 0.01);

    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let brick_plates = (BRICK_LDU / PLATE_LDU) as u32;
    let height = f64::from(g.plates / brick_plates) * mm("brick_height")
        + f64::from(g.plates % brick_plates) * mm("plate_height");
    let top = height + if g.studs { mm("stud_height") } else { 0.0 };
    let want_max = [
        f64::from(g.size[0]) * pitch - 2.0 * gap,
        f64::from(g.size[1]) * pitch - 2.0 * gap,
        top,
    ];
    let axes = m.bounds_min_mm.iter().zip(&m.bounds_max_mm).zip(&want_max);
    for (k, ((lo, hi), want)) in axes.enumerate() {
        assert!(lo.abs() < BOUNDS_TOL, "{}: min[{k}] {lo}", g.name);
        assert!(
            (hi - want).abs() < BOUNDS_TOL,
            "{}: max[{k}] {hi} vs {want}",
            g.name
        );
    }
    assert_eq!(m.body_height_mm, Some(height));

    // Connectors: one per stud position on each face that has a field.
    let [nx, ny] = g.size;
    let cells = (nx * ny) as usize;
    let of = |kind: &str| {
        b.connectors
            .iter()
            .filter(|c| c.kind == kind)
            .collect::<Vec<_>>()
    };
    let (studs, sockets) = (of("studM"), of("tubeF"));
    assert_eq!(studs.len(), if g.studs { cells } else { 0 }, "{}", g.name);
    assert_eq!(sockets.len(), if g.tubes { cells } else { 0 }, "{}", g.name);
    let corner = |y: i32| {
        (
            [
                HALF_STUD_LDU * (1 - nx as i32),
                y,
                HALF_STUD_LDU * (1 - ny as i32),
            ],
            [
                HALF_STUD_LDU * (nx as i32 - 1),
                y,
                HALF_STUD_LDU * (ny as i32 - 1),
            ],
        )
    };
    let height_ldu = g.plates as i32 * PLATE_LDU;
    for (list, y, dir) in [(&studs, 0, [0, -1, 0]), (&sockets, height_ldu, [0, 1, 0])] {
        let Some(first) = list.first() else { continue };
        let (lo, hi) = corner(y);
        assert_eq!(first.pos, lo, "{}: first {} position", g.name, first.kind);
        assert_eq!(
            list.last().map(|c| c.pos),
            Some(hi),
            "{}: last position",
            g.name
        );
        for c in list.iter() {
            assert_eq!(c.dir, dir);
            assert_eq!(
                (c.pos[0] - lo[0]) % STUD_PITCH_LDU,
                0,
                "on the stud lattice"
            );
            assert_eq!(
                (c.pos[2] - lo[2]) % STUD_PITCH_LDU,
                0,
                "on the stud lattice"
            );
        }
    }
    assert!(
        sockets.iter().all(|c| c.fit_critical),
        "the tube field is fit-critical"
    );
    assert!(studs.iter().all(|c| !c.fit_critical));
    let kinds: Vec<&str> = m
        .fit_critical_connectors
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(kinds, if g.tubes { vec!["tubeF"] } else { vec![] });

    assert_eq!(
        mesh.canonical_fingerprint(),
        g.canonical,
        "{}: mesh changed (see the module comment; evaluator revision {EVALUATOR_REVISION})",
        g.name
    );
    b
}

#[test]
fn tile_2x4() {
    let b = check(&Golden {
        name: "tile 2x4",
        op: "body.tile",
        size: [2, 4],
        plates: 1,
        studs: false,
        tubes: true,
        canonical: 0x8be7_cbad_7d05_04ab,
    });
    // Three tubes under a 2x4 (docs/plan/06 section 7) and no pins.
    let m = &b.measurements;
    assert!(m.tube_outer_diameter_mm.is_some() && m.pin_diameter_mm.is_none());
    assert!(m.stud_diameter_mm.is_none(), "tiles have no studs");
}

#[test]
fn plate_2x4() {
    check(&Golden {
        name: "plate 2x4",
        op: "body.plate",
        size: [2, 4],
        plates: 1,
        studs: true,
        tubes: true,
        canonical: 0x27f9_75a6_16c2_d3a2,
    });
}

#[test]
fn brick_2x4() {
    let b = check(&Golden {
        name: "brick 2x4",
        op: "body.brick",
        size: [2, 4],
        plates: (BRICK_LDU / PLATE_LDU) as u32,
        studs: true,
        tubes: true,
        canonical: 0x9820_f037_f760_f42b,
    });
    let m = &b.measurements;
    let p = process("fdm-0.4");
    assert_eq!(m.roof_mm, Some(mm("roof")));
    assert_eq!(m.cavity_depth_mm, Some(mm("brick_height") - mm("roof")));
    assert_eq!(
        m.min_body_wall_mm,
        Some(mm("side_wall")),
        "1.2 mm is three 0.4 mm lines"
    );
    let wall = (mm("tube_outer_diameter") - mm("tube_inner_diameter")) / 2.0;
    assert!((m.min_tube_wall_mm.unwrap() - wall).abs() < 1e-12);
    assert!(m.min_tube_wall_mm.unwrap() >= p.min_tube_wall_mm);
    assert_eq!(m.process, "fdm-0.4");
}

#[test]
fn plate_1x4_has_pins_not_tubes() {
    let b = check(&Golden {
        name: "plate 1x4",
        op: "body.plate",
        size: [1, 4],
        plates: 1,
        studs: true,
        tubes: true,
        canonical: 0x5ae0_4211_65f2_7aac,
    });
    let m = &b.measurements;
    assert_eq!(m.pin_diameter_mm, Some(mm("pin_1wide_diameter")));
    assert!(m.tube_outer_diameter_mm.is_none() && m.min_tube_wall_mm.is_none());
    // Three pins between four studs, centred in the 1-wide cavity: measure their
    // circumradius on a slice through the cavity.
    let p = process("fdm-0.4");
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let mesh = &b.objects[0].mesh;
    let z = (mm("plate_height") - mm("roof")) / 2.0;
    for j in 1..4 {
        let centre = [pitch / 2.0 - gap, f64::from(j) * pitch - gap];
        let (r, _) = ring_radii(
            mesh,
            z,
            centre,
            0.0,
            mm("pin_1wide_diameter"),
            p.stud_segments(),
        );
        assert!(
            (2.0 * r - mm("pin_1wide_diameter")).abs() < 1e-4,
            "pin {j}: diameter {}",
            2.0 * r
        );
    }
}

#[test]
fn brick_2x2() {
    check(&Golden {
        name: "brick 2x2",
        op: "body.brick",
        size: [2, 2],
        plates: (BRICK_LDU / PLATE_LDU) as u32,
        studs: true,
        tubes: true,
        canonical: 0xbc9f_b67d_5d2a_29d5,
    });
}

#[test]
fn evaluation_is_deterministic() {
    let p = process("fdm-0.4");
    let doc = part("body.brick", [2, 4], None, true, true);
    let a = build(&doc, &p, None);
    let b = build(&doc, &p, None);
    assert_eq!(
        a.objects[0].mesh.fingerprint(),
        b.objects[0].mesh.fingerprint()
    );
    assert_eq!(a.content_hash, b.content_hash);
    assert_eq!(a.connectors, b.connectors);
}
