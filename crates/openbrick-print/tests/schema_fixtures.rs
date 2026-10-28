//! Every process and fit fixture that `schemas/` accepts also deserializes into
//! `ProcessProfile` and `FitProfile`, so the schemas are never looser than serde.

use std::path::{Path, PathBuf};

use openbrick_print::{FitProfile, ProcessProfile};

fn valid_fixtures(prefix: &str) -> Vec<(String, String)> {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fixtures/valid");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("schemas/fixtures/valid exists") {
        let path = entry.expect("readable entry").path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if name.starts_with(prefix) && name.ends_with(".json") {
            out.push((
                name,
                std::fs::read_to_string(&path).expect("fixture is readable"),
            ));
        }
    }
    assert!(
        !out.is_empty(),
        "no {prefix}* fixtures in {}",
        dir.display()
    );
    out
}

#[test]
fn valid_process_fixtures_deserialize() {
    for (name, text) in valid_fixtures("process-") {
        let p: ProcessProfile = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name} passes the schema but not serde: {e}"));
        assert_eq!(p.kind, "process", "{name}");
    }
}

#[test]
fn valid_fit_fixtures_deserialize() {
    for (name, text) in valid_fixtures("fit-") {
        let f: FitProfile = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name} passes the schema but not serde: {e}"));
        assert_eq!(f.kind, "fit", "{name}");
    }
}
