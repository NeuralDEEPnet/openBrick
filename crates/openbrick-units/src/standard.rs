//! The OpenBrick3D Dimensional Standard v1, embedded from `standards/dimensions-v1.json`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Confidence of a dimension row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Verified,
    Likely,
    Inferred,
}

/// One feature row of the standard.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Feature {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub nominal_mm: Option<f64>,
    #[serde(default)]
    pub ldraw_ldu: Option<f64>,
    /// Print targets by process id (for example "fdm-0.4"), as free text ranges.
    #[serde(default)]
    pub print_target: BTreeMap<String, String>,
    pub confidence: Confidence,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub source: String,
}

/// The whole standard document.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Standard {
    pub openbrick: String,
    pub kind: String,
    pub version: String,
    pub license: String,
    /// Who made the standard; kept so a written copy keeps its attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<crate::Credit>,
    pub ldu_mm: f64,
    pub features: Vec<Feature>,
}

const SOURCE: &str = include_str!("../../../standards/dimensions-v1.json");

/// The embedded standard, parsed once.
pub static STANDARD: Lazy = Lazy(OnceLock::new());

pub struct Lazy(OnceLock<Standard>);

impl Lazy {
    pub fn get(&self) -> &Standard {
        self.0.get_or_init(|| {
            serde_json::from_str(SOURCE).expect("standards/dimensions-v1.json is valid")
        })
    }
}

impl std::ops::Deref for Lazy {
    type Target = Standard;
    fn deref(&self) -> &Standard {
        self.get()
    }
}

impl Standard {
    /// Look up a feature by id.
    pub fn feature(&self, id: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    /// Nominal millimetres of a feature, panicking if the id is unknown or has no value.
    pub fn mm(&self, id: &str) -> f64 {
        self.feature(id)
            .and_then(|f| f.nominal_mm)
            .unwrap_or_else(|| panic!("standard feature {id} has no nominal_mm"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_relations_hold() {
        let s = STANDARD.get();
        assert_eq!(s.kind, "standard");
        let module = s.mm("module");
        let stud = s.mm("stud_diameter");
        let tube_od = s.mm("tube_outer_diameter");
        let pin = s.mm("pin_1wide_diameter");
        assert!((module - 8.0).abs() < 1e-9);
        assert!(
            (tube_od - (module * 2f64.sqrt() - stud)).abs() < 0.005,
            "tube OD = 8 sqrt2 - 4.8"
        );
        assert!(
            (pin - (module - stud)).abs() < 0.03,
            "pin is 8 - 4.8 (3.18 family)"
        );
        assert!(
            (s.mm("tube_inner_diameter") - stud).abs() < 1e-9,
            "bore equals stud"
        );
        assert!((s.mm("brick_height") - 3.0 * s.mm("plate_height")).abs() < 1e-9);
        assert!((5.0 * s.mm("plate_height") - 2.0 * module).abs() < 1e-9);
        assert!((s.ldu_mm - crate::LDU_MM).abs() < 1e-12);
    }

    #[test]
    fn every_feature_has_a_source() {
        for f in &STANDARD.get().features {
            assert!(!f.source.is_empty(), "feature {} lacks a source", f.id);
        }
    }
}
