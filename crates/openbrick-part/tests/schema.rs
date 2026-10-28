//! `schemas/part.schema.json` and this crate must agree: the schema's op enum is
//! exactly `KNOWN_OPS`, and every valid part fixture parses with `PartDoc::parse`.

use std::path::{Path, PathBuf};

use openbrick_part::{PartDoc, KNOWN_OPS};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn schema_op_enum_equals_known_ops() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/part.schema.json"))
            .expect("schemas/part.schema.json is JSON");
    let ops: Vec<&str> = schema
        .pointer("/$defs/feature/properties/op/enum")
        .and_then(|v| v.as_array())
        .expect("op enum at /$defs/feature/properties/op/enum")
        .iter()
        .map(|v| v.as_str().expect("every op is a string"))
        .collect();
    assert_eq!(
        ops, KNOWN_OPS,
        "the op enum in schemas/part.schema.json and openbrick_part::KNOWN_OPS list different ops; \
         add or remove the op in both places, in the same order"
    );
}

#[test]
fn valid_part_fixtures_parse() {
    let dir = repo().join("schemas/fixtures/valid");
    let mut parsed = 0;
    for entry in std::fs::read_dir(&dir).expect("schemas/fixtures/valid exists") {
        let path = entry.expect("readable entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !(name.starts_with("part-") && name.ends_with(".json")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("fixture is readable");
        if let Err(e) = PartDoc::parse(&text) {
            panic!("{name} passes the schema but PartDoc::parse rejects it: {e}");
        }
        parsed += 1;
    }
    assert!(parsed >= 1, "no part fixtures found in {}", dir.display());
}
