//! `cargo xtask <task>`: `check` runs fmt, clippy, tests and data validation; `data`
//! validates the JSON documents under standards/, profiles/ and parts/ against the
//! JSON Schemas in schemas/ and self-checks the schema fixtures.

mod schema;

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

/// Pragma, kind and credit checks plus full schema validation of one data directory.
fn validate_json_dir(root: &Path, set: &DataSet, schema: &Schema) -> bool {
    let mut ok = true;
    let files = match schema::json_files(&root.join(set.dir)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("data: {e}");
            return false;
        }
    };
    for p in files {
        let shown = p.strip_prefix(root).unwrap_or(&p).display().to_string();
        let v = match schema::read_json(&p) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("data: {e}");
                ok = false;
                continue;
            }
        };
        let has_pragma = v.get("openbrick").and_then(|x| x.as_str()).is_some();
        let kind_ok = set
            .kind
            .map(|k| v.get("kind").and_then(|x| x.as_str()) == Some(k))
            .unwrap_or(true);
        let credit_ok =
            v.get("credit").is_some() || v.get("kind").and_then(|x| x.as_str()) == Some("process");
        if !(has_pragma && kind_ok && credit_ok) {
            eprintln!("data: {shown} fails pragma/kind/credit checks");
            ok = false;
        }
        for f in schema.failures(&v) {
            eprintln!("data: {shown} does not match {}: {f}", schema.name);
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
    for problem in schema::check_fixtures(root, &schemas) {
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
        _ => {
            eprintln!("usage: cargo xtask <check|data>");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
