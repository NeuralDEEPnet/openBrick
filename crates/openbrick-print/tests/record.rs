//! `record` (docs/plan/05 section 4): chosen coupon cells become a fit profile
//! with offsets equal to chosen minus nominal, and only cells of the coupon grid
//! are accepted.

use std::collections::BTreeMap;

use openbrick_print::{
    coupon_v1, example_fit_profile, is_profile_id, record, FitProfile, Offsets, RecordError,
    RecordInput, GRID_TOLERANCE_MM,
};
use openbrick_units::STANDARD;

fn input(cells: &[(&str, f64)]) -> RecordInput {
    RecordInput {
        process: "fdm-0.4".into(),
        printer: "Prusa MK4S".into(),
        nozzle_mm: 0.4,
        material: "PETG".into(),
        slicer: "PrusaSlicer 2.9.6".into(),
        intent: "fits-genuine-bricks".into(),
        cells: cells.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        ..RecordInput::default()
    }
}

/// The first value of a coupon cell.
fn first(cell: &str) -> f64 {
    coupon_v1().cell(cell).expect("coupon cell").start_mm
}

#[test]
fn the_worked_examples_of_the_plan() {
    // The worked examples of docs/plan/05 section 4, written out as the plan
    // gives them: stud 4.80 - 4.80 = 0.00; tube 4.95 - 4.80 = +0.15;
    // pin 4.70 - 4.80 = -0.10.
    let f = record(&input(&[("stud", 4.80), ("tube", 4.95), ("pin", 4.70)])).expect("records");
    assert_eq!(f.offsets_mm.stud_diameter, 0.0);
    assert_eq!(f.offsets_mm.tube_inner_diameter, 0.15);
    assert_eq!(f.offsets_mm.pin_diameter, -0.1);
    // Offsets of cells not chosen stay nominal.
    assert_eq!(f.offsets_mm.tube_outer_diameter, 0.0);
    assert_eq!(f.offsets_mm.xy_scale, 1.0);
    let c = f.calibration.as_ref().expect("calibration");
    assert_eq!(c.coupon_version, 1);
    assert_eq!(c.chosen_cells.len(), 3);
    assert_eq!(c.chosen_cells["tube"], 4.95);
    assert!(c.date.is_empty());
    assert_eq!(f.id, "prusa-mk4s-petg-fdm-0.4");
    assert_eq!(f.process, "fdm-0.4");
    assert_eq!(f.material["type"], "PETG");
}

#[test]
fn the_example_fit_profile_is_what_record_writes() {
    let example = example_fit_profile();
    let cal = example.calibration.as_ref().expect("calibrated");
    let mut i = input(&[]);
    i.cells = cal.chosen_cells.clone();
    i.id = Some(example.id.clone());
    i.date = Some(cal.date.clone());
    i.brand = example.material["brand"].as_str().map(String::from);
    let f = record(&i).expect("the example cells lie on the coupon grid");
    for key in Offsets::KEYS {
        assert_eq!(
            f.offsets_mm.get(key),
            example.offsets_mm.get(key),
            "{key}: chosen minus nominal"
        );
    }
    // The pocket row's tube OD is measured from the standard's tube outer
    // diameter (6.65 - 6.514 = 0.136 for the example).
    let od = cal.chosen_cells["pocketTubeOD"] - STANDARD.get().mm("tube_outer_diameter");
    assert!((f.offsets_mm.tube_outer_diameter - od).abs() < 1e-9);
    assert_eq!(
        f.calibration.as_ref().expect("cal").chosen_cells,
        cal.chosen_cells
    );
    assert_eq!(f.id, example.id);
    assert_eq!(f.calibration.as_ref().expect("cal").date, "2026-10-27");
    assert_eq!(f.material["brand"], "example");
}

#[test]
fn every_grid_value_round_trips_through_its_offset() {
    // Exhaustive over the whole coupon, in place of a property test:
    // for every cell value on the grid, nominal + recorded offset gives the
    // chosen value back, and the written numbers carry no float noise.
    let spec = coupon_v1();
    let mut tested = 0;
    for cell in spec.cells.iter().filter(|c| c.offset_field.is_some()) {
        let field = cell.offset_field.as_deref().expect("recordable");
        let nominal = STANDARD.get().mm(&cell.standard_id);
        let values = cell.values();
        assert_eq!(values.first(), Some(&cell.start_mm), "{}", cell.id);
        assert_eq!(values.last(), Some(&cell.end_mm), "{}", cell.id);
        for w in values.windows(2) {
            assert!(
                (w[1] - w[0] - cell.step_mm).abs() < GRID_TOLERANCE_MM,
                "{}: neighbouring cells differ by the step",
                cell.id
            );
        }
        for v in values {
            let f = record(&input(&[(&cell.id, v)]))
                .unwrap_or_else(|e| panic!("{} = {v}: {e}", cell.id));
            let offset = f.offsets_mm.get(field).expect("offset field");
            assert!(
                (nominal + offset - v).abs() < 1e-9,
                "{}: {nominal} + {offset} != {v}",
                cell.id
            );
            // Written with at most three decimals.
            assert_eq!(offset, (offset * 1e3).round() / 1e3, "{}", cell.id);
            let written = serde_json::to_string(&f.offsets_mm).expect("serialises");
            assert!(
                !written.contains("0000"),
                "{}: float noise in {written}",
                cell.id
            );
            // Within the grid tolerance the value snaps to the cell.
            for nudge in [-0.9 * GRID_TOLERANCE_MM, 0.9 * GRID_TOLERANCE_MM] {
                let g = record(&input(&[(&cell.id, v + nudge)])).expect("within tolerance");
                assert_eq!(g.offsets_mm.get(field), Some(offset), "{}", cell.id);
                assert_eq!(
                    g.calibration.expect("cal").chosen_cells[&cell.id],
                    v,
                    "{}",
                    cell.id
                );
            }
            tested += 1;
        }
    }
    // stud 8 + tube 6 + pocketTubeOD 6 + pocketRib 7 + technicH 7 + pin 6 +
    // axle 6 + bar 4 + clip 5 = 55 (the reference-only axleTips row adds 6 more).
    assert_eq!(tested, 55, "every recordable value of coupon v1 was tested");
}

#[test]
fn values_off_the_grid_are_refused_with_the_valid_range() {
    let stud = coupon_v1().cell("stud").expect("stud cell");
    for bad in [
        stud.start_mm - stud.step_mm,
        stud.end_mm + stud.step_mm,
        stud.start_mm + stud.step_mm / 2.0,
        stud.start_mm + 3.0 * GRID_TOLERANCE_MM,
        f64::NAN,
        f64::INFINITY,
    ] {
        let err = record(&input(&[("stud", bad)])).expect_err("off the grid");
        assert!(matches!(err, RecordError::OffGrid { .. }), "{bad}: {err}");
        let msg = err.to_string();
        let range = format!(
            "from {} to {} mm in steps of {} mm",
            stud.start_mm, stud.end_mm, stud.step_mm
        );
        assert!(msg.contains(&range), "{msg}");
    }
    // The axle example of docs/plan/05 section 3 (4.86) belongs to the 0.02 mm
    // fine pass, which coupon v1 does not have: 4.86 - 4.75 = 0.11 is 2.2 steps
    // of 0.05.
    let axle = coupon_v1().cell("axle").expect("axle cell");
    let err = record(&input(&[("axle", 4.86)])).expect_err("not a v1 cell");
    let range = format!("from {} to {} mm", axle.start_mm, axle.end_mm);
    assert!(err.to_string().contains(&range), "{err}");
}

#[test]
fn unknown_and_reference_only_cells_are_refused() {
    let err = record(&input(&[("studs", first("stud"))])).expect_err("unknown");
    assert!(matches!(err, RecordError::UnknownCell { .. }));
    assert!(
        err.to_string().contains("stud, tube, pocketTubeOD"),
        "{err}"
    );
    let err = record(&input(&[("axleTips", first("axleTips"))])).expect_err("reference only");
    assert!(matches!(err, RecordError::NotRecordable { .. }), "{err}");
    assert_eq!(
        record(&input(&[])).expect_err("empty"),
        RecordError::NoCells
    );
}

#[test]
fn inputs_are_checked() {
    let ok = input(&[("stud", first("stud"))]);
    type IsError = fn(&RecordError) -> bool;
    let cases: Vec<(RecordInput, IsError)> = vec![
        (
            RecordInput {
                intent: "tight".into(),
                ..ok.clone()
            },
            |e| matches!(e, RecordError::Intent(_)),
        ),
        (
            RecordInput {
                nozzle_mm: 0.0,
                ..ok.clone()
            },
            |e| matches!(e, RecordError::Nozzle(_)),
        ),
        (
            RecordInput {
                printer: "  ".into(),
                ..ok.clone()
            },
            |e| matches!(e, RecordError::Empty { field: "printer" }),
        ),
        (
            RecordInput {
                process: "FDM 0.4".into(),
                ..ok.clone()
            },
            |e| matches!(e, RecordError::Id(_)),
        ),
        (
            RecordInput {
                id: Some("My Profile".into()),
                ..ok.clone()
            },
            |e| matches!(e, RecordError::Id(_)),
        ),
        (
            RecordInput {
                date: Some("12.11.2026".into()),
                ..ok.clone()
            },
            |e| matches!(e, RecordError::Date(_)),
        ),
    ];
    for (bad, is) in cases {
        let err = record(&bad).expect_err("refused");
        assert!(is(&err), "{err:?}");
    }
    let f = record(&RecordInput {
        id: Some("my-mk4s-petg".into()),
        date: Some("2026-11-12".into()),
        author: Some("Ada".into()),
        notes: Some("3 walls".into()),
        ..ok
    })
    .expect("records");
    assert_eq!(f.id, "my-mk4s-petg");
    assert_eq!(f.calibration.as_ref().expect("cal").date, "2026-11-12");
    assert_eq!(f.credit.as_ref().expect("credit").author, "Ada");
    assert_eq!(f.notes, "3 walls");
}

#[test]
fn the_written_profile_reads_back_and_omits_what_is_absent() {
    let mut cells = BTreeMap::new();
    for c in coupon_v1()
        .cells
        .iter()
        .filter(|c| c.offset_field.is_some())
    {
        cells.insert(c.id.clone(), c.end_mm);
    }
    let f = record(&RecordInput {
        cells,
        ..input(&[])
    })
    .expect("records");
    assert!(is_profile_id(&f.id));
    let text = serde_json::to_string_pretty(&f).expect("serialises");
    let v: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(v["kind"], "fit");
    assert_eq!(v["credit"]["author"], "anonymous");
    assert!(v["calibration"].get("date").is_none(), "no empty date");
    assert!(v["material"].get("brand").is_none(), "no empty brand");
    let back: FitProfile = serde_json::from_str(&text).expect("reads back");
    assert_eq!(back.offsets_mm, f.offsets_mm);
    assert_eq!(
        back.calibration.expect("cal").chosen_cells,
        f.calibration.expect("cal").chosen_cells
    );
}
