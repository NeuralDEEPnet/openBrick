//! Saving a part document from the designer (docs/plan/06 sections 3 and 11):
//! the edited parameter values become the document's defaults
//! ([`PartDoc::with_values`]), the current values can be kept as a named preset
//! ([`PartDoc::with_preset`]), and [`saved_text`] writes the edited document
//! over the text it was opened from, in the one layout of [`crate::format`]:
//! only the changed defaults and the presets are rewritten, everything else
//! keeps its order and spelling. Every value is checked by the evaluator's own
//! parameter rules first ([`resolve_params`]), so a saved document always
//! builds with the values it was saved with.
//!
//! [`unsupported_features`] lists the features this evaluator cannot build
//! yet, so a front end can name them when a document is opened instead of
//! failing on the first one.

use openbrick_print::ProcessProfile;
use serde::Serialize;
use serde_json::Value;

use crate::eval::{resolve_params, EvalError, ParamValues, SUPPORTED_OPS};
use crate::format::{self, Node};
use crate::{PartDoc, PartError, Preset};

/// Largest part document, in bytes, that the CLI and the designer read: far
/// above any hand-written part (the logo tile is about 3 KB), small enough to
/// refuse a wrong file at once.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

/// A feature whose op the evaluator does not build yet ([`SUPPORTED_OPS`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnsupportedFeature {
    /// The feature id, such as `label`.
    pub feature: String,
    /// Its op, such as `relief.text`.
    pub op: String,
    /// Its `if` expression, when it has one: the feature is skipped, and the
    /// part builds without it, while that expression is false.
    pub condition: Option<String>,
}

/// The features of `doc` whose op [`crate::evaluate`] does not build yet, in
/// document order, whether or not their `if` is currently true.
pub fn unsupported_features(doc: &PartDoc) -> Vec<UnsupportedFeature> {
    doc.features
        .iter()
        .filter(|f| !SUPPORTED_OPS.contains(&f.op.as_str()))
        .map(|f| UnsupportedFeature {
            feature: f.id.clone(),
            op: f.op.clone(),
            condition: f.condition.clone(),
        })
        .collect()
}

/// Whether two parameter values are the same value: numbers by value (`1`
/// and `1.0` are the same length), arrays item by item, anything else as JSON.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_value(a, b))
        }
        _ => a == b,
    }
}

/// The text of `edited`, a document opened from `original` and changed with
/// [`PartDoc::with_values`] and [`PartDoc::with_preset`], in the saved layout
/// of [`crate::format`]. It starts from `original`: a default whose value
/// changed is rewritten (added after `type` when the parameter had none), and
/// the presets are rewritten when they changed (added after `params`, or
/// removed when none are left); every other member keeps its place and
/// spelling. Saving an unchanged document gives `original` in the saved
/// layout, which reads back as the same document with the same content hash.
pub fn saved_text(original: &str, edited: &PartDoc) -> Result<String, PartError> {
    let before = PartDoc::parse(original)?;
    let mut root =
        format::parse(original).map_err(|e| PartError::Json(serde::de::Error::custom(e)))?;
    if let Some(params) = root.get_mut("params") {
        for (name, p) in &edited.params {
            let unchanged = before
                .params
                .get(name)
                .is_some_and(|b| same_value(&b.default, &p.default));
            if unchanged || p.default.is_null() {
                continue;
            }
            if let Some(param) = params.get_mut(name) {
                param.set("default", Node::of(&p.default)?, &["type"]);
            }
        }
    }
    if before.presets != edited.presets {
        if edited.presets.is_empty() {
            root.remove("presets");
        } else {
            root.set(
                "presets",
                Node::of(&edited.presets)?,
                &["params", "license"],
            );
        }
    }
    edited.check()?;
    let text = format::layout(&root);
    // The rewrite edits the text as written, so a key spelled with an escape,
    // or written twice (serde keeps the last, the rewrite finds the first),
    // would leave the old value in place. Read the result back and refuse to
    // report a save that did not take.
    let after = PartDoc::parse(&text)?;
    for (name, p) in &edited.params {
        let written = after.params.get(name).map(|a| &a.default);
        if !p.default.is_null() && !written.is_some_and(|w| same_value(w, &p.default)) {
            return Err(PartError::SaveMismatch {
                what: format!("default of `{name}`"),
            });
        }
    }
    if after.presets != edited.presets {
        return Err(PartError::SaveMismatch {
            what: "presets".to_string(),
        });
    }
    Ok(text)
}

impl PartDoc {
    /// The document on its own, without the text it was read from, in the
    /// saved layout of [`crate::format`] (struct fields in schema order;
    /// `fitCritical` and `offGrid` are written on every feature). Reading it
    /// back gives the same document. A front end saving an opened document
    /// uses [`saved_text`], which keeps the author's order instead.
    pub fn to_saved_text(&self) -> String {
        crate::to_saved_text(self).expect("a part document serialises")
    }

    /// The document with `values` as the defaults of the parameters they name,
    /// each checked and normalised as the evaluator reads it (`"0.6mm"` becomes
    /// `0.6`, an int becomes a whole number). Parameters not named keep their
    /// defaults as written. `process` resolves `profile.` names in
    /// `visibleWhen` expressions.
    pub fn with_values(
        &self,
        values: &ParamValues,
        process: &ProcessProfile,
    ) -> Result<PartDoc, EvalError> {
        self.check()?;
        let resolved = resolve_params(self, values, process)?;
        let mut doc = self.clone();
        for name in values.values.keys() {
            if let (Some(p), Some(v)) = (doc.params.get_mut(name), resolved.values.get(name)) {
                p.default = v.clone();
            }
        }
        Ok(doc)
    }

    /// The document with a preset `name` holding `values` (checked and
    /// normalised as by [`PartDoc::with_values`], in the document's parameter
    /// order). A preset of that name is replaced in place; otherwise the new
    /// one is added last. The defaults do not change.
    pub fn with_preset(
        &self,
        name: &str,
        values: &ParamValues,
        process: &ProcessProfile,
    ) -> Result<PartDoc, EvalError> {
        self.check()?;
        let name = name.trim();
        if name.is_empty() {
            return Err(PartError::PresetName.into());
        }
        let resolved = resolve_params(self, values, process)?;
        let params = self
            .params
            .keys()
            .filter(|k| values.values.contains_key(*k))
            .filter_map(|k| resolved.values.get(k).map(|v| (k.clone(), v.clone())))
            .collect();
        let preset = Preset {
            name: name.to_string(),
            params,
        };
        let mut doc = self.clone();
        match doc.presets.iter_mut().find(|p| p.name == name) {
            Some(p) => *p = preset,
            None => doc.presets.push(preset),
        }
        Ok(doc)
    }

    /// The values of preset `name` as parameter overrides, checked against the
    /// document (an error names the preset and the parameter) and normalised
    /// as the evaluator reads them (`"0.8mm"` becomes `0.8`), so a front end
    /// can compare them with its controls' values.
    pub fn preset_values(
        &self,
        name: &str,
        process: &ProcessProfile,
    ) -> Result<ParamValues, EvalError> {
        let preset = self.preset(name).ok_or_else(|| EvalError::Preset {
            name: name.to_string(),
            message: "this part has no preset of that name".to_string(),
        })?;
        let values = ParamValues {
            values: preset
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        let resolved = resolve_params(self, &values, process).map_err(|e| EvalError::Preset {
            name: name.to_string(),
            message: format!("{e}; change the value in the preset or remove it"),
        })?;
        Ok(ParamValues {
            values: values
                .values
                .keys()
                .filter_map(|k| resolved.values.get(k).map(|v| (k.clone(), v.clone())))
                .collect(),
        })
    }

    /// Every parameter's default, checked and normalised as the evaluator
    /// reads it (`"0.6mm"` becomes `0.6`, an int a whole number), so a front
    /// end shows and compares the values the core builds with.
    pub fn default_values(&self, process: &ProcessProfile) -> Result<ParamValues, EvalError> {
        let resolved = resolve_params(self, &ParamValues::new(), process)?;
        Ok(ParamValues {
            values: resolved.values,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DOC: &str = r#"{"openbrick":"1.0","id":"t","version":"0.1.0","name":"t","license":"CC0-1.0",
        "params":{"size":{"type":"int","default":2,"min":1,"max":8}},
        "features":[{"id":"body","op":"body.tile","size":[2,"$size"],"height":"plate"}]}"#;

    fn process() -> ProcessProfile {
        openbrick_print::default_process_profiles().remove(0)
    }

    fn edit(text: &str, size: i64) -> Result<String, PartError> {
        let doc = PartDoc::parse(text)?;
        let edited = doc
            .with_values(&ParamValues::new().with("size", json!(size)), &process())
            .expect("fits");
        saved_text(text, &edited)
    }

    #[test]
    fn an_edited_default_is_saved() {
        let saved = edit(DOC, 4).expect("saves");
        assert_eq!(
            PartDoc::parse(&saved).unwrap().params["size"].default,
            json!(4)
        );
    }

    #[test]
    fn an_escaped_parameter_key_is_refused_instead_of_dropping_the_edit() {
        let escaped = DOC.replace(r#""size":{"#, r#""\u0073ize":{"#);
        assert!(PartDoc::parse(&escaped).is_ok(), "serde reads the escape");
        let e = edit(&escaped, 4).unwrap_err();
        assert!(matches!(e, PartError::SaveMismatch { .. }), "{e}");
        assert!(e.to_string().contains("`size`"), "{e}");
    }

    #[test]
    fn a_parameter_written_twice_is_refused_instead_of_dropping_the_edit() {
        let twice = DOC.replace(
            r#""params":{"#,
            r#""params":{"size":{"type":"int","default":3,"min":1,"max":8},"#,
        );
        assert!(PartDoc::parse(&twice).is_ok(), "serde keeps the last");
        let e = edit(&twice, 4).unwrap_err();
        assert!(matches!(e, PartError::SaveMismatch { .. }), "{e}");
    }

    #[test]
    fn defaults_and_presets_are_normalised_for_the_panel() {
        let text = DOC
            .replace(
                r#""params":{"#,
                r#""params":{"h":{"type":"length","default":"0.6mm","min":0.2,"max":1.2},"#,
            )
            .replace(
                r#""features""#,
                r#""presets":[{"name":"High","params":{"h":"0.8mm"}}],"features""#,
            );
        let doc = PartDoc::parse(&text).unwrap();
        let defaults = doc.default_values(&process()).unwrap();
        assert_eq!(defaults.values["h"], json!(0.6));
        assert_eq!(defaults.values["size"], json!(2));
        let preset = doc.preset_values("High", &process()).unwrap();
        assert_eq!(preset.values, [("h".to_string(), json!(0.8))].into());
    }
}
