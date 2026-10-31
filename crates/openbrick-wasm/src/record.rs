//! The calibration wizard's record step in the browser (docs/plan/05 section 4,
//! docs/plan/06 section 11): the coupon cells a person tapped become a fit
//! profile, by `openbrick_print::record`, exactly as `openbrick calibrate
//! record` makes it.
//!
//! [`record_fit`] takes the wizard's choices as JSON ([`RecordFitInput`]) and
//! answers with the fit profile file the CLI would write for the same inputs,
//! byte for byte (`openbrick_print::fit_profile_text`; the CLI's
//! `tests/record_parity.rs` checks it), its offsets (chosen cell minus
//! nominal, `openbrick_print::offset_rows`) for the summary shown before
//! saving, and the file name. A choice `record` refuses comes back as an
//! error that names the wizard field it concerns, so the page shows the
//! message next to that field.
//!
//! The same semantics as the CLI: the profile id is derived from printer,
//! material and process unless one is given (`openbrick_print::slug_id`),
//! offsets and chosen cells are rounded to micrometres
//! (`openbrick_print::RECORD_DECIMALS`), the nozzle defaults to
//! `openbrick_print::DEFAULT_NOZZLE_MM`, the intent to `fits-genuine-bricks`
//! and the pocket set to `cross`. The date is written as given. Without
//! `--date` the CLI writes today's date, which WebAssembly has no clock to
//! know, so the page always sends its date field (today's date unless the
//! person changes it); input without a date writes a profile without one.

use std::collections::BTreeMap;

use openbrick_print::{
    fit_profile_text, offset_rows, record, RecordError, RecordInput, DEFAULT_NOZZLE_MM,
    DEFAULT_POCKET_SET, FIT_INTENTS, POCKET_CELLS,
};
use serde_json::json;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::CoreError;

/// What the wizard collected, in JSON (camelCase): every field is optional
/// and takes the CLI's default, so a missing printer is refused by `record`
/// as an empty one, with its message. Unknown fields are refused.
///
/// - `process`: the process profile id the coupon was printed with;
/// - `pocketSet`: the coupon's pocket set (`cross`, `crossAndDiagonal`,
///   `full`; default `cross`);
/// - `printer`, `material`, `brand`, `slicer`, `author`, `notes`: text (an
///   absent `notes` writes the note naming the coupon version, as the CLI
///   does without `--notes`; an empty one writes an empty note);
/// - `nozzleMm` (default 0.4), `intent` (default `fits-genuine-bricks`), `id`
///   (default derived), `date` (`YYYY-MM-DD`, absent for none);
/// - `cells`: the chosen value by cell id, in mm (`stud`, `tube`,
///   `pocketTubeOD` and `pocketRib` together, ...).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct RecordFitInput {
    pub process: String,
    pub pocket_set: Option<String>,
    pub printer: String,
    pub material: String,
    pub brand: Option<String>,
    pub nozzle_mm: Option<f64>,
    pub slicer: String,
    pub intent: Option<String>,
    pub id: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub notes: Option<String>,
    pub cells: BTreeMap<String, f64>,
}

impl RecordFitInput {
    /// The `RecordInput` the CLI builds from the same choices
    /// (`crates/openbrick/src/calibrate.rs`, `record_cmd`).
    pub fn to_record_input(&self) -> RecordInput {
        RecordInput {
            process: self.process.clone(),
            printer: self.printer.clone(),
            nozzle_mm: self.nozzle_mm.unwrap_or(DEFAULT_NOZZLE_MM),
            material: self.material.clone(),
            brand: self.brand.clone(),
            slicer: self.slicer.clone(),
            intent: self
                .intent
                .clone()
                .unwrap_or_else(|| FIT_INTENTS[0].to_string()),
            id: self.id.clone(),
            date: self.date.clone(),
            author: self.author.clone(),
            notes: self.notes.clone(),
            cells: self.cells.clone(),
            pocket_set: Some(
                self.pocket_set
                    .clone()
                    .unwrap_or_else(|| DEFAULT_POCKET_SET.to_string()),
            ),
        }
    }
}

/// The wizard field a refusal concerns: a key of [`RecordFitInput`]
/// (`process`, `pocketSet`, `printer`, `material`, `nozzleMm`, `intent`, `id`,
/// `date`, `cells`), the id of the cell whose value was refused (`stud`,
/// `tube`, ...), or `pocket` for the pocket, which is one choice of two values.
pub fn error_field(err: &RecordError, input: &RecordInput) -> String {
    let cell = |id: &str| {
        if POCKET_CELLS.contains(&id) {
            "pocket".to_string()
        } else {
            id.to_string()
        }
    };
    match err {
        RecordError::NoCells => "cells".into(),
        RecordError::UnknownCell { cell: c, .. }
        | RecordError::NotRecordable { cell: c, .. }
        | RecordError::OffGrid { cell: c, .. } => cell(c),
        RecordError::Empty { field } => (*field).to_string(),
        RecordError::Nozzle(_) => "nozzleMm".into(),
        RecordError::Intent(_) => "intent".into(),
        // The process id is checked first, with the same message as the id.
        RecordError::Id(value) if *value == input.process => "process".into(),
        RecordError::Id(_) => "id".into(),
        RecordError::UnknownProcess { .. } => "process".into(),
        RecordError::Date(_) => "date".into(),
        RecordError::UnknownPocketSet(_) => "pocketSet".into(),
        RecordError::PocketIncomplete { .. } | RecordError::PocketNotPrinted { .. } => {
            "pocket".into()
        }
    }
}

/// Record the wizard's choices as a fit profile (see the module
/// documentation). Answers with JSON:
///
/// - `{ "ok": true, "fitProfile": {...}, "text": "...", "fileName":
///   "<id>.json", "offsets": [{ cell, chosenMm, nominalMm, offsetField,
///   offsetMm }] }`, where `text` is the file `openbrick calibrate record`
///   writes for the same inputs, byte for byte;
/// - `{ "ok": false, "error": { "field": "...", "message": "..." } }` when
///   `record` refuses a choice, with the message the CLI prints.
///
/// Input that is not JSON of [`RecordFitInput`]'s shape is an error.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn record_fit(input_json: &str) -> Result<String, CoreError> {
    let invalid = |e: String| {
        CoreError(format!(
            "the calibration record is not valid ({e}); it takes a JSON object of process, pocketSet, printer, material, brand, nozzleMm, slicer, intent, id, author, date, notes and cells (cell id to mm)"
        ))
    };
    // An object only: serde would also read a struct from a JSON array.
    let value: serde_json::Value =
        serde_json::from_str(input_json).map_err(|e| invalid(e.to_string()))?;
    if !value.is_object() {
        return Err(invalid("not an object".into()));
    }
    let wizard: RecordFitInput =
        serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
    let input = wizard.to_record_input();
    let profile = match record(&input) {
        Ok(p) => p,
        Err(e) => {
            return Ok(json!({
                "ok": false,
                "error": { "field": error_field(&e, &input), "message": e.to_string() },
            })
            .to_string())
        }
    };
    let text = fit_profile_text(&profile)
        .map_err(|e| CoreError(format!("cannot encode the fit profile: {e}")))?;
    Ok(json!({
        "ok": true,
        "fileName": format!("{}.json", profile.id),
        "fitProfile": profile,
        "offsets": offset_rows(&profile),
        "text": text,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_print::{coupon_v1, FitProfile, OffsetRow};
    use serde_json::Value;

    fn call(input: Value) -> Value {
        serde_json::from_str(&record_fit(&input.to_string()).expect("valid input")).unwrap()
    }

    /// The nominal of a coupon cell, from the coupon data and the standard.
    fn nominal(id: &str) -> f64 {
        coupon_v1().cell(id).expect("a coupon cell").nominal_mm()
    }

    /// The `i`-th value of a cell's sweep, so no cell value is typed in.
    fn value(id: &str, i: usize) -> f64 {
        coupon_v1().cell(id).expect("a coupon cell").values()[i]
    }

    #[test]
    fn a_record_is_the_cli_profile_with_its_offsets() {
        let pocket = coupon_v1()
            .printed_pockets("cross")
            .expect("the cross")
            .into_iter()
            .last()
            .expect("a pocket");
        let out = call(json!({
            "process": "fdm-0.4",
            "pocketSet": "cross",
            "printer": "Prusa MINI+",
            "material": "PLA",
            "date": "2026-11-12",
            "cells": {
                "stud": value("stud", 3),
                "tube": value("tube", 3),
                "pocketTubeOD": pocket.0,
                "pocketRib": pocket.1,
            },
        }));
        assert_eq!(out["ok"], true, "{out}");
        let text = out["text"].as_str().unwrap();
        assert!(text.ends_with("}\n"), "{text}");
        let profile: FitProfile = serde_json::from_str(text).unwrap();
        assert_eq!(profile.id, "prusa-mini-pla-fdm-0.4");
        assert_eq!(out["fileName"], "prusa-mini-pla-fdm-0.4.json");
        assert_eq!(out["fitProfile"], serde_json::to_value(&profile).unwrap());
        // The same input through openbrick_print gives the same file.
        let input = RecordFitInput {
            process: "fdm-0.4".into(),
            pocket_set: Some("cross".into()),
            printer: "Prusa MINI+".into(),
            material: "PLA".into(),
            date: Some("2026-11-12".into()),
            cells: BTreeMap::from([
                ("stud".into(), value("stud", 3)),
                ("tube".into(), value("tube", 3)),
                ("pocketTubeOD".into(), pocket.0),
                ("pocketRib".into(), pocket.1),
            ]),
            ..RecordFitInput::default()
        };
        let direct = record(&input.to_record_input()).unwrap();
        assert_eq!(fit_profile_text(&direct).unwrap(), text);
        // Offsets: chosen minus nominal, one row per chosen cell.
        let rows: Vec<OffsetRow> = serde_json::from_value(out["offsets"].clone()).unwrap();
        let cells: Vec<&str> = rows.iter().map(|r| r.cell.as_str()).collect();
        assert_eq!(cells, ["stud", "tube", "pocketTubeOD", "pocketRib"]);
        for r in &rows {
            assert_eq!(r.nominal_mm, nominal(&r.cell), "{r:?}");
            assert!(
                (r.offset_mm - (r.chosen_mm - r.nominal_mm)).abs() < 1e-9,
                "{r:?}"
            );
            assert_eq!(profile.offsets_mm.get(&r.offset_field), Some(r.offset_mm));
        }
        // The defaults are the CLI's.
        assert_eq!(profile.nozzle_mm, DEFAULT_NOZZLE_MM);
        assert_eq!(profile.intent, FIT_INTENTS[0]);
        let cal = profile.calibration.as_ref().unwrap();
        assert_eq!(cal.pocket_set.as_deref(), Some("cross"));
        assert_eq!(cal.date, "2026-11-12");
        assert!(profile.notes.contains("coupon v1"), "{}", profile.notes);
    }

    #[test]
    fn refusals_name_the_field() {
        let base = json!({
            "process": "fdm-0.4",
            "printer": "Prusa XL",
            "material": "PETG",
            "cells": { "stud": value("stud", 0) },
        });
        let with = |key: &str, v: Value| {
            let mut input = base.clone();
            input[key] = v;
            input
        };
        let first_pocket = coupon_v1().printed_pockets("full").unwrap()[0];
        // Half a step past the first stud cell: on no cell of the sweep.
        let stud = coupon_v1().cell("stud").unwrap();
        let off_grid = stud.values()[0] + stud.step_mm / 2.0;
        let cross = coupon_v1().printed_pockets("cross").unwrap();
        // A pocket the full cross prints and the default cross does not.
        let unprinted = coupon_v1()
            .printed_pockets("full")
            .unwrap()
            .into_iter()
            .find(|p| !cross.contains(p))
            .expect("the full cross has more pockets");
        let cases = [
            (with("printer", json!(" ")), "printer"),
            (with("material", json!("")), "material"),
            (with("process", json!("fdm-04")), "process"),
            (with("process", json!("FDM 0.4")), "process"),
            (with("id", json!("My Profile")), "id"),
            (with("date", json!("12.11.2026")), "date"),
            (with("nozzleMm", json!(0)), "nozzleMm"),
            (with("intent", json!("tight")), "intent"),
            (with("pocketSet", json!("diagonal")), "pocketSet"),
            (with("cells", json!({})), "cells"),
            (with("cells", json!({ "stud": off_grid })), "stud"),
            (with("cells", json!({ "pin": 1.0 })), "pin"),
            (with("cells", json!({ "nope": 1.0 })), "nope"),
            (
                with("cells", json!({ "pocketTubeOD": first_pocket.0 })),
                "pocket",
            ),
            (
                with(
                    "cells",
                    json!({ "pocketTubeOD": unprinted.0, "pocketRib": unprinted.1 }),
                ),
                "pocket",
            ),
        ];
        for (input, field) in cases {
            let out = call(input.clone());
            assert_eq!(out["ok"], false, "{input}: {out}");
            assert_eq!(out["error"]["field"], field, "{input}: {out}");
            // The CLI's message, word for word.
            let wizard: RecordFitInput = serde_json::from_value(input).unwrap();
            let err = record(&wizard.to_record_input()).unwrap_err();
            assert_eq!(out["error"]["message"], err.to_string());
        }
        // The printed pocket of the full set is accepted with that set.
        let out = call(with(
            "cells",
            json!({ "pocketTubeOD": unprinted.0, "pocketRib": unprinted.1 }),
        ));
        assert_eq!(out["error"]["field"], "pocket");
        let mut full = with(
            "cells",
            json!({ "pocketTubeOD": unprinted.0, "pocketRib": unprinted.1 }),
        );
        full["pocketSet"] = json!("full");
        assert_eq!(call(full)["ok"], true);
    }

    #[test]
    fn input_that_is_not_a_record_is_an_error() {
        for bad in ["", "[]", r#"{"printer": 3}"#, r#"{"fit": "example"}"#] {
            let e = record_fit(bad).unwrap_err();
            assert!(
                e.0.contains("calibration record is not valid"),
                "{bad}: {e}"
            );
        }
    }
}
