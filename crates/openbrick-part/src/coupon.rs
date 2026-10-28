//! The calibration coupon (docs/plan/05 sections 4, 6 and 12; backlog item 11
//! in docs/plan/14): the part a user prints first, pushes genuine bricks onto
//! and reads the best-fitting cells from, so that later parts fit genuine bricks.
//!
//! # What it is
//!
//! A flat tray carrying, in the order of the plan table, one line of stud cells
//! (male, the stud OD sweep), one line of tube bore cells (female, the tube ID
//! sweep) and the 2x2 pocket cells (tube OD crossed with wall rib depth). The
//! whole set is printed twice, so every cell exists twice (repeatability). Each
//! cell carries an embossed label with its value in millimetres; a pocket's
//! label has two lines, its tube OD over its rib depth. The rows Technic holes,
//! pins, axles, axle holes and bars and clips come later: a row is an entry in
//! `ROWS` plus a cell builder in `build_cell`.
//!
//! # How it prints and how it is used
//!
//! Everything opens upward and the bottom is flat, so the coupon prints with no
//! supports and no bridges (docs/plan/05 section 6): every face is vertical,
//! horizontal and facing up, or a 45 degree stud-top chamfer.
//!
//! - **Base:** as thick as a plate's roof (the standard `roof`, at least the
//!   process `minRoofMm`), with the process's first-layer chamfer on its bottom
//!   edges. The pocket and bore cells stand a plate cavity tall on it, so each of
//!   them is exactly plate height: a pocket cell is a 2x2 plate turned cavity up.
//!   A solid plate-height base would carry more than twice the plastic (the
//!   default coupon would grow from about 26 to about 68 cm3, or about 4.7 hours
//!   by the model below), which breaks the two-hour budget and adds nothing a
//!   fit depends on.
//! - **Stud cells:** one stud of the cell's diameter, built by the same code as
//!   `stud.field` (standard height, process stud-top chamfer). Press a genuine
//!   1x1 plate or brick onto it; the 1x1 footprint around each stud is kept
//!   clear so the plate sits on the base.
//! - **Bore cells:** a free-standing tube with the cell's bore, open at the top,
//!   whose outer diameter is the standard tube OD grown, exactly as a part's
//!   tube grows ([`Offsets::tube`]), so its wall stays at least `minTubeWallMm`.
//!   Turn a genuine 1x1 plate over and press its stud into the bore; the plate
//!   rests on the tube rim. A thin free tube (not a hole in solid plastic) keeps
//!   the spring of a real tube (docs/plan/04 section 5: the walls are the spring).
//! - **Pocket cells:** a 2x2 plate's underside turned up: four side walls of the
//!   process-quantised wall thickness with one rib on each wall at each stud
//!   centre line (the rib layout of `tube.field`), ribs `minFeatureMm` wide, and
//!   a central tube of the cell's outer diameter with the standard bore
//!   (narrowed only if the wall would drop below `minTubeWallMm`). Turn a
//!   genuine 2x2 plate or brick over and press its studs in: this is the grip
//!   the pull test of section 12 loads. A cell's rib depth is measured against
//!   the standard `side_wall`, as the standard's `rib_depth` is: the rib tip
//!   sits `side_wall + value` from the outer face on every process. Where the
//!   process quantises the wall thinner (fdm-0.2: five 0.22 mm lines, 1.1 mm)
//!   the rib is built deeper by the difference, exactly as a part's rib is
//!   (docs/plan/04 section 6), so the grip a cell shows is the grip a part gets.
//! - **Labels:** embossed on the base below each cell, never on a stud top
//!   (CLAUDE.md), in an original pixel font (module `font`) whose pixel is one
//!   stroke: the plan's 0.8 mm stroke, at least the process `minFeatureMm`; the
//!   plan's 0.6 mm relief, kept within the process relief limits.
//!
//! # Fit offsets are never applied
//!
//! The coupon measures the printer, so it is built from the nominal values in
//! the coupon data and nothing else: [`build_coupon`] takes no fit profile and
//! [`CouponOptions`] has no field for one. The wizard then writes
//! `offset = chosen cell - nominal` (docs/plan/05 section 4), where the nominal
//! is the standard value of the cell's `standard_id` (stud_diameter,
//! tube_inner_diameter, tube_outer_diameter, rib_depth), the same on every
//! process. Each cell is built as the evaluator builds that feature of a part
//! whose fit profile holds exactly that offset, so a part built with the
//! recorded offsets reproduces the chosen cells on every process; the tests
//! check this for the rib on fdm-0.2, where the wall is quantised.
//!
//! # Pocket sets
//!
//! The plan crosses six tube ODs with seven rib depths: 42 pockets, 84 with the
//! second copy, which neither fits a 180 x 180 mm bed nor prints in an
//! afternoon. [`PocketSet`] offers the full cross and two compact sets; the
//! default, [`PocketSet::Cross`], is the nominal row and column of the cross
//! (every OD at the nominal rib depth, every rib depth at the nominal OD): 13
//! pockets per copy. It is the largest set that fits a 180 x 180 mm bed twice
//! and that the volume model below puts under two hours on a 0.4 mm nozzle.
//!
//! # Print estimate
//!
//! [`PrintEstimate`] divides the solid volume by an extrusion rate: the
//! process line width times its thickest layer times an assumed mean speed
//! ([`CouponOptions::estimate_speed_mm_s`], 50 mm/s), 4.0 mm3/s on fdm-0.4. The
//! mass assumes PLA. Measured with manifold-rust 0.19.1 on fdm-0.4, two copies:
//!
//! | Pocket set | Footprint (mm) | Volume (mm3) | PLA (g) | Estimate |
//! |------------|----------------|--------------|---------|----------|
//! | cross (default) | 118.4 x 160.8 | 25,942 | 32.2 | 108 min |
//! | crossAndDiagonal | 118.4 x 213.6 | 34,384 | 42.6 | 143 min |
//! | full (side by side) | 202.4 x 212.8 | 62,719 | 77.8 | 261 min |
//!
//! The model is meant for extrusion processes; for resin it is meaningless.
//!
//! # Units
//!
//! Cell centres, line pitches and the base outline are whole LDU (CLAUDE.md);
//! the label strokes and cell solids are generated in millimetres from them.
//! A line's pitch is the smallest even LDU count that keeps one stroke between
//! the genuine pieces of neighbouring cells and a word space between their
//! labels (32 LDU for stud and bore cells, 42 LDU for pockets), not a whole
//! number of studs: no genuine piece spans two cells, so nothing depends on the
//! stud grid, and rounding up to whole studs (40 and 60 LDU) would widen the
//! default coupon from 118.4 to 161.6 mm, about 6,900 mm3 more base and 137
//! minutes by the model below, past the two-hour budget.
//! Every dimension comes from `standards/dimensions-v1.json` (through
//! `openbrick-units`), the [`ProcessProfile`] or the coupon data.

mod font;

use std::collections::BTreeSet;
use std::sync::OnceLock;

use openbrick_geom::{CrossSection, GeomError, Kernel, Polygon, Solid};
use openbrick_print::{Measurements, Offsets, ProcessProfile};
use openbrick_units::{ldu_to_mm, LDU_MM, STANDARD};
use serde_json::json;

use crate::eval::geometry::{self, Geo, StudGeom, TubeGeom};
use crate::eval::{
    canonical_json, nominal_rib_depth, roof_mm, sha256_hex, stud_shape, Built, EvalError, RoleMesh,
    EVALUATOR_REVISION, MIN_LENGTH_MM,
};

/// Bump when the coupon generator builds different geometry or a different
/// layout for the same inputs. 2: pocket rib values are measured against the
/// standard side wall (deeper ribs and a different nominal line where the
/// process quantises the wall, as on fdm-0.2; fdm-0.4 is unchanged).
pub const COUPON_REVISION: u32 = 2;

/// Most copies of the set one coupon may carry: a resource guard, like the
/// evaluator's `MAX_STUDS_PER_SIDE`, not a print rule (the plan prints two).
pub const MAX_COPIES: u32 = 16;

/// Density of PLA for the mass estimate, g/cm3 (NatureWorks Ingeo 4043D, the
/// usual base resin of PLA filament, lists a specific gravity of 1.24).
pub const PLA_DENSITY_G_CM3: f64 = 1.24;

// ---------------------------------------------------------------------------
// Coupon data.
//
// TODO(integrator): replace CouponSpec, CouponCell and coupon_spec() (with
// cells_from_plan() and its numbers) by openbrick_print::coupon_v1(), which
// loads standards/coupon-v1.json into types with the same fields and the same
// values() method. Until then the sweeps are the table of docs/plan/05
// section 4, and they appear nowhere else in the code.
// ---------------------------------------------------------------------------

/// The coupon data: mirrors `openbrick_print::CouponSpec`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
struct CouponSpec {
    version: u32,
    cells: Vec<CouponCell>,
}

/// One swept quantity: mirrors `openbrick_print::CouponCell`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CouponCell {
    /// The `chosenCells` key: stud, tube, pocketTubeOD, pocketRib, technicH, ...
    id: String,
    label: String,
    /// The nominal's id in standards/dimensions-v1.json.
    standard_id: String,
    start_mm: f64,
    end_mm: f64,
    step_mm: f64,
}

impl CouponCell {
    /// The swept values from start to end in steps, rounded to a nanometre so
    /// that start + n x step lands on end exactly.
    fn values(&self) -> Vec<f64> {
        if self.step_mm.is_nan() || self.step_mm <= 0.0 || self.end_mm < self.start_mm {
            return Vec::new();
        }
        let n = ((self.end_mm - self.start_mm) / self.step_mm).round() as usize;
        (0..=n)
            .map(|i| ((self.start_mm + i as f64 * self.step_mm) * 1e6).round() / 1e6)
            .collect()
    }
}

/// The sweeps of docs/plan/05 section 4 that M1 builds.
fn cells_from_plan() -> Vec<CouponCell> {
    let cell = |id: &str, label: &str, standard_id: &str, start, end, step| CouponCell {
        id: id.into(),
        label: label.into(),
        standard_id: standard_id.into(),
        start_mm: start,
        end_mm: end,
        step_mm: step,
    };
    vec![
        cell("stud", "Stud OD", "stud_diameter", 4.60, 4.95, 0.05),
        cell(
            "tube",
            "Tube bore ID",
            "tube_inner_diameter",
            4.80,
            5.05,
            0.05,
        ),
        cell(
            "pocketTubeOD",
            "2x2 pocket tube OD",
            "tube_outer_diameter",
            6.40,
            6.65,
            0.05,
        ),
        cell(
            "pocketRib",
            "2x2 pocket rib depth",
            "rib_depth",
            0.2,
            0.5,
            0.05,
        ),
    ]
}

fn coupon_spec() -> &'static CouponSpec {
    static SPEC: OnceLock<CouponSpec> = OnceLock::new();
    SPEC.get_or_init(|| CouponSpec {
        version: 1,
        cells: cells_from_plan(),
    })
}

/// The coupon-wide settings of the coupon data.
struct PlanDefaults {
    /// Identical copies of the whole set.
    copies: u32,
    label_relief_mm: f64,
    label_stroke_mm: f64,
}

/// Docs/plan/05 section 4: "two identical rows (repeatability)" and "Each cell
/// has an embossed label (relief 0.6 mm, stroke 0.8 mm)".
// TODO(integrator): read these from openbrick_print::coupon_v1() as well
// (`copies` and `label.reliefMm` / `label.strokeMm` in coupon-v1.json), so the
// coupon data is the one source of the copies and the label size.
fn defaults_from_plan() -> PlanDefaults {
    PlanDefaults {
        copies: 2,
        label_relief_mm: 0.6,
        label_stroke_mm: 0.8,
    }
}

// ---------------------------------------------------------------------------
// Public types.
// ---------------------------------------------------------------------------

/// Which pockets of the tube OD x rib depth cross the coupon carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PocketSet {
    /// The nominal row and column of the cross: one line with every tube OD at
    /// the nominal rib depth, one with every rib depth at the nominal tube OD
    /// (the pocket at both nominals is in both lines). The nominals are the
    /// sweep values nearest the standard tube OD and the standard rib depth, on
    /// every process: rib values are measured against the standard side wall
    /// (see the module docs), so the nominal rib cell is the rib a part gets
    /// with no offset, whatever the process does to the wall.
    #[default]
    Cross,
    /// [`PocketSet::Cross`] plus a line with the diagonal of the cross (the
    /// i-th OD with the i-th rib depth, loose to tight), leaving out the
    /// pockets the cross already has. Needs a larger bed than 180 x 180 mm.
    CrossAndDiagonal,
    /// Every OD with every rib depth: one line per rib depth.
    Full,
}

impl PocketSet {
    /// The name used in JSON: `cross`, `crossAndDiagonal` or `full`.
    pub fn name(self) -> &'static str {
        match self {
            PocketSet::Cross => "cross",
            PocketSet::CrossAndDiagonal => "crossAndDiagonal",
            PocketSet::Full => "full",
        }
    }
}

/// How the copies of the set are placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arrangement {
    /// One copy behind the other (along y).
    Stacked,
    /// Side by side (along x), when stacked copies do not fit the bed.
    SideBySide,
}

/// Coupon settings. There is deliberately no fit profile here: the coupon
/// measures the printer and is never compensated (see the module docs).
///
/// In JSON every field is optional and takes its default when left out (so
/// `{"pockets": "full"}` is the default coupon with the full cross), but an
/// unknown field, such as `fit`, is refused.
///
/// ```compile_fail
/// // A fit profile cannot be passed to the coupon.
/// let _ = openbrick_part::coupon::CouponOptions {
///     fit: Some(openbrick_print::example_fit_profile()),
///     ..Default::default()
/// };
/// ```
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct CouponOptions {
    pub pockets: PocketSet,
    /// Identical copies of the whole set: two (docs/plan/05 section 4, for
    /// repeatability).
    pub copies: u32,
    /// The bed the coupon must fit, x and y in mm. Default 180 x 180, the
    /// smallest common FDM bed (Prusa MINI+, Bambu Lab A1 mini).
    pub bed_mm: [f64; 2],
    /// Clearance kept free at every bed edge, mm (skirt, clips); default 5.
    pub bed_margin_mm: f64,
    /// Label relief before the process limits apply, mm (plan: 0.6).
    pub label_relief_mm: f64,
    /// Label stroke before the process minimum feature applies, mm (plan: 0.8).
    pub label_stroke_mm: f64,
    /// Mean extrusion speed assumed by the print-time estimate, mm/s. Default
    /// 50: an assumption for i3-class printers on default profiles, averaged
    /// over slow perimeters and fast infill; CoreXY printers are two to four
    /// times faster. It only ranks the pocket sets; the slicer has the real time.
    pub estimate_speed_mm_s: f64,
}

impl Default for CouponOptions {
    fn default() -> Self {
        let plan = defaults_from_plan();
        CouponOptions {
            pockets: PocketSet::default(),
            copies: plan.copies,
            bed_mm: [180.0, 180.0],
            bed_margin_mm: 5.0,
            label_relief_mm: plan.label_relief_mm,
            label_stroke_mm: plan.label_stroke_mm,
            estimate_speed_mm_s: 50.0,
        }
    }
}

/// A second swept value a cell carries (the rib depth of a pocket).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellValue {
    /// The `chosenCells` key, for example `pocketRib`.
    pub cell_id: String,
    pub value_mm: f64,
}

/// Where one cell is and what it measures: what the wizard shows and records
/// (tapping a cell records `cell_id = value_mm`, plus `cross` when present).
/// The fit offset is the value minus the standard value of its sweep's
/// `standard_id`, on every process: a part built with that offset reproduces
/// the cell (see the module docs).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponCellLayout {
    /// Line on the coupon, 0 at the back (largest y), counting through both
    /// copies; line `r` of the second copy is `r + rows_per_copy`.
    pub row: usize,
    /// Which copy of the set, 0 or 1.
    pub copy: usize,
    /// The `chosenCells` key of the value: stud, tube or pocketTubeOD.
    pub cell_id: String,
    pub value_mm: f64,
    /// The crossed value of a pocket cell (its rib depth, `pocketRib`).
    pub cross: Option<CellValue>,
    /// The embossed text, lines separated by `\n` (values in mm, two decimals).
    pub label: String,
    /// Cell centre in the kernel frame (base minimum corner at the origin), mm.
    pub center_mm: [f64; 2],
    /// The same centre in whole LDU.
    pub center_ldu: [i32; 2],
}

/// Sizes and settings of a coupon, known before any geometry is built.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponSummary {
    /// Version of the coupon data (`calibration.couponVersion` in a fit profile).
    pub coupon_version: u32,
    pub coupon_revision: u32,
    pub process: String,
    pub pockets: PocketSet,
    pub copies: u32,
    pub arrangement: Arrangement,
    /// Lines in one copy of the set.
    pub rows_per_copy: usize,
    /// Base outline in whole LDU and in mm (x, y).
    pub footprint_ldu: [i32; 2],
    pub footprint_mm: [f64; 2],
    /// Tallest point above the bed, mm.
    pub height_mm: f64,
    /// Base thickness, mm.
    pub base_mm: f64,
    /// Height of the bore tubes and pocket walls above the base, mm.
    pub cell_depth_mm: f64,
    pub label_stroke_mm: f64,
    pub label_relief_mm: f64,
    /// Ids in the coupon data that this generator does not build yet.
    pub unbuilt_cells: Vec<String>,
}

/// The cell layout without geometry (for a wizard map or a size check).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponLayout {
    pub cells: Vec<CouponCellLayout>,
    pub summary: CouponSummary,
}

/// Print estimate from the solid volume.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintEstimate {
    pub volume_mm3: f64,
    /// Mass in PLA ([`PLA_DENSITY_G_CM3`]); the coupon is thin enough that a
    /// slicer prints nearly all of it solid, so this is close to the filament used.
    pub mass_g_pla: f64,
    /// Line width x the thickest layer of the process x the assumed speed, mm3/s.
    pub flow_mm3_s: f64,
    /// Volume over flow, seconds. A model for comparing pocket sets on
    /// extrusion processes, not a slicer estimate.
    pub time_s: f64,
}

/// A built coupon.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coupon {
    /// One `body` object, the measurements, warnings, kernel id and content
    /// hash; the coupon has no connectors.
    pub built: Built,
    pub cells: Vec<CouponCellLayout>,
    pub summary: CouponSummary,
    pub estimate: PrintEstimate,
}

// ---------------------------------------------------------------------------
// Rows.
// ---------------------------------------------------------------------------

/// How one kind of cell is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellKind {
    Stud,
    Bore,
    Pocket,
}

/// One row of the plan table.
struct RowDef {
    /// The sweeps a cell of this row reads, from the coupon data.
    sweeps: &'static [&'static str],
    kind: CellKind,
    /// Side of the genuine piece the cell takes, in studs: its footprint is
    /// kept clear around the cell centre.
    footprint_studs: u32,
}

/// The rows built, in the order of the plan table.
const ROWS: &[RowDef] = &[
    RowDef {
        sweeps: &["stud"],
        kind: CellKind::Stud,
        footprint_studs: 1,
    },
    RowDef {
        sweeps: &["tube"],
        kind: CellKind::Bore,
        footprint_studs: 1,
    },
    RowDef {
        sweeps: &["pocketTubeOD", "pocketRib"],
        kind: CellKind::Pocket,
        footprint_studs: 2,
    },
];

/// One swept value of a cell and the nominal it is measured against.
#[derive(Clone, Debug)]
struct Swept {
    value: CellValue,
    /// The standard value of the sweep's `standard_id`, the same on every
    /// process: the wizard records `value - nominal` as the fit offset.
    nominal_mm: f64,
}

/// One cell to build: the swept values it carries, the primary first.
#[derive(Clone, Debug)]
struct CellPlan {
    values: Vec<Swept>,
}

impl CellPlan {
    fn label(&self) -> String {
        self.values
            .iter()
            .map(|v| format!("{:.2}", v.value.value_mm))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The `i`-th swept value, mm.
    fn value(&self, i: usize) -> Option<f64> {
        self.values.get(i).map(|v| v.value.value_mm)
    }

    /// The fit offset the wizard records for the `i`-th value: value minus
    /// nominal. A cell is built as a part is built with this offset.
    fn offset(&self, i: usize) -> Option<f64> {
        self.values.get(i).map(|v| v.value.value_mm - v.nominal_mm)
    }
}

/// One line of cells of one kind.
struct Line {
    kind: CellKind,
    footprint_studs: u32,
    cells: Vec<CellPlan>,
}

fn fail(message: impl Into<String>) -> EvalError {
    EvalError::Feature {
        feature: "coupon".into(),
        message: message.into(),
    }
}

fn kernel_err(what: impl Into<String>) -> impl FnOnce(GeomError) -> EvalError {
    let what = what.into();
    move |source| EvalError::Kernel { what, source }
}

fn mm(id: &str) -> f64 {
    STANDARD.get().mm(id)
}

/// Index of the value nearest `x` (the first on a tie).
fn nearest(values: &[f64], x: f64) -> usize {
    let mut best = 0;
    for (i, v) in values.iter().enumerate() {
        if (v - x).abs() < (values[best] - x).abs() - 1e-12 {
            best = i;
        }
    }
    best
}

fn sweep_values(cell: &CouponCell) -> Result<Vec<f64>, EvalError> {
    let values = cell.values();
    if values.is_empty() || values.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
        return Err(fail(format!(
            "coupon cell `{}` sweeps {} to {} mm in steps of {} mm, which gives no positive values",
            cell.id, cell.start_mm, cell.end_mm, cell.step_mm
        )));
    }
    Ok(values)
}

/// The standard value of a sweep's `standard_id`.
fn nominal_of(cell: &CouponCell) -> Result<f64, EvalError> {
    STANDARD
        .get()
        .feature(&cell.standard_id)
        .and_then(|f| f.nominal_mm)
        .ok_or_else(|| {
            fail(format!(
                "coupon cell `{}` names `{}`, which has no nominal in standards/dimensions-v1.json",
                cell.id, cell.standard_id
            ))
        })
}

/// The lines of one copy, in plan order, and the ids of the coupon data that no
/// row reads.
fn plan_lines(
    spec: &CouponSpec,
    pockets: PocketSet,
) -> Result<(Vec<Line>, Vec<String>), EvalError> {
    let mut lines = Vec::new();
    let mut used = BTreeSet::new();
    for row in ROWS {
        let mut sweeps = Vec::new();
        for id in row.sweeps {
            let cell = spec
                .cells
                .iter()
                .find(|c| c.id == *id)
                .ok_or_else(|| fail(format!("the coupon data has no `{id}` sweep")))?;
            used.insert(cell.id.clone());
            sweeps.push((cell, sweep_values(cell)?, nominal_of(cell)?));
        }
        let value = |cell: &CouponCell, v: f64, nominal_mm: f64| Swept {
            value: CellValue {
                cell_id: cell.id.clone(),
                value_mm: v,
            },
            nominal_mm,
        };
        let new_line = |cells: Vec<CellPlan>| Line {
            kind: row.kind,
            footprint_studs: row.footprint_studs,
            cells,
        };
        match row.kind {
            CellKind::Stud | CellKind::Bore => {
                let (cell, values, nominal) = &sweeps[0];
                lines.push(new_line(
                    values
                        .iter()
                        .map(|&v| CellPlan {
                            values: vec![value(cell, v, *nominal)],
                        })
                        .collect(),
                ));
            }
            CellKind::Pocket => {
                let (od_cell, ods, od_nominal) = &sweeps[0];
                let (rib_cell, ribs, rib_nominal) = &sweeps[1];
                // The nominal pocket is the one a part gets with no offsets, on
                // every process (rib values are measured against the standard
                // side wall; build_cell deepens the rib on a thinner wall).
                let io = nearest(ods, *od_nominal);
                let ir = nearest(ribs, *rib_nominal);
                let pocket = |o: usize, r: usize| CellPlan {
                    values: vec![
                        value(od_cell, ods[o], *od_nominal),
                        value(rib_cell, ribs[r], *rib_nominal),
                    ],
                };
                match pockets {
                    PocketSet::Full => {
                        for r in 0..ribs.len() {
                            lines.push(new_line((0..ods.len()).map(|o| pocket(o, r)).collect()));
                        }
                    }
                    PocketSet::Cross | PocketSet::CrossAndDiagonal => {
                        lines.push(new_line((0..ods.len()).map(|o| pocket(o, ir)).collect()));
                        lines.push(new_line((0..ribs.len()).map(|r| pocket(io, r)).collect()));
                        if pockets == PocketSet::CrossAndDiagonal {
                            let diagonal: Vec<CellPlan> = (0..ods.len().min(ribs.len()))
                                .filter(|&i| i != io && i != ir)
                                .map(|i| pocket(i, i))
                                .collect();
                            if !diagonal.is_empty() {
                                lines.push(new_line(diagonal));
                            }
                        }
                    }
                }
            }
        }
    }
    let unbuilt = spec
        .cells
        .iter()
        .filter(|c| !used.contains(&c.id))
        .map(|c| c.id.clone())
        .collect();
    Ok((lines, unbuilt))
}

// ---------------------------------------------------------------------------
// Layout.
// ---------------------------------------------------------------------------

/// Millimetres to whole LDU, rounding up (the slack keeps 8.8 mm at 22 LDU).
fn ldu_up(mm: f64) -> i32 {
    (mm / LDU_MM - 1e-9).ceil() as i32
}

/// [`ldu_up`] rounded up to an even number, so its half is whole LDU.
fn ldu_up_even(mm: f64) -> i32 {
    let n = ldu_up(mm);
    n + n.rem_euclid(2)
}

/// The settings every part of the layout shares.
struct Style {
    stroke: f64,
    relief: f64,
    /// Clearance between a cell's reserved footprint and its label, between
    /// lines and copies, and at the base edge: one stroke.
    gap: f64,
}

/// A line placed in its copy: per-cell labels and the line's metrics.
struct PlacedLine {
    line: Line,
    labels: Vec<(String, font::Block)>,
    /// Half the clear footprint around each cell centre, mm.
    reserve: f64,
    pitch_ldu: i32,
    extent_ldu: i32,
    above_ldu: i32,
    below_ldu: i32,
}

impl PlacedLine {
    fn width_ldu(&self) -> i32 {
        (self.line.cells.len() as i32 - 1) * self.pitch_ldu + self.extent_ldu
    }
}

/// The settled layout, with what geometry needs.
struct Plan {
    layout: CouponLayout,
    lines: Vec<PlacedLine>,
    style: Style,
    base: f64,
    depth: f64,
    warnings: Vec<String>,
}

fn place_line(line: Line, style: &Style) -> Result<PlacedLine, EvalError> {
    let s = style.stroke;
    let (module, gap_std) = (mm("module"), mm("undersize_per_side"));
    let reserve = (f64::from(line.footprint_studs) * module - 2.0 * gap_std) / 2.0;
    let mut labels = Vec::with_capacity(line.cells.len());
    let mut seen = BTreeSet::new();
    for cell in &line.cells {
        let text = cell.label();
        let block = font::layout(&text).map_err(fail)?;
        if !seen.insert(text.clone()) {
            return Err(fail(format!(
                "two cells of one line would both be labelled {text:?}; the coupon labels values to 0.01 mm, so sweep steps must be at least that"
            )));
        }
        labels.push((text, block));
    }
    let label_w = labels
        .iter()
        .map(|(_, b)| b.width as f64 * s)
        .fold(0.0, f64::max);
    let label_h = labels
        .iter()
        .map(|(_, b)| b.height as f64 * s)
        .fold(0.0, f64::max);
    let wide = label_w.max(2.0 * reserve);
    // Neighbouring cells keep a gap between their footprints and a word space
    // between their labels.
    let pitch = (2.0 * reserve + style.gap).max(label_w + font::WORD_SPACE as f64 * s);
    Ok(PlacedLine {
        line,
        labels,
        reserve,
        pitch_ldu: ldu_up_even(pitch),
        extent_ldu: ldu_up_even(wide),
        above_ldu: ldu_up(reserve),
        below_ldu: ldu_up(reserve + style.gap + label_h),
    })
}

fn plan_coupon(process: &ProcessProfile, opts: &CouponOptions) -> Result<Plan, EvalError> {
    let finite_positive = |x: f64| x.is_finite() && x > 0.0;
    if opts.copies == 0 || opts.copies > MAX_COPIES {
        return Err(fail(format!(
            "copies must be 1 to {MAX_COPIES} (the plan prints two), not {}",
            opts.copies
        )));
    }
    if !(finite_positive(opts.bed_mm[0]) && finite_positive(opts.bed_mm[1])) {
        return Err(fail(format!(
            "bed size {:?} mm must be positive",
            opts.bed_mm
        )));
    }
    if !(opts.bed_margin_mm.is_finite() && opts.bed_margin_mm >= 0.0) {
        return Err(fail("the bed margin must be zero or more"));
    }
    if !(finite_positive(opts.label_relief_mm)
        && finite_positive(opts.label_stroke_mm)
        && finite_positive(opts.estimate_speed_mm_s))
    {
        return Err(fail(
            "label relief, label stroke and the estimate speed must be positive",
        ));
    }
    // Sanity bounds that keep the layout arithmetic small: a label pixel wider
    // than a stud pitch, or a relief taller than a plate, fits no cell.
    if opts.label_stroke_mm > mm("module") || opts.label_relief_mm > mm("plate_height") {
        return Err(fail(format!(
            "a {} mm label stroke or {} mm relief is larger than any coupon cell (at most {} and {} mm)",
            opts.label_stroke_mm,
            opts.label_relief_mm,
            mm("module"),
            mm("plate_height")
        )));
    }
    let mut warnings = Vec::new();
    let stroke = opts.label_stroke_mm.max(process.min_feature_mm);
    if stroke > opts.label_stroke_mm {
        warnings.push(format!(
            "coupon: label stroke raised from {} mm to the process minimum feature {} mm (rule FEATURE.MIN)",
            opts.label_stroke_mm, process.min_feature_mm
        ));
    }
    let relief = opts.label_relief_mm.max(process.min_relief_height_mm).min(
        process
            .max_relief_height_mm
            .max(process.min_relief_height_mm),
    );
    if relief != opts.label_relief_mm {
        warnings.push(format!(
            "coupon: label relief {} mm moved to {relief} mm, inside the process limits {} to {} mm (rule RELIEF.HEIGHT)",
            opts.label_relief_mm, process.min_relief_height_mm, process.max_relief_height_mm
        ));
    }
    let style = Style {
        stroke,
        relief,
        gap: stroke,
    };

    // Pocket and bore cells are a plate cavity tall on a base as thick as a
    // plate's roof, so every such cell is exactly plate height.
    let base = roof_mm(process);
    let depth = mm("plate_height") - base;
    if depth < process.min_cavity_depth_mm {
        return Err(fail(format!(
            "a plate leaves {depth:.3} mm of cavity under a {base} mm roof, less than the {} mm a stud needs (rule CAVITY.DEPTH)",
            process.min_cavity_depth_mm
        )));
    }
    let chamfer = process.first_layer_chamfer_mm.max(0.0);
    if chamfer >= base {
        return Err(fail(format!(
            "the {chamfer} mm first-layer chamfer would cut through the {base} mm base"
        )));
    }

    let spec = coupon_spec();
    let (lines, unbuilt) = plan_lines(spec, opts.pockets)?;
    let lines = lines
        .into_iter()
        .map(|l| place_line(l, &style))
        .collect::<Result<Vec<_>, _>>()?;
    let gap = ldu_up(style.gap);

    // One copy: lines from the back, each centred across the widest line.
    let block_w = lines.iter().map(PlacedLine::width_ldu).max().unwrap_or(0);
    let mut centre_y = Vec::with_capacity(lines.len());
    let mut y = 0;
    for (k, l) in lines.iter().enumerate() {
        y += if k == 0 {
            l.above_ldu
        } else {
            lines[k - 1].below_ldu + gap + l.above_ldu
        };
        centre_y.push(y);
    }
    let block_h = y + lines.last().map_or(0, |l| l.below_ldu);

    // Copies stacked if that fits the bed, else side by side.
    let n = opts.copies as i32;
    let fits = |w: i32, d: i32| {
        let (w, d) = (ldu_to_mm(w), ldu_to_mm(d));
        let [bx, by] = opts.bed_mm.map(|b| b - 2.0 * opts.bed_margin_mm);
        (w <= bx && d <= by) || (d <= bx && w <= by)
    };
    let edge = 2 * gap;
    let stacked = [block_w + edge, n * block_h + (n - 1) * gap + edge];
    let side = [n * block_w + (n - 1) * gap + edge, block_h + edge];
    let (arrangement, footprint) = if fits(stacked[0], stacked[1]) {
        (Arrangement::Stacked, stacked)
    } else if fits(side[0], side[1]) {
        (Arrangement::SideBySide, side)
    } else {
        return Err(fail(format!(
            "the coupon ({} pockets, {} copies) is {:.1} x {:.1} mm stacked or {:.1} x {:.1} mm side by side, which does not fit a {} x {} mm bed with {} mm margins (rule SIZE.BED); use the default pocket set, fewer copies or a larger bed",
            opts.pockets.name(),
            opts.copies,
            ldu_to_mm(stacked[0]),
            ldu_to_mm(stacked[1]),
            ldu_to_mm(side[0]),
            ldu_to_mm(side[1]),
            opts.bed_mm[0],
            opts.bed_mm[1],
            opts.bed_margin_mm
        )));
    };

    let mut cells = Vec::new();
    for copy in 0..opts.copies as usize {
        let (dx, dy) = match arrangement {
            Arrangement::Stacked => (0, copy as i32 * (block_h + gap)),
            Arrangement::SideBySide => (copy as i32 * (block_w + gap), 0),
        };
        for (k, l) in lines.iter().enumerate() {
            let left = edge / 2 + dx + (block_w - l.width_ldu()) / 2 + l.extent_ldu / 2;
            let cy = footprint[1] - edge / 2 - dy - centre_y[k];
            for (i, (cell, (label, _))) in l.line.cells.iter().zip(&l.labels).enumerate() {
                let centre = [left + i as i32 * l.pitch_ldu, cy];
                let mut values = cell.values.iter().map(|v| &v.value);
                let first = values
                    .next()
                    .ok_or_else(|| fail("a cell carries no value"))?;
                cells.push(CouponCellLayout {
                    row: copy * lines.len() + k,
                    copy,
                    cell_id: first.cell_id.clone(),
                    value_mm: first.value_mm,
                    cross: values.next().cloned(),
                    label: label.clone(),
                    center_mm: centre.map(ldu_to_mm),
                    center_ldu: centre,
                });
            }
        }
    }

    // Studs rise a stud height, bore tubes and pocket walls a plate cavity.
    let mut rise = style.relief;
    for l in &lines {
        rise = rise.max(match l.line.kind {
            CellKind::Stud => mm("stud_height"),
            CellKind::Bore | CellKind::Pocket => depth,
        });
    }
    let tallest = base + rise;
    let summary = CouponSummary {
        coupon_version: spec.version,
        coupon_revision: COUPON_REVISION,
        process: process.id.clone(),
        pockets: opts.pockets,
        copies: opts.copies,
        arrangement,
        rows_per_copy: lines.len(),
        footprint_ldu: footprint,
        footprint_mm: footprint.map(ldu_to_mm),
        height_mm: tallest,
        base_mm: base,
        cell_depth_mm: depth,
        label_stroke_mm: style.stroke,
        label_relief_mm: style.relief,
        unbuilt_cells: unbuilt,
    };
    Ok(Plan {
        layout: CouponLayout { cells, summary },
        lines,
        style,
        base,
        depth,
        warnings,
    })
}

/// The cell layout and sizes of a coupon, without building geometry: for the
/// wizard's map of the coupon, or to check that a pocket set fits a bed.
pub fn coupon_layout(
    process: &ProcessProfile,
    opts: &CouponOptions,
) -> Result<CouponLayout, EvalError> {
    Ok(plan_coupon(process, opts)?.layout)
}

// ---------------------------------------------------------------------------
// Geometry.
// ---------------------------------------------------------------------------

/// What the measurements report about the cells.
#[derive(Default)]
struct CellStats {
    min_tube_wall: Option<f64>,
    pocket_wall: Option<f64>,
}

impl CellStats {
    fn tube_wall(&mut self, w: f64) {
        self.min_tube_wall = Some(self.min_tube_wall.map_or(w, |m: f64| m.min(w)));
    }
}

/// The solids of one cell, standing on the base top at `z`.
fn build_cell<K: Kernel>(
    g: &Geo<K>,
    process: &ProcessProfile,
    plan: &Plan,
    line: &PlacedLine,
    cell: &CellPlan,
    centre: [f64; 2],
    stats: &mut CellStats,
) -> Result<Vec<K::Solid>, EvalError> {
    let (k, z, depth) = (g.k, plan.base, plan.depth);
    let segments = process.stud_segments();
    let value = |i: usize| cell.value(i);
    let lift = |solids: Vec<K::Solid>| -> Vec<K::Solid> {
        solids
            .iter()
            .map(|s| k.translate(s, [0.0, 0.0, z]))
            .collect()
    };
    let label = cell.label().replace('\n', " / ");
    match line.line.kind {
        CellKind::Stud => {
            let d = value(0).ok_or_else(|| fail("a stud cell needs a diameter"))?;
            if d / 2.0 >= line.reserve {
                return Err(fail(format!(
                    "a {d} mm stud does not fit its {} mm cell",
                    2.0 * line.reserve
                )));
            }
            // The same stud as stud.field builds: standard height, process chamfer.
            let shape = stud_shape(process, d);
            StudGeom {
                centres: vec![centre],
                base: z,
                diameter: d,
                height: shape.height,
                top_chamfer: shape.top_chamfer,
                segments,
            }
            .build(g)
            .map_err(kernel_err(format!("the stud cell {label}")))
        }
        CellKind::Bore => {
            let (Some(bore), Some(offset)) = (value(0), cell.offset(0)) else {
                return Err(fail("a bore cell needs a diameter"));
            };
            let (od, id) = (mm("tube_outer_diameter"), mm("tube_inner_diameter"));
            // The tube a part gets with the tubeInnerDiameter offset the wizard
            // records for this cell (bore - nominal): Offsets::tube grows the
            // outer diameter so the wall keeps minTubeWallMm. No fit profile is
            // involved.
            let tube = Offsets {
                tube_inner_diameter: offset,
                ..Offsets::default()
            }
            .tube(od, id, process.min_tube_wall_mm);
            if tube.outer_mm / 2.0 >= line.reserve {
                return Err(fail(format!(
                    "the {:.3} mm tube around a {bore} mm bore does not fit its {} mm cell",
                    tube.outer_mm,
                    2.0 * line.reserve
                )));
            }
            stats.tube_wall(tube.wall_mm());
            let solids = TubeGeom {
                tubes: vec![centre],
                tube_outer: tube.outer_mm,
                tube_inner: tube.inner_mm,
                // The open end is at the top, away from the bed: no elephant foot.
                tube_chamfer: 0.0,
                pins: Vec::new(),
                pin_diameter: 0.0,
                pin_chamfer: 0.0,
                ribs: Vec::new(),
                rib_bottom: 0.0,
                top: depth,
                segments,
            }
            .build(g)
            .map_err(kernel_err(format!("the bore cell {label}")))?;
            Ok(lift(solids))
        }
        CellKind::Pocket => {
            let od = value(0).ok_or_else(|| fail("a pocket cell needs a tube OD"))?;
            let rib_offset = cell
                .offset(1)
                .ok_or_else(|| fail("a pocket cell needs a rib depth"))?;
            let (module, undersize) = (mm("module"), mm("undersize_per_side"));
            let side = 2.0 * module - 2.0 * undersize;
            let wall = process.quantise_wall(mm("side_wall"));
            let half_inner = side / 2.0 - wall;
            // The rib a part gets on this process with the ribDepth offset the
            // wizard records for this cell (value - nominal), as the evaluator
            // builds it: the rib tip sits side_wall + value from the outer face,
            // so a wall quantised thinner gets a deeper rib.
            let rib = nominal_rib_depth(wall) + rib_offset;
            if rib < MIN_LENGTH_MM {
                return Err(fail(format!(
                    "pocket {label}: on this process's {wall} mm wall the rib would be {rib:.3} mm deep (its tip {:.3} mm from the outer face, inside the wall), so it cannot be built (rule WALL.QUANTISED); start the rib sweep at least {:.3} mm higher",
                    mm("side_wall") + value(1).unwrap_or(0.0),
                    MIN_LENGTH_MM - rib
                )));
            }
            // The tube keeps the standard bore unless its wall would get thinner
            // than minTubeWallMm; the outer diameter is what the cell measures.
            let id = mm("tube_inner_diameter").min(od - 2.0 * process.min_tube_wall_mm);
            if id <= 0.0 || od / 2.0 >= half_inner - rib {
                return Err(fail(format!(
                    "pocket {label}: a {od} mm tube with {rib:.3} mm ribs does not fit a {side} mm pocket with {wall} mm walls"
                )));
            }
            stats.tube_wall((od - id) / 2.0);
            stats.pocket_wall = Some(wall);
            let min = [centre[0] - side / 2.0, centre[1] - side / 2.0];
            let outer = Polygon::rect(min, [side, side]);
            let inner = Polygon::rect(
                [min[0] + wall, min[1] + wall],
                [side - 2.0 * wall, side - 2.0 * wall],
            )
            .reversed();
            let walls = k
                .extrude(&CrossSection::from(vec![outer, inner]), depth, 1.0)
                .map_err(kernel_err(format!("the walls of pocket {label}")))?;
            // Ribs where tube.field puts them: each wall at each stud centre line.
            let lines: Vec<f64> = (0..2)
                .map(|i| f64::from(i) * module + module / 2.0 - undersize)
                .collect();
            let ribs = geometry::wall_ribs(
                [side, side],
                wall,
                rib,
                process.min_feature_mm,
                &lines,
                &lines,
            )
            .into_iter()
            .map(|r| geometry::Rib {
                min: [r.min[0] + min[0], r.min[1] + min[1]],
                max: [r.max[0] + min[0], r.max[1] + min[1]],
            })
            .collect();
            let mut solids = vec![walls];
            solids.extend(
                TubeGeom {
                    tubes: vec![centre],
                    tube_outer: od,
                    tube_inner: id,
                    tube_chamfer: 0.0,
                    pins: Vec::new(),
                    pin_diameter: 0.0,
                    pin_chamfer: 0.0,
                    ribs,
                    rib_bottom: 0.0,
                    top: depth,
                    segments,
                }
                .build(g)
                .map_err(kernel_err(format!("the tube and ribs of pocket {label}")))?,
            );
            Ok(lift(solids))
        }
    }
}

/// The pixels of one label as rectangles: the block centred below the cell at
/// `centre`, one gap below the cell's clear footprint of half-size `reserve`.
fn push_label(
    polygons: &mut Vec<Polygon>,
    block: &font::Block,
    centre: [f64; 2],
    reserve: f64,
    style: &Style,
) {
    let s = style.stroke;
    let left = centre[0] - block.width as f64 * s / 2.0;
    let top = centre[1] - reserve - style.gap;
    for run in &block.runs {
        let x0 = left + run.col as f64 * s;
        let y1 = top - run.row as f64 * s;
        polygons.push(Polygon::rect([x0, y1 - s], [run.len as f64 * s, s]));
    }
}

/// SHA-256 (hex) of the canonical JSON of everything that decides the coupon's
/// geometry: the generator and evaluator revisions, the standard version, the
/// kernel id, the process profile, the options (without the estimate's speed)
/// and the coupon data.
fn content_hash(kernel_id: &str, process: &ProcessProfile, opts: &CouponOptions) -> String {
    let mut options = serde_json::to_value(opts).unwrap_or(serde_json::Value::Null);
    if let Some(o) = options.as_object_mut() {
        o.remove("estimateSpeedMmS");
    }
    let content = json!({
        "coupon": COUPON_REVISION,
        "evaluator": EVALUATOR_REVISION,
        "standard": STANDARD.get().version,
        "kernel": kernel_id,
        "process": process.id,
        "processProfile": serde_json::to_value(process).unwrap_or(serde_json::Value::Null),
        "options": options,
        "couponData": serde_json::to_value(coupon_spec()).unwrap_or(serde_json::Value::Null),
    });
    sha256_hex(canonical_json(&content).as_bytes())
}

/// Build the calibration coupon for `process` (docs/plan/05 section 4). Fit
/// offsets are never applied: there is no fit profile to pass (see the module
/// docs). Fails with [`EvalError::Feature`] (feature `coupon`) when the options
/// are out of range or the coupon does not fit the bed, and with
/// [`EvalError::Kernel`] when the kernel fails.
pub fn build_coupon<K: Kernel>(
    kernel: &K,
    process: &ProcessProfile,
    opts: &CouponOptions,
) -> Result<Coupon, EvalError> {
    let plan = plan_coupon(process, opts)?;
    // No fit offsets: the horizontal scale is 1 and every size is nominal.
    let g = Geo { k: kernel, xy: 1.0 };
    let summary = &plan.layout.summary;
    let chamfer = process.first_layer_chamfer_mm.max(0.0);

    let base = g
        .chamfered_box(summary.footprint_mm, plan.base, chamfer)
        .map_err(kernel_err("the coupon base"))?;
    let mut features = Vec::new();
    let mut label_polygons = Vec::new();
    let mut stats = CellStats::default();
    // The layout lists cells copy by copy, line by line, as built here.
    let mut placed = plan.layout.cells.iter();
    for _ in 0..summary.copies {
        for line in &plan.lines {
            for (cell, (_, block)) in line.line.cells.iter().zip(&line.labels) {
                let at = placed
                    .next()
                    .ok_or_else(|| fail("the layout lost a cell"))?
                    .center_mm;
                features.extend(build_cell(&g, process, &plan, line, cell, at, &mut stats)?);
                push_label(&mut label_polygons, block, at, line.reserve, &plan.style);
            }
        }
    }
    let labels = kernel
        .extrude(&CrossSection::from(label_polygons), plan.style.relief, 1.0)
        .map_err(kernel_err("the labels"))?;
    features.push(kernel.translate(&labels, [0.0, 0.0, plan.base]));

    let feature_volume_sum =
        base.volume_mm3() + features.iter().map(|s| s.volume_mm3()).sum::<f64>();
    let solid = geometry::union_onto(kernel, Some(base), features)
        .map_err(kernel_err("the union of the coupon"))?;
    let mesh = solid.to_mesh();
    let volume = solid.volume_mm3();
    let (lo, hi) = mesh.bounds();

    // Fields left at their defaults: no fit profile, any process, and no single
    // stud, tube, pin or rib size (the coupon sweeps them; each cell's value is
    // in the layout), no connectors.
    let measurements = Measurements {
        process: process.id.clone(),
        triangle_count: mesh.triangle_count(),
        volume_mm3: volume,
        feature_volume_sum_mm3: feature_volume_sum,
        bounds_min_mm: lo.map(f64::from),
        bounds_max_mm: hi.map(f64::from),
        is_manifold: solid.is_manifold(),
        genus: solid.genus(),
        components: solid.component_count(),
        body_height_mm: Some(plan.base + plan.depth),
        // The pocket walls stand on the base, away from the bed: no chamfer.
        min_body_wall_mm: stats.pocket_wall,
        min_body_wall_at_bed_mm: stats.pocket_wall,
        min_tube_wall_mm: stats.min_tube_wall,
        min_tube_wall_at_bed_mm: stats.min_tube_wall,
        first_layer_chamfer_mm: Some(chamfer),
        roof_mm: Some(plan.base),
        cavity_depth_mm: Some(plan.depth),
        // The labels are the coupon's relief: every pixel is one stroke wide.
        min_stroke_mm: Some(plan.style.stroke),
        relief_height_mm: Some(plan.style.relief),
        ..Measurements::default()
    };

    let content_hash = content_hash(&kernel.id(), process, opts);
    let layer = process.layer_height_mm.iter().copied().fold(0.0, f64::max);
    let flow = process.line_width_mm * layer * opts.estimate_speed_mm_s;
    let estimate = PrintEstimate {
        volume_mm3: volume,
        mass_g_pla: volume / 1000.0 * PLA_DENSITY_G_CM3,
        flow_mm3_s: flow,
        time_s: if flow > 0.0 {
            volume / flow
        } else {
            f64::INFINITY
        },
    };

    let Plan {
        layout, warnings, ..
    } = plan;
    Ok(Coupon {
        built: Built {
            objects: vec![RoleMesh {
                role: "body".into(),
                mesh,
                volume_mm3: volume,
            }],
            connectors: Vec::new(),
            measurements,
            warnings,
            kernel_id: kernel.id(),
            content_hash,
        },
        cells: layout.cells,
        summary: layout.summary,
        estimate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_run_from_start_to_end_in_steps() {
        // tests/coupon.rs checks the values against docs/plan/05 itself.
        for c in &coupon_spec().cells {
            let v = c.values();
            assert!(v.len() > 1, "{}", c.id);
            assert_eq!(v[0], c.start_mm, "{}", c.id);
            assert_eq!(
                v[v.len() - 1],
                c.end_mm,
                "{}: lands on the end exactly",
                c.id
            );
            assert!(v.windows(2).all(|w| (w[1] - w[0] - c.step_mm).abs() < 1e-9));
        }
        // Every row the generator builds finds its sweeps.
        for row in ROWS {
            for id in row.sweeps {
                assert!(coupon_spec().cells.iter().any(|c| c.id == *id), "{id}");
            }
        }
    }

    #[test]
    fn content_hash_follows_the_geometry_inputs_only() {
        let p = openbrick_print::default_process_profiles();
        let d = CouponOptions::default();
        let base = content_hash("k", &p[0], &d);
        let faster = CouponOptions {
            estimate_speed_mm_s: 2.0 * d.estimate_speed_mm_s,
            ..d.clone()
        };
        assert_eq!(content_hash("k", &p[0], &faster), base);
        let full = CouponOptions {
            pockets: PocketSet::Full,
            ..d.clone()
        };
        assert_ne!(content_hash("k", &p[0], &full), base);
        assert_ne!(content_hash("k", &p[1], &d), base);
        assert_ne!(content_hash("other kernel", &p[0], &d), base);
    }

    #[test]
    fn every_standard_id_names_a_dimension() {
        for c in &coupon_spec().cells {
            assert!(
                STANDARD.get().feature(&c.standard_id).is_some(),
                "{}: {}",
                c.id,
                c.standard_id
            );
        }
    }

    #[test]
    fn empty_and_backward_sweeps_have_no_values() {
        let mut c = cells_from_plan().remove(0);
        c.step_mm = 0.0;
        assert!(c.values().is_empty());
        c.step_mm = 0.05;
        c.end_mm = c.start_mm - 1.0;
        assert!(c.values().is_empty());
        assert!(sweep_values(&c).is_err());
    }

    #[test]
    fn ldu_rounding_is_up_and_even_where_asked() {
        assert_eq!(ldu_up(8.8), 22);
        assert_eq!(ldu_up(8.81), 23);
        assert_eq!(ldu_up_even(8.81), 24);
        assert_eq!(ldu_up_even(8.8), 22);
    }

    #[test]
    fn nearest_prefers_the_first_on_a_tie() {
        assert_eq!(nearest(&[1.0, 2.0, 3.0], 2.5), 1);
        assert_eq!(nearest(&[1.0, 2.0, 3.0], 2.6), 2);
    }
}
