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
