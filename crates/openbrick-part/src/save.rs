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
    Ok(format::layout(&root))
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
    /// document (an error names the preset and the parameter).
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
        resolve_params(self, &values, process).map_err(|e| EvalError::Preset {
            name: name.to_string(),
            message: format!("{e}; change the value in the preset or remove it"),
        })?;
        Ok(values)
    }
}
