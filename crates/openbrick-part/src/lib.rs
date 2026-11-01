//! `openbrick/part@1` documents: parse, validate structure, evaluate into
//! geometry, connectors and measurements ([`eval`]), and check a built part
//! against the print rules ([`validate()`], docs/plan/05 section 5). See
//! docs/plan/06. The calibration coupon ([`coupon`], docs/plan/05 section 4) is
//! built here too, from the same stud, tube and rib code as the evaluator.
//! Saving a document ([`save`]: edited parameter values as its defaults, named
//! presets) writes the one text layout of [`format`], so every front end
//! writes the same bytes.

pub mod coupon;
pub mod eval;
pub mod format;
mod print_checks;
pub mod save;
mod validate;

pub use coupon::{
    build_coupon, coupon_layout, Coupon, CouponCellLayout, CouponLayout, CouponOptions, PocketSet,
};
pub use eval::{
    evaluate, Artwork, AssetResolver, Built, Connector, ConnectorSection, EvalError, EvalInput,
    MemoryAssets, MeshChecks, ParamValues, ResolvedParams, RoleMesh, SectionShape,
    PREVIEW_MESH_CHECK_TRIANGLES,
};
pub use format::to_saved_text;
pub use save::{saved_text, unsupported_features, UnsupportedFeature};
pub use validate::{validate, validate_on_bed};

use std::collections::BTreeMap;

use indexmap::IndexMap;

use serde_json::Value;

/// Credit block carried by every data document (shared with the standard and the profiles).
pub use openbrick_units::Credit;

/// Provenance so attribution travels with the data.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub source: String,
    /// Author, licence and link of the upstream source; omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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

/// A named set of parameter values (docs/plan/06 section 3), like an OpenSCAD
/// customizer `parameterSets` entry. Choosing it sets the parameters it names
/// and leaves the others as they are.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    /// Values by parameter name, in the order written.
    pub params: IndexMap<String, Value>,
}

/// The part document. Fields are written in the order of
/// `schemas/part.schema.json`; absent optional ones are left out, so what
/// [`to_saved_text`] writes is the document that was read.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PartDoc {
    pub openbrick: String,
    /// Optional; `part` when present (docs/plan/08 section 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub id: String,
    pub version: String,
    pub name: String,
    pub license: String,
    #[serde(default)]
    pub credit: Credit,
    #[serde(default)]
    pub provenance: Provenance,
    /// Parameters in document order, which is the order of the designer's
    /// panel; saving keeps it.
    #[serde(default)]
    pub params: IndexMap<String, Param>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<Preset>,
    pub features: Vec<Feature>,
    #[serde(default = "derived")]
    pub snap: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub print: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub export: Value,
    /// Reproducibility record (docs/plan/06 section 10); kept as written.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub lock: Value,
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
    #[error("a preset needs a name; give each preset in `presets` a short name such as \"Two-colour inlay\"")]
    PresetName,
    #[error("two presets are named `{0}`; give each preset its own name")]
    DuplicatePreset(String),
    #[error("preset `{preset}` sets `{param}`, which is not a parameter of this part; remove it from the preset or add the parameter")]
    PresetParam { preset: String, param: String },
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
        let mut names: Vec<&str> = Vec::new();
        for p in &self.presets {
            let name = p.name.trim();
            if name.is_empty() {
                return Err(PartError::PresetName);
            }
            if names.contains(&name) {
                return Err(PartError::DuplicatePreset(name.to_string()));
            }
            names.push(name);
            if let Some(param) = p.params.keys().find(|k| !self.params.contains_key(*k)) {
                return Err(PartError::PresetParam {
                    preset: p.name.clone(),
                    param: param.clone(),
                });
            }
        }
        Ok(())
    }

    /// The preset named `name`, if the document has one.
    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
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

    /// A minimal plate with a `size` parameter and the given presets.
    fn with_presets(presets: &str) -> Result<PartDoc, PartError> {
        PartDoc::parse(&format!(
            r#"{{"openbrick":"1.0","id":"t","version":"0.1.0","name":"t","license":"CC0-1.0",
            "params":{{"size":{{"type":"grid2","default":[2,2]}}}},"presets":{presets},
            "features":[{{"id":"body","op":"body.plate","size":"$size"}}]}}"#
        ))
    }

    #[test]
    fn presets_are_read_and_checked() {
        let doc = with_presets(r#"[{"name":"Big","params":{"size":[4,4]}}]"#).expect("valid");
        assert_eq!(
            doc.preset("Big").unwrap().params["size"],
            serde_json::json!([4, 4])
        );
        assert!(doc.preset("big").is_none(), "names are exact");
        assert!(matches!(
            with_presets(r#"[{"name":" ","params":{}}]"#),
            Err(PartError::PresetName)
        ));
        assert!(matches!(
            with_presets(r#"[{"name":"A","params":{}},{"name":"A ","params":{}}]"#),
            Err(PartError::DuplicatePreset(n)) if n == "A"
        ));
        let e = with_presets(r#"[{"name":"A","params":{"colour":"red"}}]"#).unwrap_err();
        assert!(
            matches!(&e, PartError::PresetParam { preset, param } if preset == "A" && param == "colour")
        );
        assert!(
            e.to_string().contains("not a parameter of this part"),
            "{e}"
        );
    }

    #[test]
    fn the_brand_tile_presets_fit_its_parameters() {
        let doc = PartDoc::parse(include_str!("../../../parts/ndn.brand-tile.obpart.json"))
            .expect("parses");
        let process = openbrick_print::default_process_profiles().remove(0);
        let names: Vec<&str> = doc.presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["Raised mark", "Pressed-in mark", "Two-colour inlay"]
        );
        for p in &doc.presets {
            let values = doc.preset_values(&p.name, &process).expect("fits");
            assert_eq!(
                values.values["relief"],
                doc.preset(&p.name).unwrap().params["relief"]
            );
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
