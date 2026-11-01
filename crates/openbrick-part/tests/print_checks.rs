//! The print checks on evaluated parts (docs/plan/05 sections 5 and 6): the
//! measurements of rules MESH.SELF_INTERSECT, ROOF.BRIDGE, OVERHANG.*,
//! ORIENT.FACE and SIZE.BED, taken by the evaluator on the meshes it builds.

mod common;

use common::{build, example_assets, part, process, run_with_assets};
use openbrick_geom::overhang::SPAN_PRECISION_MM;
use openbrick_geom::{self_intersections, Mesh};
use openbrick_part::{validate, validate_on_bed, ParamValues, PartDoc};
use openbrick_print::{example_fit_profile, printer, Bed, Severity};
use serde_json::json;

const LOGO_TILE: &str = include_str!("../../../parts/ndn.logo-tile.obpart.json");

/// The longest bridge of a 2-wide part's roof is the ceiling inside a tube
/// bore: a regular polygon of `studSegments` sides on the bore circle, whose
/// largest inscribed circle has the diameter bore x cos(pi / n).
fn bore_span(p: &openbrick_print::ProcessProfile, bore: f64) -> f64 {
    bore * (std::f64::consts::PI / f64::from(p.stud_segments())).cos()
}

#[test]
fn bricks_plates_and_tiles_print_clean_with_their_roof_bridged() {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let bore = common::mm("tube_inner_diameter") + fit.offsets_mm.tube_inner_diameter;
    for (op, size, studs) in [
        ("body.plate", [2, 4], true),
        ("body.brick", [2, 2], true),
        ("body.tile", [2, 4], false),
        ("body.plate", [1, 4], true),
    ] {
        let what = format!("{op} {size:?}");
        let b = build(&part(op, size, None, studs, true), &p, Some(&fit));
        let m = &b.measurements;
        assert_eq!(m.self_intersections, Some(0), "{what}");
        // Studs up, nothing hangs but the roof between walls and tubes.
        assert_eq!(m.overhangs, Some(Vec::new()), "{what}");
        assert!(!m.face_down, "{what}");
        assert_eq!(
            (m.bed_face_studs, m.bed_face_relief_mm),
            (None, None),
            "{what}"
        );
        let span = m.roof_span_mm.expect("the roof hangs");
        if size[0] >= 2 {
            let want = bore_span(&p, bore);
            assert!(
                span <= want + 1e-6 && span >= want - SPAN_PRECISION_MM,
                "{what}: {span} against {want}"
            );
        } else {
            // A 1-wide cavity between its walls, held by the solid pins.
            let cavity = size[0] as f64 * common::mm("module")
                - 2.0 * common::mm("undersize_per_side")
                - 2.0 * p.quantise_wall(common::mm("side_wall"));
            assert!(span > 4.0 && span <= cavity, "{what}: {span}");
        }
        assert!(span < p.max_unsupported_bridge_mm, "{what}");
        let report = validate(&b, &p, Some(&fit));
        assert!(
            report.findings.iter().all(|f| !f.rule.starts_with("ROOF.")
                && !f.rule.starts_with("OVERHANG.")
                && f.rule != "MESH.SELF_INTERSECT"
                && f.rule != "ORIENT.FACE"),
            "{what}: {:#?}",
            report.findings
        );
    }
}

fn logo_tile(params: ParamValues) -> openbrick_part::Built {
    let doc = PartDoc::parse(LOGO_TILE).expect("the logo tile parses");
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    run_with_assets(&doc, &params, &p, Some(&fit), &example_assets(), false)
        .expect("the logo tile builds")
}

#[test]
fn the_logo_tile_prints_in_the_orientation_its_document_declares() {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    for (relief, style) in [
        ("emboss", "swap"),
        ("deboss", "swap"),
        ("inlay", "swap"),
        ("inlay", "parts"),
    ] {
        let what = format!("{relief} {style}");
        let b = logo_tile(
            ParamValues::new()
                .with("relief", json!(relief))
                .with("inlayStyle", json!(style)),
        );
        let m = &b.measurements;
        let inlay = relief == "inlay";
        assert_eq!(m.self_intersections, Some(0), "{what}");
        assert_eq!(m.relief_mode.as_deref(), Some(relief), "{what}");
        assert_eq!(m.face_down, inlay, "{what}");
        assert_eq!(
            m.print_up.as_deref(),
            Some(if inlay { "bottom" } else { "top" }),
            "{what}"
        );
        if inlay {
            // Face down the face is flat and stud-free, and the roof faces up.
            assert_eq!(m.bed_face_studs, Some(0), "{what}");
            assert_eq!(m.bed_face_relief_mm, Some(0.0), "{what}");
            assert_eq!(m.roof_span_mm, None, "{what}");
        } else {
            assert!(m.roof_span_mm.is_some(), "{what}");
        }
        let overhangs = m.overhangs.as_ref().expect("measured");
        if (relief, style) == ("inlay", "swap") {
            // The face slab leaves the artwork open for its first layers, and
            // the body bridges over it at the slab's depth.
            let depth = m.relief_height_mm.expect("an inlay depth");
            assert!(!overhangs.is_empty(), "{what}");
            for o in overhangs {
                assert!((o.height_mm - depth).abs() < 1e-4, "{what}: {o:?}");
                assert!(o.angle_deg > 89.9, "{what}: {o:?}");
                let span = o.span_mm.expect("held by the slab around it");
                assert!(
                    span > 0.0 && span <= p.max_unsupported_bridge_mm,
                    "{what}: {o:?}"
                );
            }
        } else {
            assert!(overhangs.is_empty(), "{what}: {overhangs:?}");
        }
        let report = validate(&b, &p, Some(&fit));
        assert!(report.export_allowed(), "{what}: {:#?}", report.findings);
        // The sample mark's known FEATURE.MIN warning (1.03 % removed) aside,
        // only the inlay's bridges are reported.
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.rule == "OVERHANG.BRIDGED" || f.rule == "FEATURE.MIN"),
            "{what}: {:#?}",
            report.findings
        );
    }
}

/// Two copies of a mesh in one, the second moved by `by`.
fn with_copy(mesh: &Mesh, by: [f32; 3]) -> Mesh {
    let mut m = mesh.clone();
    let base = m.positions.len() as u32;
    m.positions.extend(
        mesh.positions
            .iter()
            .map(|p| [p[0] + by[0], p[1] + by[1], p[2] + by[2]]),
    );
    m.indices.extend(mesh.indices.iter().map(|i| i + base));
    m
}

#[test]
fn a_built_mesh_overlapping_a_copy_of_itself_is_found() {
    // Property: for any shift that makes the copies overlap in volume, some
    // pair of triangles crosses; for one that keeps them apart, none does.
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let b = build(
        &part("body.plate", [2, 2], None, true, true),
        &p,
        Some(&fit),
    );
    let mesh = &b.objects[0].mesh;
    assert_eq!(self_intersections(mesh).pairs, 0);
    for by in [
        [4.0, 4.0, 1.0],
        [0.3, 0.0, 0.0],
        [8.1, 0.0, 2.9],
        [0.0, -15.0, 0.5],
    ] {
        let r = self_intersections(&with_copy(mesh, by));
        assert!(r.pairs > 0, "{by:?}: {r:?}");
    }
    for by in [[16.0, 0.0, 0.0], [0.0, 0.0, 20.0], [-16.0, -16.0, 0.0]] {
        assert_eq!(self_intersections(&with_copy(mesh, by)).pairs, 0, "{by:?}");
    }
}

#[test]
fn a_bed_is_checked_only_when_given() {
    let p = process("fdm-0.4");
    // A 1x23 plate is 183.8 mm long: no fit-critical features, so no fit
    // profile is needed.
    let b = build(&part("body.plate", [1, 23], None, true, false), &p, None);
    let unchecked = |r: &openbrick_print::Report| -> Vec<String> {
        r.unchecked.iter().map(|u| u.rule.clone()).collect()
    };
    let none = validate(&b, &p, None);
    assert_eq!(unchecked(&none), vec!["SIZE.BED".to_string()]);
    assert!(none.export_allowed(), "{:#?}", none.findings);
    let mini = printer("prusa-mini-plus").expect("bundled").bed();
    let on_mini = validate_on_bed(&b, &p, None, Some(&mini));
    assert!(unchecked(&on_mini).is_empty());
    let size: Vec<_> = on_mini
        .findings
        .iter()
        .filter(|f| f.rule == "SIZE.BED")
        .collect();
    assert_eq!(size.len(), 1, "{:#?}", on_mini.findings);
    assert_eq!(size[0].severity, Severity::Blocker);
    assert_eq!(size[0].threshold, Some(180.0));
    assert!(size[0].message.contains("Original Prusa MINI+"));
    let a1 = printer("bambu-a1").expect("bundled").bed();
    assert!(validate_on_bed(&b, &p, None, Some(&a1)).export_allowed());
    // A hand-given bed turned a quarter: 190 x 10 fits 10 x 190.
    let narrow = Bed::of_size([10.0, 190.0, 10.0]).expect("a bed");
    assert!(validate_on_bed(&b, &p, None, Some(&narrow)).export_allowed());
}

#[test]
fn a_part_declared_face_down_with_studs_is_blocked() {
    let p = process("fdm-0.4");
    let doc = PartDoc::parse(
        r#"{ "openbrick": "1.0", "id": "test.upside", "version": "0.1.0", "name": "upside",
             "license": "CC0-1.0", "credit": { "author": "OpenBrick3D contributors" },
             "features": [
               { "id": "body", "op": "body.plate", "size": [2, 2] },
               { "id": "studs", "op": "stud.field", "on": "body.top" }
             ],
             "print": { "up": "bottom" } }"#,
    )
    .expect("parses");
    let b = build(&doc, &p, None);
    let m = &b.measurements;
    assert_eq!(m.print_up.as_deref(), Some("bottom"));
    assert!(!m.face_down);
    assert_eq!(m.bed_face_studs, Some(4));
    let proud = m.bed_face_relief_mm.expect("measured");
    assert!((proud - common::mm("stud_height")).abs() < 1e-4, "{proud}");
    let report = validate(&b, &p, None);
    let orient: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.rule == "ORIENT.FACE")
        .collect();
    // The declared orientation disagrees with the export, and the studs
    // would stand on the bed.
    assert_eq!(orient.len(), 2, "{:#?}", report.findings);
    assert!(orient
        .iter()
        .any(|f| f.value == Some(4.0) && f.threshold == Some(0.0)));
    assert!(!report.export_allowed());
}
