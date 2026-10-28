//! Validate a built part (docs/plan/05 section 5): the signature the plan names,
//! `validate(&Built, &ProcessProfile, Option<&FitProfile>) -> Report`.
//!
//! The rules live in [`openbrick_print::validate`], which reads the part's
//! [`Measurements`](openbrick_print::Measurements). This wrapper adds what only
//! the built part knows: `FIT.MISSING` findings name the feature whose
//! fit-critical connectors need the missing cell.

use std::collections::{BTreeMap, BTreeSet};

use openbrick_print::{check_fit_missing, FitProfile, ProcessProfile, Report};

use crate::Built;

/// Check a built part against a process profile and an optional fit profile.
/// [`Report::export_allowed`] says whether it may be exported.
pub fn validate(built: &Built, process: &ProcessProfile, fit: Option<&FitProfile>) -> Report {
    let mut report = openbrick_print::validate(&built.measurements, process, fit);
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
