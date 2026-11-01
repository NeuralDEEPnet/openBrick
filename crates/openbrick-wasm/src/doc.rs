//! Opening and saving part documents in the designer (docs/plan/06 sections 3
//! and 11): [`open_part_json`] checks a document the user opened and says what
//! the page needs to build its panel (the presets, each checked, and the
//! features this evaluator cannot build yet); [`save_part_json`] writes the
//! document with the edited values as its defaults, and [`add_preset_json`]
//! keeps the current values as a named preset. Both write the one layout of
//! `openbrick_part::format`, so a file saved here reads back as the same
//! document in the CLI, which then builds it with the same content hash
//! (`crates/openbrick/tests/cli.rs`).

use openbrick_part::save::MAX_DOCUMENT_BYTES;
use openbrick_part::{saved_text, unsupported_features, ParamValues, PartDoc};
use serde_json::{json, Value};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::part::process_profile;
use crate::CoreError;

/// The parameter values the page sends, a JSON object (empty text for none).
fn values(params_json: &str) -> Result<ParamValues, CoreError> {
    if params_json.trim().is_empty() {
        return Ok(ParamValues::new());
    }
    serde_json::from_str(params_json).map_err(|e| {
        CoreError(format!(
            "the parameter values must be a JSON object of name to value: {e}"
        ))
    })
}

/// The document, parsed and structurally checked, with a message that says
/// what to do.
fn parse(doc_json: &str) -> Result<PartDoc, CoreError> {
    if doc_json.len() > MAX_DOCUMENT_BYTES {
        return Err(CoreError(format!(
            "the file is {} bytes, and a part document can be at most {MAX_DOCUMENT_BYTES} bytes ({} MiB); choose the part's .obpart.json file",
            doc_json.len(),
            MAX_DOCUMENT_BYTES / (1024 * 1024)
        )));
    }
    let v: Value = serde_json::from_str(doc_json).map_err(|e| {
        CoreError(format!(
            "the file is not JSON ({e}); choose a part document, a .obpart.json file"
        ))
    })?;
    if v.get("openbrick").is_none() || v.get("features").is_none() {
        let what = match v.get("kind").and_then(Value::as_str) {
            Some(kind) => format!("a `{kind}` document"),
            None => "JSON, but not a part document".to_string(),
        };
        return Err(CoreError(format!(
            "this file is {what}: a part document has an `openbrick` version and a `features` list; choose a .obpart.json file"
        )));
    }
    PartDoc::parse(doc_json).map_err(|e| CoreError(format!("the part document is not valid: {e}")))
}

/// Check a part document the user opened, without building it. Never fails:
/// the answer is JSON, either `{ ok: true, id, name, version, license,
/// credit, presets, unsupported }` or `{ ok: false, error }`.
///
/// - `presets`: `[{ name, params, error }]`, where `error` is null or why the
///   preset does not fit the part (a value out of range, an option it lacks),
///   as the evaluator's parameter rules say with `process_id` (a bundled
///   process profile, for `profile.` names in `visibleWhen`).
/// - `unsupported`: `[{ feature, op, condition }]`, the features whose op this
///   evaluator does not build yet (`openbrick_part::unsupported_features`):
///   the part builds only while each one's `if` (`condition`) is false.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn open_part_json(doc_json: &str, process_id: &str) -> String {
    let answer = (|| -> Result<Value, CoreError> {
        let doc = parse(doc_json)?;
        let process = process_profile(process_id)?;
        let presets: Vec<Value> = doc
            .presets
            .iter()
            .map(|p| {
                let error = doc.preset_values(&p.name, &process).err();
                json!({
                    "name": p.name,
                    "params": p.params,
                    "error": error.map(|e| e.to_string()),
                })
            })
            .collect();
        Ok(json!({
            "ok": true,
            "id": doc.id,
            "name": doc.name,
            "version": doc.version,
            "license": doc.license,
            "credit": doc.credit,
            "presets": presets,
            "unsupported": unsupported_features(&doc),
        }))
    })();
    match answer {
        Ok(v) => v.to_string(),
        Err(e) => json!({ "ok": false, "error": e.0 }).to_string(),
    }
}

/// The part document with `params_json` (a JSON object of parameter values,
/// as [`crate::evaluate_part`] takes them) as the defaults of the parameters it
/// names, written in the saved layout. Refuses values the evaluator refuses,
/// with its message.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn save_part_json(
    doc_json: &str,
    params_json: &str,
    process_id: &str,
) -> Result<String, CoreError> {
    let doc = parse(doc_json)?;
    let saved = doc
        .with_values(&values(params_json)?, &process_profile(process_id)?)
        .map_err(|e| CoreError(e.to_string()))?;
    saved_text(doc_json, &saved)
        .map_err(|e| CoreError(format!("the part document cannot be saved: {e}")))
}

/// The part document with a preset `name` holding `params_json` (replacing a
/// preset of that name), written in the saved layout; the defaults do not
/// change. Refuses an empty name and values the evaluator refuses.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn add_preset_json(
    doc_json: &str,
    name: &str,
    params_json: &str,
    process_id: &str,
) -> Result<String, CoreError> {
    let doc = parse(doc_json)?;
    let saved = doc
        .with_preset(name, &values(params_json)?, &process_profile(process_id)?)
        .map_err(|e| CoreError(e.to_string()))?;
    saved_text(doc_json, &saved)
        .map_err(|e| CoreError(format!("the part document cannot be saved: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{example_fit_profile_json, logo_tile_part_json};
    use crate::{evaluate_part, AssetSet};

    const BRAND: &str = include_str!("../../../parts/ndn.brand-tile.obpart.json");
    const KEYCHAIN: &str = include_str!("../../../parts/ndn.nameplate-keychain.obpart.json");
    const MARK: (&str, &str) = (
        "assets/brand/neuraldeep-mark-2x4.svg",
        include_str!("../../../parts/assets/brand/neuraldeep-mark-2x4.svg"),
    );

    fn mark() -> AssetSet {
        let mut a = AssetSet::new();
        a.add(MARK.0, MARK.1.as_bytes().to_vec()).unwrap();
        a
    }

    fn open(text: &str) -> Value {
        serde_json::from_str(&open_part_json(text, "fdm-0.4")).unwrap()
    }

    #[test]
    fn opening_lists_presets_and_unsupported_features() {
        let brand = open(BRAND);
        assert_eq!(brand["ok"], json!(true), "{brand}");
        assert_eq!(brand["id"], "ndn.brand-tile");
        let presets = brand["presets"].as_array().unwrap();
        assert!(presets.len() >= 2);
        assert!(presets.iter().all(|p| p["error"].is_null()), "{presets:?}");
        assert_eq!(brand["unsupported"], json!([]));

        // The keychain's text label and key ring are not built yet: both are
        // named, with the `if` that turns each off.
        let keychain = open(KEYCHAIN);
        assert_eq!(keychain["ok"], json!(true), "{keychain}");
        assert_eq!(
            keychain["unsupported"],
            json!([
                { "feature": "label", "op": "relief.text", "condition": "!$studsOnTop" },
                { "feature": "ring", "op": "loop.keyring", "condition": "$loop != 'none'" },
            ])
        );
        assert_eq!(keychain["presets"], json!([]));
    }

    #[test]
    fn opening_the_wrong_file_says_what_to_choose() {
        for (text, want) in [
            ("not json", "not JSON"),
            (r#"{"kind":"fit","id":"x"}"#, "a `fit` document"),
            ("[1, 2]", "not a part document"),
            (
                r#"{"openbrick":"1.0","id":"t","version":"0.1.0","name":"t","license":"CC0-1.0","features":[{"id":"a","op":"nope"}]}"#,
                "unknown op `nope`",
            ),
        ] {
            let v = open(text);
            assert_eq!(v["ok"], json!(false), "{text}");
            let e = v["error"].as_str().unwrap();
            assert!(e.contains(want), "{text}: {e}");
        }
        let big = " ".repeat(MAX_DOCUMENT_BYTES + 1);
        assert!(open(&big)["error"].as_str().unwrap().contains("at most"));
        // A preset that does not fit names itself and the parameter.
        let mut doc: Value = serde_json::from_str(BRAND).unwrap();
        doc["presets"] = json!([{ "name": "Too high", "params": { "reliefHeight": 9 } }]);
        let v = open(&doc.to_string());
        let e = v["presets"][0]["error"].as_str().unwrap();
        assert!(e.contains("Too high") && e.contains("reliefHeight"), "{e}");
    }

    #[test]
    fn saving_unchanged_keeps_the_document_and_its_content_hash() {
        for text in [logo_tile_part_json(), BRAND.to_string()] {
            let saved = save_part_json(&text, "", "fdm-0.4").unwrap();
            let again = save_part_json(&saved, "", "fdm-0.4").unwrap();
            assert_eq!(saved, again, "saving is idempotent");
            let a: Value = serde_json::to_value(PartDoc::parse(&text).unwrap()).unwrap();
            let b: Value = serde_json::to_value(PartDoc::parse(&saved).unwrap()).unwrap();
            assert_eq!(a, b, "the same document");
        }
        let fit = example_fit_profile_json();
        let before = evaluate_part(BRAND, "", "fdm-0.4", &fit, &mark()).unwrap();
        let saved = save_part_json(BRAND, "", "fdm-0.4").unwrap();
        let after = evaluate_part(&saved, "", "fdm-0.4", &fit, &mark()).unwrap();
        assert_eq!(before.content_hash(), after.content_hash());
    }

    #[test]
    fn saved_values_become_defaults_and_build_the_same_part() {
        let fit = example_fit_profile_json();
        let params = r#"{"relief":"deboss","reliefHeight":"0.8mm","size":[2,6]}"#;
        let saved = save_part_json(BRAND, params, "fdm-0.4").unwrap();
        let doc = PartDoc::parse(&saved).unwrap();
        assert_eq!(doc.params["relief"].default, json!("deboss"));
        assert_eq!(doc.params["reliefHeight"].default, json!(0.8), "normalised");
        assert_eq!(doc.params["size"].default, json!([2, 6]));
        // Parameter order is the document's, so the panel keeps its order.
        let order: Vec<&str> = doc.params.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                "size",
                "height",
                "logo",
                "relief",
                "inlayStyle",
                "reliefHeight",
                "margin"
            ]
        );
        let edited = evaluate_part(BRAND, params, "fdm-0.4", &fit, &mark()).unwrap();
        let mut reopened = evaluate_part(&saved, "", "fdm-0.4", &fit, &mark()).unwrap();
        let mut original = evaluate_part(BRAND, params, "fdm-0.4", &fit, &mark()).unwrap();
        assert_eq!(edited.triangle_count(), reopened.triangle_count());
        for i in 0..original.mesh_count() {
            let (a, b) = (
                original.take_mesh(i).unwrap(),
                reopened.take_mesh(i).unwrap(),
            );
            assert_eq!(a.fingerprint(), b.fingerprint(), "mesh {i}");
        }
        // Values the evaluator refuses are refused with its message.
        let e = save_part_json(BRAND, r#"{"reliefHeight":9}"#, "fdm-0.4").unwrap_err();
        assert!(e.0.contains("reliefHeight"), "{e}");
        assert!(save_part_json(BRAND, r#"{"nope":1}"#, "fdm-0.4").is_err());
        assert!(save_part_json(BRAND, "[1]", "fdm-0.4").is_err());
    }

    #[test]
    fn presets_are_added_replaced_and_kept_in_the_saved_text() {
        let text = add_preset_json(
            BRAND,
            "  Wide deboss ",
            r#"{"size":[4,6],"relief":"deboss"}"#,
            "fdm-0.4",
        )
        .unwrap();
        let doc = PartDoc::parse(&text).unwrap();
        let brand = PartDoc::parse(BRAND).unwrap();
        assert_eq!(doc.presets.len(), brand.presets.len() + 1);
        let p = doc.preset("Wide deboss").expect("trimmed name");
        assert_eq!(
            serde_json::to_value(&p.params).unwrap(),
            json!({ "size": [4, 6], "relief": "deboss" })
        );
        // The defaults stay.
        assert_eq!(doc.params["relief"].default, brand.params["relief"].default);
        // The same name again replaces it in place.
        let again =
            add_preset_json(&text, "Wide deboss", r#"{"relief":"emboss"}"#, "fdm-0.4").unwrap();
        let doc = PartDoc::parse(&again).unwrap();
        assert_eq!(doc.presets.len(), brand.presets.len() + 1);
        assert_eq!(
            doc.presets.last().unwrap().params["relief"],
            json!("emboss")
        );
        assert!(add_preset_json(BRAND, " ", "", "fdm-0.4")
            .unwrap_err()
            .0
            .contains("needs a name"));
        assert!(add_preset_json(BRAND, "x", r#"{"relief":"glow"}"#, "fdm-0.4").is_err());
    }
}
