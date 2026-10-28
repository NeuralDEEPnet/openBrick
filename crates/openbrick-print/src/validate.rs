//! The validator (docs/plan/05 section 5): the [`Measurements`] of a built part
//! checked against a [`ProcessProfile`] and an optional [`FitProfile`].
//!
//! Every finding carries its rule id, severity, a message with the measured
//! value and unit, the value and the threshold as numbers, and a fix hint
//! (CLAUDE.md). Blockers disable export; warnings never do. Thresholds come from
//! the process profile, the coupon sweep from `standards/coupon-v1.json`
//! ([`crate::coupon_v1`]); the only threshold of the validator itself is
//! [`MESH_VOLUME_TOLERANCE_PCT`], from the rule table, and
//! [`LENGTH_TOLERANCE_MM`] only absorbs floating-point residue, so a wall of
//! exactly the minimum passes.
//!
//! Units: `value` and `threshold` are millimetres, except `MESH.VOLUME`
//! (percent deviation and percent tolerance) and `MESH.MANIFOLD` on an empty
//! mesh (component count).
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
//! - `MESH.VOLUME` warns when the volume is not positive, or deviates from
//!   [`Measurements::feature_volume_sum_mm3`] by more than 5 percent; it skips
//!   the comparison when that sum is not positive (unknown).
//! - `TUBE.WALL` blocks below `minTubeWallMm` and warns below
//!   `recommendedTubeWallMm` (0.8 mm for `fdm-0.4`, from "minTubeWallMm 0.7
//!   (warning below 0.8)" in the rule table); a profile without that field has
//!   no warning band. Hollow-stud walls are not measured yet.
//! - `WALL.QUANTISED` tests the side wall before the fit profile's `xyScale`,
//!   which compensates shrinkage and so is undone by the print: the designer's
//!   wall is what must be whole lines.
//! - `FEATURE.MIN` blocks when the thinnest stroke of fit-critical artwork
//!   ([`Measurements::min_fit_critical_stroke_mm`], a label someone reads to
//!   calibrate) is below `minFeatureMm`, and warns when any artwork stroke is.
//!   The removed area of the morphological opening is reported in the message;
//!   rounding the corners of any artwork removes some area, so it is not a
//!   trigger on its own.
//! - `FIT.PROCESS` blocks a fit profile calibrated for another process, a part
//!   built for a process other than the one it is validated against, and a
//!   process the part does not allow (`print.profiles`).
//! - `CLUTCH.RISK` warns when the stud, tube bore or tube outer diameter after
//!   offsets lies outside the coupon cells `stud`, `tube` or `pocketTubeOD`.
//!
//! Not implemented yet, because they need mesh analysis or data the evaluator
//! does not provide: `MESH.SELF_INTERSECT` (needs a self-intersection test of
//! the mesh), `ROOF.BRIDGE` and `OVERHANG.MAX` / `OVERHANG.BRIDGED` (need span
//! and face-angle analysis), `ORIENT.FACE` (needs the declared up face and
//! inlay mode), `TRADEMARK.LOGO` (needs the Q16 artwork matcher) and `SIZE.BED`
//! (process profiles carry no bed size yet).

use crate::coupon::coupon_v1;
use crate::{
    check_fit_missing, Finding, FitProfile, Measurements, ProcessProfile, Report, Severity,
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

/// Check the measurements of a built part. Findings are sorted blockers first,
/// then by rule id, feature and message, so equal inputs give equal reports.
pub fn validate(m: &Measurements, process: &ProcessProfile, fit: Option<&FitProfile>) -> Report {
    let mut v = Validator {
        m,
        p: process,
        fit,
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
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
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

struct Validator<'a> {
    m: &'a Measurements,
    p: &'a ProcessProfile,
    fit: Option<&'a FitProfile>,
    out: Vec<Finding>,
}

impl Validator<'_> {
    fn push(
        &mut self,
        rule: &str,
        severity: Severity,
        message: String,
        value: Option<f64>,
        threshold: Option<f64>,
        fix: String,
    ) {
        self.out.push(Finding {
            rule: rule.into(),
            severity,
            message,
            value,
            threshold,
            fix,
            feature: None,
        });
    }

    /// `of fdm-0.4`, for messages that name the threshold's source.
    fn of_process(&self) -> String {
        format!("of process `{}`", self.p.id)
    }

    fn fit_name(&self) -> String {
        self.fit
            .map(|f| format!("fit profile `{}`", f.id))
            .unwrap_or_else(|| "the fit profile".into())
    }

    fn mesh_manifold(&mut self) {
        let m = self.m;
        if m.components == 0 {
            self.push(
                "MESH.MANIFOLD",
                Severity::Blocker,
                format!(
                    "the mesh is empty (0 components, {} triangles): there is nothing to print",
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
        if m.volume_mm3 <= 0.0 || !m.volume_mm3.is_finite() {
            self.push(
                "MESH.VOLUME",
                Severity::Warning,
                format!(
                    "the mesh volume is {} mm3; a printable part has a positive volume",
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
                    "the mesh volume {} mm3 differs from the sum of its feature volumes {} mm3 by {:.1} %, more than {} %",
                    mm(m.volume_mm3),
                    mm(expected),
                    deviation,
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
        let Some(wall) = self.m.min_body_wall_mm else {
            return;
        };
        let min = self.p.min_wall_mm;
        if below(wall, min) {
            let msg = format!(
                "the thinnest body wall is {} mm, below the {} mm minimum wall {}",
                mm(wall),
                mm(min),
                self.of_process()
            );
            let mut fix = format!("make the side wall at least {} mm thick", mm(min));
            if let Some(f) = self.fit.filter(|f| f.offsets_mm.xy_scale < 1.0) {
                fix += &format!(
                    "; the xyScale {} of fit profile `{}` thins every wall",
                    f.offsets_mm.xy_scale, f.id
                );
            }
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
        let Some(wall) = self.m.min_tube_wall_mm else {
            return;
        };
        let min = self.p.min_tube_wall_mm;
        if below(wall, min) {
            let msg = format!(
                "the thinnest tube wall is {} mm after fit offsets, below the {} mm minimum tube wall {}",
                mm(wall),
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
            );
            return;
        }
        let Some(recommended) = self.p.recommended_tube_wall_mm else {
            return;
        };
        if below(wall, recommended) {
            let msg = format!(
                "the thinnest tube wall is {} mm after fit offsets: printable, but below the recommended {} mm {}",
                mm(wall),
                mm(recommended),
                self.of_process()
            );
            let fix = format!(
                "raise the tubeOuterDiameter offset in {} by {} mm, or record a tube cell with a smaller bore, so the wall reaches {} mm",
                self.fit_name(),
                mm(2.0 * (recommended - wall)),
                mm(recommended)
            );
            self.push(
                "TUBE.WALL",
                Severity::Warning,
                msg,
                Some(wall),
                Some(recommended),
                fix,
            );
        }
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
        let Some(wall) = self.m.min_body_wall_mm else {
            return;
        };
        let q = self.p.wall_quantise_mm;
        if q <= 0.0 {
            return;
        }
        let xy = self.fit.map(|f| f.offsets_mm.xy_scale).unwrap_or(1.0);
        let design = if xy > 0.0 { wall / xy } else { wall };
        let lines = design / q;
        if (lines - lines.round()).abs() * q <= LENGTH_TOLERANCE_MM {
            return;
        }
        let better = self.p.quantise_wall(design);
        let msg = format!(
            "the side wall is {} mm, which is {:.2} lines of {} mm (wallQuantiseMm {}); walls print solid only as whole lines",
            mm(design),
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
            Some(design),
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
                "make the roof at least {} mm: a deboss or serial on top thins it by its depth, so make it {} mm shallower",
                mm(min),
                mm(min - roof)
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
        let removed = self
            .m
            .removed_area_pct
            .map(|pct| {
                format!("; the morphological opening removed {pct:.1} % of the artwork area")
            })
            .unwrap_or_default();
        if let Some(stroke) = self.m.min_fit_critical_stroke_mm.filter(|&s| below(s, min)) {
            let msg = format!(
                "the thinnest stroke of fit-critical artwork is {} mm, below the {} mm minimum feature {}{removed}",
                mm(stroke),
                mm(min),
                self.of_process()
            );
            let fix = format!(
                "fit-critical artwork such as a calibration label must print as drawn: make its strokes at least {} mm wide (a bolder weight or a larger size)",
                mm(min)
            );
            self.push(
                "FEATURE.MIN",
                Severity::Blocker,
                msg,
                Some(stroke),
                Some(min),
                fix,
            );
            return;
        }
        if let Some(stroke) = self.m.min_stroke_mm.filter(|&s| below(s, min)) {
            let msg = format!(
                "the thinnest artwork stroke is {} mm, below the {} mm minimum feature {}{removed}",
                mm(stroke),
                mm(min),
                self.of_process()
            );
            let fix = format!(
                "make strokes at least {} mm wide (a bolder weight or a larger size), or keep minStroke at the process minimum so thinner lines are removed",
                mm(min)
            );
            self.push(
                "FEATURE.MIN",
                Severity::Warning,
                msg,
                Some(stroke),
                Some(min),
                fix,
            );
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
        let checks = [
            (self.m.stud_diameter_mm, "stud", "stud diameter"),
            (self.m.tube_inner_diameter_mm, "tube", "tube bore"),
            (
                self.m.tube_outer_diameter_mm,
                "pocketTubeOD",
                "tube outer diameter",
            ),
        ];
        for (value, cell_id, what) in checks {
            let (Some(v), Some(cell)) = (value, spec.cell(cell_id)) else {
                continue;
            };
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
                "the {what} is {} mm after offsets, outside the {} to {} mm the coupon tests (cell `{cell_id}`), so no printed cell has shown how it grips",
                mm(v),
                mm(cell.start_mm),
                mm(cell.end_mm)
            );
            let fix = format!(
                "check the {offset} offset in {}, or print the coupon again and record the `{cell_id}` cell that fits best",
                self.fit_name()
            );
            self.push(
                "CLUTCH.RISK",
                Severity::Warning,
                msg,
                Some(v),
                Some(bound),
                fix,
            );
        }
    }
}
