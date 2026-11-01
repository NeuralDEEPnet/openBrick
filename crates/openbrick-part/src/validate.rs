//! Validate a built part (docs/plan/05 section 5): the signature the plan names,
//! `validate(&Built, &ProcessProfile, Option<&FitProfile>) -> Report`.
//!
//! The rules live in [`openbrick_print::validate`], which reads the part's
//! [`Measurements`](openbrick_print::Measurements). This wrapper adds what only
//! the built part knows, the features behind the findings: `FIT.MISSING`
//! findings name the feature whose fit-critical connectors need the missing
//! cell; `TUBE.WALL` and `CLUTCH.RISK` on the tubes and the 1-wide pins name the
//! feature that emits `tubeF` connectors, and `CLUTCH.RISK` on the studs the one
//! that emits `studM` connectors. A field placed off the grid emits no
//! connectors, so its findings name no feature.

use std::collections::{BTreeMap, BTreeSet};

use openbrick_print::{check_fit_missing, Bed, FeatureNames, FitProfile, ProcessProfile, Report};

use crate::Built;

/// Check a built part against a process profile and an optional fit profile.
/// [`Report::export_allowed`] says whether it may be exported. The fit profile
/// must be the one the part was built with (rule `FIT.PROCESS`). Rule
/// `SIZE.BED` is listed as not checked; [`validate_on_bed`] checks it.
pub fn validate(built: &Built, process: &ProcessProfile, fit: Option<&FitProfile>) -> Report {
    validate_on_bed(built, process, fit, None)
}

/// [`validate`], and rule `SIZE.BED` against `bed` when one is given (a
/// printer's, `openbrick_print::printer`, or one of a given size).
pub fn validate_on_bed(
    built: &Built,
    process: &ProcessProfile,
    fit: Option<&FitProfile>,
    bed: Option<&Bed>,
) -> Report {
    let names = FeatureNames {
        studs: emitter(built, "studM"),
        tubes: emitter(built, "tubeF"),
    };
    let mut report =
        openbrick_print::validate_on_bed(&built.measurements, process, fit, &names, bed);
    if report.findings.iter().any(|f| f.rule == "FIT.MISSING") {
        // The same check per emitting feature, so each finding can say where.
        let mut by_feature: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for c in built.connectors.iter().filter(|c| c.fit_critical) {
            let kinds = by_feature.entry(c.feature.as_str()).or_default();
            if !kinds.contains(&c.kind.as_str()) {
                kinds.push(c.kind.as_str());
            }
        }
        let mut per_feature = Vec::new();
        for (feature, mut kinds) in by_feature {
            kinds.sort_unstable();
            for mut f in check_fit_missing(&kinds, fit) {
                f.feature = Some(feature.to_string());
                per_feature.push(f);
            }
        }
        // Replace each feature-less finding by its per-feature copies (same
        // message); one the connectors do not explain (measurements edited by
        // hand) stays as it was.
        let covered: BTreeSet<String> = per_feature.iter().map(|f| f.message.clone()).collect();
        report
            .findings
            .retain(|f| f.rule != "FIT.MISSING" || !covered.contains(&f.message));
        report.findings.extend(per_feature);
        report.sort();
    }
    report
}

/// The one feature that emits connectors of `kind`; `None` when no feature or
/// more than one does.
fn emitter(built: &Built, kind: &str) -> Option<String> {
    let features: BTreeSet<&str> = built
        .connectors
        .iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.feature.as_str())
        .collect();
    match features.len() {
        1 => features.into_iter().next().map(String::from),
        _ => None,
    }
}
