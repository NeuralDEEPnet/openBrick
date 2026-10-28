//! JSON Schema (draft 2020-12) validation of the data documents and the schema
//! fixtures (docs/plan/08 section 1; backlog item 5 in docs/plan/14 section 9).
//!
//! Data directories pick their schema by directory; fixtures under
//! `schemas/fixtures/` pick it by file-name prefix (`part-`, `fit-`, ...).
//! Invalid fixtures are listed in `schemas/fixtures/expected-failures.json` with
//! the one JSON pointer and keyword they are meant to break, so a fixture that
//! fails for an unrelated reason is caught too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The `$schema` every schema in `schemas/` must declare.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// One family of data documents and the schema that validates it.
pub struct DataSet {
    /// Directory of the data files, relative to the repository root.
    pub dir: &'static str,
    /// Expected `kind` field (parts carry none yet).
    pub kind: Option<&'static str>,
    /// Schema file name inside `schemas/`.
    pub schema: &'static str,
    /// File-name prefix of the fixtures for this schema.
    pub prefix: &'static str,
}

pub const DATA_SETS: &[DataSet] = &[
    DataSet {
        dir: "standards",
        kind: Some("standard"),
        schema: "standard.schema.json",
        prefix: "standard-",
    },
    DataSet {
        dir: "profiles/process",
        kind: Some("process"),
        schema: "process-profile.schema.json",
        prefix: "process-",
    },
    DataSet {
        dir: "profiles/fit",
        kind: Some("fit"),
        schema: "fit-profile.schema.json",
        prefix: "fit-",
    },
    DataSet {
        dir: "parts",
        kind: None,
        schema: "part.schema.json",
        prefix: "part-",
    },
];

/// `$defs` that every schema repeats so each file stands alone; they must not drift.
pub const SHARED_DEFS: &[&str] = &[
    "pragma",
    "semver",
    "spdx",
    "credit",
    "provenance",
    "profileId",
];

/// One schema violation in a document.
#[derive(Clone, Debug)]
pub struct Failure {
    /// JSON pointer into the document; empty for the document root.
    pub pointer: String,
    /// The schema keyword that failed (`enum`, `required`, ...).
    pub keyword: String,
    /// JSON pointer of that keyword inside the schema.
    pub schema_path: String,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let at = if self.pointer.is_empty() {
            "(document root)"
        } else {
            &self.pointer
        };
        write!(
            f,
            "at {at}: {} [schema #{}]",
            self.message, self.schema_path
        )
    }
}

/// A compiled schema.
pub struct Schema {
    pub name: String,
    pub raw: Value,
    validator: jsonschema::Validator,
}

impl Schema {
    /// Read, meta-validate and compile one schema file.
    pub fn load(path: &Path) -> Result<Schema, String> {
        let raw = read_json(path)?;
        let shown = path.display();
        if raw.get("$schema").and_then(Value::as_str) != Some(DRAFT_2020_12) {
            return Err(format!("{shown}: $schema must be {DRAFT_2020_12}"));
        }
        for key in ["$id", "title", "description"] {
            if raw
                .get(key)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(format!("{shown}: missing {key}"));
            }
        }
        jsonschema::draft202012::meta::validate(&raw)
            .map_err(|e| format!("{shown}: not a valid draft 2020-12 schema: {e}"))?;
        let validator = jsonschema::draft202012::new(&raw)
            .map_err(|e| format!("{shown}: does not compile: {e}"))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Schema {
            name,
            raw,
            validator,
        })
    }

    /// Every violation of this schema in `doc`, in the validator's order.
    pub fn failures(&self, doc: &Value) -> Vec<Failure> {
        self.validator
            .iter_errors(doc)
            .map(|e| Failure {
                pointer: e.instance_path().to_string(),
                keyword: e.kind().keyword().to_string(),
                schema_path: e.schema_path().to_string(),
                message: e.to_string(),
            })
            .collect()
    }
}

/// Parse a JSON file.
pub fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: cannot read: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: is not valid JSON: {e}", path.display()))
}

/// The `.json` files directly inside `dir`, sorted so output is stable.
pub fn json_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("missing directory {}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    files.sort();
    Ok(files)
}

/// Load the schema of every data set, keyed by schema file name.
pub fn load_all(root: &Path) -> Result<BTreeMap<&'static str, Schema>, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    for set in DATA_SETS {
        match Schema::load(&root.join("schemas").join(set.schema)) {
            Ok(s) => {
                out.insert(set.schema, s);
            }
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// The data set whose fixtures start with the prefix of this file name.
fn set_for_fixture(file_name: &str) -> Option<&'static DataSet> {
    DATA_SETS.iter().find(|s| file_name.starts_with(s.prefix))
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Expected failure of one invalid fixture.
struct Expected {
    pointer: String,
    keyword: String,
}

fn expected_failures(path: &Path) -> Result<BTreeMap<String, Expected>, String> {
    let doc = read_json(path)?;
    let Some(map) = doc.get("fixtures").and_then(Value::as_object) else {
        return Err(format!("{}: needs a fixtures object", path.display()));
    };
    let mut out = BTreeMap::new();
    for (name, v) in map {
        let pointer = v.get("pointer").and_then(Value::as_str);
        let keyword = v.get("keyword").and_then(Value::as_str);
        let (Some(pointer), Some(keyword)) = (pointer, keyword) else {
            return Err(format!(
                "{}: {name} needs string pointer and keyword",
                path.display()
            ));
        };
        out.insert(
            name.clone(),
            Expected {
                pointer: pointer.to_string(),
                keyword: keyword.to_string(),
            },
        );
    }
    Ok(out)
}

/// Self-check of `schemas/fixtures/`: every valid fixture passes its schema; every
/// invalid one fails, and only at the pointer and keyword it is labelled with in
/// `expected-failures.json`. Each schema needs at least one valid and two invalid
/// fixtures. Returns one line per problem; empty means all good.
pub fn check_fixtures(root: &Path, schemas: &BTreeMap<&'static str, Schema>) -> Vec<String> {
    let dir = root.join("schemas/fixtures");
    let mut problems = Vec::new();
    let expected = match expected_failures(&dir.join("expected-failures.json")) {
        Ok(e) => e,
        Err(e) => return vec![e],
    };
    let mut valid_count: BTreeMap<&str, usize> = BTreeMap::new();
    let mut invalid_count: BTreeMap<&str, usize> = BTreeMap::new();

    for (sub, should_pass) in [("valid", true), ("invalid", false)] {
        let files = match json_files(&dir.join(sub)) {
            Ok(f) => f,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        for path in files {
            let name = file_name(&path);
            let shown = format!("schemas/fixtures/{sub}/{name}");
            let Some(set) = set_for_fixture(&name) else {
                problems.push(format!(
                    "{shown}: name must start with one of standard-, process-, fit-, part-"
                ));
                continue;
            };
            let schema = &schemas[set.schema];
            let doc = match read_json(&path) {
                Ok(d) => d,
                Err(e) => {
                    problems.push(e);
                    continue;
                }
            };
            let failures = schema.failures(&doc);
            if should_pass {
                *valid_count.entry(set.schema).or_default() += 1;
                for f in failures {
                    problems.push(format!("{shown} should pass {} but fails {f}", schema.name));
                }
                continue;
            }
            *invalid_count.entry(set.schema).or_default() += 1;
            let Some(want) = expected.get(&name) else {
                problems.push(format!(
                    "{shown}: add it to schemas/fixtures/expected-failures.json with the pointer and keyword it breaks"
                ));
                continue;
            };
            if failures.is_empty() {
                problems.push(format!(
                    "{shown} should fail {} at {} ({}) but passes",
                    schema.name, want.pointer, want.keyword
                ));
            }
            for f in failures {
                if f.pointer != want.pointer || f.keyword != want.keyword {
                    problems.push(format!(
                        "{shown} should break only `{}` at \"{}\" but fails `{}` {f}",
                        want.keyword, want.pointer, f.keyword
                    ));
                }
            }
        }
    }

    for name in expected.keys() {
        if !dir.join("invalid").join(name).is_file() {
            problems.push(format!(
                "schemas/fixtures/expected-failures.json lists {name}, which is not in schemas/fixtures/invalid/"
            ));
        }
    }
    for set in DATA_SETS {
        let v = valid_count.get(set.schema).copied().unwrap_or(0);
        let i = invalid_count.get(set.schema).copied().unwrap_or(0);
        if v < 1 || i < 2 {
            problems.push(format!(
                "{} has {v} valid and {i} invalid fixtures; it needs at least 1 and 2",
                set.schema
            ));
        }
    }
    problems
}

/// The shared `$defs` must be identical in every schema; returns one line per drift.
pub fn check_shared_defs(schemas: &BTreeMap<&'static str, Schema>) -> Vec<String> {
    let mut problems = Vec::new();
    for def in SHARED_DEFS {
        let mut first: Option<(&str, &Value)> = None;
        for schema in schemas.values() {
            let Some(v) = schema.raw.pointer(&format!("/$defs/{def}")) else {
                continue;
            };
            match first {
                None => first = Some((&schema.name, v)),
                Some((name, w)) if w != v => problems.push(format!(
                    "$defs/{def} differs between {name} and {}; keep the copies identical",
                    schema.name
                )),
                Some(_) => {}
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn schemas() -> BTreeMap<&'static str, Schema> {
        load_all(&root()).unwrap_or_else(|e| panic!("schemas load: {e:#?}"))
    }

    #[test]
    fn fixtures_pass_and_fail_as_labelled() {
        let problems = check_fixtures(&root(), &schemas());
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn repository_data_matches_its_schemas() {
        let schemas = schemas();
        let mut problems = Vec::new();
        for set in DATA_SETS {
            for path in json_files(&root().join(set.dir)).expect("data directory") {
                let doc = read_json(&path).expect("data file parses");
                for f in schemas[set.schema].failures(&doc) {
                    problems.push(format!("{}: {f}", path.display()));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn shared_defs_do_not_drift() {
        let schemas = schemas();
        let problems = check_shared_defs(&schemas);
        assert!(problems.is_empty(), "{problems:#?}");
        for def in SHARED_DEFS {
            let users = schemas
                .values()
                .filter(|s| s.raw.pointer(&format!("/$defs/{def}")).is_some())
                .count();
            assert!(users >= 2, "$defs/{def} is shared by {users} schema(s)");
        }
    }

    #[test]
    fn failures_carry_the_json_pointer() {
        let schemas = schemas();
        let part = &schemas["part.schema.json"];
        let doc = serde_json::json!({
            "openbrick": "1.0", "id": "test.pointer", "version": "0.1.0", "name": "t",
            "license": "CC0-1.0", "credit": { "author": "t" },
            "features": [ { "id": "body", "op": "body.plate" }, { "id": "x", "op": "no.such-op" } ]
        });
        let failures = part.failures(&doc);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].pointer, "/features/1/op");
        assert_eq!(failures[0].keyword, "enum");
        assert!(failures[0].to_string().starts_with("at /features/1/op: "));
    }
}
