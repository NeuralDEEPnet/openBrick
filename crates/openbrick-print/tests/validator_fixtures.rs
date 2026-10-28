//! Table-driven validator test: every rule in `IMPLEMENTED_RULES` has a
//! directory under `tests/fixtures/validator/<RULE>/` with one passing fixture
//! (`pass*.json`, an empty report) and at least one failing fixture
//! (`fail*.json`) that fails that rule and no other.
//!
//! A fixture holds the measurements of a part, the process id, an optional fit
//! profile (an id from `profiles/fit/`, an inline profile, or null, plus an
//! optional JSON merge patch `fitPatch`, RFC 7396) and the expected report. The
//! expected reports are checked against `schemas/report.schema.json` by
//! `cargo xtask data`.
//!
//! After a deliberate change to messages, rerun with `OPENBRICK_BLESS=1` to
//! rewrite the expected reports, then review the diff.

use std::path::{Path, PathBuf};

use openbrick_print::{
    default_process_profiles, validate, FitProfile, Measurements, Report, Severity,
    IMPLEMENTED_RULES,
};
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/validator")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// RFC 7396 JSON merge patch: objects merge key by key, null deletes, anything
/// else replaces.
fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(p) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let t = target.as_object_mut().expect("object");
    for (k, v) in p {
        if v.is_null() {
            t.remove(k);
        } else {
            merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
        }
    }
}

/// A fit profile from `profiles/fit/` by id.
fn bundled_fit(id: &str) -> Value {
    let dir = repo_root().join("profiles/fit");
    for entry in std::fs::read_dir(&dir)
        .expect("profiles/fit exists")
        .flatten()
    {
        let text = std::fs::read_to_string(entry.path()).expect("fit profile reads");
        let v: Value = serde_json::from_str(&text).expect("fit profile is JSON");
        if v["id"] == id {
            return v;
        }
    }
    panic!("no fit profile with id `{id}` in profiles/fit/");
}

/// Numbers equal within a relative 1e-9; everything else exactly.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (
                x.as_f64().unwrap_or(f64::NAN),
                y.as_f64().unwrap_or(f64::NAN),
            );
            (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

struct Case {
    path: PathBuf,
    doc: Value,
    report: Report,
}

fn run(path: &Path) -> Case {
    let shown = path.display();
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{shown}: {e}"));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{shown}: {e}"));
    for k in doc.as_object().expect("fixture is an object").keys() {
        assert!(
            FIELDS.contains(&k.as_str()),
            "{shown}: field `{k}` is not one of {FIELDS:?}"
        );
    }
    let process_id = doc["process"]
        .as_str()
        .unwrap_or_else(|| panic!("{shown}: needs a process id"));
    let process = default_process_profiles()
        .into_iter()
        .find(|p| p.id == process_id)
        .unwrap_or_else(|| panic!("{shown}: no bundled process `{process_id}`"));
    let fit: Option<FitProfile> = match &doc["fit"] {
        Value::Null => None,
        f => {
            let mut v = match f {
                Value::String(id) => bundled_fit(id),
                inline => inline.clone(),
            };
            if let Some(patch) = doc.get("fitPatch") {
                merge_patch(&mut v, patch);
            }
            Some(serde_json::from_value(v).unwrap_or_else(|e| panic!("{shown}: fit: {e}")))
        }
    };
    let raw = &doc["measurements"];
    let m: Measurements =
        serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{shown}: {e}"));
    // Measurements ignore unknown fields; a misspelt one would silently test
    // nothing, so every field the fixture sets must survive the round trip.
    let back = serde_json::to_value(&m).expect("serialises");
    for (k, v) in raw.as_object().expect("measurements is an object") {
        assert!(
            v.is_null() || back.get(k).is_some(),
            "{shown}: measurements field `{k}` is not a Measurements field"
        );
    }
    let report = validate(&m, &process, fit.as_ref());
    Case {
        path: path.to_path_buf(),
        doc,
        report,
    }
}

/// The top-level fields of a fixture, in reading order.
const FIELDS: [&str; 7] = [
    "description",
    "rule",
    "process",
    "fit",
    "fitPatch",
    "measurements",
    "report",
];

/// The fixture as pretty JSON with its top-level keys in reading order (serde_json
/// would sort them).
fn write_fixture(doc: &Value) -> String {
    let obj = doc.as_object().expect("fixture is an object");
    let fields: Vec<String> = FIELDS
        .iter()
        .filter_map(|k| obj.get(*k).map(|v| (k, v)))
        .map(|(k, v)| {
            let value = serde_json::to_string_pretty(v)
                .expect("serialises")
                .replace('\n', "\n  ");
            format!("  \"{k}\": {value}")
        })
        .collect();
    format!("{{\n{}\n}}\n", fields.join(",\n"))
}

/// Whether `message` shows `x` as a number of its own, written with zero to
/// six decimals, with or without trailing zeros (the validator writes lengths
/// with at most three decimals and trailing zeros dropped, and percentages
/// with one decimal or more): `0.8` is found in "is 0.8 mm" but not in
/// "is 0.85 mm" or "10.8".
fn shows_number(message: &str, x: f64) -> bool {
    let bytes = message.as_bytes();
    // Digits inside a word or an id such as `fdm-0.4` are not a number of their own.
    let joined = |i: usize| {
        let c = bytes[i];
        c.is_ascii_alphanumeric()
            || c == b'.'
            || c == b'_'
            || (c == b'-' && i > 0 && bytes[i - 1].is_ascii_alphanumeric())
    };
    let found = |s: &str| {
        message.match_indices(s).any(|(at, _)| {
            let end = at + s.len();
            let before = at == 0 || !joined(at - 1);
            // A full stop that ends a sentence is not part of the number.
            let after = end == bytes.len()
                || !(bytes[end].is_ascii_digit()
                    || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)));
            before && after
        })
    };
    (0..=6).any(|places| {
        let s = format!("{x:.places$}");
        let trimmed = if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.clone()
        };
        found(&s) || found(&trimmed)
    })
}

#[test]
fn numbers_are_found_only_on_their_own() {
    assert!(shows_number("the wall is 0.8 mm", 0.8));
    assert!(shows_number("the wall is 0.80 mm", 0.8));
    assert!(shows_number("removed 1.03 % of it", 1.0324211814310704));
    assert!(shows_number("ends at 4.95.", 4.95));
    assert!(shows_number("the volume is -5 mm3", -5.0));
    assert!(!shows_number("the wall is 0.85 mm", 0.8));
    assert!(!shows_number("process `fdm-0.4`", 0.8));
    assert!(!shows_number("process `fdm-0.4`", 0.4));
    assert!(!shows_number("profile `mk4s-petg`", 4.0));
    assert!(!shows_number("is 10.8 mm", 0.8));
    assert!(!shows_number("is 0.8.1", 0.8));
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_rule_passes_and_fails_its_fixtures() {
    let bless = std::env::var_os("OPENBRICK_BLESS").is_some();
    let mut dirs: Vec<String> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures/validator exists")
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    let mut want: Vec<String> = IMPLEMENTED_RULES.iter().map(|r| r.to_string()).collect();
    want.sort();
    assert_eq!(dirs, want, "one fixture directory per implemented rule");

    let mut problems = Vec::new();
    for rule in &dirs {
        let files = json_files(&fixtures_dir().join(rule));
        let name = |p: &PathBuf| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let passes = files.iter().filter(|p| name(p).starts_with("pass")).count();
        let fails = files.iter().filter(|p| name(p).starts_with("fail")).count();
        if passes < 1 || fails < 1 || passes + fails != files.len() {
            problems.push(format!(
                "{rule}: needs pass*.json and fail*.json files only, at least one of each ({passes} pass, {fails} fail, {} files)",
                files.len()
            ));
        }
        for path in &files {
            let mut case = run(path);
            let shown = format!("{rule}/{}", name(path));
            if case.doc["rule"] != rule.as_str() {
                problems.push(format!("{shown}: its rule field must be {rule}"));
            }
            let findings = &case.report.findings;
            if name(path).starts_with("pass") {
                if !findings.is_empty() {
                    problems.push(format!(
                        "{shown}: should pass every rule, got {findings:#?}"
                    ));
                }
            } else {
                if findings.is_empty() {
                    problems.push(format!("{shown}: should fail {rule}, but passes"));
                }
                if let Some(other) = findings.iter().find(|f| &f.rule != rule) {
                    problems.push(format!(
                        "{shown}: should fail only {rule}, but also fails {}: {other:#?}",
                        other.rule
                    ));
                }
                for f in findings {
                    // A finding's message shows the value and the threshold it
                    // carries (CLAUDE.md), as numbers of their own.
                    for (what, x) in [("value", f.value), ("threshold", f.threshold)] {
                        let Some(x) = x else { continue };
                        if !x.is_finite() {
                            problems.push(format!("{shown}: {what} {x} is not finite"));
                        } else if !shows_number(&f.message, x) {
                            problems.push(format!(
                                "{shown}: message does not show its {what} {x}: {}",
                                f.message
                            ));
                        }
                    }
                }
                for f in findings {
                    if f.fix.trim().is_empty() || f.message.trim().is_empty() {
                        problems.push(format!("{shown}: empty message or fix: {f:#?}"));
                    }
                }
            }
            let produced = serde_json::to_value(&case.report).expect("serialises");
            if bless {
                case.doc["report"] = produced;
                // Write beside and rename, so the other tests reading fixtures in
                // parallel never see a half-written file.
                let tmp = case.path.with_extension("json.tmp");
                std::fs::write(&tmp, write_fixture(&case.doc)).expect("fixture writes");
                std::fs::rename(&tmp, &case.path).expect("fixture is replaced");
            } else if !same(&case.doc["report"], &produced) {
                problems.push(format!(
                    "{shown}: report differs from the fixture (rerun with OPENBRICK_BLESS=1 after a deliberate change, then review the diff)\nexpected: {}\nproduced: {}",
                    serde_json::to_string_pretty(&case.doc["report"]).unwrap_or_default(),
                    serde_json::to_string_pretty(&produced).unwrap_or_default()
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn blockers_and_warnings_follow_the_rule_table() {
    // docs/plan/05 section 5: which failing fixtures block export and which only
    // warn. Warnings never disable export.
    let expect = [
        ("MESH.MANIFOLD/fail", false),
        ("MESH.MANIFOLD/fail-empty", false),
        ("MESH.VOLUME/fail", true),
        ("MESH.VOLUME/fail-zero-volume", true),
        ("WALL.MIN/fail", false),
        ("TUBE.WALL/fail", false),
        ("TUBE.WALL/fail-warning", true),
        ("TUBE.WALL/fail-warning-grown", true),
        ("CAVITY.DEPTH/fail", false),
        ("WALL.QUANTISED/fail", true),
        ("ROOF.MIN/fail", false),
        ("FEATURE.MIN/fail", true),
        ("FEATURE.MIN/fail-removed-area", true),
        ("FEATURE.MIN/fail-fit-critical", false),
        ("FEATURE.MIN/fail-fit-critical-removed-area", false),
        ("FEATURE.MIN/fail-not-measured", true),
        ("FEATURE.MIN/fail-fit-critical-not-measured", false),
        ("RELIEF.HEIGHT/fail", true),
        ("RELIEF.HEIGHT/fail-too-high", true),
        ("FIT.PROCESS/fail", false),
        ("FIT.PROCESS/fail-not-allowed", false),
        ("FIT.PROCESS/fail-built-for-another-process", false),
        ("FIT.PROCESS/fail-built-without-fit", false),
        ("FIT.PROCESS/fail-validated-without-fit", false),
        ("FIT.PROCESS/fail-built-with-other-offsets", false),
        ("FIT.MISSING/fail", false),
        ("FIT.MISSING/fail-cell-missing", false),
        ("CLUTCH.RISK/fail", true),
        ("CLUTCH.RISK/fail-tube", true),
        ("CLUTCH.RISK/fail-solid-pin", true),
    ];
    for (name, exportable) in expect {
        let case = run(&fixtures_dir().join(format!("{name}.json")));
        let r = &case.report;
        assert_eq!(r.export_allowed(), exportable, "{name}: {:#?}", r.findings);
        let want = if exportable {
            Severity::Warning
        } else {
            Severity::Blocker
        };
        assert!(
            r.findings.iter().all(|f| f.severity == want),
            "{name}: {:#?}",
            r.findings
        );
    }
}

#[test]
fn findings_are_sorted_blockers_first_then_by_rule() {
    // Break several rules at once and check the order.
    let mut case = run(&fixtures_dir().join("MESH.VOLUME/fail.json"));
    let mut m: Measurements =
        serde_json::from_value(case.doc["measurements"].clone()).expect("measurements");
    // Built without a fit profile, as it is validated (rule FIT.PROCESS).
    m.fit_profile = None;
    m.roof_mm = Some(0.5);
    m.min_tube_wall_mm = Some(0.75);
    m.relief_height_mm = Some(0.1);
    m.is_manifold = false;
    let process = default_process_profiles()
        .into_iter()
        .find(|p| p.id == "fdm-0.4")
        .expect("fdm-0.4");
    // A relief comes with its measured stroke, here at the minimum; without one
    // FEATURE.MIN would report a stroke that was not measured.
    m.min_stroke_mm = Some(process.min_feature_mm);
    case.report = validate(&m, &process, None);
    let order: Vec<(&str, Severity)> = case
        .report
        .findings
        .iter()
        .map(|f| (f.rule.as_str(), f.severity))
        .collect();
    assert_eq!(
        order,
        vec![
            ("FIT.MISSING", Severity::Blocker),
            ("FIT.MISSING", Severity::Blocker),
            ("FIT.MISSING", Severity::Blocker),
            ("MESH.MANIFOLD", Severity::Blocker),
            ("ROOF.MIN", Severity::Blocker),
            ("MESH.VOLUME", Severity::Warning),
            ("RELIEF.HEIGHT", Severity::Warning),
            ("TUBE.WALL", Severity::Warning),
        ]
    );
    // The same input gives the same report.
    assert_eq!(validate(&m, &process, None), case.report);
}
