//! `openbrick calibrate record` (docs/plan/05 section 4): the coupon cells a
//! person chose become a [`FitProfile`] whose offsets are the chosen cell minus
//! the nominal of its row in the Dimensional Standard.
//!
//! Worked examples from the plan: stud 4.80 - 4.80 = 0.00; tube 4.95 - 4.80 =
//! +0.15; pin 4.70 - 4.80 = -0.10. Which offset each cell sets, and which
//! nominal it is measured from, is data: `offsetField` and `standardId` of the
//! cell in `standards/coupon-v1.json` ([`crate::coupon_v1`]).

use std::collections::BTreeMap;

use openbrick_units::Credit;

use crate::coupon::{coupon_v1, round_dp, CouponSpec};
use crate::{Calibration, FitProfile, Offsets};

/// Offsets and chosen cells are written with this many decimal places
/// (micrometres), so the JSON carries no floating-point noise such as
/// 0.15000000000000036. Every coupon value has at most two decimals and every
/// nominal at most three, so nothing is lost.
pub const RECORD_DECIMALS: i32 = 3;

/// The fit intents a profile may declare (`intent` in
/// `schemas/fit-profile.schema.json`; open question Q05 in docs/plan/13).
pub const FIT_INTENTS: &[&str] = &["fits-genuine-bricks", "fits-itself"];

/// Credit author written when the caller names none.
pub const ANONYMOUS_AUTHOR: &str = "anonymous";

/// What the calibration wizard or `openbrick calibrate record` collected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordInput {
    /// Process profile id the coupon was printed with (`fdm-0.4`).
    pub process: String,
    /// Printer name (`Prusa MK4S`).
    pub printer: String,
    pub nozzle_mm: f64,
    /// Material type (`PETG`); written as `material.type`.
    pub material: String,
    /// Material brand, if known; written as `material.brand`.
    pub brand: Option<String>,
    /// Slicer and version (`PrusaSlicer 2.9.6`); may be empty.
    pub slicer: String,
    /// One of [`FIT_INTENTS`].
    pub intent: String,
    /// Profile id; `None` derives one from printer, material and process
    /// ([`slug_id`]).
    pub id: Option<String>,
    /// Calibration date, `YYYY-MM-DD`; `None` leaves it out.
    pub date: Option<String>,
    /// Who calibrated; `None` writes [`ANONYMOUS_AUTHOR`].
    pub author: Option<String>,
    /// Free text; `None` writes a note naming the coupon version.
    pub notes: Option<String>,
    /// Chosen value by cell id (`stud`, `tube`, `pocketTubeOD`, `pocketRib`,
    /// `technicH`, `pin`, `axle`, `bar`, `clip`; any subset), in millimetres.
    pub cells: BTreeMap<String, f64>,
}

/// Why a record was refused. Every message names what to change.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RecordError {
    #[error("no coupon cells were chosen; record at least one, for example --stud 4.80")]
    NoCells,
    #[error("`{cell}` is not a cell of coupon v{version}; the cells are {known}")]
    UnknownCell {
        cell: String,
        version: u32,
        known: String,
    },
    #[error("the `{cell}` row ({label}) is printed for reference only: no fit offset takes it yet, so leave it out")]
    NotRecordable { cell: String, label: String },
    #[error("{cell} = {value} mm is not a cell of coupon v{version}: choose a value from {start} to {end} mm in steps of {step} mm")]
    OffGrid {
        cell: String,
        value: f64,
        version: u32,
        start: f64,
        end: f64,
        step: f64,
    },
    #[error("{field} is empty; it is required")]
    Empty { field: &'static str },
    #[error("nozzle diameter {0} mm must be a positive number of millimetres")]
    Nozzle(f64),
    #[error("intent `{0}` is not one of fits-genuine-bricks, fits-itself")]
    Intent(String),
    #[error("`{0}` is not a profile id: use lower-case words joined by hyphens, as in fdm-0.4 or mk4s-petg-fdm-0.4")]
    Id(String),
    #[error("process `{process}` is not a known process profile: the coupon must be printed and recorded with one of {known}")]
    UnknownProcess { process: String, known: String },
    #[error("date `{0}` is not a calendar date written YYYY-MM-DD")]
    Date(String),
}

/// Record against the bundled coupon, [`coupon_v1`].
pub fn record(input: &RecordInput) -> Result<FitProfile, RecordError> {
    record_with(coupon_v1(), input)
}

/// Record against any coupon sweep (for a future coupon version or a test).
pub fn record_with(spec: &CouponSpec, input: &RecordInput) -> Result<FitProfile, RecordError> {
    if input.cells.is_empty() {
        return Err(RecordError::NoCells);
    }
    for (field, value) in [
        ("process", &input.process),
        ("printer", &input.printer),
        ("material", &input.material),
    ] {
        if value.trim().is_empty() {
            return Err(RecordError::Empty { field });
        }
    }
    if !is_profile_id(&input.process) {
        return Err(RecordError::Id(input.process.clone()));
    }
    // A typo such as `fdm-04` would write a profile that rule FIT.PROCESS then
    // refuses for every part.
    let processes = crate::default_process_profiles();
    if !processes.iter().any(|p| p.id == input.process) {
        return Err(RecordError::UnknownProcess {
            process: input.process.clone(),
            known: processes
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    if !(input.nozzle_mm.is_finite() && input.nozzle_mm > 0.0) {
        return Err(RecordError::Nozzle(input.nozzle_mm));
    }
    if !FIT_INTENTS.contains(&input.intent.as_str()) {
        return Err(RecordError::Intent(input.intent.clone()));
    }
    let id = match &input.id {
        Some(id) if is_profile_id(id) => id.clone(),
        Some(id) => return Err(RecordError::Id(id.clone())),
        None => slug_id(&[&input.printer, &input.material, &input.process]),
    };
    let date = match &input.date {
        Some(d) if is_date(d) => d.clone(),
        Some(d) => return Err(RecordError::Date(d.clone())),
        None => String::new(),
    };

    let mut offsets = Offsets::default();
    let mut chosen = BTreeMap::new();
    for (cell_id, &value) in &input.cells {
        let cell = spec.cell(cell_id).ok_or_else(|| RecordError::UnknownCell {
            cell: cell_id.clone(),
            version: spec.version,
            known: spec
                .cells
                .iter()
                .filter(|c| c.offset_field.is_some())
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        })?;
        let Some(field) = &cell.offset_field else {
            return Err(RecordError::NotRecordable {
                cell: cell.id.clone(),
                label: cell.label.clone(),
            });
        };
        let on_grid = cell.grid_value(value).ok_or(RecordError::OffGrid {
            cell: cell.id.clone(),
            value,
            version: spec.version,
            start: cell.start_mm,
            end: cell.end_mm,
            step: cell.step_mm,
        })?;
        let offset = round_dp(on_grid - cell.nominal_mm(), RECORD_DECIMALS);
        // `check()` guarantees the field exists for a loaded spec; a spec built
        // in code may still name a wrong one.
        let slot = offsets.get_mut(field).ok_or(RecordError::NotRecordable {
            cell: cell.id.clone(),
            label: cell.label.clone(),
        })?;
        *slot = offset;
        chosen.insert(cell.id.clone(), round_dp(on_grid, RECORD_DECIMALS));
    }

    let mut material = serde_json::Map::new();
    material.insert("type".into(), input.material.trim().into());
    if let Some(brand) = input
        .brand
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        material.insert("brand".into(), brand.into());
    }
    let author = input
        .author
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .unwrap_or(ANONYMOUS_AUTHOR);
    Ok(FitProfile {
        openbrick: "1.0".into(),
        kind: "fit".into(),
        id,
        credit: Some(Credit {
            author: author.into(),
            url: String::new(),
        }),
        process: input.process.clone(),
        printer: input.printer.trim().into(),
        nozzle_mm: input.nozzle_mm,
        material: serde_json::Value::Object(material),
        slicer: input.slicer.trim().into(),
        intent: input.intent.clone(),
        offsets_mm: offsets,
        calibration: Some(Calibration {
            coupon_version: spec.version,
            chosen_cells: chosen,
            date,
            pull_test_n: None,
            control_pull_test_n: None,
        }),
        notes: input.notes.clone().unwrap_or_else(|| {
            format!(
                "Recorded from calibration coupon v{} (offset = chosen cell minus nominal).",
                spec.version
            )
        }),
    })
}

/// A profile id made of the given parts: lower case, every run of characters
/// other than letters and digits becomes one hyphen, a dot survives only
/// between two digits or letters after the first word (`fdm-0.4`), and an id
/// that would not start with a letter gets the prefix `fit-`. Prusa MK4S, PETG
/// and fdm-0.4 give `prusa-mk4s-petg-fdm-0.4`.
pub fn slug_id(parts: &[&str]) -> String {
    let mut words: Vec<String> = Vec::new();
    for part in parts {
        let lower = part.to_lowercase();
        let chars: Vec<char> = lower.chars().collect();
        let mut word = String::new();
        for (i, &c) in chars.iter().enumerate() {
            let alnum = |k: usize| chars.get(k).is_some_and(|c| c.is_ascii_alphanumeric());
            if c.is_ascii_alphanumeric() {
                word.push(c);
            } else if c == '.' && i > 0 && alnum(i - 1) && alnum(i + 1) {
                word.push('.');
            } else if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    if let Some(first) = words.first_mut() {
        // The first word of a profile id may not hold a dot.
        if first.contains('.') {
            let parts: Vec<String> = first.split('.').map(String::from).collect();
            words.splice(0..1, parts);
        }
    }
    if words
        .first()
        .and_then(|w| w.chars().next())
        .is_none_or(|c| !c.is_ascii_lowercase())
    {
        words.insert(0, "fit".into());
    }
    words.join("-")
}

/// Whether `s` is a profile id: `^[a-z][a-z0-9]*(-[a-z0-9]+(\.[a-z0-9]+)*)*$`
/// (`$defs/profileId` in the schemas).
pub fn is_profile_id(s: &str) -> bool {
    let mut words = s.split('-');
    let Some(first) = words.next() else {
        return false;
    };
    let mut first_chars = first.chars();
    if !first_chars.next().is_some_and(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let lower_alnum = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    if !first_chars.all(lower_alnum) {
        return false;
    }
    words.all(|w| {
        w.split('.')
            .all(|piece| !piece.is_empty() && piece.chars().all(lower_alnum))
    })
}

/// Whether `s` is `YYYY-MM-DD` with a month 01 to 12 and a day 01 to 31 (the
/// pattern of `calibration.date` in `schemas/fit-profile.schema.json`).
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s[r].parse::<u32>().unwrap_or(0);
    (1..=12).contains(&num(5..7)) && (1..=31).contains(&num(8..10))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_profile_ids() {
        assert_eq!(
            slug_id(&["Prusa MK4S", "PETG", "fdm-0.4"]),
            "prusa-mk4s-petg-fdm-0.4"
        );
        assert_eq!(
            slug_id(&["A1 mini", "PLA+", "fdm-0.2"]),
            "a1-mini-pla-fdm-0.2"
        );
        assert_eq!(
            slug_id(&["3D Printer v2.1", "PETG", "fdm-0.4"]),
            "fit-3d-printer-v2.1-petg-fdm-0.4"
        );
        assert_eq!(
            slug_id(&["x1.carbon", "abs", "fdm-0.4"]),
            "x1-carbon-abs-fdm-0.4"
        );
        assert_eq!(slug_id(&["..", "", "fdm-0.4"]), "fdm-0.4");
        assert_eq!(slug_id(&["", ""]), "fit");
        for s in [
            slug_id(&["Prusa MK4S", "PETG", "fdm-0.4"]),
            slug_id(&["3D Printer v2.1", "PETG", "fdm-0.4"]),
            slug_id(&["Ender-3 V2.", ".PLA", "resin-msla"]),
            slug_id(&["Ünïcode Drücker", "PETG", "fdm-0.4"]),
        ] {
            assert!(is_profile_id(&s), "{s}");
        }
    }

    #[test]
    fn profile_ids_follow_the_schema_pattern() {
        for ok in [
            "fdm-0.4",
            "resin-msla",
            "example-mk4s-petg-0.4",
            "a",
            "fdm-0.4.1",
        ] {
            assert!(is_profile_id(ok), "{ok}");
        }
        for bad in [
            "", "FDM-0.4", "0.4-fdm", "fdm.0-4", "fdm--0.4", "fdm-", "fdm-0..4", "fdm-0.",
            "fdm 0.4",
        ] {
            assert!(!is_profile_id(bad), "{bad}");
        }
    }

    #[test]
    fn dates_follow_the_schema_pattern() {
        for ok in ["2026-11-12", "2000-01-31", "2026-12-01"] {
            assert!(is_date(ok), "{ok}");
        }
        for bad in [
            "2026-13-01",
            "2026-00-10",
            "2026-1-01",
            "26-11-12",
            "2026-11-32",
            "2026/11/12",
            "2026-11-1a",
        ] {
            assert!(!is_date(bad), "{bad}");
        }
    }
}
