//! `openbrick build` and `openbrick validate` (docs/plan/05 sections 5 and 9):
//! a part document plus command-line parameters, a process profile and an
//! optional fit profile are evaluated, validated and, for `build`, written as a
//! 3MF or STL. A report with blockers writes nothing and exits 1.

use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_io::ExportMeta;
use openbrick_part::coupon::PLA_DENSITY_G_CM3;
use openbrick_part::eval::resolve_params;
use openbrick_part::{evaluate, validate, Built, EvalError, EvalInput, PartDoc};
use openbrick_print::{
    default_process_profiles, example_fit_profile, FitProfile, ProcessProfile, Report,
};
use serde_json::{json, Value};

use crate::export::{self, Format, Solid};
use crate::files::{absolute, read_text_file, FileAssets, MAX_DOCUMENT_BYTES, MAX_PROFILE_BYTES};
use crate::params;
use crate::ui::{num, print_findings, Fail, Outcome, Ui, EXIT_BLOCKED, GENERATOR};

/// What `--fit example` prints, every time.
pub const EXAMPLE_FIT_NOTE: &str = "the bundled example fit profile holds illustrative values: a starting point, not a calibration of your printer. Print the coupon (openbrick calibrate coupon) and record your own (openbrick calibrate record)";

/// The part, its parameters and the profiles (shared by build and validate).
#[derive(Args, Debug)]
pub struct PartArgs {
    /// Part document (.obpart.json). Asset paths inside it (such as the
    /// default logo assets/mark.svg) are relative to the part file's folder.
    pub file: PathBuf,
    /// Set a parameter, NAME=VALUE; repeat for more. Footprints as 2x4 (or
    /// [2,4]), lengths in mm (0.6 or 0.6mm), enum options by value
    /// (relief=inlay), booleans as true/false. SVG and other asset parameters
    /// are file paths relative to the current directory (logo=./my-logo.svg).
    #[arg(short = 'p', long = "param", value_name = "NAME=VALUE")]
    pub param: Vec<String>,
    /// Several parameters at once (--params size=2x4 relief=inlay); put the
    /// part file before it.
    #[arg(long = "params", value_name = "NAME=VALUE", num_args = 1..)]
    pub params: Vec<String>,
    /// Process profile id (see `openbrick profiles`). Default: the fit
    /// profile's process when --fit is given and the part allows it, else the
    /// first entry of the part's print.profiles.
    #[arg(long, value_name = "ID")]
    pub process: Option<String>,
    /// Fit profile: a fit profile JSON written by `openbrick calibrate
    /// record`, or `example` for the bundled example (a starting point, not a
    /// calibration of your printer; use ./example for a file of that name).
    /// Fit-critical parts (studs, tubes) are blocked without one (FIT.MISSING).
    #[arg(long, value_name = "PATH|example")]
    pub fit: Option<String>,
    /// Print one JSON object on stdout (report per schemas/report.schema.json,
    /// measurements, content hash, outputs, timings); human text goes to stderr.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct BuildArgs {
    #[command(flatten)]
    pub part: PartArgs,
    /// Output file. .3mf: one object per colour role (body, accent), each with
    /// its own filament in the slicer, plus traceability metadata. .stl: one
    /// single-colour solid. Missing folders are created.
    #[arg(short = 'o', long, value_name = "FILE")]
    pub out: PathBuf,
    /// 3MF only: write one fused single-colour object instead of one object
    /// per colour role.
    #[arg(long)]
    pub fuse: bool,
}

/// Where the fit profile came from.
pub struct FitChoice {
    pub profile: FitProfile,
    /// `None` for the bundled example.
    pub path: Option<PathBuf>,
}

impl FitChoice {
    fn json(&self) -> Value {
        json!({
            "id": self.profile.id,
            "process": self.profile.process,
            "source": if self.path.is_some() { "file" } else { "example" },
            "path": self.path.as_ref().map(|p| absolute(p).to_string_lossy().into_owned()),
            "note": if self.path.is_none() { Some(EXAMPLE_FIT_NOTE) } else { None },
        })
    }
}

/// Load `--fit`: `example` or a fit profile file.
pub fn load_fit(arg: &str) -> Result<FitChoice, Fail> {
    if arg == "example" {
        return Ok(FitChoice {
            profile: example_fit_profile(),
            path: None,
        });
    }
    let path = PathBuf::from(arg);
    let text = read_text_file(&path, MAX_PROFILE_BYTES, "fit profile").map_err(Fail::Input)?;
    let profile: FitProfile = serde_json::from_str(&text).map_err(|e| {
        Fail::Input(format!(
            "{arg} is not a fit profile ({e}); write one with openbrick calibrate record, or pass --fit example"
        ))
    })?;
    if profile.kind != "fit" {
        return Err(Fail::Input(format!(
            "{arg} is a `{}` document, not a fit profile (kind \"fit\")",
            profile.kind
        )));
    }
    Ok(FitChoice {
        profile,
        path: Some(path),
    })
}

/// The process profile ids the part document allows (`print.profiles`).
fn allowed_processes(doc: &PartDoc) -> Vec<String> {
    doc.print
        .get("profiles")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Pick the process profile: `--process`, else the fit profile's process when
/// the part allows it, else the part's first allowed process, else the first
/// bundled profile.
pub fn choose_process(
    arg: Option<&str>,
    doc: Option<&PartDoc>,
    fit: Option<&FitProfile>,
) -> Result<ProcessProfile, Fail> {
    let all = default_process_profiles();
    let known = |id: &str| all.iter().any(|p| p.id == id);
    let allowed = doc.map(allowed_processes).unwrap_or_default();
    let id = match arg {
        Some(id) => id.to_string(),
        None => fit
            .map(|f| f.process.clone())
            .filter(|p| known(p) && (allowed.is_empty() || allowed.contains(p)))
            .or_else(|| allowed.first().cloned())
            .or_else(|| all.first().map(|p| p.id.clone()))
            .unwrap_or_default(),
    };
    let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
    all.iter().find(|p| p.id == id).cloned().ok_or_else(|| {
        Fail::Input(format!(
            "`{id}` is not a bundled process profile; use one of {} (openbrick profiles lists them)",
            ids.join(", ")
        ))
    })
}

pub(crate) fn eval_fail(e: EvalError) -> Fail {
    match e {
        EvalError::Kernel { .. } => Fail::Internal(format!(
            "{e}. This is a bug; please report it with the part file and parameters"
        )),
        other => Fail::Input(other.to_string()),
    }
}

/// Milliseconds since `t`.
fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1e6).round() / 1e3
}

/// A part read, evaluated and validated.
struct Evaluated {
    doc: PartDoc,
    process: ProcessProfile,
    fit: Option<FitChoice>,
    built: Built,
    report: Report,
    params: Value,
    timings: serde_json::Map<String, Value>,
}

fn evaluate_part(args: &PartArgs) -> Result<Evaluated, Fail> {
    let mut timings = serde_json::Map::new();
    let t = Instant::now();
    let text =
        read_text_file(&args.file, MAX_DOCUMENT_BYTES, "part document").map_err(Fail::Input)?;
    let doc =
        PartDoc::parse(&text).map_err(|e| Fail::Input(format!("{}: {e}", args.file.display())))?;
    let pairs: Vec<String> = args.param.iter().chain(&args.params).cloned().collect();
    let cli = params::parse(&doc, &pairs).map_err(Fail::Input)?;
    let part_dir = match args.file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let mut assets = FileAssets::new(part_dir);
    for path in &cli.asset_paths {
        assets.add_given(path).map_err(Fail::Input)?;
    }
    let fit = args.fit.as_deref().map(load_fit).transpose()?;
    let process = choose_process(
        args.process.as_deref(),
        Some(&doc),
        fit.as_ref().map(|f| &f.profile),
    )?;
    timings.insert("readMs".into(), json!(ms(t)));

    let t = Instant::now();
    let built = evaluate(
        &ManifoldRust,
        &EvalInput {
            doc: &doc,
            params: &cli.values,
            process: &process,
            fit: fit.as_ref().map(|f| &f.profile),
            assets: &assets,
            skip_unsupported: false,
        },
    )
    .map_err(eval_fail)?;
    timings.insert("evaluateMs".into(), json!(ms(t)));

    let t = Instant::now();
    let mut report = validate(&built, &process, fit.as_ref().map(|f| &f.profile));
    report.sort();
    timings.insert("validateMs".into(), json!(ms(t)));
    let params = resolve_params(&doc, &cli.values, &process)
        .map(|r| json!(r.values))
        .unwrap_or(Value::Null);
    Ok(Evaluated {
        doc,
        process,
        fit,
        built,
        report,
        params,
        timings,
    })
}

/// The artwork summary for JSON: everything but the removed polygons, which
/// are counted instead.
fn artwork_json(built: &Built) -> Value {
    let Some(a) = &built.artwork else {
        return Value::Null;
    };
    let mut v = serde_json::to_value(a).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.remove("removed");
        m.insert("removedRegions".into(), json!(a.removed.len()));
    }
    v
}

/// Whether the part prints with its top face on the bed (inlay).
fn face_down(built: &Built) -> bool {
    built
        .artwork
        .as_ref()
        .is_some_and(|a| a.face_on_bed == "top")
}

/// The first layer whose middle lies above `z` mm, with a first layer `first`
/// mm thick and `layer` mm layers after it: (1-based layer number, height
/// where it starts). Slicers give a layer the outline at its middle, so this
/// is the first layer that holds nothing below `z`.
pub fn first_layer_above(z: f64, first: f64, layer: f64) -> (u32, f64) {
    if first / 2.0 > z || layer <= 0.0 {
        return (1, 0.0);
    }
    // Layer k >= 2 spans first + (k - 2) layer .. first + (k - 1) layer.
    let k = ((z - first) / layer + 1.5 + 1e-9).floor().max(1.0) + 1.0;
    (k as u32, first + (k - 2.0) * layer)
}

/// Where a single-extruder filament change goes for a two-colour relief, for
/// each layer height of the process (first layer as thick as the thickest).
fn colour_changes(built: &Built, process: &ProcessProfile) -> Vec<Value> {
    let Some(a) = &built.artwork else {
        return Vec::new();
    };
    if built.objects.len() < 2 {
        return Vec::new();
    }
    // Emboss: the artwork starts at the body top. Inlay (face down): the body
    // starts above the face slab.
    let z = if a.face_on_bed == "top" {
        a.height_mm
    } else {
        a.face_z_mm
    };
    let first = process
        .layer_height_mm
        .iter()
        .copied()
        .fold(0.0_f64, f64::max);
    process
        .layer_height_mm
        .iter()
        .map(|&h| {
            let (layer, start) = first_layer_above(z, first, h);
            let end = start + if layer == 1 { first } else { h };
            json!({
                "firstLayerMm": first,
                "layerHeightMm": h,
                "layer": layer,
                "startsAtMm": (start * 1e6).round() / 1e6,
                // Slicer layer sliders show a layer by its top.
                "endsAtMm": (end * 1e6).round() / 1e6,
            })
        })
        .collect()
}

fn header(ev: &Evaluated) -> String {
    let fit = match &ev.fit {
        Some(f) => format!("fit profile {}", f.profile.id),
        None => "no fit profile".into(),
    };
    format!(
        "{} {} ({}) on {} with {fit}",
        ev.doc.id, ev.doc.version, ev.doc.name, ev.process.id
    )
}

fn params_line(params: &Value) -> String {
    match params {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => format!("{k}={s}"),
                other => format!("{k}={other}"),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// The JSON fields build and validate share.
fn base_json(command: &str, code: u8, ev: &Evaluated) -> serde_json::Map<String, Value> {
    let m = &ev.built.measurements;
    let size: Vec<f64> = (0..3)
        .map(|k| m.bounds_max_mm[k] - m.bounds_min_mm[k])
        .collect();
    let volume: f64 = ev.built.objects.iter().map(|o| o.volume_mm3).sum();
    let mut out = serde_json::Map::new();
    out.insert("command".into(), json!(command));
    out.insert("generator".into(), json!(GENERATOR));
    out.insert("ok".into(), json!(code == 0));
    out.insert("exitCode".into(), json!(code));
    out.insert(
        "part".into(),
        json!({
            "id": ev.doc.id,
            "version": ev.doc.version,
            "name": ev.doc.name,
        }),
    );
    out.insert("params".into(), ev.params.clone());
    out.insert("process".into(), json!(ev.process.id));
    out.insert(
        "fitProfile".into(),
        ev.fit.as_ref().map(FitChoice::json).unwrap_or(Value::Null),
    );
    out.insert("contentHash".into(), json!(ev.built.content_hash));
    out.insert("kernel".into(), json!(ev.built.kernel_id));
    out.insert("exportAllowed".into(), json!(ev.report.export_allowed()));
    out.insert(
        "report".into(),
        serde_json::to_value(&ev.report).unwrap_or(Value::Null),
    );
    out.insert(
        "measurements".into(),
        serde_json::to_value(m).unwrap_or(Value::Null),
    );
    out.insert("warnings".into(), json!(ev.built.warnings));
    out.insert("artwork".into(), artwork_json(&ev.built));
    out.insert(
        "summary".into(),
        json!({
            "sizeMm": size,
            "triangles": ev.built.objects.iter().map(|o| o.mesh.triangle_count()).sum::<usize>(),
            "objects": ev.built.objects.iter().map(|o| o.role.clone()).collect::<Vec<_>>(),
            "volumeMm3": volume,
            "massGPla": volume / 1000.0 * PLA_DENSITY_G_CM3,
            "faceOnBed": ev.built.artwork.as_ref().map(|a| a.face_on_bed.clone()).unwrap_or_else(|| "bottom".into()),
        }),
    );
    out
}

/// Evaluator notes and validator warnings, which never block.
fn print_notes(ui: Ui, ev: &Evaluated) {
    let warnings: Vec<_> = ev
        .report
        .findings
        .iter()
        .filter(|f| f.severity == openbrick_print::Severity::Warning)
        .collect();
    if !warnings.is_empty() {
        ui.say(format!(
            "Validator warnings ({}; they never block):",
            warnings.len()
        ));
        for f in warnings {
            ui.say(format!(
                "  {}",
                crate::ui::finding_text(f).replace('\n', "\n  ")
            ));
        }
    }
    if !ev.built.warnings.is_empty() {
        ui.say(format!("Evaluator notes ({}):", ev.built.warnings.len()));
        for w in &ev.built.warnings {
            ui.say(format!("  - {w}"));
        }
    }
}

/// `openbrick validate`.
pub fn validate_cmd(args: &PartArgs, ui: Ui) -> Result<Outcome, Fail> {
    let total = Instant::now();
    let ev = evaluate_part(args)?;
    let blockers = ev.report.blockers().count();
    let warnings = ev.report.findings.len() - blockers;
    let code = if blockers > 0 { EXIT_BLOCKED } else { 0 };
    ui.say(format!(
        "Validated {}: {blockers} blocker(s), {warnings} warning(s).",
        header(&ev)
    ));
    if ev.fit.as_ref().is_some_and(|f| f.path.is_none()) {
        ui.say(format!("  note: {EXAMPLE_FIT_NOTE}."));
    }
    print_findings(&ev.report, |t| {
        ui.say(format!("  {}", t.replace('\n', "\n  ")))
    });
    if !ev.built.warnings.is_empty() {
        ui.say(format!("Evaluator notes ({}):", ev.built.warnings.len()));
        for w in &ev.built.warnings {
            ui.say(format!("  - {w}"));
        }
    }
    ui.say(if blockers > 0 {
        "Export is blocked until the blockers are fixed (warnings never block)."
    } else {
        "Export allowed."
    });
    let mut out = base_json("validate", code, &ev);
    let mut timings = ev.timings.clone();
    timings.insert("totalMs".into(), json!(ms(total)));
    out.insert("timingsMs".into(), Value::Object(timings));
    Ok(Outcome {
        code,
        json: Value::Object(out),
    })
}

/// `openbrick build`.
pub fn build(args: &BuildArgs, ui: Ui) -> Result<Outcome, Fail> {
    let total = Instant::now();
    let format = Format::of(&args.out)?;
    if args.fuse && format == Format::Stl {
        return Err(Fail::Input(
            "--fuse is for 3MF output; an STL is always one fused single-colour solid".into(),
        ));
    }
    let ev = evaluate_part(&args.part)?;
    let blockers = ev.report.blockers().count();
    if blockers > 0 {
        ui.err(format!(
            "Not written: the validator found {blockers} blocker(s) in {}; {} was not touched.",
            header(&ev),
            args.out.display()
        ));
        print_findings(&ev.report, |t| {
            ui.err(format!("  {}", t.replace('\n', "\n  ")))
        });
        ui.err("Fix the blockers and build again (warnings never block).");
        let mut out = base_json("build", EXIT_BLOCKED, &ev);
        out.insert("outputs".into(), json!([]));
        let mut timings = ev.timings.clone();
        timings.insert("totalMs".into(), json!(ms(total)));
        out.insert("timingsMs".into(), Value::Object(timings));
        return Ok(Outcome {
            code: EXIT_BLOCKED,
            json: Value::Object(out),
        });
    }

    let t = Instant::now();
    let mut solids = solids_for(&ev, format, args.fuse)?;
    let turned = face_down(&ev.built);
    if turned {
        export::turn_face_down(&mut solids);
    }
    let meta = ExportMeta {
        part_id: ev.doc.id.clone(),
        version: ev.doc.version.clone(),
        content_hash: ev.built.content_hash.clone(),
        fit_profile_id: ev
            .fit
            .as_ref()
            .map(|f| f.profile.id.clone())
            .unwrap_or_else(|| "none".into()),
        process_id: ev.process.id.clone(),
        generator: GENERATOR.into(),
        kernel_id: ev.built.kernel_id.clone(),
        title: ev.doc.name.clone(),
        serial: None,
    };
    let (bytes, entry) = export::encode(&solids, &meta, format)?;
    let entry = export::write(&args.out, &bytes, entry)?;
    let mut timings = ev.timings.clone();
    timings.insert("writeMs".into(), json!(ms(t)));
    timings.insert("totalMs".into(), json!(ms(total)));

    // The human summary.
    let summary = base_json("build", 0, &ev);
    let s = &summary["summary"];
    let size: Vec<String> = s["sizeMm"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_f64)
                .map(|v| format!("{v:.2}"))
                .collect()
        })
        .unwrap_or_default();
    ui.say(format!("Built {}.", header(&ev)));
    if ev.fit.as_ref().is_some_and(|f| f.path.is_none()) {
        ui.say(format!("  note: {EXAMPLE_FIT_NOTE}."));
    }
    ui.say(format!("  params      {}", params_line(&ev.params)));
    ui.say(format!("  size        {} mm", size.join(" x ")));
    let written: Vec<String> = solids
        .iter()
        .map(|o| format!("{} {} triangles", o.role, o.mesh.triangle_count()))
        .collect();
    ui.say(format!(
        "  objects     {} ({})",
        solids.len(),
        written.join(", ")
    ));
    let volume = s["volumeMm3"].as_f64().unwrap_or(0.0);
    ui.say(format!(
        "  volume      {volume:.0} mm3, about {:.1} g of PLA ({PLA_DENSITY_G_CM3} g/cm3)",
        s["massGPla"].as_f64().unwrap_or(0.0)
    ));
    let changes = colour_changes(&ev.built, &ev.process);
    if let Some(a) = &ev.built.artwork {
        let how = if turned {
            "turned face down in the file: the artwork face is on the bed"
        } else {
            "prints flat side down, artwork up, as written"
        };
        ui.say(format!(
            "  artwork     {} {} mm; {how}",
            a.mode,
            num(a.height_mm)
        ));
    }
    if !changes.is_empty() && format == Format::ThreeMf && !args.fuse {
        ui.say("  colours     multi-material (tool changer, AMS): give each object its own filament in the slicer.");
        for c in &changes {
            let mm = |key: &str| format!("{:.2}", c[key].as_f64().unwrap_or(0.0));
            ui.say(format!(
                "              one extruder, {} mm first layer and {} mm layers: filament change before layer {} ({} to {} mm)",
                num(c["firstLayerMm"].as_f64().unwrap_or(0.0)),
                num(c["layerHeightMm"].as_f64().unwrap_or(0.0)),
                c["layer"],
                mm("startsAtMm"),
                mm("endsAtMm"),
            ));
        }
        ui.say("              (other layer settings: before the first layer the slicer preview shows in the second colour)");
    }
    ui.say(format!("  hash        {}", ev.built.content_hash));
    ui.say(format!(
        "  wrote       {} ({} bytes, {})",
        entry["path"].as_str().unwrap_or_default(),
        entry["bytes"],
        format.name().to_uppercase()
    ));
    if format == Format::Stl && ev.built.objects.len() > 1 {
        ui.say("  note: an STL has no colours, so every colour role is one solid here; write a .3mf for two colours.");
    }
    print_notes(ui, &ev);

    let mut out = summary;
    out.insert(
        "orientation".into(),
        json!({
            "faceOnBed": if turned { "top" } else { "bottom" },
            "turnedFaceDown": turned,
        }),
    );
    out.insert("colourChanges".into(), Value::Array(changes));
    out.insert("outputs".into(), json!([entry]));
    out.insert("timingsMs".into(), Value::Object(timings));
    Ok(Outcome {
        code: 0,
        json: Value::Object(out),
    })
}

/// The solids to write: one per colour role, or the fused single-colour solid
/// (STL, or 3MF with --fuse).
fn solids_for(ev: &Evaluated, format: Format, fuse: bool) -> Result<Vec<Solid>, Fail> {
    let one = format == Format::Stl || fuse;
    if one {
        let mesh = match (&ev.built.fused, ev.built.objects.as_slice()) {
            (Some(f), _) => &f.mesh,
            (None, [only]) => &only.mesh,
            _ => {
                return Err(Fail::Internal(
                    "the evaluator returned several objects but no fused solid; this is a bug, please report it".into(),
                ))
            }
        };
        return Ok(vec![Solid {
            name: ev.doc.id.clone(),
            role: "body".into(),
            mesh: mesh.clone(),
        }]);
    }
    if ev.built.objects.is_empty() {
        return Err(Fail::Internal(
            "the evaluator returned no objects; this is a bug, please report it".into(),
        ));
    }
    Ok(ev
        .built
        .objects
        .iter()
        .map(|o| Solid {
            name: format!("{} {}", ev.doc.id, o.role),
            role: o.role.clone(),
            mesh: o.mesh.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_that_holds_only_the_second_colour() {
        // Emboss on a plate-height tile: the body top is at 3.2 mm. With a
        // 0.2 mm first layer and 0.2 mm layers, layer 16 ends at 3.2 and layer
        // 17 starts there; with 0.16 mm layers, layer 20 spans 3.08 to 3.24
        // (middle 3.16, still body) and layer 21 starts at 3.24.
        let (k, z) = first_layer_above(3.2, 0.2, 0.2);
        assert_eq!(k, 17);
        assert!((z - 3.2).abs() < 1e-9);
        let (k, z) = first_layer_above(3.2, 0.2, 0.16);
        assert_eq!(k, 21);
        assert!((z - 3.24).abs() < 1e-9);
        // Inlay face slab of 0.4 mm: layers 1 and 2 hold the slab either way.
        assert_eq!(first_layer_above(0.4, 0.2, 0.2).0, 3);
        assert_eq!(first_layer_above(0.4, 0.2, 0.16).0, 3);
        // A slab thinner than half the first layer leaves none of its own.
        assert_eq!(first_layer_above(0.05, 0.2, 0.2), (1, 0.0));
    }

    #[test]
    fn process_defaults() {
        let doc = PartDoc::parse(include_str!("../../../parts/ndn.logo-tile.obpart.json"))
            .expect("parses");
        assert_eq!(
            choose_process(None, Some(&doc), None).map(|p| p.id).ok(),
            Some("fdm-0.4".into())
        );
        let mut fit = example_fit_profile();
        fit.process = "fdm-0.2".into();
        assert_eq!(
            choose_process(None, Some(&doc), Some(&fit))
                .map(|p| p.id)
                .ok(),
            Some("fdm-0.2".into())
        );
        assert_eq!(
            choose_process(Some("resin-msla"), Some(&doc), Some(&fit))
                .map(|p| p.id)
                .ok(),
            Some("resin-msla".into())
        );
        let e = choose_process(Some("fdm-0.6"), Some(&doc), None)
            .err()
            .map(|f| f.message().to_string());
        assert!(e.is_some_and(|m| m.contains("fdm-0.4, fdm-0.2, resin-msla")));
    }
}
