//! Kernel conformance corpus (docs/plan/13 Q01, docs/plan/14 backlog items 1 and 2).
//!
//! Brick primitives are built from the Dimensional Standard only (no hard-coded
//! brick numbers) and checked for watertightness, topology, analytic volume and
//! determinism. Every check is a generic function over the kernel; the `corpus!`
//! macro at the end turns each one into a `#[test]` per enabled backend
//! (`manifold_rust::*` by default, `manifold_csg::*` with `--features manifold-csg`),
//! so every backend passes the same corpus. With both backends enabled,
//! `backends_agree_on_volume_and_topology` compares them shape by shape; run it with
//! `-- --nocapture` to see the table in docs/spikes/q01b-manifold-csg-comparison.md.

use openbrick_geom::{CrossSection, Kernel, Polygon, Solid};
use openbrick_units::STANDARD;

const SEGMENTS: u32 = 64;
/// Allowed relative volume error against the analytic value (0.1 percent).
const VOL_TOL: f64 = 1e-3;
/// Line width of the fdm-0.4 process profile; minimum feature is two lines.
const FDM_LINE_MM: f64 = 0.4;

/// What the corpus needs beyond the `Kernel` trait. Rotation is not in the trait
/// yet (the part evaluator will decide how lattice rotations reach the kernel), so
/// each backend supplies it here for the Technic hole check.
trait Backend: Kernel {
    /// Rotate by Euler angles in degrees, Manifold convention (x, then y, then z);
    /// multiples of 90 degrees are exact in both backends.
    fn rotate(&self, solid: &Self::Solid, deg: [f64; 3]) -> Self::Solid;
}

#[cfg(feature = "manifold-rust")]
impl Backend for openbrick_geom::manifold_rust::ManifoldRust {
    fn rotate(
        &self,
        solid: &openbrick_geom::manifold_rust::MrSolid,
        [x, y, z]: [f64; 3],
    ) -> openbrick_geom::manifold_rust::MrSolid {
        openbrick_geom::manifold_rust::MrSolid(solid.0.rotate(x, y, z))
    }
}

#[cfg(feature = "manifold-csg")]
impl Backend for openbrick_geom::manifold_csg::ManifoldCsg {
    fn rotate(
        &self,
        solid: &openbrick_geom::manifold_csg::CsgSolid,
        [x, y, z]: [f64; 3],
    ) -> openbrick_geom::manifold_csg::CsgSolid {
        openbrick_geom::manifold_csg::CsgSolid(solid.0.rotate(x, y, z))
    }
}

fn mm(id: &str) -> f64 {
    STANDARD.get().mm(id)
}

fn rel_err(actual: f64, expected: f64) -> f64 {
    ((actual - expected) / expected).abs()
}

fn assert_ok<S: Solid>(s: &S, name: &str, genus: i32, components: usize) {
    assert!(s.is_manifold(), "{name}: not manifold");
    assert_eq!(s.genus(), genus, "{name}: genus");
    assert_eq!(s.component_count(), components, "{name}: components");
}

fn disc(r: f64) -> CrossSection {
    CrossSection::from(vec![Polygon::circle([0.0, 0.0], r, SEGMENTS)])
}

fn annulus(r_out: f64, r_in: f64) -> CrossSection {
    CrossSection::from(vec![
        Polygon::circle([0.0, 0.0], r_out, SEGMENTS),
        Polygon::circle([0.0, 0.0], r_in, SEGMENTS).reversed(),
    ])
}

// ---------------------------------------------------------------------------
// Corpus shapes. Each builder returns the solid and, where one exists, its
// analytic volume (the polygon area times the height, so faceting is not an error).
// ---------------------------------------------------------------------------

fn stud<K: Kernel>(k: &K) -> (K::Solid, f64) {
    let section = disc(mm("stud_diameter") / 2.0);
    let stud = k.extrude(&section, mm("stud_height"), 1.0).unwrap();
    (stud, section.area() * mm("stud_height"))
}

/// The chamfer route in D03: a stud extruded with `scale_top` 0.9.
fn tapered_stud<K: Kernel>(k: &K) -> K::Solid {
    k.extrude(&disc(mm("stud_diameter") / 2.0), mm("stud_height"), 0.9)
        .unwrap()
}

fn tube<K: Kernel>(k: &K) -> (K::Solid, f64) {
    let section = annulus(
        mm("tube_outer_diameter") / 2.0,
        mm("tube_inner_diameter") / 2.0,
    );
    let h = mm("brick_height") - mm("roof");
    let tube = k.extrude(&section, h, 1.0).unwrap();
    (tube, section.area() * h)
}

/// A stud made by revolving its half profile (radius by height) about the axis.
/// With the same segment count it is the same prism as the extruded stud, so its
/// analytic volume is the polygon area of the disc times the height.
fn revolved_stud<K: Kernel>(k: &K) -> (K::Solid, f64) {
    let r = mm("stud_diameter") / 2.0;
    let h = mm("stud_height");
    let solid = k
        .revolve(&Polygon::rect([0.0, 0.0], [r, h]), SEGMENTS)
        .unwrap();
    (solid, disc(r).area() * h)
}

/// A nominal 2x4 brick: shell, roof, eight studs on top, three tubes under the roof.
/// Ribs and chamfers are omitted; they arrive with the part evaluator (M1).
fn brick_2x4<K: Kernel>(k: &K) -> (K::Solid, f64) {
    let pitch = mm("module");
    let gap = mm("undersize_per_side");
    let wall = mm("side_wall");
    let roof = mm("roof");
    let height = mm("brick_height");
    let (w, d) = (2.0 * pitch - 2.0 * gap, 4.0 * pitch - 2.0 * gap);
    let cavity_h = height - roof;

    let outer = Polygon::rect([0.0, 0.0], [w, d]);
    let inner = Polygon::rect([wall, wall], [w - 2.0 * wall, d - 2.0 * wall]).reversed();
    let shell_section = CrossSection::from(vec![outer.clone(), inner]);
    let shell = k.extrude(&shell_section, cavity_h, 1.0).unwrap();
    let roof_solid = k.translate(
        &k.extrude(&CrossSection::from(vec![outer.clone()]), roof, 1.0)
            .unwrap(),
        [0.0, 0.0, cavity_h],
    );

    let stud = k
        .extrude(&disc(mm("stud_diameter") / 2.0), mm("stud_height"), 1.0)
        .unwrap();
    let mut parts = vec![shell, roof_solid];
    let centre = |i: f64| i * pitch + pitch / 2.0 - gap;
    for ix in 0..2 {
        for iy in 0..4 {
            parts.push(k.translate(&stud, [centre(ix as f64), centre(iy as f64), height]));
        }
    }
    // Tubes at the interior 2x2 stud intersections.
    let tube_section = annulus(
        mm("tube_outer_diameter") / 2.0,
        mm("tube_inner_diameter") / 2.0,
    );
    let tube = k.extrude(&tube_section, cavity_h, 1.0).unwrap();
    for iy in 1..4 {
        parts.push(k.translate(&tube, [pitch - gap, iy as f64 * pitch - gap, 0.0]));
    }
    let brick = k.union_all(&parts).unwrap();

    let expected = shell_section.area() * cavity_h
        + CrossSection::from(vec![outer]).area() * roof
        + 8.0 * disc(mm("stud_diameter") / 2.0).area() * mm("stud_height")
        + 3.0 * tube_section.area() * cavity_h;
    (brick, expected)
}

/// A 1x2 block with a horizontal Technic hole at the standard axis height.
/// Returns (undrilled body, drilled block).
fn drilled_block<K: Backend>(k: &K) -> (K::Solid, K::Solid) {
    let pitch = mm("module");
    let gap = mm("undersize_per_side");
    let (w, d) = (2.0 * pitch - 2.0 * gap, pitch - 2.0 * gap);
    let body = k
        .extrude(
            &CrossSection::from(vec![Polygon::rect([0.0, 0.0], [w, d])]),
            mm("brick_height"),
            1.0,
        )
        .unwrap();
    let hole = k
        .extrude(&disc(mm("technic_hole_diameter") / 2.0), w + 2.0, 1.0)
        .unwrap();
    // Lay the hole along X at the standard axis height.
    let hole = k.rotate(&hole, [0.0, 90.0, 0.0]);
    let hole = k.translate(&hole, [-1.0, d / 2.0, mm("technic_hole_axis_height")]);
    let drilled = k.difference(&body, &hole).unwrap();
    (body, drilled)
}

/// A 10 mm square with a 0.3 mm hairline attached, opened (inset then outset) at the
/// fdm-0.4 minimum feature of two lines.
fn opened_artwork<K: Kernel>(k: &K) -> CrossSection {
    let min_feature = 2.0 * FDM_LINE_MM;
    let art = CrossSection::from(vec![
        Polygon::rect([0.0, 0.0], [10.0, 10.0]),
        Polygon::rect([10.0, 4.85], [6.0, 0.3]),
    ]);
    k.offset2d(
        &k.offset2d(&art, -min_feature / 2.0).unwrap(),
        min_feature / 2.0,
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Checks, one per corpus test. Each runs against any backend.
// ---------------------------------------------------------------------------

fn stud_is_a_watertight_cylinder_with_analytic_volume<K: Kernel>(k: &K) {
    let (stud, expected) = stud(k);
    assert_ok(&stud, "stud", 0, 1);
    assert!(
        rel_err(stud.volume_mm3(), expected) < VOL_TOL,
        "stud volume {} vs {}",
        stud.volume_mm3(),
        expected
    );
}

fn chamfered_stud_via_scale_top_is_smaller_than_plain<K: Kernel>(k: &K) {
    let (plain, _) = stud(k);
    let tapered = tapered_stud(k);
    assert_ok(&tapered, "tapered stud", 0, 1);
    assert!(tapered.volume_mm3() < plain.volume_mm3());
}

fn tube_is_a_ring_with_analytic_volume<K: Kernel>(k: &K) {
    let (tube, expected) = tube(k);
    assert_ok(&tube, "tube", 1, 1);
    assert!(
        rel_err(tube.volume_mm3(), expected) < VOL_TOL,
        "tube volume {} vs {}",
        tube.volume_mm3(),
        expected
    );
}

fn revolved_stud_matches_the_extruded_prism<K: Kernel>(k: &K) {
    let (stud, expected) = revolved_stud(k);
    assert_ok(&stud, "revolved stud", 0, 1);
    assert!(
        rel_err(stud.volume_mm3(), expected) < VOL_TOL,
        "revolved stud volume {} vs {}",
        stud.volume_mm3(),
        expected
    );
}

fn brick_2x4_is_one_watertight_genus_zero_body_with_analytic_volume<K: Kernel>(k: &K) {
    let (brick, expected) = brick_2x4(k);
    // Tube bores are blind holes closed by the roof, so the body stays genus 0.
    assert_ok(&brick, "2x4 brick", 0, 1);
    assert!(
        rel_err(brick.volume_mm3(), expected) < VOL_TOL,
        "brick volume {} vs {}",
        brick.volume_mm3(),
        expected
    );
    let (lo, hi) = brick.to_mesh().bounds();
    let pitch = mm("module");
    let gap = mm("undersize_per_side");
    let tol = 1e-4;
    assert!((((hi[0] - lo[0]) as f64) - (2.0 * pitch - 2.0 * gap)).abs() < tol);
    assert!((((hi[1] - lo[1]) as f64) - (4.0 * pitch - 2.0 * gap)).abs() < tol);
    assert!((((hi[2] - lo[2]) as f64) - (mm("brick_height") + mm("stud_height"))).abs() < tol);
}

fn brick_is_deterministic<K: Kernel>(k: &K) {
    let a = brick_2x4(k).0.to_mesh();
    let b = brick_2x4(k).0.to_mesh();
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.triangle_count(), b.triangle_count());
}

fn morphological_opening_removes_hairlines<K: Kernel>(k: &K) {
    // A 10 mm square with a 0.3 mm hairline attached, opened at the fdm-0.4 minimum
    // feature (two lines): the hairline must disappear and the square survive.
    let opened = opened_artwork(k);
    let max_x = opened
        .polygons
        .iter()
        .flat_map(|p| p.points.iter())
        .map(|p| p[0])
        .fold(f64::MIN, f64::max);
    // The 6 mm hairline must be gone. A true opening keeps a nub of about 0.03 mm
    // where the hairline met the square: at the concave corners the inset boundary
    // stays at x = 10 - sqrt(0.4^2 - 0.15^2) = 9.629, and the outset returns it to
    // about 10.03. That is far below the printable minimum, so allow 0.05 mm.
    assert!(max_x < 10.0 + 0.05, "hairline survived: max x {max_x}");
    assert!(
        opened.area() > 99.0,
        "square lost too much area: {}",
        opened.area()
    );
}

fn difference_cuts_a_horizontal_technic_hole<K: Backend>(k: &K) {
    let (body, drilled) = drilled_block(k);
    assert_ok(&drilled, "drilled block", 1, 1);
    assert!(drilled.volume_mm3() < body.volume_mm3());
}

/// Instantiates every check above as a `#[test]` for one backend.
macro_rules! corpus {
    ($kernel:expr) => {
        corpus!(@each $kernel;
            stud_is_a_watertight_cylinder_with_analytic_volume,
            chamfered_stud_via_scale_top_is_smaller_than_plain,
            tube_is_a_ring_with_analytic_volume,
            revolved_stud_matches_the_extruded_prism,
            brick_2x4_is_one_watertight_genus_zero_body_with_analytic_volume,
            brick_is_deterministic,
            morphological_opening_removes_hairlines,
            difference_cuts_a_horizontal_technic_hole,
        );
    };
    (@each $kernel:expr; $($check:ident),+ $(,)?) => {
        $(
            #[test]
            fn $check() {
                super::$check(&$kernel);
            }
        )+
    };
}

#[cfg(feature = "manifold-rust")]
mod manifold_rust {
    corpus!(openbrick_geom::manifold_rust::ManifoldRust);
}

#[cfg(feature = "manifold-csg")]
mod manifold_csg {
    corpus!(openbrick_geom::manifold_csg::ManifoldCsg);
}

// ---------------------------------------------------------------------------
// Cross-backend comparison (docs/plan/14 backlog item 2, decision D02).
// ---------------------------------------------------------------------------

/// Every corpus solid, built on one backend.
#[cfg(all(feature = "manifold-rust", feature = "manifold-csg"))]
fn corpus_solids<K: Backend>(k: &K) -> Vec<(&'static str, K::Solid)> {
    vec![
        ("stud", stud(k).0),
        ("tapered stud", tapered_stud(k)),
        ("tube", tube(k).0),
        ("revolved stud", revolved_stud(k).0),
        ("2x4 brick", brick_2x4(k).0),
        ("drilled block", drilled_block(k).1),
    ]
}

/// Volumes must agree to 1e-6 relative and topology exactly. Triangle counts and
/// fingerprints are recorded, not asserted: D02 does not require identical meshes,
/// only that hashes from different kernels are never mixed (the kernel id is part of
/// every content hash).
#[cfg(all(feature = "manifold-rust", feature = "manifold-csg"))]
#[test]
fn backends_agree_on_volume_and_topology() {
    use openbrick_geom::manifold_csg::ManifoldCsg;
    use openbrick_geom::manifold_rust::ManifoldRust;
    const CROSS_TOL: f64 = 1e-6;

    let (kr, kc) = (ManifoldRust, ManifoldCsg);
    println!("A = {}, B = {}", kr.id(), kc.id());
    println!(
        "{:<14} {:>12} {:>12} {:>8} {:>5} {:>5} {:>6} {:>6} {:>18} {:>18} {:>18} {:>18}",
        "shape",
        "volume A",
        "volume B",
        "rel diff",
        "genus",
        "comps",
        "tris A",
        "tris B",
        "fingerprint A",
        "fingerprint B",
        "canonical A",
        "canonical B"
    );
    for ((name, a), (_, b)) in corpus_solids(&kr).iter().zip(corpus_solids(&kc).iter()) {
        let (ma, mb) = (a.to_mesh(), b.to_mesh());
        let (va, vb) = (a.volume_mm3(), b.volume_mm3());
        println!(
            "{:<14} {:>12.6} {:>12.6} {:>8.1e} {:>5} {:>5} {:>6} {:>6} 0x{:016x} 0x{:016x} 0x{:016x} 0x{:016x}",
            name,
            va,
            vb,
            rel_err(vb, va),
            a.genus(),
            a.component_count(),
            ma.triangle_count(),
            mb.triangle_count(),
            ma.fingerprint(),
            mb.fingerprint(),
            ma.canonical_fingerprint(),
            mb.canonical_fingerprint(),
        );
        assert!(a.is_manifold() && b.is_manifold(), "{name}: not manifold");
        assert!(
            rel_err(vb, va) < CROSS_TOL,
            "{name}: volume {va} (A) vs {vb} (B)"
        );
        assert_eq!(a.genus(), b.genus(), "{name}: genus");
        assert_eq!(
            a.component_count(),
            b.component_count(),
            "{name}: components"
        );
    }

    // The 2D opening goes through Clipper2 in both kernels (a Rust port in A, the
    // C++ library in B). Recorded only: its own check above holds on each backend.
    let (oa, ob) = (opened_artwork(&kr), opened_artwork(&kc));
    let verts = |c: &CrossSection| c.polygons.iter().map(|p| p.points.len()).sum::<usize>();
    println!(
        "opening        area A {:.9} area B {:.9} rel diff {:.1e}; vertices A {} B {}; identical {}",
        oa.area(),
        ob.area(),
        rel_err(ob.area(), oa.area()),
        verts(&oa),
        verts(&ob),
        oa == ob
    );
}
