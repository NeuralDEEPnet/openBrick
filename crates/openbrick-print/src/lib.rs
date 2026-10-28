//! Print pipeline data model (docs/plan/05): process profiles, fit profiles,
//! validator report types, and the bundled default profiles.

use std::collections::BTreeMap;

pub use openbrick_units::Credit;

/// What a manufacturing process can resolve.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessProfile {
    pub openbrick: String,
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<Credit>,
    pub line_width_mm: f64,
    pub layer_height_mm: Vec<f64>,
    pub min_wall_mm: f64,
    pub wall_quantise_mm: f64,
    pub min_feature_mm: f64,
    pub min_tube_wall_mm: f64,
    pub min_cavity_depth_mm: f64,
    pub min_relief_height_mm: f64,
    pub max_relief_height_mm: f64,
    pub min_roof_mm: f64,
    pub max_unsupported_bridge_mm: f64,
    pub max_overhang_deg: f64,
    pub first_layer_chamfer_mm: f64,
    pub stud_top_chamfer_mm: f64,
    pub wall_roof_fillet_mm: BTreeMap<String, f64>,
    pub supports_allowed: bool,
    pub horizontal_hole_strategy: String,
    #[serde(default)]
    pub experimental: bool,
}

/// Offsets applied to nominal geometry at export, in millimetres on diameter (or depth for ribs).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offsets {
    pub stud_diameter: f64,
    pub tube_inner_diameter: f64,
    pub tube_outer_diameter: f64,
    pub rib_depth: f64,
    pub pin_diameter: f64,
    pub horizontal_hole_diameter: f64,
    pub axle_hole: f64,
    pub bar_diameter: f64,
    pub clip_opening: f64,
    pub first_layer_chamfer: f64,
    #[serde(default = "one")]
    pub xy_scale: f64,
}

fn one() -> f64 {
    1.0
}

/// The calibration record written by the coupon wizard.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calibration {
    #[serde(deserialize_with = "whole_u32")]
    pub coupon_version: u32,
    /// Chosen coupon cells by connector type: stud, tube, pocketTubeOD, pocketRib, technicH, pin, axle, bar, clip.
    pub chosen_cells: BTreeMap<String, f64>,
    /// Calendar date, YYYY-MM-DD; omitted when written if unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    #[serde(default)]
    pub pull_test_n: Option<f64>,
    #[serde(default)]
    pub control_pull_test_n: Option<f64>,
}

/// A whole number in `u32` range, written either as `1` or as `1.0`. JSON does not
/// tell the two apart and neither does JSON Schema (draft 2020-12 counts `1.0` as an
/// integer), so a document that passes `schemas/fit-profile.schema.json` must load.
fn whole_u32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    use serde::de::{Deserialize, Error};
    let v = f64::deserialize(d)?;
    if v.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&v) {
        Ok(v as u32)
    } else {
        Err(D::Error::custom(format!(
            "expected a whole number from 0 to {}, found {v}",
            u32::MAX
        )))
    }
}

/// One printer, nozzle and material combination after calibration.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FitProfile {
    pub openbrick: String,
    pub kind: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<Credit>,
    pub process: String,
    pub printer: String,
    pub nozzle_mm: f64,
    pub material: serde_json::Value,
    #[serde(default)]
    pub slicer: String,
    pub intent: String,
    pub offsets_mm: Offsets,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<Calibration>,
    #[serde(default)]
    pub notes: String,
}

/// Which coupon cells a connector type needs before export of a fit-critical part.
pub fn required_cells(connector: &str) -> &'static [&'static str] {
    match connector {
        "studM" => &["stud"],
        "tubeF" | "studF" => &["tube", "pocketTubeOD", "pocketRib"],
        "pinF" => &["technicH"],
        "pinM" => &["pin"],
        "axleM" | "axleF" => &["axle"],
        "barM" => &["bar"],
        "clipF" => &["clip"],
        _ => &[],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Blocker,
    Warning,
}

/// One validator message; every message carries rule, value, threshold and a fix hint.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub value: Option<f64>,
    pub threshold: Option<f64>,
    pub fix: String,
    #[serde(default)]
    pub feature: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn blockers(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Blocker)
    }
    pub fn export_allowed(&self) -> bool {
        self.blockers().next().is_none()
    }
}

/// Bundled default process profiles.
pub fn default_process_profiles() -> Vec<ProcessProfile> {
    [
        include_str!("../../../profiles/process/fdm-0.4.json"),
        include_str!("../../../profiles/process/fdm-0.2.json"),
        include_str!("../../../profiles/process/resin-msla.json"),
    ]
    .iter()
    .map(|s| serde_json::from_str(s).expect("bundled process profile is valid"))
    .collect()
}

/// The example fit profile shipped with the repository.
pub fn example_fit_profile() -> FitProfile {
    serde_json::from_str(include_str!(
        "../../../profiles/fit/example-mk4s-petg-0.4.json"
    ))
    .expect("example fit profile is valid")
}

/// Rule `FIT.MISSING`: every fit-critical connector type must have its coupon cells chosen.
pub fn check_fit_missing(connector_types: &[&str], fit: Option<&FitProfile>) -> Vec<Finding> {
    let mut out = Vec::new();
    for ct in connector_types {
        for cell in required_cells(ct) {
            let present = fit
                .and_then(|f| f.calibration.as_ref())
                .map(|c| c.chosen_cells.contains_key(*cell))
                .unwrap_or(false);
            if !present {
                out.push(Finding {
                    rule: "FIT.MISSING".into(),
                    severity: Severity::Blocker,
                    message: format!("fit-critical connector {ct} needs calibration cell `{cell}`"),
                    value: None,
                    threshold: None,
                    fix: "print the calibration coupon and record the best-fitting cells (openbrick calibrate)".into(),
                    feature: None,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_profiles_are_consistent() {
        for p in default_process_profiles() {
            assert_eq!(p.kind, "process");
            assert!(
                (p.min_feature_mm - 2.0 * p.line_width_mm).abs() < 1e-9 || p.experimental,
                "{}: minFeature = 2 x lineWidth",
                p.id
            );
            assert!(p.min_tube_wall_mm <= p.min_wall_mm);
            assert!(p.min_cavity_depth_mm >= 1.8 - 1e-9);
        }
    }

    #[test]
    fn example_fit_profile_offsets_equal_chosen_minus_nominal() {
        let f = example_fit_profile();
        let s = openbrick_units::STANDARD.get();
        let c = f.calibration.as_ref().expect("calibrated");
        let stud = c.chosen_cells["stud"] - s.mm("stud_diameter");
        let tube = c.chosen_cells["tube"] - s.mm("tube_inner_diameter");
        let od = c.chosen_cells["pocketTubeOD"] - s.mm("tube_outer_diameter");
        assert!((f.offsets_mm.stud_diameter - stud).abs() < 1e-9);
        assert!((f.offsets_mm.tube_inner_diameter - tube).abs() < 1e-9);
        assert!((f.offsets_mm.tube_outer_diameter - od).abs() < 0.01);
        let wall = (s.mm("tube_outer_diameter") + f.offsets_mm.tube_outer_diameter
            - (s.mm("tube_inner_diameter") + f.offsets_mm.tube_inner_diameter))
            / 2.0;
        assert!(wall >= 0.7, "tube wall stays printable: {wall}");
    }

    #[test]
    fn fit_missing_blocks_uncalibrated_tubes() {
        let f = example_fit_profile();
        assert!(check_fit_missing(&["tubeF", "studM"], Some(&f)).is_empty());
        assert_eq!(check_fit_missing(&["tubeF"], None).len(), 3);
    }
}
