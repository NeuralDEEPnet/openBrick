//! The validator (docs/plan/05 section 5): the [`Measurements`] of a built part
//! checked against a [`ProcessProfile`] and an optional [`FitProfile`].
//!
//! Every finding carries its rule id, severity, a message with the measured
//! value and unit, the value and the threshold as numbers, and a fix hint
//! (CLAUDE.md). Blockers disable export; warnings never do. Thresholds come from
//! the process profile, the coupon sweep from `standards/coupon-v1.json`
//! ([`crate::coupon_v1`]) and nominals from the Dimensional Standard; the only
//! threshold of the validator itself is [`MESH_VOLUME_TOLERANCE_PCT`], from the
//! rule table, and [`LENGTH_TOLERANCE_MM`] only absorbs floating-point residue,
//! so a wall of exactly the minimum passes.
//!
//! Units: `value` and `threshold` are millimetres, except `MESH.VOLUME` (percent
//! deviation and percent tolerance; on a volume that is not positive, the volume
//! in mm3 against 0, and on one that is not a finite number, both null),
//! `FEATURE.MIN` on the removed area (percent of the artwork area) and
//! `MESH.MANIFOLD` on an empty mesh (component count). A rule that measured
//! no number has a null value; its threshold is null too, except on an artwork
//! stroke that was not measured, which carries the `minFeatureMm` it could not
//! be compared with.
//!
//! **One frame for lengths in the bed plane.** The measurements are taken on the
//! compensated mesh, after fit offsets and the fit profile's `xyScale`. `xyScale`
//! is a uniform shrinkage compensation of a few percent (the fit-profile schema
//! allows 0.9 to 1.1) that the print undoes and that the slicer absorbs in its
//! line widths, so every rule that compares a length in the bed plane (`WALL.MIN`,
//! `TUBE.WALL`, `WALL.QUANTISED`, `FEATURE.MIN` strokes and `CLUTCH.RISK`) first
//! divides it by `xyScale`: it judges the part as drawn plus fit offsets, which
//! is also how the coupon cells are printed: at scale 1, because no fit profile
//! reaches the coupon. A fit profile whose `xyScale` is not 1 therefore also
//! scales the calibrated diameters away from the chosen cells (`record` writes 1;
//! see `openbrick_part::coupon`). A calibration therefore cannot turn a wall drawn at the minimum
//! into a blocker, nor a wall of whole lines into a warning. Messages show the
//! mesh length too when `xyScale` is not 1. Heights (`ROOF.MIN`,
//! `CAVITY.DEPTH`, `RELIEF.HEIGHT`) are not scaled.
//!
//! Implemented rules ([`IMPLEMENTED_RULES`]): `MESH.MANIFOLD`, `MESH.VOLUME`,
//! `WALL.MIN`, `TUBE.WALL`, `CAVITY.DEPTH`, `WALL.QUANTISED`, `ROOF.MIN`,
//! `FEATURE.MIN`, `RELIEF.HEIGHT`, `FIT.PROCESS`, `FIT.MISSING` and
//! `CLUTCH.RISK`. Decisions the table leaves open:
//!
//! - `MESH.MANIFOLD` blocks a mesh the kernel reports as not manifold, and an
//!   empty mesh (zero components). More than one component is **not** a
//!   finding: [`Measurements::components`] adds up the colour-role objects, and
//!   a logo in the `accent` role legitimately has one component per letter, so
//!   the count alone cannot tell a loose piece from a multi-colour part. A
//!   per-object count would let a later revision warn about loose pieces of the
//!   body.
//! - `MESH.VOLUME` warns when the volume is not positive or not a finite number,
//!   or deviates from [`Measurements::feature_volume_sum_mm3`] by more than 5
//!   percent; it skips the comparison when that sum is not positive (unknown).
//! - `TUBE.WALL` blocks below `minTubeWallMm` and warns below
//!   `recommendedTubeWallMm` (0.8 mm for `fdm-0.4`, from "minTubeWallMm 0.7
//!   (warning below 0.8)" in the rule table); a profile without that field has
//!   no warning band. Hollow-stud walls are not measured yet.
//! - `FEATURE.MIN` has three triggers: the thinnest artwork stroke
//!   ([`Measurements::min_stroke_mm`], after the morphological opening) below
//!   `minFeatureMm`; a stroke that was not measured (the part has artwork but
//!   no `min_stroke_mm`, because the estimate ran out of its work budget); and
//!   the share of the artwork area the opening removed
//!   ([`Measurements::removed_area_pct`]) more than the process's
//!   `maxRemovedAreaPct` (exactly that much passes). The evaluator's
//!   removed-area warning in openbrick-part is meant to use the same boundary,
//!   but until its comparison changes from `>=` to `>` (the part lane's change
//!   to `eval/relief.rs`) it also warns at exactly `maxRemovedAreaPct`, where
//!   this rule passes. All three are blockers when
//!   the part's artwork is fit-critical ([`Measurements::fit_critical_artwork`],
//!   a label someone reads to calibrate, which must print as drawn) and
//!   warnings otherwise. The opening also rounds convex corners (round joins of
//!   8 chords per turn, 2 per right-angled corner): on a 13.2 mm square at
//!   0.8 mm that is 4 r^2 (1 - sin 45 deg) = 0.19 mm2 with r = 0.4 mm, 0.11 %
//!   of the area, under the 1 % of the bundled profiles, but small text at the
//!   minimum stroke can lose more than 1 % to its corners alone, so
//!   fit-critical text needs bolder strokes or a larger size.
//! - `FIT.PROCESS` blocks when the part, its build and the profiles disagree: a
//!   fit profile calibrated for another process; a process the part does not
//!   allow (`print.profiles`); a part built for another process than the one it
//!   is validated against; and a part built with another fit profile than the
//!   one it is validated against, or with one when none is given, or without
//!   one when one is given ([`Measurements::fit_profile`]), or with the same
//!   id but other offsets ([`Measurements::fit_offsets`]: recording the coupon
//!   again keeps the id). The last ones keep a nominal preview or a stale build
//!   from passing as a calibrated export: the report must describe the mesh
//!   that is exported.
//! - `CLUTCH.RISK` warns when the stud, tube bore or tube outer diameter after
//!   offsets lies outside the coupon cells `stud`, `tube` or `pocketTubeOD`; and
//!   when the solid pin under a 1-wide part is not at its nominal. The fit
//!   offset that sizes that pin, `pinDiameter`, is calibrated on the Technic pin
//!   row of the coupon (cell `pin`), and no coupon cell tests the solid pin, so
//!   any change to it is untested (an open question for docs/plan/05: a
//!   separate offset or a 1-wide pin row). The rib depth is not checked: the
//!   rule table names the stud, the bore and the tube OD, and
//!   [`Measurements::rib_depth_mm`] is the built depth, which a check against
//!   the `pocketRib` sweep would first have to bring into the coupon's frame.
//!
//! Not implemented yet, because they need mesh analysis or data the evaluator
//! does not provide: `MESH.SELF_INTERSECT` (needs a self-intersection test of
//! the mesh), `ROOF.BRIDGE` and `OVERHANG.MAX` / `OVERHANG.BRIDGED` (need span
//! and face-angle analysis), `ORIENT.FACE` (needs the declared up face and
//! inlay mode), `TRADEMARK.LOGO` (needs the Q16 artwork matcher) and `SIZE.BED`
//! (process profiles carry no bed size yet).

use openbrick_units::STANDARD;

use crate::coupon::coupon_v1;
use crate::{
    check_fit_missing, Finding, FitProfile, Measurements, Offsets, ProcessProfile, Report, Severity,
};

/// `MESH.VOLUME` tolerance: "matches expected within 5 percent of the nominal
/// feature sum" (docs/plan/05 section 5).
pub const MESH_VOLUME_TOLERANCE_PCT: f64 = 5.0;

/// Measured lengths this close to a threshold count as meeting it: one
/// nanometre, far below what any process resolves and far above the
/// floating-point residue of the evaluator's arithmetic.
pub const LENGTH_TOLERANCE_MM: f64 = 1e-6;

/// The rules [`validate`] checks, in the order of the table in docs/plan/05
/// section 5.
pub const IMPLEMENTED_RULES: &[&str] = &[
    "MESH.MANIFOLD",
    "MESH.VOLUME",
    "WALL.MIN",
    "TUBE.WALL",
    "CAVITY.DEPTH",
    "WALL.QUANTISED",
    "ROOF.MIN",
    "FEATURE.MIN",
    "RELIEF.HEIGHT",
    "FIT.PROCESS",
    "FIT.MISSING",
    "CLUTCH.RISK",
];

/// Rules of the table in docs/plan/05 section 5 that [`validate`] does not
/// check yet (see the module documentation for why).
pub const PLANNED_RULES: &[&str] = &[
    "MESH.SELF_INTERSECT",
    "ROOF.BRIDGE",
    "OVERHANG.MAX",
    "OVERHANG.BRIDGED",
    "ORIENT.FACE",
    "TRADEMARK.LOGO",
    "SIZE.BED",
];

/// Ids of the part features the measurements come from, when the caller knows
/// them, so findings can name them (`Finding::feature`).
/// `openbrick_part::validate` fills it from the built part's connectors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureNames {
    /// The stud field (`studM` connectors): `CLUTCH.RISK` on the stud diameter.
    pub studs: Option<String>,
    /// The tube field (`tubeF` connectors): `TUBE.WALL`, and `CLUTCH.RISK` on
    /// the tube bore, the tube outer diameter and the solid 1-wide pin.
    pub tubes: Option<String>,
}

/// Check the measurements of a built part. Findings are sorted blockers first,
/// then by rule id, feature and message, so equal inputs give equal reports.
pub fn validate(m: &Measurements, process: &ProcessProfile, fit: Option<&FitProfile>) -> Report {
    validate_with(m, process, fit, &FeatureNames::default())
}

/// [`validate`], naming the features in `names` on the findings they concern.
pub fn validate_with(
    m: &Measurements,
    process: &ProcessProfile,
    fit: Option<&FitProfile>,
    names: &FeatureNames,
) -> Report {
    let mut v = Validator {
        m,
        p: process,
        fit,
        names,
        out: Vec::new(),
    };
    v.mesh_manifold();
    v.mesh_volume();
    v.wall_min();
    v.tube_wall();
    v.cavity_depth();
    v.wall_quantised();
    v.roof_min();
    v.feature_min();
    v.relief_height();
    v.fit_process();
    v.fit_missing();
    v.clutch_risk();
    let mut report = Report { findings: v.out };
    report.sort();
    report
}

impl Report {
    /// Sort the findings deterministically: blockers first, then by rule id,
    /// feature and message.
    pub fn sort(&mut self) {
        self.findings.sort_by(|a, b| {
            let rank = |f: &Finding| match f.severity {
                Severity::Blocker => 0,
                Severity::Warning => 1,
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.rule.cmp(&b.rule))
                .then_with(|| a.feature.cmp(&b.feature))
                .then_with(|| a.message.cmp(&b.message))
        });
    }
}

/// A length for messages: at most three decimals, trailing zeros dropped
/// (0.8, 0.857, 4.95).
fn mm(x: f64) -> String {
    decimals(x, 3)
}

/// `x` with at most `places` decimals, trailing zeros dropped.
fn decimals(x: f64, places: usize) -> String {
    let s = format!("{x:.places$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// Two different numbers written as [`mm`] writes them, or with as many more
/// decimals as it takes to tell them apart (xyScale 0.9998 against 1, not 1
/// against 1): a message never shows a difference as two equal numbers.
fn apart(a: f64, b: f64) -> (String, String) {
    for places in 3..=12 {
        let (x, y) = (decimals(a, places), decimals(b, places));
        if x != y {
            return (x, y);
        }
    }
    (format!("{a}"), format!("{b}"))
}

/// `1 component`, `2 components`.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// `a`, `a or b`, `a, b or c`.
fn either(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// `value` is below `threshold`, beyond the tolerance.
fn below(value: f64, threshold: f64) -> bool {
    value < threshold - LENGTH_TOLERANCE_MM
}

/// A percentage `pct` that is above `max`, with one decimal or as many more as
/// it takes to read as above `max` (1.03, not 1.0, against 1): a message never
/// seems to contradict its own threshold.
fn pct_above(pct: f64, max: f64) -> String {
    for places in 1..=12 {
        let s = format!("{pct:.places$}");
        if s.parse::<f64>().is_ok_and(|v| v > max) {
            return s;
        }
    }
    format!("{pct}")
}

struct Validator<'a> {
    m: &'a Measurements,
    p: &'a ProcessProfile,
    fit: Option<&'a FitProfile>,
    names: &'a FeatureNames,
    out: Vec<Finding>,
}

impl Validator<'_> {
    /// Add a finding; set `feature` on the returned finding when it is known.
    fn push(
        &mut self,
        rule: &str,
        severity: Severity,
        message: String,
        value: Option<f64>,
        threshold: Option<f64>,
        fix: String,
    ) -> &mut Finding {
        self.out.push(Finding {
            rule: rule.into(),
            severity,
            message,
            value,
            threshold,
            fix,
            feature: None,
        });
        self.out.last_mut().expect("just pushed")
    }

    /// `of process `fdm-0.4``, for messages that name the threshold's source.
    fn of_process(&self) -> String {
        format!("of process `{}`", self.p.id)
    }

    fn fit_name(&self) -> String {
        self.fit
            .map(|f| format!("fit profile `{}`", f.id))
            .unwrap_or_else(|| "the fit profile".into())
    }

    /// The fit profile's `xyScale`, 1 without a fit profile (or with a scale
    /// that is not a positive number, which the schema refuses).
    fn xy_scale(&self) -> f64 {
        self.fit
            .map(|f| f.offsets_mm.xy_scale)
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0)
    }

    /// A length in the bed plane as drawn plus fit offsets: the measured
    /// (compensated mesh) length divided by `xyScale` (see the module
    /// documentation).
    fn drawn(&self, mesh_mm: f64) -> f64 {
        mesh_mm / self.xy_scale()
    }

    /// ` (0.784 mm on the mesh, scaled by xyScale 0.98 of fit profile `x`)`
    /// when `xyScale` is not 1, else nothing.
    fn on_mesh(&self, mesh_mm: f64) -> String {
        let xy = self.xy_scale();
        if (xy - 1.0).abs() < 1e-12 {
            return String::new();
        }
        format!(
            " ({} mm on the mesh, scaled by xyScale {xy} of {})",
            mm(mesh_mm),
            self.fit_name()
        )
    }

    fn mesh_manifold(&mut self) {
        let m = self.m;
        if m.components == 0 {
            self.push(
                "MESH.MANIFOLD",
                Severity::Blocker,
                format!(
                    "the mesh is empty (0 components, {} triangles; a printable part has at least 1): there is nothing to print",
                    m.triangle_count
                ),
                Some(0.0),
                Some(1.0),
                "check that the part has a body feature and that its size is at least 1 x 1 stud"
                    .into(),
            );
        }
        if !m.is_manifold {
            self.push(
                "MESH.MANIFOLD",
                Severity::Blocker,
                format!(
                    "the mesh is not watertight and manifold ({}, {} triangles), so a slicer cannot tell inside from outside",
                    count(m.components, "component"),
                    m.triangle_count
                ),
                None,
                None,
                "this is a bug in the evaluator or the kernel, not in your part: please report it with the part file and its parameters; other parameter values may avoid it meanwhile"
                    .into(),
            );
        }
    }

    fn mesh_volume(&mut self) {
        let m = self.m;
        let tol = MESH_VOLUME_TOLERANCE_PCT;
        if !m.volume_mm3.is_finite() {
            // No number to report: the value and the threshold are both null.
            self.push(
                "MESH.VOLUME",
                Severity::Warning,
                format!(
                    "the mesh volume could not be measured (it came out as {}), so it cannot be compared with the sum of its feature volumes",
                    m.volume_mm3
                ),
                None,
                None,
                "this is a bug in the evaluator or the kernel, not in your part: please report it with the part file and its parameters"
                    .into(),
            );
            return;
        }
        if m.volume_mm3 <= 0.0 {
            // The volume itself, in mm3, against zero (not a percentage).
            self.push(
                "MESH.VOLUME",
                Severity::Warning,
                format!(
                    "the mesh volume is {} mm3; a printable part has a volume above 0 mm3",
                    mm(m.volume_mm3)
                ),
                Some(m.volume_mm3),
                Some(0.0),
                "check that the body has a positive size and that no feature cuts it away completely"
                    .into(),
            );
            return;
        }
        let expected = m.feature_volume_sum_mm3;
        if expected <= 0.0 || !expected.is_finite() {
            return;
        }
        let deviation = (m.volume_mm3 - expected).abs() / expected * 100.0;
        if deviation > tol + 1e-9 {
            self.push(
                "MESH.VOLUME",
                Severity::Warning,
                format!(
                    "the mesh volume {} mm3 differs from the sum of its feature volumes {} mm3 by {} %, more than {} %",
                    mm(m.volume_mm3),
                    mm(expected),
                    pct_above(deviation, tol),
                    mm(tol)
                ),
                Some(deviation),
                Some(tol),
                "look for features that overlap a lot or float free of the body; if the part looks right, please report it with the part file so the expected volume can be corrected"
                    .into(),
            );
        }
    }

    fn wall_min(&mut self) {
        let Some(mesh) = self.m.min_body_wall_mm else {
            return;
        };
        let wall = self.drawn(mesh);
        let min = self.p.min_wall_mm;
        if below(wall, min) {
            let msg = format!(
                "the thinnest body wall is {} mm{}, below the {} mm minimum wall {}",
                mm(wall),
                self.on_mesh(mesh),
                mm(min),
                self.of_process()
            );
            let fix = format!("make the side wall at least {} mm thick", mm(min));
            self.push(
                "WALL.MIN",
                Severity::Blocker,
                msg,
                Some(wall),
                Some(min),
                fix,
            );
        }
    }

    fn tube_wall(&mut self) {
        let Some(mesh) = self.m.min_tube_wall_mm else {
            return;
        };
        let wall = self.drawn(mesh);
        let feature = self.names.tubes.clone();
        let min = self.p.min_tube_wall_mm;
        if below(wall, min) {
            let msg = format!(
                "the thinnest tube wall is {} mm after fit offsets{}, below the {} mm minimum tube wall {}",
                mm(wall),
                self.on_mesh(mesh),
                mm(min),
                self.of_process()
            );
            let fix = format!(
                "raise the tubeOuterDiameter offset or lower tubeInnerDiameter in {} until (OD - ID) / 2 is at least {} mm",
                self.fit_name(),
                mm(min)
            );
            self.push(
                "TUBE.WALL",
                Severity::Blocker,
                msg,
                Some(wall),
                Some(min),
                fix,
            )
            .feature = feature;
            return;
        }
        let Some(recommended) = self.p.recommended_tube_wall_mm else {
            return;
        };
        if below(wall, recommended) {
            let msg = format!(
                "the thinnest tube wall is {} mm after fit offsets{}: printable, but below the recommended {} mm {}",
                mm(wall),
                self.on_mesh(mesh),
                mm(recommended),
                self.of_process()
            );
            // The wall is (OD - ID) / 2, and OD is nominal plus the offset plus
            // what the evaluator grew it by to keep minTubeWallMm. Raising the
            // offset first uses up that growth, so it must rise by the growth
            // plus twice the missing wall.
            let grown = self.tube_grown();
            let raise = 2.0 * (recommended - wall) + grown;
            let fix = if grown > LENGTH_TOLERANCE_MM {
                format!(
                    "raise the tubeOuterDiameter offset in {} by {} mm, or record a tube cell with a smaller bore, so the wall reaches {} mm (the tube was grown {} mm past nominal plus that offset to keep the {} mm minimum, and the offset has to make up that growth first)",
                    self.fit_name(),
                    mm(raise),
                    mm(recommended),
                    mm(grown),
                    mm(min)
                )
            } else {
                format!(
                    "raise the tubeOuterDiameter offset in {} by {} mm, or record a tube cell with a smaller bore, so the wall reaches {} mm",
                    self.fit_name(),
                    mm(raise),
                    mm(recommended)
                )
            };
            self.push(
                "TUBE.WALL",
                Severity::Warning,
                msg,
                Some(wall),
                Some(recommended),
                fix,
            )
            .feature = feature;
        }
    }

    /// How much the evaluator grew the tube outer diameter past nominal plus
    /// the fit profile's `tubeOuterDiameter` offset to keep `minTubeWallMm`
    /// ([`crate::Offsets::tube`]), as drawn; zero when the measurements carry
    /// no tube outer diameter.
    fn tube_grown(&self) -> f64 {
        let Some(mesh) = self.m.tube_outer_diameter_mm else {
            return 0.0;
        };
        let offset = self.fit.map_or(0.0, |f| f.offsets_mm.tube_outer_diameter);
        let planned = STANDARD.get().mm("tube_outer_diameter") + offset;
        (self.drawn(mesh) - planned).max(0.0)
    }

    fn cavity_depth(&mut self) {
        let Some(depth) = self.m.cavity_depth_mm else {
            return;
        };
        let min = self.p.min_cavity_depth_mm;
        if below(depth, min) {
            let msg = format!(
                "the cavity under the roof is {} mm deep, below the {} mm the studs of a part underneath need (minCavityDepthMm {})",
                mm(depth),
                mm(min),
                self.of_process()
            );
            let fix = format!(
                "make the deboss or underside serial {} mm shallower, or use a taller body, so {} mm of cavity remains",
                mm(min - depth),
                mm(min)
            );
            self.push(
                "CAVITY.DEPTH",
                Severity::Blocker,
                msg,
                Some(depth),
                Some(min),
                fix,
            );
        }
    }

    fn wall_quantised(&mut self) {
        let Some(mesh) = self.m.min_body_wall_mm else {
            return;
        };
        let q = self.p.wall_quantise_mm;
        if q <= 0.0 {
            return;
        }
        let wall = self.drawn(mesh);
        let lines = wall / q;
        if (lines - lines.round()).abs() * q <= LENGTH_TOLERANCE_MM {
            return;
        }
        let better = self.p.quantise_wall(wall);
        let msg = format!(
            "the side wall is {} mm{}, which is {:.2} lines of {} mm (wallQuantiseMm {}); walls print solid only as whole lines",
            mm(wall),
            self.on_mesh(mesh),
            lines,
            mm(q),
            self.of_process()
        );
        let fix = format!(
            "use a {} mm wall ({} lines of {} mm)",
            mm(better),
            (better / q).round(),
            mm(q)
        );
        self.push(
            "WALL.QUANTISED",
            Severity::Warning,
            msg,
            Some(wall),
            Some(q),
            fix,
        );
    }

    fn roof_min(&mut self) {
        let Some(roof) = self.m.roof_mm else {
            return;
        };
        let min = self.p.min_roof_mm;
        if below(roof, min) {
            let msg = format!(
                "the roof is {} mm thick, below the {} mm minimum roof {}",
                mm(roof),
                mm(min),
                self.of_process()
            );
            let fix = format!(
                "build the part again for process `{}`: the evaluator makes the roof the standard roof or minRoofMm, whichever is larger, and thickens it under a deboss or serial rather than thinning it, so the rebuilt roof is at least {} mm",
                self.p.id,
                mm(min)
            );
            self.push(
                "ROOF.MIN",
                Severity::Blocker,
                msg,
                Some(roof),
                Some(min),
                fix,
            );
        }
    }

    fn feature_min(&mut self) {
        let min = self.p.min_feature_mm;
        let critical = self.m.fit_critical_artwork;
        let (severity, artwork) = if critical {
            (Severity::Blocker, "fit-critical artwork")
        } else {
            (Severity::Warning, "artwork")
        };
        let has_artwork = self.m.removed_area_pct.is_some() || self.m.relief_height_mm.is_some();
        if let Some(mesh) = self.m.min_stroke_mm {
            let stroke = self.drawn(mesh);
            if below(stroke, min) {
                let msg = format!(
                    "the thinnest stroke of the {artwork} is {} mm{}, below the {} mm minimum feature {}",
                    mm(stroke),
                    self.on_mesh(mesh),
                    mm(min),
                    self.of_process()
                );
                let fix = if critical {
                    format!(
                        "fit-critical artwork, such as a label someone reads to calibrate, must print as drawn: make its strokes at least {} mm wide (a bolder weight or a larger size), and leave minFeature of the relief.svg feature out or at the process minimum",
                        mm(min)
                    )
                } else {
                    format!(
                        "make strokes at least {} mm wide (a bolder weight or a larger size), or leave minFeature of the relief.svg feature out (it defaults to the process minimum) so thinner lines are removed",
                        mm(min)
                    )
                };
                self.push("FEATURE.MIN", severity, msg, Some(stroke), Some(min), fix);
            }
        } else if has_artwork {
            // The stroke estimate ran out of its work budget: no number shows
            // that the strokes reach the minimum. The value is null, and the
            // threshold is the minimum it could not be compared with.
            let msg = format!(
                "the thinnest stroke of the {artwork} was not measured: the artwork is too detailed for the stroke estimate's work limit, so nothing shows that its strokes reach the {} mm minimum feature {}",
                mm(min),
                self.of_process()
            );
            let fix = if critical {
                "fit-critical artwork, such as a label someone reads to calibrate, must be shown to print as drawn: simplify it (fewer, smoother outlines; Inkscape: Path > Simplify, and no texture finer than the minimum feature) until its strokes are measured".to_string()
            } else {
                "simplify the artwork (fewer, smoother outlines; Inkscape: Path > Simplify) if you want its strokes checked; the opening still removed every stroke thinner than the relief.svg feature's minFeature (the process minimum unless the feature sets one), so you may export as it is".to_string()
            };
            self.push("FEATURE.MIN", severity, msg, None, Some(min), fix);
        }
        if let (Some(pct), Some(max)) = (self.m.removed_area_pct, self.p.max_removed_area_pct) {
            // More than maxRemovedAreaPct is reported; exactly that much passes.
            if pct.is_finite() && pct > max + 1e-9 {
                let msg = format!(
                    "the morphological opening removed {} % of the {artwork} area, more than the {} % allowed (maxRemovedAreaPct {}): strokes thinner than the {} mm minimum feature do not print, and the removed regions are shown in red in the preview",
                    pct_above(pct, max),
                    mm(max),
                    self.of_process(),
                    mm(min)
                );
                let fix = if critical {
                    format!(
                        "fit-critical artwork, such as a label someone reads to calibrate, must print as drawn: make its strokes wider than {} mm (a bolder weight or a larger size) until the opening removes at most {} % (the rule reports more than maxRemovedAreaPct)",
                        mm(min),
                        mm(max)
                    )
                } else {
                    format!(
                        "if the red regions matter, make those lines wider than {} mm or the artwork larger, until the opening removes at most {} % (the rule reports more than maxRemovedAreaPct); otherwise you may export as it is",
                        mm(min),
                        mm(max)
                    )
                };
                self.push("FEATURE.MIN", severity, msg, Some(pct), Some(max), fix);
            }
        }
    }

    fn relief_height(&mut self) {
        let Some(h) = self.m.relief_height_mm else {
            return;
        };
        let (lo, hi) = (self.p.min_relief_height_mm, self.p.max_relief_height_mm);
        if below(h, lo) {
            let msg = format!(
                "the relief is {} mm high, below the {} mm minimum relief height {}",
                mm(h),
                mm(lo),
                self.of_process()
            );
            let fix = format!("raise reliefHeight to {} mm", mm(lo));
            self.push(
                "RELIEF.HEIGHT",
                Severity::Warning,
                msg,
                Some(h),
                Some(lo),
                fix,
            );
        } else if h > hi + LENGTH_TOLERANCE_MM {
            let msg = format!(
                "the relief is {} mm high, above the {} mm maximum relief height {}",
                mm(h),
                mm(hi),
                self.of_process()
            );
            let fix = format!("lower reliefHeight to {} mm", mm(hi));
            self.push(
                "RELIEF.HEIGHT",
                Severity::Warning,
                msg,
                Some(h),
                Some(hi),
                fix,
            );
        }
    }

    fn fit_process(&mut self) {
        let m = self.m;
        let process = &self.p.id;
        if !m.process.is_empty() && &m.process != process {
            self.push(
                "FIT.PROCESS",
                Severity::Blocker,
                format!(
                    "the part was built for process `{}` but is validated against `{process}`",
                    m.process
                ),
                None,
                None,
                format!("build the part again for `{process}` before validating and exporting it"),
            );
        }
        // The report must describe the mesh that is exported: the fit profile
        // the part was built with is the one it is validated against.
        let built = m.fit_profile.as_deref();
        let given = self.fit.map(|f| f.id.as_str());
        if built != given {
            let (message, fix) = match (built, given) {
                (None, Some(g)) => (
                    format!(
                        "the part was built without a fit profile but is validated against fit profile `{g}`, so its mesh has none of that profile's offsets"
                    ),
                    format!("build the part again with fit profile `{g}` before exporting it"),
                ),
                (Some(b), None) => (
                    format!(
                        "the part was built with fit profile `{b}` but is validated without a fit profile"
                    ),
                    format!(
                        "validate it with fit profile `{b}`, the one it was built with, or build it again without a fit profile"
                    ),
                ),
                (b, g) => (
                    format!(
                        "the part was built with fit profile `{}` but is validated against fit profile `{}`",
                        b.unwrap_or_default(),
                        g.unwrap_or_default()
                    ),
                    format!(
                        "build the part again with fit profile `{}` before exporting it",
                        g.unwrap_or_default()
                    ),
                ),
            };
            self.push("FIT.PROCESS", Severity::Blocker, message, None, None, fix);
        } else if let (Some(built), Some(fit)) = (&m.fit_offsets, self.fit) {
            // The same id: a profile recorded again keeps it, so compare what
            // the part was built with.
            let given = &fit.offsets_mm;
            let differs = Offsets::KEYS
                .iter()
                .copied()
                .chain(["firstLayerChamfer", "xyScale"])
                .find(|k| {
                    let (a, b) = (built.get(k), given.get(k));
                    !matches!((a, b), (Some(a), Some(b)) if (a - b).abs() <= LENGTH_TOLERANCE_MM)
                });
            if let Some(key) = differs {
                let unit = if key == "xyScale" { "" } else { " mm" };
                let (b, g) = apart(
                    built.get(key).unwrap_or(f64::NAN),
                    given.get(key).unwrap_or(f64::NAN),
                );
                self.push(
                    "FIT.PROCESS",
                    Severity::Blocker,
                    format!(
                        "the part was built with other offsets than fit profile `{}` holds now (for example {key} {b}{unit} built, {g}{unit} given): it was built with another recording of that profile",
                        fit.id
                    ),
                    None,
                    None,
                    format!(
                        "build the part again with the current fit profile `{}` before exporting it",
                        fit.id
                    ),
                );
            }
        }
        if !m.allowed_processes.is_empty() && !m.allowed_processes.contains(process) {
            let allowed: Vec<String> = m
                .allowed_processes
                .iter()
                .map(|p| format!("`{p}`"))
                .collect();
            self.push(
                "FIT.PROCESS",
                Severity::Blocker,
                format!(
                    "the part document allows only {} (print.profiles), not `{process}`",
                    either(&allowed)
                ),
                None,
                None,
                format!(
                    "build the part for {} with a fit profile calibrated for that process",
                    either(&allowed)
                ),
            );
        }
        if let Some(fit) = self.fit {
            if &fit.process != process {
                self.push(
                    "FIT.PROCESS",
                    Severity::Blocker,
                    format!(
                        "fit profile `{}` was calibrated for process `{}`, not `{process}`",
                        fit.id, fit.process
                    ),
                    None,
                    None,
                    format!(
                        "choose a fit profile calibrated for `{process}`, or print the coupon with `openbrick calibrate coupon --process {process}` and record one"
                    ),
                );
            }
        }
    }

    fn fit_missing(&mut self) {
        let kinds: Vec<&str> = self
            .m
            .fit_critical_connectors
            .iter()
            .map(String::as_str)
            .collect();
        self.out.extend(check_fit_missing(&kinds, self.fit));
    }

    fn clutch_risk(&mut self) {
        let spec = coupon_v1();
        let (m, names) = (self.m, self.names);
        let checks = [
            (m.stud_diameter_mm, "stud", "stud diameter", &names.studs),
            (m.tube_inner_diameter_mm, "tube", "tube bore", &names.tubes),
            (
                m.tube_outer_diameter_mm,
                "pocketTubeOD",
                "tube outer diameter",
                &names.tubes,
            ),
        ];
        for (value, cell_id, what, feature) in checks {
            let (Some(mesh), Some(cell)) = (value, spec.cell(cell_id)) else {
                continue;
            };
            let v = self.drawn(mesh);
            if cell.covers(v) {
                continue;
            }
            let bound = if v < cell.start_mm {
                cell.start_mm
            } else {
                cell.end_mm
            };
            let offset = cell.offset_field.as_deref().unwrap_or("matching");
            let msg = format!(
                "the {what} is {} mm after offsets{}, outside the {} to {} mm the coupon tests (cell `{cell_id}`), so no printed cell has shown how it grips",
                mm(v),
                self.on_mesh(mesh),
                mm(cell.start_mm),
                mm(cell.end_mm)
            );
            let fix = format!(
                "check the {offset} offset in {}, or print the coupon again and record the `{cell_id}` cell that fits best",
                self.fit_name()
            );
            let feature = feature.clone();
            self.push(
                "CLUTCH.RISK",
                Severity::Warning,
                msg,
                Some(v),
                Some(bound),
                fix,
            )
            .feature = feature;
        }
        self.solid_pin();
    }

    /// `CLUTCH.RISK` on the solid pin under 1-wide parts: no coupon cell tests
    /// it, so only its nominal is known to grip (see the module documentation).
    fn solid_pin(&mut self) {
        let Some(mesh) = self.m.pin_diameter_mm else {
            return;
        };
        let pin = self.drawn(mesh);
        let nominal = STANDARD.get().mm("pin_1wide_diameter");
        if (pin - nominal).abs() <= LENGTH_TOLERANCE_MM {
            return;
        }
        let spec = coupon_v1();
        let row = spec
            .cells
            .iter()
            .find(|c| c.offset_field.as_deref() == Some("pinDiameter"));
        let calibrated_on = row
            .map(|c| {
                format!(
                    "on coupon cell `{}` ({}, {} to {} mm)",
                    c.id,
                    c.label,
                    mm(c.start_mm),
                    mm(c.end_mm)
                )
            })
            .unwrap_or_else(|| "on another coupon row".into());
        let msg = format!(
            "the solid pin under the 1-wide part is {} mm after offsets{}, {} mm {} its {} mm nominal: the pinDiameter offset of {} is calibrated {calibrated_on}, and no coupon cell tests the solid pin, so no printed cell has shown how it grips",
            mm(pin),
            self.on_mesh(mesh),
            mm((pin - nominal).abs()),
            if pin < nominal { "under" } else { "over" },
            mm(nominal),
            self.fit_name()
        );
        let fix = format!(
            "print the part once and check how its pin holds between studs; if it is too loose or too tight, move pinDiameter in {} towards 0 (it also sizes Technic pins)",
            self.fit_name()
        );
        let feature = self.names.tubes.clone();
        self.push(
            "CLUTCH.RISK",
            Severity::Warning,
            msg,
            Some(pin),
            Some(nominal),
            fix,
        )
        .feature = feature;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_process_profiles, example_fit_profile};

    fn fdm04() -> ProcessProfile {
        default_process_profiles()
            .into_iter()
            .find(|p| p.id == "fdm-0.4")
            .expect("bundled")
    }

    #[test]
    fn a_volume_that_is_not_a_number_reports_no_numbers() {
        // JSON cannot hold NaN or infinity, so this case has no data fixture.
        let fit = example_fit_profile();
        for volume in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let m = Measurements {
                process: "fdm-0.4".into(),
                fit_profile: Some(fit.id.clone()),
                components: 1,
                is_manifold: true,
                volume_mm3: volume,
                feature_volume_sum_mm3: 100.0,
                ..Measurements::default()
            };
            let r = validate(&m, &fdm04(), Some(&fit));
            assert_eq!(r.findings.len(), 1, "{volume}: {:#?}", r.findings);
            let f = &r.findings[0];
            assert_eq!(
                (f.rule.as_str(), f.severity),
                ("MESH.VOLUME", Severity::Warning)
            );
            assert_eq!((f.value, f.threshold), (None, None), "{volume}");
            // The JSON form has null for both, as the report schema asks.
            let json = serde_json::to_value(f).expect("serialises");
            assert!(json["value"].is_null() && json["threshold"].is_null());
            assert!(r.export_allowed());
        }
    }

    #[test]
    fn percentages_above_a_threshold_read_as_above_it() {
        // The sample mark on fdm-0.4: 1.03 %, not 1.0 %, against 1 %.
        assert_eq!(pct_above(1.0324211814310704, 1.0), "1.03");
        assert_eq!(pct_above(3.1, 1.0), "3.1");
        assert_eq!(pct_above(40.0, 1.0), "40.0");
        assert_eq!(pct_above(1.0000004, 1.0), "1.0000004");
        // MESH.VOLUME: a 5.04 % deviation against 5 %, not "5.0 %, more than 5 %".
        assert_eq!(pct_above(5.04, 5.0), "5.04");
        for (pct, max) in [(1.0 + 2e-9, 1.0), (0.11, 0.1), (99.99, 99.9)] {
            let shown: f64 = pct_above(pct, max).parse().expect("a number");
            assert!(shown > max, "{pct} against {max}");
        }
    }

    #[test]
    fn different_numbers_read_as_different() {
        assert_eq!(apart(0.1, 0.0), ("0.1".into(), "0".into()));
        assert_eq!(apart(0.9998, 1.0), ("0.9998".into(), "1".into()));
        assert_eq!(apart(0.15, 0.150002), ("0.15".into(), "0.150002".into()));
        assert_eq!(apart(-0.0000004, 0.0), ("-0.0000004".into(), "0".into()));
    }

    #[test]
    fn same_profile_id_with_a_slightly_other_scale_shows_both_scales() {
        // A hand-edited xyScale 0.9998 against the profile's 1: mm() would
        // write both as 1.
        let fit = example_fit_profile();
        let mut built = fit.offsets_mm.clone();
        built.xy_scale = 0.9998;
        let m = Measurements {
            process: "fdm-0.4".into(),
            fit_profile: Some(fit.id.clone()),
            fit_offsets: Some(built),
            components: 1,
            is_manifold: true,
            volume_mm3: 100.0,
            feature_volume_sum_mm3: 100.0,
            ..Measurements::default()
        };
        let r = validate(&m, &fdm04(), Some(&fit));
        assert_eq!(r.findings.len(), 1, "{:#?}", r.findings);
        let f = &r.findings[0];
        assert_eq!(
            (f.rule.as_str(), f.severity),
            ("FIT.PROCESS", Severity::Blocker)
        );
        assert!(
            f.message.contains("xyScale 0.9998 built, 1 given"),
            "{}",
            f.message
        );
    }

    #[test]
    fn feature_names_reach_the_findings_they_concern() {
        let fit = example_fit_profile();
        let m = Measurements {
            process: "fdm-0.4".into(),
            fit_profile: Some(fit.id.clone()),
            components: 1,
            is_manifold: true,
            volume_mm3: 100.0,
            feature_volume_sum_mm3: 100.0,
            min_tube_wall_mm: Some(0.75),
            stud_diameter_mm: Some(5.0),
            ..Measurements::default()
        };
        let names = FeatureNames {
            studs: Some("top".into()),
            tubes: Some("bottom".into()),
        };
        let r = validate_with(&m, &fdm04(), Some(&fit), &names);
        let got: Vec<(&str, Option<&str>)> = r
            .findings
            .iter()
            .map(|f| (f.rule.as_str(), f.feature.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![("CLUTCH.RISK", Some("top")), ("TUBE.WALL", Some("bottom"))]
        );
        // Without names the same findings carry none.
        let r = validate(&m, &fdm04(), Some(&fit));
        assert!(r.findings.iter().all(|f| f.feature.is_none()));
    }
}
