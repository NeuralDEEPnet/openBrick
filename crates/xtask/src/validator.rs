//! Data checks for the print pipeline that go beyond one schema per file
//! (docs/plan/05 sections 4 and 5):
//!
//! - the expected reports of the validator fixtures
//!   (`crates/openbrick-print/tests/fixtures/validator/<RULE>/*.json`) match
//!   `schemas/report.schema.json`, the contract of `openbrick validate --json`,
//!   and that schema's rule ids are exactly the rules of the plan's table as
//!   openbrick-print lists them;
//! - every coupon sweep in `standards/coupon-*.json` records, at both ends of
//!   every recordable cell, a fit profile that `schemas/fit-profile.schema.json`
//!   accepts (so no coupon cell can produce an offset the schema refuses). The
//!   two pocket cells are one choice, so they are recorded together, from the
//!   `full` pocket set, which prints every pair.

use std::path::Path;

use serde_json::Value;

use crate::schema::{file_name, json_files, Schema, DATA_SETS};

/// The schema of validator reports, inside `schemas/`.
pub const REPORT_SCHEMA: &str = "report.schema.json";

/// The validator fixtures, relative to the repository root.
pub const VALIDATOR_FIXTURES: &str = "crates/openbrick-print/tests/fixtures/validator";

/// Each validator fixture's `report` passes the report schema, loads as
/// `openbrick_print::Report`, and writes back as a report the schema accepts.
/// Returns one line per problem.
pub fn check_validator_reports(root: &Path, report: &Schema) -> Vec<String> {
    let mut problems = Vec::new();
    let base = root.join(VALIDATOR_FIXTURES);
    let mut dirs: Vec<_> = match std::fs::read_dir(&base) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => return vec![format!("missing directory {}: {e}", base.display())],
    };
    dirs.sort();
    if dirs.is_empty() {
        problems.push(format!("{VALIDATOR_FIXTURES}: no rule directories"));
    }
    for dir in dirs {
        let files = match json_files(&dir) {
            Ok(f) => f,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        for path in files {
            let shown = format!(
                "{VALIDATOR_FIXTURES}/{}/{}",
                file_name(&dir),
                file_name(&path)
            );
            let doc: Value = match crate::schema::read_json(&path) {
                Ok(v) => v,
                Err(e) => {
                    problems.push(e);
                    continue;
                }
            };
            let Some(expected) = doc.get("report") else {
                problems.push(format!("{shown}: has no expected report"));
                continue;
            };
            problems.extend(
                report
                    .failures(expected)
                    .into_iter()
                    .map(|f| format!("{shown}: report does not match {REPORT_SCHEMA}: {f}")),
            );
            match serde_json::from_value::<openbrick_print::Report>(expected.clone()) {
                Err(e) => problems.push(format!(
                    "{shown}: openbrick_print::Report cannot load the report: {e}"
                )),
                Ok(r) => match serde_json::to_value(&r) {
                    Ok(back) => problems.extend(report.failures(&back).into_iter().map(|f| {
                        format!("{shown}: openbrick_print::Report writes the report back as one {REPORT_SCHEMA} rejects: {f}")
                    })),
                    Err(e) => problems.push(format!("{shown}: cannot write the report: {e}")),
                },
            }
        }
    }
    problems
}

/// The report schema's rule enum is exactly `IMPLEMENTED_RULES` plus
/// `PLANNED_RULES`, so a new rule cannot ship without its contract.
pub fn check_rule_ids(report: &Schema) -> Vec<String> {
    let Some(listed) = report
        .raw
        .pointer("/$defs/finding/properties/rule/enum")
        .and_then(Value::as_array)
    else {
        return vec![format!(
            "{REPORT_SCHEMA}: lacks $defs/finding/properties/rule/enum"
        )];
    };
    let mut listed: Vec<&str> = listed.iter().filter_map(Value::as_str).collect();
    listed.sort_unstable();
    let mut known: Vec<&str> = openbrick_print::IMPLEMENTED_RULES
        .iter()
        .chain(openbrick_print::PLANNED_RULES)
        .copied()
        .collect();
    known.sort_unstable();
    if listed == known {
        Vec::new()
    } else {
        vec![format!(
            "{REPORT_SCHEMA}: the rule enum {listed:?} differs from openbrick_print::IMPLEMENTED_RULES and PLANNED_RULES {known:?}"
        )]
    }
}

/// Record the first and the last value of every recordable cell of every coupon
/// sweep, one cell at a time (the two pocket cells as one pocket) and all cells
/// together, and check each fit profile against the fit-profile schema. Returns
/// one line per problem.
pub fn check_coupon_records(root: &Path, fit: &Schema) -> Vec<String> {
    let mut problems = Vec::new();
    for set in DATA_SETS.iter().filter(|s| s.kind == Some("coupon")) {
        let files = match set.data_files(root) {
            Ok(f) => f,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        for path in files {
            let shown = format!("{}/{}", set.dir, file_name(&path));
            let spec: openbrick_print::CouponSpec = match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
            {
                Ok(s) => s,
                Err(e) => {
                    problems.push(format!("{shown}: {e}"));
                    continue;
                }
            };
            let recordable: Vec<_> = spec
                .cells
                .iter()
                .filter(|c| c.offset_field.is_some())
                .collect();
            let mut choices: Vec<(String, Vec<(String, f64)>)> = Vec::new();
            for end in ["first", "last"] {
                let pick = |c: &openbrick_print::CouponCell| {
                    let v = c.values();
                    if end == "first" {
                        v.first().copied()
                    } else {
                        v.last().copied()
                    }
                };
                let all: Vec<(String, f64)> = recordable
                    .iter()
                    .filter_map(|c| pick(c).map(|v| (c.id.clone(), v)))
                    .collect();
                let pocket = |id: &str| openbrick_print::POCKET_CELLS.contains(&id);
                for one in all.iter().filter(|c| !pocket(&c.0)) {
                    choices.push((format!("{} = {}", one.0, one.1), vec![one.clone()]));
                }
                let pair: Vec<(String, f64)> =
                    all.iter().filter(|c| pocket(&c.0)).cloned().collect();
                if !pair.is_empty() {
                    choices.push((format!("the pocket at its {end} values"), pair));
                }
                choices.push((format!("every cell at its {end} value"), all));
            }
            for (what, cells) in choices {
                let input = openbrick_print::RecordInput {
                    process: "fdm-0.4".into(),
                    printer: "Coupon check".into(),
                    nozzle_mm: 0.4,
                    material: "PLA".into(),
                    intent: "fits-genuine-bricks".into(),
                    cells: cells.into_iter().collect(),
                    pocket_set: Some("full".into()),
                    ..openbrick_print::RecordInput::default()
                };
                let profile = match openbrick_print::record_with(&spec, &input) {
                    Ok(p) => p,
                    Err(e) => {
                        problems.push(format!("{shown}: recording {what} fails: {e}"));
                        continue;
                    }
                };
                match serde_json::to_value(&profile) {
                    Ok(v) => problems.extend(fit.failures(&v).into_iter().map(|f| {
                        format!(
                            "{shown}: recording {what} writes a fit profile {} rejects: {f}",
                            fit.name
                        )
                    })),
                    Err(e) => problems.push(format!("{shown}: {what}: {e}")),
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn schema(name: &str) -> Schema {
        Schema::load(&root().join("schemas").join(name)).expect("schema loads")
    }

    #[test]
    fn validator_fixture_reports_match_the_report_schema() {
        let problems = check_validator_reports(&root(), &schema(REPORT_SCHEMA));
        assert!(problems.is_empty(), "{problems:#?}");
        assert!(check_rule_ids(&schema(REPORT_SCHEMA)).is_empty());
    }

    #[test]
    fn a_report_that_breaks_the_contract_is_reported() {
        let s = schema(REPORT_SCHEMA);
        let ok = serde_json::json!({ "findings": [ {
            "rule": "WALL.MIN", "severity": "blocker", "message": "the wall is 0.4 mm",
            "value": 0.4, "threshold": 0.8, "fix": "make it 0.8 mm", "feature": null } ] });
        assert!(s.failures(&ok).is_empty());
        let mut bad = ok.clone();
        bad["findings"][0]["severity"] = "error".into();
        let failures = s.failures(&bad);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].pointer, "/findings/0/severity");
        let mut bad = ok;
        bad["findings"][0]["rule"] = "WALL.THIN".into();
        bad["findings"][0]
            .as_object_mut()
            .expect("object")
            .remove("fix");
        assert_eq!(s.failures(&bad).len(), 2);
    }

    #[test]
    fn a_rule_missing_from_the_report_schema_is_reported() {
        let mut raw = schema(REPORT_SCHEMA).raw;
        raw.pointer_mut("/$defs/finding/properties/rule/enum")
            .and_then(Value::as_array_mut)
            .expect("rule enum")
            .retain(|r| r != "CLUTCH.RISK");
        let s = Schema::from_value(REPORT_SCHEMA, raw).expect("compiles");
        assert_eq!(check_rule_ids(&s).len(), 1);
    }

    #[test]
    fn every_coupon_cell_records_a_valid_fit_profile() {
        let problems = check_coupon_records(&root(), &schema("fit-profile.schema.json"));
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_coupon_whose_offsets_the_fit_schema_refuses_is_reported() {
        // A stud sweep running to 4.60 + 32 x 0.05 = 6.20 mm records 6.20 - 4.80 =
        // +1.40 mm, outside the schema's plus or minus 1 mm (a unit mistake by the
        // schema's rule).
        let mut spec = openbrick_print::coupon_v1().clone();
        let stud = spec
            .cells
            .iter_mut()
            .find(|c| c.id == "stud")
            .expect("stud");
        stud.end_mm = stud.start_mm + 32.0 * stud.step_mm; // 4.60 + 1.60 = 6.20
        let input = openbrick_print::RecordInput {
            process: "fdm-0.4".into(),
            printer: "Coupon check".into(),
            nozzle_mm: 0.4,
            material: "PLA".into(),
            intent: "fits-genuine-bricks".into(),
            cells: [("stud".to_string(), stud.end_mm)].into_iter().collect(),
            ..openbrick_print::RecordInput::default()
        };
        let profile = openbrick_print::record_with(&spec, &input).expect("records");
        let v = serde_json::to_value(&profile).expect("serialises");
        let failures = schema("fit-profile.schema.json").failures(&v);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].pointer, "/offsetsMm/studDiameter");
    }
}
