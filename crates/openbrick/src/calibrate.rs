//! `openbrick calibrate coupon` and `openbrick calibrate record` (docs/plan/05
//! section 4): print the calibration coupon, push genuine bricks onto its
//! cells, and record the cells that fit best as a fit profile whose offsets
//! are the chosen cell minus the standard nominal.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand};
use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_io::ExportMeta;
use openbrick_part::coupon::{build_coupon, Arrangement, CouponOptions, PocketSet};
use openbrick_print::{
    coupon_v1, record, FitProfile, RecordInput, DEFAULT_POCKET_SET, FIT_INTENTS, POCKET_SETS,
};
use serde_json::{json, Value};

use crate::export::{self, Format, Solid};
use crate::files::{absolute, relative_to, write_file};
use crate::part::{choose_process, eval_fail, is_fdm};
use crate::ui::{num, signed, Fail, Outcome, Ui, GENERATOR};

#[derive(Subcommand, Debug)]
pub enum CalibrateCmd {
    /// Build the calibration coupon as 3MF or STL.
    ///
    /// Prints which cell is which. Print it flat side down with no supports,
    /// press genuine bricks onto the cells and record the best fit with
    /// `calibrate record`.
    Coupon(CouponArgs),
    /// Record the coupon cells that fit best as a fit profile.
    ///
    /// The offsets are the chosen cell minus the standard nominal; pass the
    /// file to `build --fit`.
    Record(Box<RecordArgs>),
}

impl CalibrateCmd {
    pub fn json(&self) -> bool {
        match self {
            CalibrateCmd::Coupon(a) => a.json,
            CalibrateCmd::Record(a) => a.json,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            CalibrateCmd::Coupon(_) => "calibrate coupon",
            CalibrateCmd::Record(_) => "calibrate record",
        }
    }

    pub fn run(&self, ui: Ui) -> Result<Outcome, Fail> {
        match self {
            CalibrateCmd::Coupon(a) => coupon(a, ui),
            CalibrateCmd::Record(a) => record_cmd(a, ui),
        }
    }
}

#[derive(Args, Debug)]
pub struct CouponArgs {
    /// Process profile id the coupon is printed with (see `openbrick profiles`).
    #[arg(long, default_value = "fdm-0.4", value_name = "ID")]
    pub process: String,
    /// Which 2x2 pockets (tube OD x rib depth) to print: cross (the default,
    /// fits a 180 x 180 mm bed), cross-and-diagonal or full (larger beds).
    /// Record with the same --pocket-set.
    #[arg(long, alias = "pocket-set", default_value = "cross", value_parser = parse_pocket_set, value_name = "SET")]
    pub pockets: PocketSet,
    /// Bed size the coupon must fit, XxY in mm (default 180x180, a Prusa
    /// MINI+ or Bambu Lab A1 mini; 5 mm stay free at each edge).
    #[arg(long, value_parser = parse_bed, value_name = "XxY")]
    pub bed: Option<[f64; 2]>,
    /// Identical copies of the cell set, for repeatability (default 2).
    #[arg(long, value_name = "N")]
    pub copies: Option<u32>,
    /// Output file, .3mf or .stl (default coupon-<process>-<pockets>.3mf).
    #[arg(short = 'o', long, value_name = "FILE")]
    pub out: Option<PathBuf>,
    /// Also write the cell map as JSON: what --json prints, with the output
    /// path relative to the manifest.
    #[arg(long, value_name = "FILE")]
    pub manifest: Option<PathBuf>,
    /// Print one JSON object on stdout; human text goes to stderr.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct RecordArgs {
    /// Process profile id the coupon was printed with.
    #[arg(long, value_name = "ID")]
    pub process: String,
    /// Printer name, for example "Prusa MINI+".
    #[arg(long)]
    pub printer: String,
    /// Material type, for example PLA or PETG.
    #[arg(long)]
    pub material: String,
    /// Material brand (optional).
    #[arg(long)]
    pub brand: Option<String>,
    /// Nozzle diameter in mm.
    #[arg(long, default_value_t = 0.4, value_parser = finite, value_name = "MM")]
    pub nozzle: f64,
    /// Slicer and version, for example "PrusaSlicer 2.9.2".
    #[arg(long, default_value = "")]
    pub slicer: String,
    /// What the fit aims at: fits-genuine-bricks or fits-itself.
    #[arg(long, default_value = FIT_INTENTS[0])]
    pub intent: String,
    /// Profile id (default: derived from printer, material and process).
    #[arg(long)]
    pub id: Option<String>,
    /// Who calibrated (default anonymous).
    #[arg(long)]
    pub author: Option<String>,
    /// Calibration date, YYYY-MM-DD (default: today, UTC).
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,
    /// Free-text notes.
    #[arg(long)]
    pub notes: Option<String>,
    /// The pocket set of the coupon you printed: cross (default),
    /// cross-and-diagonal or full.
    #[arg(long, default_value = DEFAULT_POCKET_SET, value_parser = parse_pocket_set_name, value_name = "SET")]
    pub pocket_set: String,
    /// Best stud cell (outer diameter, mm), as its label shows it.
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub stud: Option<f64>,
    /// Best tube bore cell (inner diameter, mm).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub tube: Option<f64>,
    /// Best 2x2 pocket as its two-line label reads, tube OD over rib depth,
    /// for example 6.65/0.30. A pocket is one choice: both numbers come from
    /// the same cell.
    #[arg(long, value_parser = parse_pocket, value_name = "OD/RIB", conflicts_with_all = ["pocket_tube_od", "pocket_rib"])]
    pub pocket: Option<(f64, f64)>,
    /// The best pocket's tube OD (with --pocket-rib; same as --pocket OD/RIB).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub pocket_tube_od: Option<f64>,
    /// The best pocket's rib depth (with --pocket-tube-od).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub pocket_rib: Option<f64>,
    /// Best horizontal Technic hole cell (inner diameter, mm).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub technic_h: Option<f64>,
    /// Best Technic pin cell (outer diameter, mm).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub pin: Option<f64>,
    /// Best axle-hole cell (cross envelope, mm).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub axle: Option<f64>,
    /// Best bar cell (diameter, mm).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub bar: Option<f64>,
    /// Best clip cell (opening, mm).
    #[arg(long, value_parser = finite, value_name = "MM")]
    pub clip: Option<f64>,
    /// Where to write the fit profile (JSON).
    #[arg(short = 'o', long, value_name = "FILE")]
    pub out: PathBuf,
    /// Print one JSON object on stdout; human text goes to stderr.
    #[arg(long)]
    pub json: bool,
}

fn finite(s: &str) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(format!("expected a number of millimetres, found `{s}`")),
    }
}

fn parse_pocket(s: &str) -> Result<(f64, f64), String> {
    let bad = || {
        format!("expected the pocket's label, tube OD/rib depth, such as 6.65/0.30; found `{s}`")
    };
    let (od, rib) = s.split_once('/').ok_or_else(bad)?;
    Ok((
        finite(od).map_err(|_| bad())?,
        finite(rib).map_err(|_| bad())?,
    ))
}

fn parse_pocket_set(s: &str) -> Result<PocketSet, String> {
    match s {
        "cross" => Ok(PocketSet::Cross),
        "cross-and-diagonal" | "crossAndDiagonal" => Ok(PocketSet::CrossAndDiagonal),
        "full" => Ok(PocketSet::Full),
        _ => Err(format!(
            "`{s}` is not a pocket set; use cross, cross-and-diagonal or full"
        )),
    }
}

/// The pocket set name as a fit profile records it (`crossAndDiagonal`).
fn parse_pocket_set_name(s: &str) -> Result<String, String> {
    let name = parse_pocket_set(s)?.name();
    debug_assert!(POCKET_SETS.contains(&name));
    Ok(name.to_string())
}

fn parse_bed(s: &str) -> Result<[f64; 2], String> {
    let bad = || format!("expected the bed size as XxY in mm, such as 180x180; found `{s}`");
    let (x, y) = s.split_once(['x', 'X']).ok_or_else(bad)?;
    let x = finite(x).map_err(|_| bad())?;
    let y = finite(y).map_err(|_| bad())?;
    if x <= 0.0 || y <= 0.0 {
        return Err(bad());
    }
    Ok([x, y])
}

/// Today's date (UTC) as YYYY-MM-DD, from the days since 1970-01-01
/// (H. Hinnant's civil_from_days).
pub fn today_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_date((secs / 86_400) as i64)
}

fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// How the copies of the cell set lie on the coupon, as a sentence.
fn arrangement_text(arrangement: Arrangement, copies: u32) -> String {
    match (arrangement, copies) {
        (_, 0 | 1) => "One copy of the cell set.".into(),
        (Arrangement::Stacked, _) => format!(
            "The {copies} copies of the cell set lie one behind the other: copy 1 at the back."
        ),
        (Arrangement::SideBySide, _) => format!(
            "The {copies} copies of the cell set lie side by side: copy 1 on the left, so each line has a twin beside it at the same depth."
        ),
    }
}

/// Where a copy lies, for the table: back or front, left or right (two
/// copies), or nothing (one copy, or more than two).
fn copy_place(arrangement: Arrangement, copy: usize, copies: u32) -> Option<&'static str> {
    if copies != 2 {
        return None;
    }
    Some(match (arrangement, copy) {
        (Arrangement::Stacked, 0) => "back",
        (Arrangement::Stacked, _) => "front",
        (Arrangement::SideBySide, 0) => "left",
        (Arrangement::SideBySide, _) => "right",
    })
}

/// A short name for the line a cell belongs to.
fn line_name(cell_id: &str) -> String {
    match cell_id {
        "pocketTubeOD" => "2x2 pockets, tube OD/rib depth".into(),
        id => coupon_v1()
            .cell(id)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| id.to_string()),
    }
}

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1e6).round() / 1e3
}

/// `openbrick calibrate coupon`.
pub fn coupon(args: &CouponArgs, ui: Ui) -> Result<Outcome, Fail> {
    let total = Instant::now();
    let process = choose_process(Some(&args.process), None, None)?;
    let pockets = args.pockets.name();
    let out_path = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("coupon-{}-{pockets}.3mf", process.id)));
    let format = Format::of(&out_path)?;
    if let Some(manifest) = &args.manifest {
        let same = std::path::absolute(manifest).ok() == std::path::absolute(&out_path).ok();
        if same {
            return Err(Fail::Input(format!(
                "--manifest and --out both name {}; give the manifest its own .json file",
                manifest.display()
            )));
        }
    }
    let mut opts = CouponOptions {
        pockets: args.pockets,
        ..CouponOptions::default()
    };
    if let Some(bed) = args.bed {
        opts.bed_mm = bed;
    }
    if let Some(copies) = args.copies {
        opts.copies = copies;
    }
    let t = Instant::now();
    let coupon = build_coupon(&ManifoldRust, &process, &opts).map_err(eval_fail)?;
    let build_ms = ms(t);
    let s = &coupon.summary;
    let e = &coupon.estimate;
    let mesh = coupon
        .built
        .objects
        .first()
        .map(|o| o.mesh.clone())
        .ok_or_else(|| {
            Fail::Internal("the coupon has no mesh; this is a bug, please report it".into())
        })?;
    let solids = [Solid::new("calibration coupon".into(), "body".into(), mesh)];
    let meta = ExportMeta {
        part_id: "openbrick.calibration-coupon".into(),
        version: format!("{}.{}.0", s.coupon_version, s.coupon_revision),
        content_hash: coupon.built.content_hash.clone(),
        // The coupon measures the printer and is never compensated.
        fit_profile_id: "none".into(),
        process_id: process.id.clone(),
        generator: GENERATOR.into(),
        kernel_id: coupon.built.kernel_id.clone(),
        title: format!(
            "Calibration coupon v{} ({}, {pockets} pockets)",
            s.coupon_version, process.id
        ),
        serial: None,
    };
    let t = Instant::now();
    let (bytes, entry) = export::encode(&solids, &meta, format)?;
    let entry = export::write(&out_path, &bytes, entry)?;
    let write_ms = ms(t);

    // Lines from the back (largest y) to the front, and left to right where
    // copies stand side by side; cells left to right.
    let mut lines: BTreeMap<usize, Vec<&openbrick_part::CouponCellLayout>> = BTreeMap::new();
    for c in &coupon.cells {
        lines.entry(c.row).or_default().push(c);
    }
    let mut lines: Vec<_> = lines.into_values().collect();
    for cells in &mut lines {
        cells.sort_by(|a, b| a.center_mm[0].total_cmp(&b.center_mm[0]));
    }
    lines.sort_by(|a, b| {
        b[0].center_mm[1]
            .total_cmp(&a[0].center_mm[1])
            .then(a[0].center_mm[0].total_cmp(&b[0].center_mm[0]))
    });
    let mut lines_json = Vec::new();
    ui.say(format!(
        "Calibration coupon v{} (revision {}) for {}: {pockets} pockets, {} copies, {} x {} mm, {} mm tall.",
        s.coupon_version,
        s.coupon_revision,
        process.id,
        s.copies,
        num(s.footprint_mm[0]),
        num(s.footprint_mm[1]),
        num(s.height_mm)
    ));
    ui.say(arrangement_text(s.arrangement, s.copies));
    ui.say("Lines from the back of the coupon (labels read upright), cells left to right:");
    ui.say(format!(
        "  {:>4} {:<9}  {:<46} values (mm)",
        "line", "copy", "cell"
    ));
    for cells in &lines {
        let first = cells[0];
        let values: Vec<String> = cells
            .iter()
            .map(|c| match &c.cross {
                Some(x) => format!("{:.2}/{:.2}", c.value_mm, x.value_mm),
                None => format!("{:.2}", c.value_mm),
            })
            .collect();
        let place = copy_place(s.arrangement, first.copy, s.copies);
        let copy = match place {
            Some(p) => format!("{} {p}", first.copy + 1),
            None => format!("{}", first.copy + 1),
        };
        ui.say(format!(
            "  {:>4} {copy:<9}  {:<46} {}",
            first.row + 1,
            line_name(&first.cell_id),
            values.join(" ")
        ));
        lines_json.push(json!({
            "line": first.row + 1,
            "copy": first.copy + 1,
            "place": place,
            "cellId": first.cell_id,
            "crossCellId": first.cross.as_ref().map(|x| x.cell_id.clone()),
            "name": line_name(&first.cell_id),
            "centerYMm": first.center_mm[1],
            "values": values,
        }));
    }
    if !s.unbuilt_cells.is_empty() {
        ui.say(format!(
            "  Not on this coupon yet: {} (later coupon revisions add them).",
            s.unbuilt_cells.join(", ")
        ));
    }
    let layers: Vec<String> = process.layer_height_mm.iter().map(|h| num(*h)).collect();
    let fdm = is_fdm(&process);
    if fdm {
        ui.say(format!(
            "Plastic: {:.0} mm3, about {:.0} g of PLA; roughly {:.0} min at {} mm3/s (the slicer has the real time).",
            e.volume_mm3,
            e.mass_g_pla,
            e.time_s / 60.0,
            num(e.flow_mm3_s)
        ));
        ui.say(format!(
            "Slice it flat side down as written, no supports, {} mm layers ({}); XY and elephant-foot compensation 0 (the first-layer chamfer is in the model).",
            layers.join(" or "),
            process.id
        ));
    } else {
        ui.say(format!(
            "Resin: {:.0} mm3, about {:.0} ml before supports (the slicer has the print time).",
            e.volume_mm3,
            e.volume_mm3 / 1000.0
        ));
        ui.say(format!(
            "Place it flat on the build plate as written, no supports and no tilt, {} mm layers ({}); note your slicer's exposure and any XY or elephant-foot compensation with --notes when you record.",
            layers.join(" or "),
            process.id
        ));
    }
    ui.say(format!(
        "Then press genuine bricks onto the cells and record the best fit:\n  openbrick calibrate record --process {} --pocket-set {pockets} --printer \"YOUR PRINTER\" --material {} --stud V --tube V --pocket OD/RIB --out my-fit.json",
        process.id,
        if fdm { "PLA" } else { "\"tough resin\"" }
    ));
    ui.say(format!(
        "Wrote {} ({} bytes, {}).",
        entry["path"].as_str().unwrap_or_default(),
        entry["bytes"],
        format.name().to_uppercase()
    ));
    for w in &coupon.built.warnings {
        ui.say(format!("  note: {w}"));
    }

    let mut timings = serde_json::Map::new();
    timings.insert("buildMs".into(), json!(build_ms));
    timings.insert("writeMs".into(), json!(write_ms));
    let mut object = serde_json::Map::new();
    object.insert("command".into(), json!("calibrate coupon"));
    object.insert("generator".into(), json!(GENERATOR));
    object.insert("ok".into(), json!(true));
    object.insert("exitCode".into(), json!(0));
    object.insert("process".into(), json!(process.id));
    object.insert("pockets".into(), json!(pockets));
    object.insert("contentHash".into(), json!(coupon.built.content_hash));
    object.insert("kernel".into(), json!(coupon.built.kernel_id));
    object.insert(
        "summary".into(),
        serde_json::to_value(s).unwrap_or(Value::Null),
    );
    object.insert(
        "estimate".into(),
        serde_json::to_value(e).unwrap_or(Value::Null),
    );
    object.insert("lines".into(), Value::Array(lines_json));
    object.insert(
        "cells".into(),
        serde_json::to_value(&coupon.cells).unwrap_or(Value::Null),
    );
    object.insert("warnings".into(), json!(coupon.built.warnings));
    let mut outputs = vec![entry.clone()];
    if let Some(manifest) = &args.manifest {
        // The manifest finds the coupon relative to itself.
        let mut local = entry;
        let base = match manifest.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        // Exists before the paths are compared, so both resolve the same way.
        std::fs::create_dir_all(&base).map_err(|e| {
            Fail::Input(format!("cannot create the folder {}: {e}", base.display()))
        })?;
        local["path"] = json!(relative_to(&out_path, &base));
        let mut m = object.clone();
        m.insert("outputs".into(), json!([local]));
        m.insert("timingsMs".into(), Value::Object(timings.clone()));
        let mut text = serde_json::to_string_pretty(&Value::Object(m))
            .map_err(|e| Fail::Internal(format!("cannot encode the manifest: {e}")))?;
        text.push('\n');
        write_file(manifest, text.as_bytes()).map_err(Fail::Input)?;
        ui.say(format!("Wrote the cell map {}.", manifest.display()));
        outputs.push(json!({
            "path": absolute(manifest).to_string_lossy(),
            "format": "json",
            "bytes": text.len(),
        }));
    }
    timings.insert("totalMs".into(), json!(ms(total)));
    object.insert("outputs".into(), Value::Array(outputs));
    object.insert("timingsMs".into(), Value::Object(timings));
    Ok(Outcome {
        code: 0,
        json: Value::Object(object),
    })
}

/// The chosen cells, by the cell ids of the coupon data.
fn chosen_cells(args: &RecordArgs) -> BTreeMap<String, f64> {
    let (od, rib) = match args.pocket {
        Some((od, rib)) => (Some(od), Some(rib)),
        None => (args.pocket_tube_od, args.pocket_rib),
    };
    [
        ("stud", args.stud),
        ("tube", args.tube),
        ("pocketTubeOD", od),
        ("pocketRib", rib),
        ("technicH", args.technic_h),
        ("pin", args.pin),
        ("axle", args.axle),
        ("bar", args.bar),
        ("clip", args.clip),
    ]
    .into_iter()
    .filter_map(|(id, v)| v.map(|v| (id.to_string(), v)))
    .collect()
}

/// The rows of the offsets table, in the order of the coupon data: (cell,
/// chosen, nominal, offset field, offset).
fn offset_rows(profile: &FitProfile) -> Vec<(String, f64, f64, String, f64)> {
    let Some(cal) = &profile.calibration else {
        return Vec::new();
    };
    coupon_v1()
        .cells
        .iter()
        .filter_map(|cell| {
            let &chosen = cal.chosen_cells.get(&cell.id)?;
            let field = cell.offset_field.clone()?;
            let offset = profile.offsets_mm.get(&field)?;
            Some((cell.id.clone(), chosen, cell.nominal_mm(), field, offset))
        })
        .collect()
}

/// `openbrick calibrate record`.
pub fn record_cmd(args: &RecordArgs, ui: Ui) -> Result<Outcome, Fail> {
    let input = RecordInput {
        process: args.process.clone(),
        printer: args.printer.clone(),
        nozzle_mm: args.nozzle,
        material: args.material.clone(),
        brand: args.brand.clone(),
        slicer: args.slicer.clone(),
        intent: args.intent.clone(),
        id: args.id.clone(),
        date: Some(args.date.clone().unwrap_or_else(today_utc)),
        author: args.author.clone(),
        notes: args.notes.clone(),
        cells: chosen_cells(args),
        pocket_set: Some(args.pocket_set.clone()),
    };
    let profile = record(&input).map_err(|e| Fail::Input(e.to_string()))?;
    let mut text = serde_json::to_string_pretty(&profile)
        .map_err(|e| Fail::Internal(format!("cannot encode the fit profile: {e}")))?;
    text.push('\n');
    write_file(&args.out, text.as_bytes()).map_err(Fail::Input)?;

    let rows = offset_rows(&profile);
    ui.say(format!(
        "Recorded fit profile {} ({}, {} pocket set) to {}.",
        profile.id,
        profile.process,
        args.pocket_set,
        args.out.display()
    ));
    ui.say(format!(
        "  {:<13} {:>7} {:>8} {:>8}  offset field",
        "cell", "chosen", "nominal", "offset"
    ));
    for (cell, chosen, nominal, field, offset) in &rows {
        ui.say(format!(
            "  {cell:<13} {chosen:>7.2} {nominal:>8.3} {:>8}  {field}",
            signed(*offset)
        ));
    }
    ui.say(format!(
        "Use it: openbrick build parts/ndn.logo-tile.obpart.json --fit {} --out tile.3mf",
        args.out.display()
    ));
    ui.say("Then run the pull test (docs/guides/print-test.md) and share the result if you like.");
    let offsets: Vec<Value> = rows
        .iter()
        .map(|(cell, chosen, nominal, field, offset)| {
            json!({
                "cell": cell,
                "chosenMm": chosen,
                "nominalMm": nominal,
                "offsetField": field,
                "offsetMm": offset,
            })
        })
        .collect();
    Ok(Outcome {
        code: 0,
        json: json!({
            "command": "calibrate record",
            "generator": GENERATOR,
            "ok": true,
            "exitCode": 0,
            "fitProfile": profile,
            "offsets": offsets,
            "outputs": [{
                "path": absolute(&args.out).to_string_lossy(),
                "format": "json",
                "bytes": text.len(),
            }],
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        // 30 years of 365 days plus 7 leap days, then January and February.
        assert_eq!(civil_date(10_957 + 31 + 28), "2000-02-29");
        assert_eq!(civil_date(10_957 + 31 + 29), "2000-03-01");
        // 54 years of 365 days plus 13 leap days (1972 to 2020).
        assert_eq!(civil_date(365 * 54 + 13), "2024-01-01");
        let today = today_utc();
        assert_eq!(today.len(), 10, "{today}");
    }

    #[test]
    fn option_parsers() {
        assert_eq!(parse_pocket("6.65/0.30"), Ok((6.65, 0.3)));
        assert!(parse_pocket("6.65").is_err());
        assert!(parse_pocket("6.65/x").is_err());
        assert_eq!(parse_bed("256x256"), Ok([256.0, 256.0]));
        assert!(parse_bed("256").is_err());
        assert!(parse_bed("0x10").is_err());
        assert_eq!(
            parse_pocket_set_name("cross-and-diagonal").as_deref(),
            Ok("crossAndDiagonal")
        );
        assert!(parse_pocket_set_name("diagonal").is_err());
        assert!(finite("inf").is_err());
    }
}
