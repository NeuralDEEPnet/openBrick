//! Print pipeline data model (docs/plan/05): process profiles, fit profiles,
//! validator report types, and the bundled default profiles; the validator
//! ([`validate()`]), the calibration coupon sweep ([`coupon_v1`]) and the coupon
//! record that turns chosen cells into a fit profile ([`record()`]).

use std::collections::BTreeMap;

pub use openbrick_units::Credit;

mod coupon;
mod record;
mod validate;

pub use coupon::{
    coupon_v1, CouponCell, CouponLabel, CouponSpec, DEFAULT_POCKET_SET, GRID_TOLERANCE_MM,
    POCKET_CELLS, POCKET_SETS,
};
pub use record::{
    is_profile_id, record, record_with, slug_id, RecordError, RecordInput, ANONYMOUS_AUTHOR,
    FIT_INTENTS, RECORD_DECIMALS,
};
pub use validate::{
    validate, validate_with, FeatureNames, IMPLEMENTED_RULES, LENGTH_TOLERANCE_MM,
    MESH_VOLUME_TOLERANCE_PCT, PLANNED_RULES,
};

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
    /// Tube walls from `min_tube_wall_mm` up to this print but get a
    /// `TUBE.WALL` warning ("minTubeWallMm 0.7 (warning below 0.8)",
    /// docs/plan/05 section 5); absent means no warning band.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_tube_wall_mm: Option<f64>,
    pub min_cavity_depth_mm: f64,
    /// Share of artwork area, in percent, the morphological opening may remove:
    /// rule `FEATURE.MIN` reports more than this (a blocker on fit-critical
    /// artwork, a warning otherwise; docs/plan/05 section 5 asks the rule to
    /// report the removed area), and exactly this passes. Absent means the
    /// removed area is never a trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_removed_area_pct: Option<f64>,
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
    /// Mesh resolution defaults; absent means the kernel defaults
    /// ([`openbrick_geom::Tessellation::default`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tessellation: Option<ProcessTessellation>,
}

/// Mesh resolution defaults of a process (`tessellation` in a process profile).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessTessellation {
    /// Segments per full circle of studs, tubes and pins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stud_segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_edge_mm: Option<f64>,
}

impl ProcessProfile {
    /// Segments per full circle for studs, tubes and pins: the profile's
    /// `tessellation.studSegments`, else the kernel default.
    pub fn stud_segments(&self) -> u32 {
        self.tessellation
            .and_then(|t| t.stud_segments)
            .unwrap_or_else(|| openbrick_geom::Tessellation::default().stud_segments)
            .max(3)
    }

    /// A wall of `nominal_mm` rounded to the nearest whole number of
    /// `wallQuantiseMm` lines, and never below `minWallMm` (docs/plan/05 section 5,
    /// rule `WALL.QUANTISED`; docs/plan/04 section 6: walls are multiples of a
    /// line, because 1.1 mm prints hollow on a 0.4 mm nozzle).
    pub fn quantise_wall(&self, nominal_mm: f64) -> f64 {
        let q = self.wall_quantise_mm;
        if q <= 0.0 {
            return nominal_mm.max(self.min_wall_mm);
        }
        // round() keeps 1.2 / 0.4 = 2.9999999999999996 at three lines; the slack
        // keeps a minimum that is itself a whole number of lines from rounding up.
        let lines = (nominal_mm / q).round().max(1.0);
        let min_lines = (self.min_wall_mm / q - 1e-9).ceil();
        let wall = lines.max(min_lines) * q;
        // A wall that already is whole lines keeps its exact value (3 x 0.4 is
        // 1.2000000000000002 in binary floating point).
        if (wall - nominal_mm).abs() < 1e-9 {
            nominal_mm
        } else {
            wall
        }
    }

    /// The wall-to-roof chamfer for a body: `wallRoofFilletMm.brick` on
    /// brick-height bodies, `wallRoofFilletMm.plate` otherwise (docs/plan/05
    /// section 6). A missing key means no chamfer.
    pub fn wall_roof_fillet(&self, brick_height: bool) -> f64 {
        let key = if brick_height { "brick" } else { "plate" };
        self.wall_roof_fillet_mm
            .get(key)
            .copied()
            .unwrap_or(0.0)
            .max(0.0)
    }
}

/// Offsets applied to nominal geometry at export, in millimetres on diameter (or depth for ribs).
///
/// `Default` is the uncompensated set: every offset zero and `xy_scale` 1.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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

impl Default for Offsets {
    fn default() -> Self {
        Offsets {
            stud_diameter: 0.0,
            tube_inner_diameter: 0.0,
            tube_outer_diameter: 0.0,
            rib_depth: 0.0,
            pin_diameter: 0.0,
            horizontal_hole_diameter: 0.0,
            axle_hole: 0.0,
            bar_diameter: 0.0,
            clip_opening: 0.0,
            first_layer_chamfer: 0.0,
            xy_scale: 1.0,
        }
    }
}

/// Underside tube diameters after fit offsets (see [`Offsets::tube`]).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TubeSize {
    pub outer_mm: f64,
    pub inner_mm: f64,
    /// How much the outer diameter grew beyond nominal plus offset to keep the
    /// wall at the process minimum; zero when it did not need to.
    pub grown_mm: f64,
}

impl TubeSize {
    /// Wall thickness, (outer minus inner) / 2.
    pub fn wall_mm(&self) -> f64 {
        (self.outer_mm - self.inner_mm) / 2.0
    }
}

impl Offsets {
    /// The `offsetsMm` keys of the diameter and depth offsets, as written in a
    /// fit profile (the cells of the coupon calibrate these).
    pub const KEYS: [&'static str; 9] = [
        "studDiameter",
        "tubeInnerDiameter",
        "tubeOuterDiameter",
        "ribDepth",
        "pinDiameter",
        "horizontalHoleDiameter",
        "axleHole",
        "barDiameter",
        "clipOpening",
    ];

    /// The offset with this `offsetsMm` key (camelCase, as in the JSON): any of
    /// [`Offsets::KEYS`], `firstLayerChamfer` or `xyScale`; `None` for an
    /// unknown key.
    pub fn get(&self, key: &str) -> Option<f64> {
        Some(match key {
            "studDiameter" => self.stud_diameter,
            "tubeInnerDiameter" => self.tube_inner_diameter,
            "tubeOuterDiameter" => self.tube_outer_diameter,
            "ribDepth" => self.rib_depth,
            "pinDiameter" => self.pin_diameter,
            "horizontalHoleDiameter" => self.horizontal_hole_diameter,
            "axleHole" => self.axle_hole,
            "barDiameter" => self.bar_diameter,
            "clipOpening" => self.clip_opening,
            "firstLayerChamfer" => self.first_layer_chamfer,
            "xyScale" => self.xy_scale,
            _ => return None,
        })
    }

    /// Mutable access to the offset with this `offsetsMm` key (see
    /// [`Offsets::get`]).
    pub fn get_mut(&mut self, key: &str) -> Option<&mut f64> {
        Some(match key {
            "studDiameter" => &mut self.stud_diameter,
            "tubeInnerDiameter" => &mut self.tube_inner_diameter,
            "tubeOuterDiameter" => &mut self.tube_outer_diameter,
            "ribDepth" => &mut self.rib_depth,
            "pinDiameter" => &mut self.pin_diameter,
            "horizontalHoleDiameter" => &mut self.horizontal_hole_diameter,
            "axleHole" => &mut self.axle_hole,
            "barDiameter" => &mut self.bar_diameter,
            "clipOpening" => &mut self.clip_opening,
            "firstLayerChamfer" => &mut self.first_layer_chamfer,
            "xyScale" => &mut self.xy_scale,
            _ => return None,
        })
    }

    /// The offsets of an optional fit profile; no profile means nominal geometry.
    pub fn of(fit: Option<&FitProfile>) -> Offsets {
        fit.map(|f| f.offsets_mm.clone()).unwrap_or_default()
    }

    /// Underside tube diameters: nominal plus the `tubeOuterDiameter` and
    /// `tubeInnerDiameter` offsets, with the outer diameter grown with the bore so
    /// the wall never drops below `min_wall_mm` (docs/plan/05 section 3).
    pub fn tube(&self, nominal_outer_mm: f64, nominal_inner_mm: f64, min_wall_mm: f64) -> TubeSize {
        let inner = nominal_inner_mm + self.tube_inner_diameter;
        let outer = nominal_outer_mm + self.tube_outer_diameter;
        let grown = (inner + 2.0 * min_wall_mm - outer).max(0.0);
        TubeSize {
            outer_mm: outer + grown,
            inner_mm: inner,
            grown_mm: grown,
        }
    }
}

/// What the part evaluator measured on a built part, for the validator
/// (docs/plan/05 section 5). Lengths are millimetres on the compensated mesh
/// (after fit offsets and `xyScale`); the validator divides lengths in the bed
/// plane by `xyScale` before comparing them (see [`validate()`]). `None` means
/// the part has no such feature.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurements {
    /// Process profile id the part was built for.
    pub process: String,
    /// Fit profile id, if one was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_profile: Option<String>,
    /// The offsets of that fit profile, as the part was built with them. A
    /// profile recorded again keeps its id ([`crate::record`] derives it from
    /// printer, material and process), so rule `FIT.PROCESS` compares these
    /// with the given profile's offsets too. `None` without a fit profile, and
    /// in measurements written before the field existed (then only the ids
    /// are compared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_offsets: Option<Offsets>,
    /// Process ids the part document allows (`print.profiles`); empty means any
    /// (rule `FIT.PROCESS`).
    #[serde(default)]
    pub allowed_processes: Vec<String>,
    pub triangle_count: usize,
    pub volume_mm3: f64,
    /// Sum of the volumes of the feature solids before the union, less the
    /// designed overlap of tubes, pins and ribs that reach into a deboss or
    /// inlay's thickened roof. Solids that only touch add up to the union volume
    /// exactly; the few that overlap (wall ribs inside the wall-to-roof chamfer)
    /// count twice, so the union is slightly smaller. Rule `MESH.VOLUME` compares
    /// the two.
    pub feature_volume_sum_mm3: f64,
    pub bounds_min_mm: [f64; 3],
    pub bounds_max_mm: [f64; 3],
    pub is_manifold: bool,
    pub genus: i32,
    pub components: usize,
    /// Body height without studs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_height_mm: Option<f64>,
    /// Thinnest body side wall, wall ribs and tubes excluded (rule `WALL.MIN`):
    /// the full wall above the first-layer chamfer. The chamfer replaces slicer
    /// elephant-foot compensation (docs/plan/05 section 6), so the squashed first
    /// layer spreads back towards this width; the modelled width in the bed plane
    /// is [`Measurements::min_body_wall_at_bed_mm`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_body_wall_mm: Option<f64>,
    /// Thinnest body side wall at z = 0, the bottom edges (the bed plane, except
    /// for an inlay printed face down, whose face on the bed
    /// `Built.artwork.faceChamferMm` describes), after the 45 degree
    /// first-layer chamfers on its outer face and on the cavity rim:
    /// `min_body_wall_mm` minus twice the chamfer. Equal to `min_body_wall_mm`
    /// when the process has no first-layer chamfer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_body_wall_at_bed_mm: Option<f64>,
    /// Thinnest tube wall after offsets, (OD - ID) / 2 as rule `TUBE.WALL`
    /// defines it (docs/plan/05 section 5): the full wall above the first-layer
    /// chamfer; see [`Measurements::min_tube_wall_at_bed_mm`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_tube_wall_mm: Option<f64>,
    /// Thinnest tube wall at z = 0 (see `min_body_wall_at_bed_mm`), after the
    /// first-layer chamfers on
    /// the outside and in the bore: `min_tube_wall_mm` minus twice the tube's
    /// chamfer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_tube_wall_at_bed_mm: Option<f64>,
    /// Height of the 45 degree first-layer chamfer on the body's bottom edges
    /// (the larger of the process and fit profile values; `xyScale` stretches
    /// its width, not its height).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_layer_chamfer_mm: Option<f64>,
    /// Roof (top) thickness over the cavity (rule `ROOF.MIN`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roof_mm: Option<f64>,
    /// Cavity depth under the flat part of the roof (rule `CAVITY.DEPTH`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cavity_depth_mm: Option<f64>,
    /// Stud diameter after the `studDiameter` offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stud_diameter_mm: Option<f64>,
    /// Tube outer diameter after offsets and growth (rule `CLUTCH.RISK`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tube_outer_diameter_mm: Option<f64>,
    /// Tube bore after the `tubeInnerDiameter` offset (rule `CLUTCH.RISK`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tube_inner_diameter_mm: Option<f64>,
    /// Solid pin diameter under 1-wide parts after the `pinDiameter` offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_diameter_mm: Option<f64>,
    /// Wall rib depth as built, after the `ribDepth` offset: measured from the
    /// wall the rib stands on, so on a process that quantises the side wall
    /// thinner than the standard `side_wall` the rib is built deeper by the
    /// difference (docs/plan/04 section 6; fdm-0.2: 0.1 mm deeper). The coupon's
    /// `pocketRib` values are measured against the standard side wall instead,
    /// so the comparable value is `min_body_wall_mm + rib_depth_mm - side_wall`
    /// (lengths divided by `xyScale`). No rule reads it yet: `CLUTCH.RISK` covers
    /// the stud, the tube bore and the tube outer diameter (the rule table of
    /// docs/plan/05 section 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rib_depth_mm: Option<f64>,
    /// Connector kinds of fit-critical features (for example `tubeF`, `studM`),
    /// sorted and without repeats (rule `FIT.MISSING`).
    #[serde(default)]
    pub fit_critical_connectors: Vec<String>,
    /// Reserved for artwork (filled by `relief.*`): thinnest stroke after the
    /// morphological opening (rule `FEATURE.MIN`). `None` on a part with
    /// artwork (`removed_area_pct` or `relief_height_mm` set) means the stroke
    /// was not measured, because the estimate ran out of its work budget; rule
    /// `FEATURE.MIN` then reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_stroke_mm: Option<f64>,
    /// Reserved for artwork: the area that does not print as drawn, in percent
    /// of the artwork area. This is what the opening removed, plus, for deboss
    /// and inlay, face material strictly thinner than minFeature measured at the
    /// bottom of the cut (still built).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_area_pct: Option<f64>,
    /// Reserved for artwork: relief height (rule `RELIEF.HEIGHT`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief_height_mm: Option<f64>,
    /// The part's artwork is fit-critical: a `relief.*` feature the part lists
    /// as fit-critical (`fitCritical`), such as a label someone reads to
    /// calibrate. The evaluator sets it from the part document, so it does not
    /// depend on how the artwork op measures strokes. Rule `FEATURE.MIN` then
    /// blocks where it would otherwise warn.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fit_critical_artwork: bool,
}

/// The calibration record written by the coupon wizard.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calibration {
    #[serde(deserialize_with = "whole_u32")]
    pub coupon_version: u32,
    /// Chosen coupon cells by connector type: stud, tube, pocketTubeOD, pocketRib, technicH, pin, axle, bar, clip.
    pub chosen_cells: BTreeMap<String, f64>,
    /// The pocket set of the coupon the pocket was chosen from (one of
    /// [`POCKET_SETS`]; since schema 1.1). `pocketTubeOD` and `pocketRib` are
    /// one pocket of that set, not the best of each line. Absent in profiles
    /// without pocket cells and in 1.0 profiles, which do not say (the default
    /// coupon prints [`DEFAULT_POCKET_SET`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pocket_set: Option<String>,
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
pub(crate) fn whole_u32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
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

/// How a finding affects export: blockers disable it, warnings never do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Blocker,
    Warning,
}

/// One validator message; every message carries rule, value, threshold and a fix hint.
/// The JSON form is `$defs/finding` in `schemas/report.schema.json`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    /// Rule id from docs/plan/05 section 5 (`WALL.MIN`, ...).
    pub rule: String,
    pub severity: Severity,
    /// What was measured, with its value and unit, and the threshold.
    pub message: String,
    /// The measured value (millimetres unless the rule says otherwise, see
    /// [`validate()`]); `None` when the rule measures no number.
    pub value: Option<f64>,
    /// The threshold the value broke, in the unit of `value`; `None` when the
    /// rule has no numeric threshold. A rule with a threshold but no measured
    /// value (`FEATURE.MIN` on an artwork stroke that was not measured) keeps
    /// the threshold.
    pub threshold: Option<f64>,
    /// What to change, kindly worded.
    pub fix: String,
    /// The part feature concerned, when known.
    #[serde(default)]
    pub feature: Option<String>,
}

/// What [`validate()`] found; the JSON form is `schemas/report.schema.json`,
/// the contract of `openbrick validate --json`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
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
                    message: match fit {
                        Some(f) => format!(
                            "fit-critical connector {ct} needs the calibration cell `{cell}`, which fit profile `{}` does not record",
                            f.id
                        ),
                        None => format!(
                            "fit-critical connector {ct} needs the calibration cell `{cell}`, and no fit profile was given"
                        ),
                    },
                    value: None,
                    threshold: None,
                    fix: format!(
                        "print the calibration coupon (openbrick calibrate coupon) and record the best-fitting `{cell}` cell (openbrick calibrate record)"
                    ),
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
            if let Some(pct) = p.max_removed_area_pct {
                assert!((0.0..=100.0).contains(&pct), "{}: a percentage", p.id);
            }
            if let Some(r) = p.recommended_tube_wall_mm {
                assert!(
                    r >= p.min_tube_wall_mm,
                    "{}: warning band above the blocker",
                    p.id
                );
            }
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
        let rib = c.chosen_cells["pocketRib"] - s.mm("rib_depth");
        assert!((f.offsets_mm.rib_depth - rib).abs() < 1e-9);
        // Its pocket is one the coupon of its pocket set printed.
        let set = c.pocket_set.as_deref().unwrap_or(DEFAULT_POCKET_SET);
        let printed = coupon_v1().printed_pockets(set).expect("a pocket set");
        let pocket = (c.chosen_cells["pocketTubeOD"], c.chosen_cells["pocketRib"]);
        assert!(
            printed
                .iter()
                .any(|&(o, r)| (o - pocket.0).abs() < 1e-9 && (r - pocket.1).abs() < 1e-9),
            "{pocket:?} is not a pocket of `{set}`"
        );
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

    fn process(id: &str) -> ProcessProfile {
        default_process_profiles()
            .into_iter()
            .find(|p| p.id == id)
            .expect("bundled profile")
    }

    #[test]
    fn walls_are_quantised_to_whole_lines() {
        let wall = openbrick_units::STANDARD.get().mm("side_wall");
        // fdm-0.4: the standard wall is exactly three 0.4 mm lines.
        let p = process("fdm-0.4");
        assert!((p.quantise_wall(wall) - wall).abs() < 1e-12);
        // fdm-0.2: wall / 0.22 = 5.45 lines, which rounds to five lines.
        let p = process("fdm-0.2");
        let lines = (wall / p.wall_quantise_mm).round();
        assert_eq!(lines, 5.0);
        assert!((p.quantise_wall(wall) - lines * p.wall_quantise_mm).abs() < 1e-12);
        // Never below the process minimum wall, even for a tiny nominal.
        assert!(p.quantise_wall(p.wall_quantise_mm / 10.0) >= p.min_wall_mm - 1e-12);
    }

    #[test]
    fn tube_outer_diameter_grows_with_the_bore() {
        let s = openbrick_units::STANDARD.get();
        let (od, id) = (s.mm("tube_outer_diameter"), s.mm("tube_inner_diameter"));
        let p = process("fdm-0.4");
        // Nominal: the wall is the standard tube wall and nothing grows.
        let nominal = Offsets::default().tube(od, id, p.min_tube_wall_mm);
        assert_eq!(nominal.grown_mm, 0.0);
        assert!((nominal.wall_mm() - (od - id) / 2.0).abs() < 1e-12);
        // A bore offset larger than the wall can spare: the outer diameter grows so
        // the wall stays at the process minimum.
        let wide_bore = Offsets {
            tube_inner_diameter: 0.4,
            ..Offsets::default()
        };
        let t = wide_bore.tube(od, id, p.min_tube_wall_mm);
        assert!(t.grown_mm > 0.0);
        assert!((t.wall_mm() - p.min_tube_wall_mm).abs() < 1e-12);
        assert!((t.inner_mm - (id + 0.4)).abs() < 1e-12);
    }

    #[test]
    fn default_offsets_are_nominal_and_tessellation_is_read() {
        assert_eq!(Offsets::default().xy_scale, 1.0);
        assert_eq!(Offsets::of(None), Offsets::default());
        assert_eq!(process("fdm-0.4").stud_segments(), 64);
        assert_eq!(process("fdm-0.2").stud_segments(), 96);
        let written = serde_json::to_value(process("fdm-0.4")).expect("serialises");
        assert_eq!(written["tessellation"]["studSegments"], 64);
    }

    #[test]
    fn measurements_serialise_in_camel_case_without_absent_fields() {
        let m = Measurements {
            process: "fdm-0.4".into(),
            min_tube_wall_mm: Some(0.85),
            fit_critical_connectors: vec!["tubeF".into()],
            ..Measurements::default()
        };
        let v = serde_json::to_value(&m).expect("serialises");
        assert_eq!(v["minTubeWallMm"], 0.85);
        assert!(v.get("minTubeWallAtBedMm").is_none());
        assert_eq!(v["fitCriticalConnectors"][0], "tubeF");
        assert!(
            v.get("minStrokeMm").is_none(),
            "reserved artwork fields are omitted"
        );
        assert!(
            v.get("fitCriticalArtwork").is_none(),
            "false is the default and is omitted"
        );
        let back: Measurements = serde_json::from_value(v).expect("reads back");
        assert_eq!(back, m);
    }
}
