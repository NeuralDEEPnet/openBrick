//! Kernel conformance corpus (docs/plan/13 Q01, docs/plan/14 backlog item 1).
//!
//! Brick primitives are built from the Dimensional Standard only (no hard-coded
//! brick numbers) and checked for watertightness, topology, analytic volume and
//! determinism. Every future backend must pass the same corpus.

use openbrick_geom::manifold_rust::{ManifoldRust, MrSolid};
use openbrick_geom::{CrossSection, Kernel, Polygon, Solid};
use openbrick_units::STANDARD;

const SEGMENTS: u32 = 64;
/// Allowed relative volume error against the analytic value (0.1 percent).
const VOL_TOL: f64 = 1e-3;
/// Line width of the fdm-0.4 process profile; minimum feature is two lines.
const FDM_LINE_MM: f64 = 0.4;

fn mm(id: &str) -> f64 {
    STANDARD.get().mm(id)
}

fn rel_err(actual: f64, expected: f64) -> f64 {
    ((actual - expected) / expected).abs()
}

fn assert_ok(s: &MrSolid, name: &str, genus: i32, components: usize) {
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

#[test]
fn stud_is_a_watertight_cylinder_with_analytic_volume() {
    let k = ManifoldRust;
    let section = disc(mm("stud_diameter") / 2.0);
    let stud = k.extrude(&section, mm("stud_height"), 1.0).unwrap();
    assert_ok(&stud, "stud", 0, 1);
    let expected = section.area() * mm("stud_height");
    assert!(
        rel_err(stud.volume_mm3(), expected) < VOL_TOL,
        "stud volume {} vs {}",
        stud.volume_mm3(),
        expected
    );
}

#[test]
fn chamfered_stud_via_scale_top_is_smaller_than_plain() {
    let k = ManifoldRust;
    let r = mm("stud_diameter") / 2.0;
    let plain = k.extrude(&disc(r), mm("stud_height"), 1.0).unwrap();
    let tapered = k.extrude(&disc(r), mm("stud_height"), 0.9).unwrap();
    assert_ok(&tapered, "tapered stud", 0, 1);
    assert!(tapered.volume_mm3() < plain.volume_mm3());
}

#[test]
fn tube_is_a_ring_with_analytic_volume() {
    let k = ManifoldRust;
    let section = annulus(
        mm("tube_outer_diameter") / 2.0,
        mm("tube_inner_diameter") / 2.0,
    );
    let h = mm("brick_height") - mm("roof");
    let tube = k.extrude(&section, h, 1.0).unwrap();
    assert_ok(&tube, "tube", 1, 1);
    let expected = section.area() * h;
    assert!(
        rel_err(tube.volume_mm3(), expected) < VOL_TOL,
        "tube volume {} vs {}",
        tube.volume_mm3(),
        expected
    );
}

/// A nominal 2x4 brick: shell, roof, eight studs on top, three tubes under the roof.
/// Ribs and chamfers are omitted; they arrive with the part evaluator (M1).
fn brick_2x4(k: &ManifoldRust) -> (MrSolid, f64) {
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

#[test]
fn brick_2x4_is_one_watertight_genus_zero_body_with_analytic_volume() {
    let k = ManifoldRust;
    let (brick, expected) = brick_2x4(&k);
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

#[test]
fn brick_is_deterministic() {
    let k = ManifoldRust;
    let a = brick_2x4(&k).0.to_mesh();
    let b = brick_2x4(&k).0.to_mesh();
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.triangle_count(), b.triangle_count());
}

#[test]
fn morphological_opening_removes_hairlines() {
    // A 10 mm square with a 0.3 mm hairline attached, opened at the fdm-0.4 minimum
    // feature (two lines): the hairline must disappear and the square survive.
    let k = ManifoldRust;
    let min_feature = 2.0 * FDM_LINE_MM;
    let art = CrossSection::from(vec![
        Polygon::rect([0.0, 0.0], [10.0, 10.0]),
        Polygon::rect([10.0, 4.85], [6.0, 0.3]),
    ]);
    let opened = k
        .offset2d(
            &k.offset2d(&art, -min_feature / 2.0).unwrap(),
            min_feature / 2.0,
        )
        .unwrap();
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

#[test]
fn difference_cuts_a_horizontal_technic_hole() {
    let k = ManifoldRust;
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
    let hole = MrSolid(hole.0.rotate(0.0, 90.0, 0.0));
    let hole = k.translate(&hole, [-1.0, d / 2.0, mm("technic_hole_axis_height")]);
    let drilled = k.difference(&body, &hole).unwrap();
    assert_ok(&drilled, "drilled block", 1, 1);
    assert!(drilled.volume_mm3() < body.volume_mm3());
}
