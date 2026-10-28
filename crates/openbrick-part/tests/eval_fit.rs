//! Fit profile offsets reach the geometry (docs/plan/05 sections 1 and 3, backlog
//! item 7): studs, tube bores, tube outer diameters and pins change by exactly the
//! offset, measured on cross-sections of the built mesh; the tube wall never drops
//! below the process minimum; `xyScale` applies last; and the volume change equals
//! the analytic change.
//!
//! Diameters are circumdiameters of the regular polygons the kernel builds (the
//! vertices lie on the nominal circle); the slice recovers them from the polygon
//! area. Tolerance 1e-4 mm: the mesh stores f32, so a vertex can be off by about
//! 4e-6 mm, and the slice adds no error of its own.

mod common;

use common::*;
use openbrick_part::Built;
use openbrick_print::{example_fit_profile, FitProfile, Offsets, ProcessProfile};
use openbrick_units::{BRICK_LDU, PLATE_LDU};

const DIAMETER_TOL: f64 = 1e-4;

/// The example fit profile with its offsets replaced.
fn fit_with(offsets: Offsets) -> FitProfile {
    FitProfile {
        offsets_mm: offsets,
        ..example_fit_profile()
    }
}

/// Stud diameter from a slice through the middle of the straight part of the studs.
fn stud_diameter(b: &Built, p: &ProcessProfile, studs: usize) -> f64 {
    let z = mm("plate_height") + (mm("stud_height") - p.stud_top_chamfer_mm) / 2.0;
    let area = section_area(&b.objects[0].mesh, z, |_| true);
    2.0 * (area / (studs as f64 * ngon(segments(p)))).sqrt()
}

/// (outer, inner) tube diameters from a slice half-way up the cavity, around
/// the first tube of a 2x4 plate.
fn tube_diameters(b: &Built, p: &ProcessProfile) -> (f64, f64) {
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let centre = [pitch - gap, pitch - gap];
    let z = (mm("plate_height") - mm("roof")) / 2.0;
    // Split between the bore and the outside at the nominal mid-wall radius;
    // reach no farther than the gap to the studs' positions (half a pitch away
    // diagonally, well clear of walls and ribs).
    let split = (mm("tube_outer_diameter") + mm("tube_inner_diameter")) / 4.0;
    let (ro, ri) = ring_radii(
        &b.objects[0].mesh,
        z,
        centre,
        split,
        pitch / 2.0,
        segments(p),
    );
    (2.0 * ro, 2.0 * ri)
}

#[test]
fn fit_offsets_change_stud_and_tube_diameters_by_the_offset() {
    let p = process("fdm-0.4");
    let doc = part("body.plate", [2, 4], None, true, true);
    let nominal = build(&doc, &p, None);
    let offsets = Offsets {
        stud_diameter: 0.1,
        tube_inner_diameter: 0.15,
        tube_outer_diameter: 0.136,
        rib_depth: 0.1,
        ..Offsets::default()
    };
    let fit = fit_with(offsets.clone());
    let fitted = build(&doc, &p, Some(&fit));
    assert!(fitted.warnings.is_empty(), "{:?}", fitted.warnings);
    assert!(fitted.measurements.is_manifold && fitted.measurements.components == 1);

    let studs = 8;
    let d0 = stud_diameter(&nominal, &p, studs);
    let d1 = stud_diameter(&fitted, &p, studs);
    assert!(
        (d0 - mm("stud_diameter")).abs() < DIAMETER_TOL,
        "nominal stud {d0}"
    );
    assert!(
        (d1 - (mm("stud_diameter") + offsets.stud_diameter)).abs() < DIAMETER_TOL,
        "stud with offset {d1}"
    );

    let (od0, id0) = tube_diameters(&nominal, &p);
    let (od1, id1) = tube_diameters(&fitted, &p);
    assert!(
        (od0 - mm("tube_outer_diameter")).abs() < DIAMETER_TOL,
        "nominal tube OD {od0}"
    );
    assert!(
        (id0 - mm("tube_inner_diameter")).abs() < DIAMETER_TOL,
        "nominal bore {id0}"
    );
    assert!(
        (od1 - od0 - offsets.tube_outer_diameter).abs() < DIAMETER_TOL,
        "tube OD grew by {} for an offset of {}",
        od1 - od0,
        offsets.tube_outer_diameter
    );
    assert!(
        (id1 - id0 - offsets.tube_inner_diameter).abs() < DIAMETER_TOL,
        "bore grew by {} for an offset of {}",
        id1 - id0,
        offsets.tube_inner_diameter
    );

    // The measurements report the compensated sizes.
    let m = &fitted.measurements;
    assert_eq!(m.fit_profile.as_deref(), Some(fit.id.as_str()));
    assert!((m.stud_diameter_mm.unwrap() - d1).abs() < DIAMETER_TOL);
    assert!((m.tube_outer_diameter_mm.unwrap() - od1).abs() < DIAMETER_TOL);
    assert!((m.tube_inner_diameter_mm.unwrap() - id1).abs() < DIAMETER_TOL);
    let rib = mm("side_wall") + mm("rib_depth") - m.min_body_wall_mm.unwrap() + offsets.rib_depth;
    assert!((m.rib_depth_mm.unwrap() - rib).abs() < 1e-12);

    // Volume: the whole compensated part matches the analytic model, and so does
    // the change against the nominal part.
    let shape = Shape {
        size: [2, 4],
        plates: 1,
        studs: true,
        tubes: true,
        top_chamfer: 0.0,
    };
    let v0 = analytic_volume(shape, &p, &Offsets::default());
    let v1 = analytic_volume(shape, &p, &offsets);
    assert!(
        ((m.volume_mm3 - v1) / v1).abs() < 1e-6,
        "{} vs {v1}",
        m.volume_mm3
    );
    let delta = m.volume_mm3 - nominal.measurements.volume_mm3;
    assert!(
        (delta - (v1 - v0)).abs() < 1e-5,
        "volume change {delta} vs analytic {}",
        v1 - v0
    );
    // Offsets never move connectors: they are lattice positions.
    assert_eq!(nominal.connectors, fitted.connectors);
    assert_ne!(nominal.content_hash, fitted.content_hash);
}

#[test]
fn pin_offset_changes_the_pin_diameter() {
    let p = process("fdm-0.4");
    let doc = part("body.plate", [1, 4], None, true, true);
    let offsets = Offsets {
        pin_diameter: -0.1,
        ..Offsets::default()
    };
    let b = build(&doc, &p, Some(&fit_with(offsets.clone())));
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let z = (mm("plate_height") - mm("roof")) / 2.0;
    let centre = [pitch / 2.0 - gap, pitch - gap];
    let want = mm("pin_1wide_diameter") + offsets.pin_diameter;
    let reach = pin_reach(want, b.measurements.min_body_wall_mm.unwrap());
    let (r, _) = ring_radii(&b.objects[0].mesh, z, centre, 0.0, reach, segments(&p));
    assert!(
        (2.0 * r - want).abs() < DIAMETER_TOL,
        "pin {} vs {want}",
        2.0 * r
    );
    assert_eq!(b.measurements.pin_diameter_mm, Some(want));
}

#[test]
fn tube_wall_after_offsets_stays_at_the_process_minimum() {
    // A bore offset of 0.4 mm with no outer offset would leave a wall of
    // (6.514 - 5.2) / 2 = 0.657 mm, under the 0.7 mm minimum of fdm-0.4, so the
    // outer diameter grows by 2 x (0.7 - 0.657) = 0.086 mm.
    let p = process("fdm-0.4");
    let doc = part("body.plate", [2, 4], None, false, true);
    let offsets = Offsets {
        tube_inner_diameter: 0.4,
        ..Offsets::default()
    };
    let b = build(&doc, &p, Some(&fit_with(offsets.clone())));
    let (od, id) = tube_diameters(&b, &p);
    let wall = (od - id) / 2.0;
    assert!(
        (wall - p.min_tube_wall_mm).abs() < DIAMETER_TOL,
        "measured tube wall {wall} mm"
    );
    assert!(wall >= p.min_tube_wall_mm - DIAMETER_TOL);
    assert!((id - (mm("tube_inner_diameter") + 0.4)).abs() < DIAMETER_TOL);
    let m = &b.measurements;
    assert!((m.min_tube_wall_mm.unwrap() - p.min_tube_wall_mm).abs() < 1e-12);
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("tube outer diameter grown")),
        "{:?}",
        b.warnings
    );
    // The same offsets on the analytic model give the same volume, so the grown
    // tube is exactly what was built.
    let shape = Shape {
        size: [2, 4],
        plates: 1,
        studs: false,
        tubes: true,
        top_chamfer: 0.0,
    };
    let v = analytic_volume(shape, &p, &offsets);
    assert!(((m.volume_mm3 - v) / v).abs() < 1e-6);
}

#[test]
fn xy_scale_applies_last_to_every_horizontal_length() {
    let p = process("fdm-0.4");
    let doc = part("body.brick", [2, 2], None, true, true);
    let scale = 1.01;
    let offsets = Offsets {
        xy_scale: scale,
        ..Offsets::default()
    };
    let nominal = build(&doc, &p, None);
    let scaled = build(&doc, &p, Some(&fit_with(offsets)));
    let (a, b) = (&nominal.measurements, &scaled.measurements);
    for k in 0..2 {
        assert!((b.bounds_max_mm[k] - a.bounds_max_mm[k] * scale).abs() < 1e-4);
    }
    assert!(
        (b.bounds_max_mm[2] - a.bounds_max_mm[2]).abs() < 1e-6,
        "heights do not scale"
    );
    // A uniform XY scale multiplies every volume by the scale squared.
    assert!(((b.volume_mm3 - a.volume_mm3 * scale * scale) / b.volume_mm3).abs() < 1e-6);
    assert!((b.min_body_wall_mm.unwrap() - a.min_body_wall_mm.unwrap() * scale).abs() < 1e-12);
    assert_eq!(nominal.connectors, scaled.connectors);
    assert!(b.is_manifold && b.components == 1 && b.genus == 0);
}

#[test]
fn first_layer_chamfer_takes_the_larger_of_process_and_fit() {
    // The fit profile can ask for a larger chamfer than the process; the volume
    // follows the analytic model with that chamfer.
    let p = process("fdm-0.4");
    let doc = part("body.plate", [2, 2], None, false, false);
    let offsets = Offsets {
        first_layer_chamfer: p.first_layer_chamfer_mm + 0.1,
        ..Offsets::default()
    };
    let b = build(&doc, &p, Some(&fit_with(offsets.clone())));
    let shape = Shape {
        size: [2, 2],
        plates: 1,
        studs: false,
        tubes: false,
        top_chamfer: 0.0,
    };
    let v = analytic_volume(shape, &p, &offsets);
    assert!(((b.measurements.volume_mm3 - v) / v).abs() < 1e-6);
    let smaller = Offsets {
        first_layer_chamfer: 0.0,
        ..Offsets::default()
    };
    let b0 = build(&doc, &p, Some(&fit_with(smaller.clone())));
    let v0 = analytic_volume(shape, &p, &smaller);
    assert!(((b0.measurements.volume_mm3 - v0) / v0).abs() < 1e-6);
    assert!(b.measurements.volume_mm3 < b0.measurements.volume_mm3);
}

#[test]
fn fit_profile_for_another_process_is_reported() {
    let p = process("fdm-0.2");
    let doc = part("body.plate", [1, 2], None, true, true);
    let fit = example_fit_profile();
    assert_ne!(fit.process, p.id);
    let b = build(&doc, &p, Some(&fit));
    assert!(
        b.warnings.iter().any(|w| w.contains("FIT.PROCESS")),
        "{:?}",
        b.warnings
    );
    // fdm-0.2 quantises the wall to whole 0.22 mm lines and deepens the rib so
    // wall plus rib stays the standard side_wall + rib_depth.
    let m = &b.measurements;
    let wall = m.min_body_wall_mm.unwrap();
    assert!(((wall / p.wall_quantise_mm).round() * p.wall_quantise_mm - wall).abs() < 1e-12);
    let total = mm("side_wall") + mm("rib_depth") + fit.offsets_mm.rib_depth;
    assert!((wall + m.rib_depth_mm.unwrap() - total).abs() < 1e-12);
    assert!(m.is_manifold && m.components == 1);
}

#[test]
fn brick_height_bodies_get_the_brick_wall_to_roof_chamfer() {
    // The analytic model uses wallRoofFilletMm.brick for brick-height bodies and
    // .plate otherwise; both volumes agree, so the evaluator picked the same.
    let p = process("fdm-0.4");
    for (plates, op) in [
        (1, "body.plate"),
        ((BRICK_LDU / PLATE_LDU) as u32, "body.brick"),
    ] {
        let doc = part(op, [2, 2], None, false, false);
        let b = build(&doc, &p, None);
        let shape = Shape {
            size: [2, 2],
            plates,
            studs: false,
            tubes: false,
            top_chamfer: 0.0,
        };
        let v = analytic_volume(shape, &p, &Offsets::default());
        assert!(((b.measurements.volume_mm3 - v) / v).abs() < 1e-6, "{op}");
    }
}

#[test]
fn a_rib_offset_that_cancels_the_rib_leaves_the_ribs_out() {
    // On fdm-0.4 the wall is the standard side_wall, so the rib is the standard
    // rib_depth; a ribDepth offset of minus that leaves 1.2 + 0.3 - 1.2 - 0.3 =
    // 5.6e-17 mm in floating point. The ribs are left out with a warning instead
    // of reaching the kernel as a sliver it cannot extrude.
    let p = process("fdm-0.4");
    let doc = part("body.plate", [2, 4], None, true, true);
    let offsets = Offsets {
        rib_depth: -mm("rib_depth"),
        ..Offsets::default()
    };
    let b = build(&doc, &p, Some(&fit_with(offsets.clone())));
    assert!(
        b.warnings.iter().any(|w| w.contains("ribs are left out")),
        "{:?}",
        b.warnings
    );
    let m = &b.measurements;
    assert_eq!(m.rib_depth_mm, None);
    assert!(m.is_manifold && m.components == 1);
    let shape = Shape {
        size: [2, 4],
        plates: 1,
        studs: true,
        tubes: true,
        top_chamfer: 0.0,
    };
    let v = analytic_volume(shape, &p, &offsets);
    assert!(
        ((m.volume_mm3 - v) / v).abs() < 1e-6,
        "{} vs {v}",
        m.volume_mm3
    );
    // The nominal part is larger by exactly its ribs.
    let nominal = build(&doc, &p, None);
    assert!(nominal.measurements.volume_mm3 > m.volume_mm3);
}

#[test]
fn first_layer_chamfers_are_measured_at_the_bed_and_warned_when_too_thin() {
    let p = process("fdm-0.4");
    let wall = mm("side_wall");
    // Body: the widest chamfer that still leaves some wall (2c < wall) builds,
    // but its first layer is far narrower than one line.
    let c = wall / 2.0 - 0.01;
    let doc = part("body.plate", [2, 2], None, false, false);
    let offsets = Offsets {
        first_layer_chamfer: c,
        ..Offsets::default()
    };
    let b = build(&doc, &p, Some(&fit_with(offsets.clone())));
    let m = &b.measurements;
    assert_eq!(m.first_layer_chamfer_mm, Some(c));
    assert_eq!(
        m.min_body_wall_mm,
        Some(wall),
        "the full wall above the chamfer"
    );
    assert!((m.min_body_wall_at_bed_mm.unwrap() - (wall - 2.0 * c)).abs() < 1e-12);
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("`body`") && w.contains("narrower than one")),
        "{:?}",
        b.warnings
    );
    let shape = Shape {
        size: [2, 2],
        plates: 1,
        studs: false,
        tubes: false,
        top_chamfer: 0.0,
    };
    let v = analytic_volume(shape, &p, &offsets);
    assert!(((m.volume_mm3 - v) / v).abs() < 1e-6);

    // Tubes: a bore offset that takes the tube wall down to minTubeWallMm, with a
    // larger chamfer. The tube chamfer is capped at a third of the wall, and the
    // first layer (half the thinnest layer above the bed) is narrower than a line.
    let doc = part("body.plate", [2, 4], None, false, true);
    let grow =
        2.0 * ((mm("tube_outer_diameter") - mm("tube_inner_diameter")) / 2.0 - p.min_tube_wall_mm);
    let offsets = Offsets {
        tube_inner_diameter: grow,
        first_layer_chamfer: p.first_layer_chamfer_mm + 0.1,
        ..Offsets::default()
    };
    let b = build(&doc, &p, Some(&fit_with(offsets.clone())));
    let m = &b.measurements;
    let tube_wall = m.min_tube_wall_mm.unwrap();
    assert!((tube_wall - p.min_tube_wall_mm).abs() < 1e-12);
    let ct = offsets.first_layer_chamfer.min(tube_wall / 3.0);
    assert!((m.min_tube_wall_at_bed_mm.unwrap() - (tube_wall - 2.0 * ct)).abs() < 1e-12);
    let layer = p
        .layer_height_mm
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let first = tube_wall - 2.0 * (ct - layer / 2.0);
    assert!(first < p.line_width_mm, "the case under test: {first}");
    assert!(
        b.warnings.iter().any(|w| w.contains("`bottom`")
            && w.contains("tube wall")
            && w.contains("narrower than one")),
        "{:?}",
        b.warnings
    );
    assert!(
        !b.warnings.iter().any(|w| w.contains("`body`")),
        "the body wall keeps more than a line: {:?}",
        b.warnings
    );
    let shape = Shape {
        size: [2, 4],
        plates: 1,
        studs: false,
        tubes: true,
        top_chamfer: 0.0,
    };
    let v = analytic_volume(shape, &p, &offsets);
    assert!(((m.volume_mm3 - v) / v).abs() < 1e-6);
}
