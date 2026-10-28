//! JSON Schema (draft 2020-12) validation of the data documents and the schema
//! fixtures (docs/plan/08 section 1; backlog item 5 in docs/plan/14 section 9).
//!
//! Every data file and every valid fixture is checked three ways: against its
//! schema, by loading it with the Rust reader of its kind, and by validating what
//! that reader writes back. Data directories pick their schema by directory;
//! fixtures under `schemas/fixtures/` pick it by file-name prefix (`part-`, `fit-`,
//! ...). Invalid fixtures are listed in `schemas/fixtures/expected-failures.json`
//! with the one JSON pointer, keyword and message they are meant to break, so a
//! fixture that fails for an unrelated reason is caught too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The `$schema` every schema in `schemas/` must declare.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Load a document with the Rust reader of its kind and serialize it back.
pub type Reader = fn(&str) -> Result<Value, String>;

/// One family of data documents, the schema that validates it and the Rust reader
/// that loads it.
pub struct DataSet {
    /// Directory of the data files, relative to the repository root.
    pub dir: &'static str,
    /// File-name prefix of the data files in `dir` that belong to this set; empty
    /// for every `.json` file there. Every data file must belong to exactly one
    /// set ([`unclaimed_data_files`]).
    pub files: &'static str,
    /// Expected `kind` field (parts carry none yet).
    pub kind: Option<&'static str>,
    /// Schema file name inside `schemas/`.
    pub schema: &'static str,
    /// File-name prefix of the fixtures for this schema.
    pub prefix: &'static str,
    /// The Rust type that reads these documents, for messages.
    pub reader_name: &'static str,
    pub reader: Reader,
}

pub const DATA_SETS: &[DataSet] = &[
    DataSet {
        dir: "standards",
        files: "dimensions-",
        kind: Some("standard"),
        schema: "standard.schema.json",
        prefix: "standard-",
        reader_name: "openbrick_units::Standard",
        reader: read_standard,
    },
    DataSet {
        dir: "standards",
        files: "coupon-",
        kind: Some("coupon"),
        schema: "coupon.schema.json",
        prefix: "coupon-",
        reader_name: "openbrick_print::CouponSpec",
        reader: read_coupon,
    },
    DataSet {
        dir: "profiles/process",
        files: "",
        kind: Some("process"),
        schema: "process-profile.schema.json",
        prefix: "process-",
        reader_name: "openbrick_print::ProcessProfile",
        reader: read_process,
    },
    DataSet {
        dir: "profiles/fit",
        files: "",
        kind: Some("fit"),
        schema: "fit-profile.schema.json",
        prefix: "fit-",
        reader_name: "openbrick_print::FitProfile",
        reader: read_fit,
    },
    DataSet {
        dir: "parts",
        files: "",
        kind: None,
        schema: "part.schema.json",
        prefix: "part-",
        reader_name: "openbrick_part::PartDoc",
        reader: read_part,
    },
];

/// `$defs` that every schema repeats so each file stands alone; they must not drift.
/// `atThisMinor` is repeated too but follows each schema's own minor version
/// ([`check_minor_version`]).
pub const SHARED_DEFS: &[&str] = &[
    "pragma",
    "semver",
    "spdx",
    "credit",
    "provenance",
    "profileId",
];

fn written(v: Result<Value, serde_json::Error>) -> Result<Value, String> {
    v.map_err(|e| format!("cannot be written back: {e}"))
}

/// The standard, plus the one rule its schema cannot state: row ids are unique,
/// because `Standard::mm(id)` returns the first row with that id.
fn read_standard(text: &str) -> Result<Value, String> {
    let s: openbrick_units::Standard = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut seen = BTreeSet::new();
    for f in &s.features {
        if !seen.insert(f.id.as_str()) {
            return Err(format!(
                "feature id `{}` is used by two rows; STANDARD.mm(\"{}\") only ever finds the first, so rename or merge them",
                f.id, f.id
            ));
        }
    }
    written(serde_json::to_value(&s))
}

fn read_process(text: &str) -> Result<Value, String> {
    let p: openbrick_print::ProcessProfile =
        serde_json::from_str(text).map_err(|e| e.to_string())?;
    written(serde_json::to_value(&p))
}

fn read_fit(text: &str) -> Result<Value, String> {
    let f: openbrick_print::FitProfile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    written(serde_json::to_value(&f))
}

/// The coupon sweep, plus what its schema cannot state (`CouponSpec::check`):
/// unique cell ids, standard ids with a nominal, one cell per offset field and
/// whole numbers of steps.
fn read_coupon(text: &str) -> Result<Value, String> {
    let c: openbrick_print::CouponSpec = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let problems = c.check();
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    written(serde_json::to_value(&c))
}

impl DataSet {
    /// The data files of this set, sorted.
    pub fn data_files(&self, root: &Path) -> Result<Vec<PathBuf>, String> {
        Ok(json_files(&root.join(self.dir))?
            .into_iter()
            .filter(|p| file_name(p).starts_with(self.files))
            .collect())
    }
}

/// `.json` files in a data directory that no data set, or more than one, claims;
/// one line per problem.
pub fn unclaimed_data_files(root: &Path) -> Vec<String> {
    let dirs: BTreeSet<&str> = DATA_SETS.iter().map(|s| s.dir).collect();
    let mut problems = Vec::new();
    for dir in dirs {
        let Ok(files) = json_files(&root.join(dir)) else {
            continue; // validate_json_dir reports a missing directory
        };
        for p in files {
            let name = file_name(&p);
            let claims = DATA_SETS
                .iter()
                .filter(|s| s.dir == dir && name.starts_with(s.files))
                .count();
            if claims != 1 {
                problems.push(format!(
                    "{dir}/{name}: {claims} data sets claim it; name it so exactly one schema validates it (in {dir}/: {})",
                    DATA_SETS
                        .iter()
                        .filter(|s| s.dir == dir)
                        .map(|s| format!("{}* -> {}", s.files, s.schema))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    problems
}

/// Parts also get `PartDoc::check`: unique feature ids, known ops, anchors defined
/// before use.
fn read_part(text: &str) -> Result<Value, String> {
    let d = openbrick_part::PartDoc::parse(text).map_err(|e| e.to_string())?;
    written(serde_json::to_value(&d))
}

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
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Schema::from_value(&name, raw).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Meta-validate and compile a schema already parsed.
    pub fn from_value(name: &str, raw: Value) -> Result<Schema, String> {
        if raw.get("$schema").and_then(Value::as_str) != Some(DRAFT_2020_12) {
            return Err(format!("$schema must be {DRAFT_2020_12}"));
        }
        for key in ["$id", "title", "description"] {
            if raw
                .get(key)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(format!("missing {key}"));
            }
        }
        jsonschema::draft202012::meta::validate(&raw)
            .map_err(|e| format!("not a valid draft 2020-12 schema: {e}"))?;
        let validator =
            jsonschema::draft202012::new(&raw).map_err(|e| format!("does not compile: {e}"))?;
        Ok(Schema {
            name: name.to_string(),
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

/// Check one document that must be valid: it passes its schema, the Rust reader
/// loads it, and what the reader writes back passes the schema too. `shown` names
/// the document in messages. Returns one line per problem; empty means all good.
pub fn check_document(shown: &str, set: &DataSet, schema: &Schema, text: &str) -> Vec<String> {
    let doc: Value = match serde_json::from_str(text) {
        Ok(d) => d,
        Err(e) => return vec![format!("{shown}: is not valid JSON: {e}")],
    };
    let mut problems: Vec<String> = schema
        .failures(&doc)
        .into_iter()
        .map(|f| format!("{shown} does not match {}: {f}", schema.name))
        .collect();
    match (set.reader)(text) {
        Err(e) => problems.push(format!("{shown}: {} cannot load it: {e}", set.reader_name)),
        Ok(back) => problems.extend(schema.failures(&back).into_iter().map(|f| {
            format!(
                "{shown}: {} writes it back as a document {} rejects: {f}",
                set.reader_name, schema.name
            )
        })),
    }
    problems
}

/// The data set whose fixtures start with the prefix of this file name.
fn set_for_fixture(file_name: &str) -> Option<&'static DataSet> {
    DATA_SETS.iter().find(|s| file_name.starts_with(s.prefix))
}

pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Expected failure of one invalid fixture.
struct Expected {
    pointer: String,
    keyword: String,
    /// A substring of the validator's message that names the broken value or
    /// property, so two errors at the same pointer and keyword (two missing
    /// required properties, say) are told apart.
    message: String,
}

impl std::fmt::Display for Expected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` at \"{}\" with a message containing {:?}",
            self.keyword, self.pointer, self.message
        )
    }
}

fn expected_failures(path: &Path) -> Result<BTreeMap<String, Expected>, String> {
    let doc = read_json(path)?;
    let Some(map) = doc.get("fixtures").and_then(Value::as_object) else {
        return Err(format!("{}: needs a fixtures object", path.display()));
    };
    let mut out = BTreeMap::new();
    for (name, v) in map {
        let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let (Some(pointer), Some(keyword), Some(message)) =
            (field("pointer"), field("keyword"), field("message"))
        else {
            return Err(format!(
                "{}: {name} needs string pointer, keyword and message",
                path.display()
            ));
        };
        if message.is_empty() {
            return Err(format!(
                "{}: {name} needs a non-empty message",
                path.display()
            ));
        }
        out.insert(
            name.clone(),
            Expected {
                pointer,
                keyword,
                message,
            },
        );
    }
    Ok(out)
}

/// Self-check of `schemas/fixtures/`: every valid fixture passes [`check_document`];
/// every invalid one fails its schema, and only in the way it is labelled with in
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
                    "{shown}: name must start with one of {}",
                    DATA_SETS
                        .iter()
                        .map(|s| s.prefix)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                continue;
            };
            let schema = &schemas[set.schema];
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    problems.push(format!("{shown}: cannot read: {e}"));
                    continue;
                }
            };
            if should_pass {
                *valid_count.entry(set.schema).or_default() += 1;
                problems.extend(check_document(&shown, set, schema, &text));
                continue;
            }
            *invalid_count.entry(set.schema).or_default() += 1;
            let doc: Value = match serde_json::from_str(&text) {
                Ok(d) => d,
                Err(e) => {
                    problems.push(format!("{shown}: is not valid JSON: {e}"));
                    continue;
                }
            };
            let Some(want) = expected.get(&name) else {
                problems.push(format!(
                    "{shown}: add it to schemas/fixtures/expected-failures.json with the pointer, keyword and message it breaks"
                ));
                continue;
            };
            let failures = schema.failures(&doc);
            if failures.is_empty() {
                problems.push(format!(
                    "{shown} should fail {} with {want} but passes",
                    schema.name
                ));
            }
            for f in failures {
                if f.pointer != want.pointer
                    || f.keyword != want.keyword
                    || !f.message.contains(&want.message)
                {
                    problems.push(format!(
                        "{shown} should break only {want} but fails `{}` {f}",
                        f.keyword
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

/// The `pattern` of `$defs/atThisMinor` for a schema at version 1.`minor`: the
/// pragmas 1.0 to 1.`minor` and their patches (docs/plan/08 section 9; minor
/// versions only add fields, so the schema of the newest minor checks the
/// documents of every older one strictly).
pub fn at_this_minor_pattern(minor: u32) -> String {
    if minor == 0 {
        r"^1\.0(\.|$)".to_string()
    } else {
        let minors: Vec<String> = (0..=minor).map(|m| m.to_string()).collect();
        format!(r"^1\.({})(\.|$)", minors.join("|"))
    }
}

/// A schema's `$id` names its version (`.../schemas/1.1/process-profile.schema.json`),
/// and its `$defs/atThisMinor` must match exactly the pragmas of that version
/// and the minors before it ([`at_this_minor_pattern`]). Returns one line per
/// problem.
pub fn check_minor_version(schema: &Schema) -> Vec<String> {
    let id = schema.raw.get("$id").and_then(Value::as_str).unwrap_or("");
    let minor = id
        .rsplit('/')
        .nth(1)
        .and_then(|v| v.strip_prefix("1."))
        .and_then(|m| m.parse::<u32>().ok());
    let Some(minor) = minor else {
        return vec![format!(
            "{}: $id `{id}` does not name a 1.x schema version (.../schemas/1.<minor>/<file>)",
            schema.name
        )];
    };
    let want = at_this_minor_pattern(minor);
    let got = schema
        .raw
        .pointer("/$defs/atThisMinor/properties/openbrick/pattern")
        .and_then(Value::as_str);
    if got == Some(want.as_str()) {
        Vec::new()
    } else {
        vec![format!(
            "{}: $id names version 1.{minor}, so $defs/atThisMinor must match the pragmas 1.0 to 1.{minor} with the pattern {want:?}, not {got:?}",
            schema.name
        )]
    }
}

/// Follow local `$ref`s (`#/$defs/...`) until a schema without one.
fn resolve<'a>(root: &'a Value, mut node: &'a Value) -> &'a Value {
    for _ in 0..16 {
        let Some(ptr) = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix('#'))
        else {
            break;
        };
        match root.pointer(ptr) {
            Some(target) => node = target,
            None => break,
        }
    }
    node
}

/// Walk the main schema next to `$defs/knownFields`. Every object schema with fixed
/// `properties` is either listed in knownFields with exactly the same property
/// names, or marked open with an explicit `additionalProperties`.
fn walk_known(
    root: &Value,
    main: &Value,
    known: Option<&Value>,
    at: &str,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > 32 {
        out.push(format!("{at}: schema nests too deeply to check"));
        return;
    }
    let main = resolve(root, main);
    let known = known.map(|k| resolve(root, k)).filter(|k| k.is_object());
    let shown = if at.is_empty() { "(document root)" } else { at };
    let open = main.get("additionalProperties").is_some();
    let listed = known
        .and_then(|k| k.get("properties"))
        .and_then(Value::as_object);
    if let Some(props) = main.get("properties").and_then(Value::as_object) {
        match listed {
            Some(_) if open => out.push(format!(
                "{shown}: marked open with additionalProperties, but $defs/knownFields closes it; do one or the other"
            )),
            Some(listed) => {
                if known.and_then(|k| k.get("additionalProperties")) != Some(&Value::Bool(false)) {
                    out.push(format!(
                        "{shown}: its $defs/knownFields entry must set additionalProperties to false"
                    ));
                }
                let declared: BTreeSet<&str> = props.keys().map(String::as_str).collect();
                let named: BTreeSet<&str> = listed.keys().map(String::as_str).collect();
                for missing in declared.difference(&named) {
                    out.push(format!(
                        "{shown}: declares `{missing}`, which $defs/knownFields does not list, so 1.0 documents could not use it"
                    ));
                }
                for extra in named.difference(&declared) {
                    out.push(format!(
                        "{shown}: $defs/knownFields lists `{extra}`, which the schema does not declare"
                    ));
                }
            }
            None if !open => out.push(format!(
                "{shown}: has fixed properties but is neither listed in $defs/knownFields nor marked open with additionalProperties"
            )),
            None => {}
        }
        for (k, sub) in props {
            let child_known = listed.and_then(|l| l.get(k));
            walk_known(root, sub, child_known, &format!("{at}/{k}"), depth + 1, out);
        }
    } else if listed.is_some() {
        out.push(format!(
            "{shown}: $defs/knownFields lists properties, but the schema declares none here"
        ));
    }
    if let Some(items) = main.get("items") {
        let child_known = known.and_then(|k| k.get("items"));
        walk_known(root, items, child_known, &format!("{at}/*"), depth + 1, out);
    }
    if let Some(extra) = main.get("additionalProperties").filter(|v| v.is_object()) {
        walk_known(root, extra, None, &format!("{at}/*"), depth + 1, out);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        for branch in main
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk_known(root, branch, None, at, depth + 1, out);
        }
    }
}

/// `$defs/knownFields` must list exactly the properties each closed object
/// declares, and the schema must apply it to documents at its own minor version
/// (docs/plan/08 section 9). Returns one line per problem.
pub fn check_known_fields(schema: &Schema) -> Vec<String> {
    let raw = &schema.raw;
    let mut out = Vec::new();
    let wiring = [
        ("if", "#/$defs/atThisMinor"),
        ("then", "#/$defs/knownFields"),
    ];
    for (key, target) in wiring {
        if raw.get(key) != Some(&serde_json::json!({ "$ref": target })) {
            out.push(format!("must have \"{key}\": {{ \"$ref\": \"{target}\" }}"));
        }
    }
    match raw.pointer("/$defs/knownFields") {
        Some(known) => walk_known(raw, raw, Some(known), "", 0, &mut out),
        None => out.push("lacks $defs/knownFields".to_string()),
    }
    out.into_iter()
        .map(|p| format!("{}: {p}", schema.name))
        .collect()
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

    fn set(schema: &str) -> &'static DataSet {
        DATA_SETS
            .iter()
            .find(|s| s.schema == schema)
            .expect("data set")
    }

    fn fixture(name: &str) -> Value {
        read_json(&root().join("schemas/fixtures/valid").join(name)).expect("fixture")
    }

    #[test]
    fn fixtures_pass_and_fail_as_labelled() {
        let problems = check_fixtures(&root(), &schemas());
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn repository_data_matches_schemas_and_loads() {
        let schemas = schemas();
        let mut problems = Vec::new();
        for set in DATA_SETS {
            for path in set.data_files(&root()).expect("data directory") {
                let text = std::fs::read_to_string(&path).expect("data file reads");
                let shown = path.display().to_string();
                problems.extend(check_document(&shown, set, &schemas[set.schema], &text));
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn every_data_file_is_claimed_by_exactly_one_schema() {
        let problems = unclaimed_data_files(&root());
        assert!(problems.is_empty(), "{problems:#?}");
        // A stray file in standards/ would otherwise go unchecked.
        let dir =
            std::env::temp_dir().join(format!("openbrick-xtask-unclaimed-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("standards")).expect("temp dir");
        std::fs::write(dir.join("standards/coupon-v9.json"), "{}").expect("writes");
        std::fs::write(dir.join("standards/stray.json"), "{}").expect("writes");
        let problems = unclaimed_data_files(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(
            problems[0].starts_with("standards/stray.json: 0 data sets"),
            "{problems:#?}"
        );
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
    fn at_this_minor_follows_the_schema_version() {
        for schema in schemas().values() {
            let problems = check_minor_version(schema);
            assert!(problems.is_empty(), "{problems:#?}");
        }
        assert_eq!(at_this_minor_pattern(0), r"^1\.0(\.|$)");
        assert_eq!(at_this_minor_pattern(2), r"^1\.(0|1|2)(\.|$)");
        // A schema bumped to 1.1 that still applies its field list to 1.0 only.
        let mut raw =
            read_json(&root().join("schemas/process-profile.schema.json")).expect("schema");
        raw["$defs"]["atThisMinor"]["properties"]["openbrick"]["pattern"] =
            Value::String(at_this_minor_pattern(0));
        let schema = Schema::from_value("process-profile.schema.json", raw).expect("compiles");
        let problems = check_minor_version(&schema);
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].contains("version 1.1"), "{problems:#?}");
    }

    #[test]
    fn process_profiles_at_1_1_are_strict_and_later_minors_are_open() {
        let schemas = schemas();
        let process = &schemas["process-profile.schema.json"];
        let mut doc = fixture("process-minimal.json");
        doc["openbrick"] = serde_json::json!("1.1");
        doc["recommendedTubeWallMm"] = serde_json::json!(0.8);
        doc["maxRemovedAreaPct"] = serde_json::json!(1.0);
        assert!(process.failures(&doc).is_empty(), "1.1 names both fields");
        doc["maxRemovedAreaPtc"] = serde_json::json!(1.0);
        let failures = process.failures(&doc);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].keyword, "additionalProperties");
        doc["openbrick"] = serde_json::json!("1.2");
        assert!(process.failures(&doc).is_empty(), "1.2 may add fields");
        doc["maxRemovedAreaPct"] = serde_json::json!(101);
        let failures = process.failures(&doc);
        assert_eq!(
            failures.len(),
            1,
            "known fields stay checked: {failures:#?}"
        );
        assert_eq!(failures[0].keyword, "maximum");
    }

    #[test]
    fn known_fields_match_the_declared_properties() {
        for schema in schemas().values() {
            let problems = check_known_fields(schema);
            assert!(problems.is_empty(), "{problems:#?}");
        }
    }

    #[test]
    fn known_fields_drift_is_reported() {
        let mut raw = read_json(&root().join("schemas/fit-profile.schema.json")).expect("schema");
        let calibration = raw
            .pointer_mut("/$defs/knownFields/properties/calibration/properties")
            .and_then(Value::as_object_mut)
            .expect("calibration is listed");
        calibration.remove("date");
        calibration.insert("dat".into(), Value::Bool(true));
        let schema = Schema::from_value("fit-profile.schema.json", raw).expect("compiles");
        let problems = check_known_fields(&schema);
        assert_eq!(problems.len(), 2, "{problems:#?}");
        assert!(
            problems[0].contains("/calibration: declares `date`"),
            "{problems:#?}"
        );
        assert!(problems[1].contains("lists `dat`"), "{problems:#?}");
    }

    #[test]
    fn unknown_fields_fail_at_1_0_and_pass_at_later_minors() {
        let schemas = schemas();
        let fit = &schemas["fit-profile.schema.json"];
        let mut doc = fixture("fit-calibrated.json");
        doc["offsetsMm"]["studDiamter"] = serde_json::json!(0.1);
        let failures = fit.failures(&doc);
        assert_eq!(failures.len(), 1, "{failures:#?}");
        assert_eq!(failures[0].pointer, "/offsetsMm");
        assert_eq!(failures[0].keyword, "additionalProperties");
        // Fit profiles are at schema 1.1, so 1.2 is the first later minor.
        doc["openbrick"] = serde_json::json!("1.1");
        assert_eq!(fit.failures(&doc).len(), 1, "1.1 is strict too");
        doc["openbrick"] = serde_json::json!("1.2");
        assert!(fit.failures(&doc).is_empty(), "1.2 may add offsets");
        doc["offsetsMm"]["studDiameter"] = serde_json::json!(5.0);
        let failures = fit.failures(&doc);
        assert_eq!(
            failures.len(),
            1,
            "known fields stay checked: {failures:#?}"
        );
        assert_eq!(failures[0].keyword, "maximum");
    }

    #[test]
    fn readers_write_back_valid_documents() {
        // Writers omit absent optional fields instead of writing null, keep the credit,
        // and write no empty date: each of these used to fail the schema on the way back.
        let schemas = schemas();
        for (name, schema) in [
            ("part-minimal.json", "part.schema.json"),
            ("fit-uncalibrated.json", "fit-profile.schema.json"),
            ("standard-minimal.json", "standard.schema.json"),
            ("process-minimal.json", "process-profile.schema.json"),
        ] {
            let text = serde_json::to_string(&fixture(name)).expect("serialises");
            let back = (set(schema).reader)(&text).expect("reader loads the fixture");
            let failures = schemas[schema].failures(&back);
            assert!(failures.is_empty(), "{name}: {failures:#?}");
            assert!(back.get("credit").is_some(), "{name}: credit survives");
        }
    }

    #[test]
    fn coupon_version_is_read_as_json_schema_counts_it() {
        let schemas = schemas();
        let fit = set("fit-profile.schema.json");
        let mut doc = fixture("fit-calibrated.json");
        // Draft 2020-12 counts 1.0 as an integer, so the schema accepts it; the
        // reader must too.
        doc["calibration"]["couponVersion"] = serde_json::json!(1.0);
        assert!(schemas[fit.schema].failures(&doc).is_empty());
        let text = serde_json::to_string(&doc).expect("serialises");
        let back = (fit.reader)(&text).expect("1.0 loads as coupon version 1");
        assert_eq!(back["calibration"]["couponVersion"], serde_json::json!(1));
        // 2^32 is one past u32::MAX: the schema rejects it (fixture
        // fit-coupon-version-too-large.json) and so does the reader.
        doc["calibration"]["couponVersion"] = serde_json::json!(4_294_967_296_u64);
        let text = serde_json::to_string(&doc).expect("serialises");
        assert!((fit.reader)(&text).is_err());
        doc["calibration"]["couponVersion"] = serde_json::json!(1.5);
        let text = serde_json::to_string(&doc).expect("serialises");
        assert!((fit.reader)(&text).is_err());
    }

    #[test]
    fn duplicate_standard_ids_are_reported() {
        let schemas = schemas();
        let standard = set("standard.schema.json");
        let mut doc = fixture("standard-minimal.json");
        let first = doc["features"][0].clone();
        doc["features"]
            .as_array_mut()
            .expect("features")
            .push(first);
        let text = serde_json::to_string(&doc).expect("serialises");
        assert!(
            schemas[standard.schema].failures(&doc).is_empty(),
            "the schema cannot see duplicate ids"
        );
        let problems = check_document("dup", standard, &schemas[standard.schema], &text);
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].contains("is used by two rows"), "{problems:#?}");
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
