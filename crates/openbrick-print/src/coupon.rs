//! The calibration coupon sweep as data (docs/plan/05 section 4):
//! `standards/coupon-v1.json`, read by [`coupon_v1`].
//!
//! The coupon generator prints these cells, [`crate::record`] turns the chosen
//! cells into fit offsets, and rule `CLUTCH.RISK` ([`crate::validate`]) warns when
//! a fit falls outside them, so all three agree on one set of numbers. Every
//! number in the file cites its source (docs/plan/05 section 4 and the
//! `print_target` notes in `standards/dimensions-v1.json`).

use std::collections::BTreeSet;
use std::sync::OnceLock;

use openbrick_units::{Credit, STANDARD};

use crate::Offsets;

/// How close a value must be to a grid value to count as that cell, in
/// millimetres: one nanometre, far below any printer and far above the
/// floating-point residue of `start + i x step` (about 1e-15 mm).
pub const GRID_TOLERANCE_MM: f64 = 1e-6;

/// The whole coupon document.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponSpec {
    pub openbrick: String,
    pub kind: String,
    /// Coupon layout version; a fit profile records it as
    /// `calibration.couponVersion`. Read like that field: `1` and `1.0` are the
    /// same whole number to JSON Schema, so to the reader too.
    #[serde(deserialize_with = "crate::whole_u32")]
    pub version: u32,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<Credit>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Where `copies` and `label` come from.
    pub source: String,
    /// Identical rows of every sweep, for repeatability.
    #[serde(deserialize_with = "crate::whole_u32")]
    pub copies: u32,
    /// The embossed label on each cell.
    pub label: CouponLabel,
    /// The sweeps, in print order.
    pub cells: Vec<CouponCell>,
}

/// The embossed label on each coupon cell.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponLabel {
    pub relief_mm: f64,
    pub stroke_mm: f64,
}

/// One swept dimension of the coupon.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouponCell {
    /// Key of `calibration.chosenCells` (`stud`, `tube`, `pocketTubeOD`, ...).
    pub id: String,
    /// What the cell sweeps, for people.
    pub label: String,
    /// Coupon row the cell is printed in; cells that share a row are printed
    /// together (the pocket row crosses tube outer diameter with rib depth).
    pub row: String,
    /// Row of `standards/dimensions-v1.json` that gives the nominal.
    pub standard_id: String,
    pub start_mm: f64,
    pub end_mm: f64,
    pub step_mm: f64,
    /// The `offsetsMm` key of a fit profile this cell calibrates (camelCase, as
    /// in the JSON: `studDiameter`, ...); `None` for a row printed for
    /// reference only, which [`crate::record`] refuses.
    pub offset_field: Option<String>,
    pub source: String,
}

impl CouponCell {
    /// Number of steps from start to end.
    fn steps(&self) -> usize {
        let n = (self.end_mm - self.start_mm) / self.step_mm;
        // A broken cell (zero or negative step, NaN) has no steps rather than an
        // endless sweep; `CouponSpec::check` reports it.
        if self.step_mm > 0.0 && n.is_finite() {
            n.round().max(0.0) as usize
        } else {
            0
        }
    }

    /// Every value of the sweep, start to end, rounded to six decimals (a
    /// nanometre) so they read back as the decimals of the coupon
    /// (`4.6 + 3 x 0.05` is 4.749999999999999 in binary; this gives 4.75).
    pub fn values(&self) -> Vec<f64> {
        (0..=self.steps())
            .map(|i| round_dp(self.start_mm + i as f64 * self.step_mm, 6))
            .collect()
    }

    /// The nominal from the Dimensional Standard.
    pub fn nominal_mm(&self) -> f64 {
        STANDARD.get().mm(&self.standard_id)
    }

    /// The grid value `value` stands for, if it lies within
    /// [`GRID_TOLERANCE_MM`] of one; `None` when it is outside the sweep or
    /// between two cells.
    pub fn grid_value(&self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        let i = ((value - self.start_mm) / self.step_mm).round();
        if i < 0.0 || i > self.steps() as f64 {
            return None;
        }
        let v = round_dp(self.start_mm + i * self.step_mm, 6);
        ((v - value).abs() <= GRID_TOLERANCE_MM).then_some(v)
    }

    /// Whether `value` lies inside the swept range, within [`GRID_TOLERANCE_MM`]
    /// (on the grid or between cells).
    pub fn covers(&self, value: f64) -> bool {
        value >= self.start_mm - GRID_TOLERANCE_MM && value <= self.end_mm + GRID_TOLERANCE_MM
    }
}

impl CouponSpec {
    /// The cell with this id.
    pub fn cell(&self, id: &str) -> Option<&CouponCell> {
        self.cells.iter().find(|c| c.id == id)
    }

    /// What JSON Schema cannot check: unique ids, nominals that exist, offset
    /// fields that exist and are set by one cell only, a positive step and an
    /// end a whole number of steps after the start. Returns one line per
    /// problem; empty means all good.
    pub fn check(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut fields = BTreeSet::new();
        for c in &self.cells {
            if !seen.insert(c.id.as_str()) {
                out.push(format!("cell id `{}` is used twice", c.id));
            }
            if let Some(field) = &c.offset_field {
                if !fields.insert(field.as_str()) {
                    out.push(format!(
                        "cell `{}`: offset field `{field}` is already set by another cell",
                        c.id
                    ));
                }
            }
            if STANDARD
                .get()
                .feature(&c.standard_id)
                .and_then(|f| f.nominal_mm)
                .is_none()
            {
                out.push(format!(
                    "cell `{}`: standard id `{}` is not a row of standards/dimensions-v1.json with a nominal_mm",
                    c.id, c.standard_id
                ));
            }
            if let Some(field) = &c.offset_field {
                if !Offsets::KEYS.contains(&field.as_str()) {
                    out.push(format!(
                        "cell `{}`: offset field `{field}` is not one of the offsetsMm diameter and depth offsets {:?}",
                        c.id,
                        Offsets::KEYS
                    ));
                }
            }
            let finite = c.start_mm.is_finite() && c.end_mm.is_finite() && c.step_mm.is_finite();
            if !finite || c.step_mm <= 0.0 || c.end_mm < c.start_mm {
                out.push(format!(
                    "cell `{}`: needs a positive step and start <= end, found {} to {} step {}",
                    c.id, c.start_mm, c.end_mm, c.step_mm
                ));
                continue;
            }
            let last = c.start_mm + c.steps() as f64 * c.step_mm;
            if (last - c.end_mm).abs() > GRID_TOLERANCE_MM {
                out.push(format!(
                    "cell `{}`: {} to {} mm is not a whole number of {} mm steps",
                    c.id, c.start_mm, c.end_mm, c.step_mm
                ));
            }
        }
        out
    }
}

/// `x` rounded to `places` decimal places, as the `f64` closest to that decimal
/// (0.136, not 0.13600000000000012). `10^places` is exact for the few places
/// used here, so the division rounds once.
pub(crate) fn round_dp(x: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (x * scale).round() / scale
}

const COUPON_V1: &str = include_str!("../../../standards/coupon-v1.json");

/// The coupon sweep of `standards/coupon-v1.json`, parsed once. Panics if the
/// bundled file is broken; `cargo xtask data` and the tests check that it is not.
pub fn coupon_v1() -> &'static CouponSpec {
    static SPEC: OnceLock<CouponSpec> = OnceLock::new();
    SPEC.get_or_init(|| {
        let spec: CouponSpec =
            serde_json::from_str(COUPON_V1).expect("standards/coupon-v1.json is valid");
        let problems = spec.check();
        assert!(
            problems.is_empty(),
            "standards/coupon-v1.json: {problems:?}"
        );
        spec
    })
}
