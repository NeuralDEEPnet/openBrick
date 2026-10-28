//! Every standard fixture that `schemas/standard.schema.json` accepts also
//! deserializes into `Standard`, so the schema is never looser than serde.

use std::path::Path;

use openbrick_units::Standard;

#[test]
fn valid_standard_fixtures_deserialize() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fixtures/valid");
    let mut parsed = 0;
    for entry in std::fs::read_dir(&dir).expect("schemas/fixtures/valid exists") {
        let path = entry.expect("readable entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !(name.starts_with("standard-") && name.ends_with(".json")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("fixture is readable");
        let s: Standard = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name} passes the schema but not serde: {e}"));
        assert_eq!(s.kind, "standard", "{name}");
        parsed += 1;
    }
    assert!(parsed >= 1, "no standard fixtures in {}", dir.display());
}
