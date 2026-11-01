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
//! `FEATURE.MIN` on the removed area (percent of the artwork area),
//! `MESH.MANIFOLD` on an empty mesh (component count), `MESH.SELF_INTERSECT`
//! (pairs of triangles against 0), `OVERHANG.MAX` (degrees from vertical) and
//! `ORIENT.FACE` on studs (a count against 0). A rule that measured
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
//! **Not checked.** A rule whose input is missing is listed in
//! [`Report::unchecked`] with the reason, never silently passed: `SIZE.BED`
//! without a bed ([`validate_on_bed`]), `MESH.SELF_INTERSECT`, `ROOF.BRIDGE`
//! and `OVERHANG.*` on measurements written before those analyses existed, and
//! `TRADEMARK.LOGO` (still planned) on every part with artwork.
//!
//! Implemented rules ([`IMPLEMENTED_RULES`]): every rule of the table but
//! `TRADEMARK.LOGO`. Decisions the table leaves open:
//!
//! - `MESH.SELF_INTERSECT` blocks when any pair of triangles of the exported
//!   objects or of the fused single-colour mesh pass through each other
//!   ([`Measurements::self_intersections`]; the definition, its exact
//!   predicates and its 10 nm dead band are in `openbrick_geom::intersect`).
//! - `ROOF.BRIDGE` warns when the roof's longest bridge in print orientation
//!   ([`Measurements::roof_span_mm`]: the farthest any point of the cavity
//!   ceiling lies from the walls, tubes, pins and ribs that hold it, twice) is
//!   longer than `maxUnsupportedBridgeMm`. A part printed face down has no
//!   hanging roof.
//! - `OVERHANG.MAX` and `OVERHANG.BRIDGED` read the regions of downward faces
//!   outside the roof steeper than `maxOverhangDeg` from vertical
//!   ([`Measurements::overhangs`], print orientation). A region whose span
//!   (twice its farthest distance from an edge with material below) is at
//!   most `maxUnsupportedBridgeMm` prints as a bridge: one `OVERHANG.BRIDGED`
//!   warning names the longest. The others (longer, or held nowhere) make one
//!   `OVERHANG.MAX` finding with the steepest angle: a blocker when the process
//!   has `supportsAllowed` false, a warning telling to turn on supports
//!   otherwise. Faces no higher than the first-layer chamfer above the bed are
//!   not overhangs: the evaluator starts the wall ribs there so the chamfer
//!   stays clear, and the squashed first layer carries them.
//! - `ORIENT.FACE` ("the declared up face is flat and has no studs when inlay
//!   mode") reads the declared up face as the print orientation the part
//!   document states (`print.up`, per relief mode), and the face that must be
//!   flat and stud-free as the one that orientation puts on the bed: the top
//!   face of an inlay (`print.up` `bottom`), as docs/plan/05 section 6 says.
//!   It blocks when the document's `print.up` disagrees with how the
//!   exporters lay the part (an inlay face down, everything else bottom down;
//!   no numbers), and, for a face-down print, when studs stand on the face
//!   (a count against 0) or anything stands proud of it (mm against 0).
//! - `SIZE.BED` blocks when the part's box in print orientation (the mesh as
//!   exported, after `xyScale`, which the printer prints as it is) does not fit
//!   the bed: its footprint may turn a quarter (longer side against the bed's
//!   longer side), not diagonally, and its height must fit the build height.
//!   The bed's full published build volume counts; the slicer keeps its own
//!   margins.
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
//! Not implemented yet: `TRADEMARK.LOGO`, which needs the Q16 artwork matcher
//! ([`PLANNED_RULES`]).

use openbrick_units::STANDARD;

use crate::coupon::coupon_v1;
use crate::{
    check_fit_missing, Bed, Finding, FitProfile, Measurements, Offsets, ProcessProfile, Report,
    Severity, Unchecked,
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
    "MESH.SELF_INTERSECT",
    "MESH.VOLUME",
    "WALL.MIN",
    "TUBE.WALL",
    "CAVITY.DEPTH",
    "WALL.QUANTISED",
    "ROOF.MIN",
    "ROOF.BRIDGE",
    "OVERHANG.MAX",
    "OVERHANG.BRIDGED",
    "FEATURE.MIN",
    "RELIEF.HEIGHT",
    "FIT.PROCESS",
    "FIT.MISSING",
    "CLUTCH.RISK",
    "ORIENT.FACE",
    "SIZE.BED",
];

/// Rules of the table in docs/plan/05 section 5 that [`validate`] does not
/// check yet: `TRADEMARK.LOGO` needs the Q16 artwork matcher (see the module
/// documentation).
pub const PLANNED_RULES: &[&str] = &["TRADEMARK.LOGO"];

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
/// Without a bed, `SIZE.BED` is not checked (see [`validate_on_bed`]).
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
    validate_on_bed(m, process, fit, names, None)
}

/// [`validate_with`], and rule `SIZE.BED` against `bed` when one is given;
/// without one the report lists `SIZE.BED` as not checked.
pub fn validate_on_bed(
    m: &Measurements,
    process: &ProcessProfile,
    fit: Option<&FitProfile>,
    names: &FeatureNames,
    bed: Option<&Bed>,
) -> Report {
    let mut v = Validator {
        m,
        p: process,
        fit,
        names,
        out: Vec::new(),
        unchecked: Vec::new(),
    };
    v.mesh_manifold();
    v.mesh_self_intersect();
    v.mesh_volume();
    v.wall_min();
    v.tube_wall();
    v.cavity_depth();
    v.wall_quantised();
    v.roof_min();
    v.roof_bridge();
    v.overhang();
    v.feature_min();
    v.relief_height();
    v.fit_process();
    v.fit_missing();
    v.clutch_risk();
    v.orient_face();
    v.trademark_logo();
    v.size_bed(bed);
    let mut report = Report {
        findings: v.out,
        unchecked: v.unchecked,
    };
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

/// The unit of a finding's `value` and `threshold`, as a symbol to show after
/// the numbers: `mm`, `%`, `mm³`, or empty when the numbers count something (the
/// components of an empty mesh) or there are none. `m` and `process` are the
/// measurements and process profile the report was made from.
///
/// This is the unit table of the module documentation, decided from where the
/// numbers come from, never from the wording of the message: `MESH.VOLUME`
/// compares a percentage with [`MESH_VOLUME_TOLERANCE_PCT`], or a volume that
/// is not positive with 0 mm³; `FEATURE.MIN` on the removed area carries
/// [`Measurements::removed_area_pct`] against the process's
/// `maxRemovedAreaPct` unchanged, while its stroke triggers carry lengths
/// against `minFeatureMm`. (Only a stroke of exactly the removed percentage in
/// millimetres on a process whose two thresholds are equal could be mistaken for
/// the area; no bundled process has equal thresholds.) The validator fixture
/// test checks every fixture's message shows its numbers in this unit.
pub fn finding_unit(f: &Finding, m: &Measurements, process: &ProcessProfile) -> &'static str {
    if f.value.is_none() && f.threshold.is_none() {
        return "";
    }
    match f.rule.as_str() {
        // The empty-mesh finding: 0 components against at least 1.
        "MESH.MANIFOLD" => "",
        // Pairs of triangles against 0.
        "MESH.SELF_INTERSECT" => "",
        // The steepest face against maxOverhangDeg.
        "OVERHANG.MAX" => "°",
        // Studs on the face on the bed against 0; with none there, the
        // finding is the height something stands above that face.
        "ORIENT.FACE" if m.bed_face_studs.unwrap_or(0) > 0 => "",
        "MESH.VOLUME" if f.threshold == Some(MESH_VOLUME_TOLERANCE_PCT) => "%",
        "MESH.VOLUME" => "mm³",
        "FEATURE.MIN"
            if f.value.is_some()
                && f.value == m.removed_area_pct
                && f.threshold == process.max_removed_area_pct =>
        {
            "%"
        }
        _ => "mm",
    }
}

/// A length for messages: at most three decimals, trailing zeros dropped
/// (0.8, 0.857, 4.95).
pub(crate) fn mm(x: f64) -> String {
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

/// The longest span of these regions, if any has one.
fn longest_span<'a>(regions: impl Iterator<Item = &'a crate::Overhang>) -> Option<f64> {
    regions
        .filter_map(|r| r.span_mm)
        .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))
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
    unchecked: Vec<Unchecked>,
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

    /// Record a rule this report could not check.
    fn not_checked(&mut self, rule: &str, reason: String) {
        self.unchecked.push(Unchecked {
            rule: rule.into(),
            reason,
        });
    }

    fn mesh_self_intersect(&mut self) {
        let Some(pairs) = self.m.self_intersections else {
            self.not_checked(
                "MESH.SELF_INTERSECT",
                "the measurements hold no self-intersection count (they were written before the test existed, or the mesh lies outside its +-1024 mm range); build the part again to check it".into(),
            );
            return;
        };
        if pairs == 0 {
            return;
        }
        self.push(
            "MESH.SELF_INTERSECT",
            Severity::Blocker,
            format!(
                "the mesh passes through itself: {pairs} {} of triangles cross (a printable part has 0), so a slicer cannot tell inside from outside there",
                if pairs == 1 { "pair" } else { "pairs" }
            ),
            Some(pairs as f64),
            Some(0.0),
            "this is a bug in the evaluator or the kernel, not in your part: please report it with the part file and its parameters; other parameter values may avoid it meanwhile".into(),
        );
    }

    fn roof_bridge(&mut self) {
        if self.m.overhangs.is_none() {
            self.not_checked(
                "ROOF.BRIDGE",
                "the measurements hold no overhang analysis (they were written before it existed); build the part again to check it".into(),
            );
            return;
        }
        let Some(span) = self.m.roof_span_mm else {
            return;
        };
        let max = self.p.max_unsupported_bridge_mm;
        if span > max + LENGTH_TOLERANCE_MM {
            let msg = format!(
                "the roof bridges {} mm between walls and tubes, longer than the {} mm maxUnsupportedBridgeMm {}: the middle of the span may sag",
                mm(span),
                mm(max),
                self.of_process()
            );
            self.push(
                "ROOF.BRIDGE",
                Severity::Warning,
                msg,
                Some(span),
                Some(max),
                "print a bridging test first, or keep the part's cavity spans shorter (tubes or pins under the roof hold it); you may export as it is".into(),
            );
        }
    }

    fn overhang(&mut self) {
        let Some(regions) = &self.m.overhangs else {
            for rule in ["OVERHANG.MAX", "OVERHANG.BRIDGED"] {
                self.not_checked(
                    rule,
                    "the measurements hold no overhang analysis (they were written before it existed); build the part again to check it".into(),
                );
            }
            return;
        };
        let (deg, max_bridge) = (self.p.max_overhang_deg, self.p.max_unsupported_bridge_mm);
        let steep: Vec<_> = regions
            .iter()
            .filter(|r| r.angle_deg > deg + 1e-9)
            .collect();
        let bridged = |r: &&crate::Overhang| {
            r.span_mm
                .is_some_and(|s| s <= max_bridge + LENGTH_TOLERANCE_MM)
        };
        let (short, unsupported): (Vec<_>, Vec<_>) = steep.into_iter().partition(bridged);
        if let Some(worst) = unsupported
            .iter()
            .max_by(|a, b| a.angle_deg.total_cmp(&b.angle_deg))
        {
            let n = unsupported.len();
            let floating = unsupported.iter().filter(|r| r.span_mm.is_none()).count();
            let long = longest_span(unsupported.iter().copied());
            let mut why = Vec::new();
            if floating > 0 {
                why.push(match (n, floating) {
                    (1, _) => "it has no material below any edge".to_string(),
                    (_, 1) => "1 has no material below any edge".to_string(),
                    (_, f) => format!("{f} have no material below any edge"),
                });
            }
            if let Some(s) = long {
                why.push(format!(
                    "{} would bridge {} mm, more than the {} mm maxUnsupportedBridgeMm",
                    if n == 1 { "it" } else { "the longest" },
                    mm(s),
                    mm(max_bridge)
                ));
            }
            let [x, y] = worst.centre_mm;
            let msg = format!(
                "{} of downward faces {} up to {}° from vertical, beyond the {}° maxOverhangDeg {}, without a short bridge ({}); the steepest is {} mm above the bed near x {} mm, y {} mm in print orientation",
                count(n, "region"),
                if n == 1 { "hangs" } else { "hang" },
                mm(worst.angle_deg),
                mm(deg),
                self.of_process(),
                either(&why),
                mm(worst.height_mm),
                mm(x),
                mm(y)
            );
            let (severity, fix) = if self.p.supports_allowed {
                (
                    Severity::Warning,
                    "turn on supports in the slicer for this part, or chamfer the overhang at 45 degrees so it needs none".to_string(),
                )
            } else {
                (
                    Severity::Blocker,
                    format!(
                        "chamfer the overhang at 45 degrees or less from vertical, hold it from below, or keep a bridge between two supports within {} mm; process `{}` prints without supports",
                        mm(max_bridge),
                        self.p.id
                    ),
                )
            };
            self.push(
                "OVERHANG.MAX",
                severity,
                msg,
                Some(worst.angle_deg),
                Some(deg),
                fix,
            );
        }
        if let Some(longest) = longest_span(short.iter().copied()) {
            let n = short.len();
            let msg = format!(
                "{} of downward faces steeper than the {}° maxOverhangDeg {} {}, the longest {} mm, within the {} mm maxUnsupportedBridgeMm: {} may come out a little rough",
                count(n, "region"),
                mm(deg),
                self.of_process(),
                if n == 1 { "prints as a short bridge" } else { "print as short bridges" },
                mm(longest),
                mm(max_bridge),
                if n == 1 { "its underside" } else { "their undersides" }
            );
            self.push(
                "OVERHANG.BRIDGED",
                Severity::Warning,
                msg,
                Some(longest),
                Some(max_bridge),
                "nothing to change for most parts; for a cleaner underside, turn on the slicer's bridge detection (PrusaSlicer: detect bridging perimeters)".into(),
            );
        }
    }

    fn orient_face(&mut self) {
        let m = self.m;
        let declared_bottom = m.print_up.as_deref() == Some("bottom");
        if let Some(up) = &m.print_up {
            let mode = m
                .relief_mode
                .as_deref()
                .map(|r| format!(" for relief mode `{r}`"))
                .unwrap_or_default();
            if m.face_down && up != "bottom" {
                self.push(
                    "ORIENT.FACE",
                    Severity::Blocker,
                    format!(
                        "the part document declares the `{up}` face up{mode}, but the part prints face down, its top face on the bed (an inlay), as the exporters turn it"
                    ),
                    None,
                    None,
                    format!("set print.up{} to \"bottom\" in the part document", if mode.is_empty() { String::new() } else { format!(" for `{}`", m.relief_mode.as_deref().unwrap_or("")) }),
                );
            } else if !m.face_down && up != "top" {
                self.push(
                    "ORIENT.FACE",
                    Severity::Blocker,
                    format!(
                        "the part document declares the `{up}` face up{mode}, but the exporters lay the part bottom down as designed; only an inlay prints face down"
                    ),
                    None,
                    None,
                    format!("set print.up{} to \"top\" in the part document, or use relief mode inlay for a face-down print", if mode.is_empty() { String::new() } else { format!(" for `{}`", m.relief_mode.as_deref().unwrap_or("")) }),
                );
            }
        }
        if !(m.face_down || declared_bottom) {
            return;
        }
        let studs = m.bed_face_studs.unwrap_or(0);
        if studs > 0 {
            self.push(
                "ORIENT.FACE",
                Severity::Blocker,
                format!(
                    "the part prints face down, but {} stand on that face (a face on the bed has 0): the face must be flat and stud-free",
                    count(studs as usize, "stud")
                ),
                Some(f64::from(studs)),
                Some(0.0),
                "use body.tile (a smooth top) for a face-down part, or turn the stud field off with its `if`".into(),
            );
        } else if let Some(h) = m.bed_face_relief_mm.filter(|h| *h > LENGTH_TOLERANCE_MM) {
            self.push(
                "ORIENT.FACE",
                Severity::Blocker,
                format!(
                    "the part prints face down, but something stands {} mm proud of that face (a flat face stands 0 mm): it would rest on that and not on the face",
                    mm(h)
                ),
                Some(h),
                Some(0.0),
                "use relief mode inlay or deboss for a face-down part, which keep the face flat".into(),
            );
        }
    }

    /// `TRADEMARK.LOGO` is planned: say so on parts with uploaded artwork (a
    /// relief mode; the coupon's own labels have none).
    fn trademark_logo(&mut self) {
        if self.m.relief_mode.is_some() {
            self.not_checked(
                "TRADEMARK.LOGO",
                "the artwork is not compared with the blocklist of protected marks yet: that needs the artwork matcher of the Q16 spike (docs/plan/12); please make sure you may use the artwork".into(),
            );
        }
    }

    fn size_bed(&mut self, bed: Option<&Bed>) {
        let Some(bed) = bed else {
            self.not_checked(
                "SIZE.BED",
                "no printer or bed was given, so the part was not checked against a build volume; choose a printer (openbrick validate --printer, or the playground's printer menu) or give a bed (--bed WxDxH)".into(),
            );
            return;
        };
        let m = self.m;
        let size: Vec<f64> = (0..3)
            .map(|k| m.bounds_max_mm[k] - m.bounds_min_mm[k])
            .collect();
        if !size.iter().all(|s| s.is_finite() && *s >= 0.0) {
            self.not_checked(
                "SIZE.BED",
                "the part's bounds are not finite numbers, so its size could not be measured"
                    .into(),
            );
            return;
        }
        let [bx, by, bz] = bed.size_mm;
        let dims = format!("{} x {} x {} mm", mm(bx), mm(by), mm(bz));
        let of = if bed.printer.is_some() {
            format!("the {} ({dims})", bed.name)
        } else {
            format!("the given {dims} build volume")
        };
        let shown = format!("{} x {} x {} mm", mm(size[0]), mm(size[1]), mm(size[2]));
        // The footprint may turn a quarter on the bed: its longer side against
        // the bed's longer side, its shorter against the shorter.
        let (short, long) = (size[0].min(size[1]), size[0].max(size[1]));
        let (bed_short, bed_long) = (bx.min(by), bx.max(by));
        let over = |v: f64, t: f64| v > t + LENGTH_TOLERANCE_MM;
        let too_big = if over(long, bed_long) {
            Some(("longer side", long, bed_long))
        } else if over(short, bed_short) {
            Some(("shorter side", short, bed_short))
        } else {
            None
        };
        if let Some((side, v, t)) = too_big {
            self.push(
                "SIZE.BED",
                Severity::Blocker,
                format!(
                    "the part is {shown} in print orientation, and its {side} of {} mm is longer than the {} mm bed side of {of}, even turned a quarter",
                    mm(v),
                    mm(t)
                ),
                Some(v),
                Some(t),
                format!(
                    "make the part smaller, or print it on a printer with a larger bed (the part needs at least {} x {} mm)",
                    mm(long),
                    mm(short)
                ),
            );
        }
        if over(size[2], bz) {
            self.push(
                "SIZE.BED",
                Severity::Blocker,
                format!(
                    "the part is {shown} in print orientation, and its height of {} mm is more than the {} mm build height of {of}",
                    mm(size[2]),
                    mm(bz)
                ),
                Some(size[2]),
                Some(bz),
                "make the part lower, or print it on a printer with a taller build volume".into(),
            );
        }
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
    fn units_come_from_where_the_numbers_come_from() {
        let fit = example_fit_profile();
        let p = fdm04();
        let part = Measurements {
            process: "fdm-0.4".into(),
            fit_profile: Some(fit.id.clone()),
            components: 1,
            is_manifold: true,
            volume_mm3: 100.0,
            feature_volume_sum_mm3: 100.0,
            ..Measurements::default()
        };
        let units = |m: &Measurements, fit: Option<&FitProfile>| -> Vec<(String, &'static str)> {
            validate(m, &p, fit)
                .findings
                .iter()
                .map(|f| (f.rule.clone(), finding_unit(f, m, &p)))
                .collect()
        };
        let pair = |rule: &str, unit: &'static str| (rule.to_string(), unit);
        // Both FEATURE.MIN triggers on one part: a stroke below the minimum (a
        // length) and too much area removed (a percentage), sorted by message.
        let art = Measurements {
            min_stroke_mm: Some(0.5),
            removed_area_pct: Some(3.0),
            relief_height_mm: Some(0.6),
            ..part.clone()
        };
        assert_eq!(
            units(&art, Some(&fit)),
            vec![pair("FEATURE.MIN", "%"), pair("FEATURE.MIN", "mm")]
        );
        // A stroke of 0.5 mm with 0.5 % removed (under the limit): the stroke
        // finding carries the same number as the removed area, and its
        // threshold tells it apart.
        let same_number = Measurements {
            removed_area_pct: Some(0.5),
            ..art.clone()
        };
        assert_eq!(
            units(&same_number, Some(&fit)),
            vec![pair("FEATURE.MIN", "mm")]
        );
        // A stroke that was not measured keeps its threshold, a length.
        let unmeasured = Measurements {
            min_stroke_mm: None,
            removed_area_pct: Some(0.5),
            ..art.clone()
        };
        assert_eq!(
            units(&unmeasured, Some(&fit)),
            vec![pair("FEATURE.MIN", "mm")]
        );
        // MESH.VOLUME: a deviation in percent, a volume that is not positive in
        // mm³, one that is not a number without numbers.
        let off = Measurements {
            volume_mm3: 120.0,
            ..part.clone()
        };
        assert_eq!(units(&off, Some(&fit)), vec![pair("MESH.VOLUME", "%")]);
        let negative = Measurements {
            volume_mm3: -5.0,
            ..part.clone()
        };
        assert_eq!(
            units(&negative, Some(&fit)),
            vec![pair("MESH.VOLUME", "mm³")]
        );
        let nan = Measurements {
            volume_mm3: f64::NAN,
            ..part.clone()
        };
        assert_eq!(units(&nan, Some(&fit)), vec![pair("MESH.VOLUME", "")]);
        // MESH.MANIFOLD on an empty mesh counts components; lengths are mm; FIT
        // rules have no numbers.
        let empty = Measurements {
            components: 0,
            ..part.clone()
        };
        assert_eq!(units(&empty, Some(&fit)), vec![pair("MESH.MANIFOLD", "")]);
        let wall = Measurements {
            min_body_wall_mm: Some(0.5),
            ..part.clone()
        };
        // 0.5 mm is also not a whole number of 0.4 mm lines.
        assert_eq!(
            units(&wall, Some(&fit)),
            vec![pair("WALL.MIN", "mm"), pair("WALL.QUANTISED", "mm")]
        );
        let other = Measurements {
            process: "fdm-0.2".into(),
            ..part.clone()
        };
        assert!(units(&other, Some(&fit))
            .iter()
            .all(|(rule, unit)| rule == "FIT.PROCESS" && unit.is_empty()));
        // The bundled processes keep the two FEATURE.MIN thresholds apart, so
        // the threshold alone tells the triggers apart.
        for p in default_process_profiles() {
            assert_ne!(Some(p.min_feature_mm), p.max_removed_area_pct, "{}", p.id);
        }
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
