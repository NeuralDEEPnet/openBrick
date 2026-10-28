//! `cargo xtask <task>`: `check` runs fmt, clippy, tests and data validation; `data`
//! validates the JSON documents under standards/, profiles/ and parts/ against the
//! JSON Schemas in schemas/, loads each with its Rust reader and validates what the
//! reader writes back, and self-checks the schemas and their fixtures; it also checks
//! the validator fixtures' expected reports against `schemas/report.schema.json` and
//! that every coupon cell records a valid fit profile (see `validator.rs`); `wasm`
//! builds `openbrick-wasm` for the browser into `apps/web/src/wasm/` (see `wasm.rs`).

mod schema;
mod validator;
mod wasm;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, ExitCode};

use schema::{DataSet, Schema, DATA_SETS};

fn run(args: &[&str]) -> bool {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .expect("cargo runs");
    status.success()
}

/// Pragma, kind and credit checks, schema validation, and the Rust reader round
/// trip of one data directory.
fn validate_json_dir(root: &Path, set: &DataSet, schema: &Schema) -> bool {
    let mut ok = true;
    let files = match set.data_files(root) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("data: {e}");
            return false;
        }
    };
    for p in files {
        let shown = p.strip_prefix(root).unwrap_or(&p).display().to_string();
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("data: {shown}: cannot read: {e}");
                ok = false;
                continue;
            }
        };
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("data: {shown}: is not valid JSON: {e}");
                ok = false;
                continue;
            }
        };
        // Every data file carries a pragma, its kind and a credit (LICENSE-POLICY.md).
        let has_pragma = v.get("openbrick").and_then(|x| x.as_str()).is_some();
        let kind_ok = set
            .kind
            .map(|k| v.get("kind").and_then(|x| x.as_str()) == Some(k))
            .unwrap_or(true);
        let credit_ok = v.get("credit").is_some();
        if !(has_pragma && kind_ok && credit_ok) {
            eprintln!("data: {shown} fails pragma/kind/credit checks");
            ok = false;
        }
        for problem in schema::check_document(&shown, set, schema, &text) {
            eprintln!("data: {problem}");
            ok = false;
        }
    }
    ok
}

fn data(root: &Path) -> bool {
    let schemas: BTreeMap<&str, Schema> = match schema::load_all(root) {
        Ok(s) => s,
        Err(errors) => {
            for e in errors {
                eprintln!("data: schema {e}");
            }
            return false;
        }
    };
    let mut ok = true;
    for set in DATA_SETS {
        ok &= validate_json_dir(root, set, &schemas[set.schema]);
    }
    for problem in schema::check_shared_defs(&schemas) {
        eprintln!("data: {problem}");
        ok = false;
    }
    for problem in schemas.values().flat_map(schema::check_minor_version) {
        eprintln!("data: {problem}");
        ok = false;
    }
    for problem in schemas.values().flat_map(schema::check_known_fields) {
        eprintln!("data: {problem}");
        ok = false;
    }
    for problem in schema::check_fixtures(root, &schemas) {
        eprintln!("data: {problem}");
        ok = false;
    }
    for problem in schema::unclaimed_data_files(root) {
        eprintln!("data: {problem}");
        ok = false;
    }
    // The validator report contract and the coupon sweep (docs/plan/05 sections 4
    // and 5).
    match Schema::load(&root.join("schemas").join(validator::REPORT_SCHEMA)) {
        Ok(report) => {
            for problem in validator::check_validator_reports(root, &report)
                .into_iter()
                .chain(validator::check_rule_ids(&report))
            {
                eprintln!("data: {problem}");
                ok = false;
            }
        }
        Err(e) => {
            eprintln!("data: schema {e}");
            ok = false;
        }
    }
    for problem in validator::check_coupon_records(root, &schemas["fit-profile.schema.json"]) {
        eprintln!("data: {problem}");
        ok = false;
    }
    if ok {
        println!("data: ok");
    }
    ok
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    if !Path::new("Cargo.toml").exists() {
        eprintln!("run from the repository root");
        return ExitCode::from(2);
    }
    let root = Path::new(".");
    let ok = match task.as_str() {
        "check" => {
            run(&["fmt", "--all", "--check"])
                && run(&[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ])
                && run(&["test", "--workspace"])
                && data(root)
        }
        "data" => data(root),
        "wasm" => wasm::wasm(root),
        _ => {
            eprintln!("usage: cargo xtask <check|data|wasm>");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
