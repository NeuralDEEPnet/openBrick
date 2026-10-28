//! The validator on evaluated parts (docs/plan/05 section 5): a fit-critical
//! 2x4 tile exports with a calibrated fit profile and is blocked without one.

mod common;

use common::{build, part, process};
use openbrick_part::validate;
use openbrick_print::{example_fit_profile, Severity};

#[test]
fn the_2x4_tile_with_the_example_fit_profile_may_be_exported() {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let doc = part("body.tile", [2, 4], None, false, true);
    let built = build(&doc, &p, Some(&fit));
    let report = validate(&built, &p, Some(&fit));
    assert!(report.export_allowed(), "{:#?}", report.findings);
    // The calibrated tile passes every rule outright, warnings included.
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

#[test]
fn the_2x4_tile_without_a_fit_profile_is_blocked_by_fit_missing() {
    let p = process("fdm-0.4");
    let doc = part("body.tile", [2, 4], None, false, true);
    let built = build(&doc, &p, None);
    let report = validate(&built, &p, None);
    assert!(!report.export_allowed());
    // tubeF needs the tube, pocketTubeOD and pocketRib cells (required_cells),
    // and nothing else fails: nominal geometry lies inside the coupon sweep.
    let rules: Vec<(&str, Severity, Option<&str>)> = report
        .findings
        .iter()
        .map(|f| (f.rule.as_str(), f.severity, f.feature.as_deref()))
        .collect();
    assert_eq!(
        rules,
        vec![("FIT.MISSING", Severity::Blocker, Some("bottom")); 3],
        "{:#?}",
        report.findings
    );
    for cell in ["tube", "pocketTubeOD", "pocketRib"] {
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.message.contains(&format!("`{cell}`"))),
            "{cell}: {:#?}",
            report.findings
        );
    }
}

#[test]
fn a_part_without_fit_critical_features_needs_no_fit_profile() {
    // docs/plan/05 section 4: "Skip for now" is allowed for non-fit-critical parts.
    let p = process("fdm-0.4");
    let doc = part("body.plate", [2, 2], None, true, false);
    let built = build(&doc, &p, None);
    let report = validate(&built, &p, None);
    assert!(report.export_allowed(), "{:#?}", report.findings);
}

#[test]
fn a_fit_profile_for_another_process_blocks_export() {
    let p = process("fdm-0.2");
    let fit = example_fit_profile();
    assert_eq!(fit.process, "fdm-0.4");
    let doc = part("body.tile", [2, 4], None, false, true);
    let built = build(&doc, &p, Some(&fit));
    let report = validate(&built, &p, Some(&fit));
    assert!(!report.export_allowed());
    assert!(
        report
            .blockers()
            .all(|f| f.rule == "FIT.PROCESS" && f.message.contains("fdm-0.4")),
        "{:#?}",
        report.findings
    );
}

#[test]
fn the_fit_profile_must_be_the_one_the_part_was_built_with() {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let doc = part("body.tile", [2, 4], None, false, true);
    // A nominal preview validated for a calibrated export: its mesh has none of
    // the profile's offsets, so the report would describe another part.
    let nominal = build(&doc, &p, None);
    let report = validate(&nominal, &p, Some(&fit));
    assert!(!report.export_allowed());
    let blockers: Vec<&str> = report.blockers().map(|f| f.rule.as_str()).collect();
    assert_eq!(blockers, vec!["FIT.PROCESS"], "{:#?}", report.findings);
    assert!(report.findings[0]
        .message
        .contains("built without a fit profile"));
    assert!(report.findings[0].fix.contains("example-mk4s-petg-0.4"));
    // The other way round: built with the profile, validated without one.
    let fitted = build(&doc, &p, Some(&fit));
    let report = validate(&fitted, &p, None);
    assert!(report
        .blockers()
        .any(|f| f.rule == "FIT.PROCESS" && f.message.contains("validated without")));
    // Built with another profile than the one given.
    let mut other = fit.clone();
    other.id = "another-profile".into();
    let report = validate(&fitted, &p, Some(&other));
    assert!(report
        .blockers()
        .any(|f| f.rule == "FIT.PROCESS" && f.message.contains("`another-profile`")));
}

#[test]
fn findings_name_the_feature_they_concern() {
    // A fit profile whose stud lies past the coupon sweep (4.80 + 0.20 = 5.00 mm,
    // the sweep ends at 4.95) and whose bore offset thins the tube wall to
    // (6.514 - 5.05) / 2 = 0.732 mm, under the recommended 0.8 mm of fdm-0.4.
    let p = process("fdm-0.4");
    let mut fit = example_fit_profile();
    fit.offsets_mm.stud_diameter = 0.20;
    fit.offsets_mm.tube_inner_diameter = 0.25;
    fit.offsets_mm.tube_outer_diameter = 0.0;
    let doc = part("body.brick", [2, 4], None, true, true);
    let built = build(&doc, &p, Some(&fit));
    let report = validate(&built, &p, Some(&fit));
    assert!(report.export_allowed(), "{:#?}", report.findings);
    let got: Vec<(&str, Severity, Option<&str>)> = report
        .findings
        .iter()
        .map(|f| (f.rule.as_str(), f.severity, f.feature.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("CLUTCH.RISK", Severity::Warning, Some("studs")),
            ("TUBE.WALL", Severity::Warning, Some("bottom")),
        ],
        "{:#?}",
        report.findings
    );
}

#[test]
fn a_solid_pin_moved_by_the_technic_pin_offset_is_a_clutch_risk() {
    // The example profile records pin 4.70 on the Technic pin row, so
    // pinDiameter is -0.10 mm, and the evaluator applies it to the solid pin
    // under 1-wide parts, which no coupon cell tests: a warning, not a blocker.
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let doc = part("body.plate", [1, 4], None, true, true);
    let built = build(&doc, &p, Some(&fit));
    let report = validate(&built, &p, Some(&fit));
    assert!(report.export_allowed(), "{:#?}", report.findings);
    let pin: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.rule == "CLUTCH.RISK" && f.message.contains("solid pin"))
        .collect();
    assert_eq!(pin.len(), 1, "{:#?}", report.findings);
    let f = pin[0];
    assert_eq!(f.feature.as_deref(), Some("bottom"));
    let nominal = common::mm("pin_1wide_diameter");
    assert!((f.threshold.expect("nominal") - nominal).abs() < 1e-9);
    let want = nominal + fit.offsets_mm.pin_diameter;
    assert!((f.value.expect("pin") - want).abs() < 1e-9, "{f:#?}");
    // With a zero pin offset the pin is nominal and there is nothing to report.
    let mut zero = fit.clone();
    zero.offsets_mm.pin_diameter = 0.0;
    let built = build(&doc, &p, Some(&zero));
    let report = validate(&built, &p, Some(&zero));
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.message.contains("solid pin")),
        "{:#?}",
        report.findings
    );
}

#[test]
fn the_report_is_deterministic_and_round_trips_as_json() {
    let p = process("fdm-0.4");
    let doc = part("body.tile", [2, 4], None, false, true);
    let a = validate(&build(&doc, &p, None), &p, None);
    let b = validate(&build(&doc, &p, None), &p, None);
    assert_eq!(a, b);
    let text = serde_json::to_string(&a).expect("serialises");
    let back: openbrick_print::Report = serde_json::from_str(&text).expect("reads back");
    assert_eq!(back, a);
}

/// Prints the measurements of the calibrated 2x4 tile, the base of the
/// validator fixtures in crates/openbrick-print/tests/fixtures/validator/
/// (`cargo test -p openbrick-part --test validate -- --ignored --nocapture`).
#[test]
#[ignore = "prints fixture input; run by hand"]
fn print_the_measurements_of_the_calibrated_2x4_tile() {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let doc = part("body.tile", [2, 4], None, false, true);
    let built = build(&doc, &p, Some(&fit));
    println!(
        "{}",
        serde_json::to_string_pretty(&built.measurements).expect("serialises")
    );
}
