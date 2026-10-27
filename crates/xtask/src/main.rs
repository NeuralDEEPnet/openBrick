//! `cargo xtask <task>`: `check` runs fmt, clippy, tests and data validation; `data`
//! validates the JSON documents under standards/, profiles/ and parts/.

use std::path::Path;
use std::process::{Command, ExitCode};

fn run(args: &[&str]) -> bool {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .expect("cargo runs");
    status.success()
}

fn validate_json_dir(dir: &str, kind: Option<&str>) -> bool {
    let mut ok = true;
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("data: missing directory {dir}");
        return false;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            Some(v) => {
                let has_pragma = v.get("openbrick").and_then(|x| x.as_str()).is_some();
                let kind_ok = kind
                    .map(|k| v.get("kind").and_then(|x| x.as_str()) == Some(k))
                    .unwrap_or(true);
                let credit_ok = v.get("credit").is_some()
                    || v.get("kind").and_then(|x| x.as_str()) == Some("process");
                if !(has_pragma && kind_ok && credit_ok) {
                    eprintln!("data: {} fails pragma/kind/credit checks", p.display());
                    ok = false;
                }
            }
            None => {
                eprintln!("data: {} is not valid JSON", p.display());
                ok = false;
            }
        }
    }
    ok
}

fn data() -> bool {
    let mut ok = true;
    ok &= validate_json_dir("standards", Some("standard"));
    ok &= validate_json_dir("profiles/process", Some("process"));
    ok &= validate_json_dir("profiles/fit", Some("fit"));
    ok &= validate_json_dir("parts", None);
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
                && data()
        }
        "data" => data(),
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
