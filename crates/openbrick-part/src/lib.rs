//! `openbrick/part@1` documents: parse, validate structure, and (from M1) evaluate
//! into geometry and connectors. See docs/plan/06.

use std::collections::BTreeMap;

use serde_json::Value;

/// Credit block carried by every data document (shared with the standard and the profiles).
pub use openbrick_units::Credit;

/// Provenance so attribution travels with the data.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub modified: bool,
    #[serde(default)]
    pub history: Vec<String>,
}

/// A typed, UI-bearing parameter. Absent optional fields are omitted when written
/// rather than written as `null`, so a written document passes `schemas/part.schema.json`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Param {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(
        default,
        rename = "visibleWhen",
        skip_serializing_if = "Option::is_none"
    )]
    pub visible_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manipulator: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One feature operation; op-specific fields stay as JSON until the evaluator reads them.
/// Absent optional fields are omitted when written, as for [`Param`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Feature {
    pub id: String,
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, rename = "colorRole", skip_serializing_if = "Option::is_none")]
    pub color_role: Option<String>,
    #[serde(default, rename = "fitCritical")]
    pub fit_critical: bool,
    #[serde(default, rename = "offGrid")]
    pub off_grid: bool,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

/// The part document.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PartDoc {
    pub openbrick: String,
    pub id: String,
    pub version: String,
    pub name: String,
    pub license: String,
    #[serde(default)]
    pub credit: Credit,
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    pub features: Vec<Feature>,
    #[serde(default = "derived")]
    pub snap: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub print: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub export: Value,
}

fn derived() -> Value {
    Value::String("derived".into())
}

/// Feature ops known to the evaluator (docs/plan/06 section 4).
pub const KNOWN_OPS: &[&str] = &[
    "body.tile",
    "body.plate",
    "body.brick",
    "body.baseplate",
    "stud.field",
    "tube.field",
    "technic.hole",
    "axle.hole",
    "axle",
    "pin",
    "bar",
    "clip",
    "hinge.finger",
    "ball",
    "socket",
    "relief.svg",
    "relief.text",
    "relief.image",
    "mosaic",
    "loop.keyring",
    "cut.svg",
    "cut.shape",
    "round",
    "adapter.stud-technic",
];

#[derive(Debug, thiserror::Error)]
pub enum PartError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported document version {0}; this build understands 1.x")]
    Version(String),
    #[error("feature `{id}` uses unknown op `{op}`")]
    UnknownOp { id: String, op: String },
    #[error("feature `{0}` has a duplicate id")]
    DuplicateId(String),
    #[error("feature `{id}` anchors on `{on}`, which is not defined before it")]
    DanglingAnchor { id: String, on: String },
}

impl PartDoc {
    /// Parse and structurally validate a document.
    pub fn parse(text: &str) -> Result<PartDoc, PartError> {
        let doc: PartDoc = serde_json::from_str(text)?;
        doc.check()?;
        Ok(doc)
    }

    /// Structural checks that need no geometry.
    pub fn check(&self) -> Result<(), PartError> {
        if !self.openbrick.starts_with("1.") {
            return Err(PartError::Version(self.openbrick.clone()));
        }
        let mut seen: Vec<&str> = Vec::new();
        for f in &self.features {
            if seen.contains(&f.id.as_str()) {
                return Err(PartError::DuplicateId(f.id.clone()));
            }
            if !KNOWN_OPS.contains(&f.op.as_str()) {
                return Err(PartError::UnknownOp {
                    id: f.id.clone(),
                    op: f.op.clone(),
                });
            }
            if let Some(on) = &f.on {
                let base = on.split('.').next().unwrap_or(on);
                if !seen.contains(&base) {
                    return Err(PartError::DanglingAnchor {
                        id: f.id.clone(),
                        on: on.clone(),
                    });
                }
            }
            seen.push(&f.id);
        }
        Ok(())
    }

    /// Ids of features marked fit-critical, plus those listed in `print.fitCritical`.
    pub fn fit_critical_features(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .features
            .iter()
            .filter(|f| f.fit_critical)
            .map(|f| f.id.clone())
            .collect();
        if let Some(list) = self.print.get("fitCritical").and_then(|v| v.as_array()) {
            for v in list {
                if let Some(s) = v.as_str() {
                    if !ids.iter().any(|i| i == s) {
                        ids.push(s.to_string());
                    }
                }
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_parts_parse() {
        for src in [
            include_str!("../../../parts/ndn.logo-tile.obpart.json"),
            include_str!("../../../parts/ndn.nameplate-keychain.obpart.json"),
        ] {
            let doc = PartDoc::parse(src).expect("example part parses");
            assert!(doc.fit_critical_features().contains(&"bottom".to_string()));
        }
    }

    #[test]
    fn dangling_anchor_is_rejected() {
        let src = r#"{"openbrick":"1.0","id":"t","version":"0.1.0","name":"t","license":"CC0-1.0",
            "features":[{"id":"a","op":"relief.svg","on":"body.top","src":"x.svg"}]}"#;
        assert!(matches!(
            PartDoc::parse(src),
            Err(PartError::DanglingAnchor { .. })
        ));
    }
}
