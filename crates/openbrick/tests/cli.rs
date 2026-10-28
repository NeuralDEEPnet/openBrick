//! The `openbrick` binary end to end (docs/plan/05 sections 4, 5 and 9):
//! `build`, `validate` and `calibrate coupon` / `calibrate record`, run as a
//! user runs them, through `std::process::Command`.
//!
//! The commands run from the repository root, so the part path is
//! `parts/ndn.logo-tile.obpart.json` and its default artwork is the sample
//! `parts/assets/mark.svg`, found relative to the part file. Outputs go to a
//! fresh folder under the system temp directory, removed when a test ends.
//! Every number a test compares comes from the coupon data, the Dimensional
//! Standard or the CLI's own JSON, never from a literal.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use openbrick_part::coupon::CouponOptions;
use openbrick_print::{coupon_v1, FitProfile};
use openbrick_units::STANDARD;
use serde_json::Value;

const LOGO_TILE: &str = "parts/ndn.logo-tile.obpart.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "openbrick-cli-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the scratch folder");
        Scratch(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn arg(&self, name: &str) -> String {
        self.path(name).to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn openbrick(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .args(args)
        .current_dir(repo())
        .output()
        .expect("the openbrick binary runs")
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("exited normally, not by a signal")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The one JSON object a `--json` run prints on stdout.
fn json_of(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not one JSON object ({e}):\n{}\nstderr:\n{}",
            stdout(o),
            stderr(o)
        )
    })
}

fn schema(name: &str) -> jsonschema::Validator {
    let text = std::fs::read_to_string(repo().join("schemas").join(name)).expect("schema");
    let raw: Value = serde_json::from_str(&text).expect("schema JSON");
    jsonschema::draft202012::new(&raw).expect("schema compiles")
}

fn assert_valid(validator: &jsonschema::Validator, doc: &Value, what: &str) {
    let errors: Vec<String> = validator.iter_errors(doc).map(|e| e.to_string()).collect();
    assert!(
        errors.is_empty(),
        "{what} breaks its schema: {errors:#?}\n{doc:#}"
    );
}

/// The triangle count in a binary STL header.
fn stl_triangles(bytes: &[u8]) -> usize {
    u32::from_le_bytes(bytes[80..84].try_into().expect("4 bytes")) as usize
}

#[test]
fn builds_the_logo_tile_to_3mf_and_stl() {
    let dir = Scratch::new("build");
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        "example",
        "--out",
        &dir.arg("tile.3mf"),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let threemf = std::fs::read(dir.path("tile.3mf")).expect("the 3MF is written");
    assert!(threemf.starts_with(b"PK"), "a 3MF is a zip package");
    let human = stdout(&o);
    for part in [
        "Built ndn.logo-tile",
        "size",
        "triangles",
        "g of PLA",
        "wrote",
    ] {
        assert!(human.contains(part), "summary lacks `{part}`:\n{human}");
    }
    assert!(
        human.contains("not a calibration of your printer"),
        "--fit example says what it is:\n{human}"
    );

    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        "example",
        "--out",
        &dir.arg("tile.stl"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let stl = std::fs::read(dir.path("tile.stl")).expect("the STL is written");
    let triangles = stl_triangles(&stl);
    assert_eq!(stl.len(), 84 + 50 * triangles, "binary STL layout");
    let j = json_of(&o);
    let out = &j["outputs"][0];
    assert_eq!(out["format"], "stl");
    assert_eq!(out["triangles"].as_u64(), Some(triangles as u64));
    assert_eq!(out["bytes"].as_u64(), Some(stl.len() as u64));
    // One single-colour solid: the fused body and relief, as much plastic as
    // the two colour objects together.
    let fused = out["volumeMm3"].as_f64().expect("volume");
    let parts = j["summary"]["volumeMm3"].as_f64().expect("volume");
    assert!((fused - parts).abs() < 1e-3 * parts, "{fused} vs {parts}");
}

#[test]
fn json_output_follows_the_report_schema() {
    let dir = Scratch::new("json");
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        "example",
        "--out",
        &dir.arg("tile.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    assert_valid(&schema("report.schema.json"), &j["report"], "the report");
    assert_eq!(j["command"], "build");
    assert_eq!(j["ok"], true);
    assert_eq!(j["exitCode"], 0);
    assert_eq!(j["exportAllowed"], true);
    let hash = j["contentHash"].as_str().expect("content hash");
    assert!(hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(j["process"], "fdm-0.4", "the part's first print profile");
    assert_eq!(j["measurements"]["process"], "fdm-0.4");
    assert_eq!(j["fitProfile"]["source"], "example");
    assert!(j["timingsMs"]["totalMs"].as_f64().is_some_and(|t| t > 0.0));
    let out = &j["outputs"][0];
    assert!(Path::new(out["path"].as_str().expect("path")).is_file());
    // What scripts/validate-3mf.py compares with lib3mf: objects per colour
    // role and the traceability metadata.
    let roles: Vec<&str> = out["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .filter_map(|o| o["role"].as_str())
        .collect();
    assert_eq!(roles, ["body", "accent"]);
    let meta = &out["metadata"];
    assert_eq!(meta["openbrick:partId"], "ndn.logo-tile");
    assert_eq!(meta["openbrick:contentHash"], hash);
    assert_eq!(meta["openbrick:fitProfile"], j["fitProfile"]["id"]);
    assert_eq!(meta["openbrick:process"], "fdm-0.4");
    // Human text went to stderr, so stdout held only the object.
    assert!(stderr(&o).contains("Built ndn.logo-tile"));
}

#[test]
fn blockers_write_nothing_and_exit_1() {
    let dir = Scratch::new("blocked");
    // The tube field is fit-critical: without a fit profile, FIT.MISSING.
    let o = openbrick(&["build", LOGO_TILE, "--out", &dir.arg("tile.3mf")]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(!dir.path("tile.3mf").exists(), "nothing is written");
    let err = stderr(&o);
    for part in ["blocker FIT.MISSING", "measured", "threshold", "fix:"] {
        assert!(err.contains(part), "blockers lack `{part}`:\n{err}");
    }

    let o = openbrick(&["build", LOGO_TILE, "--out", &dir.arg("tile.3mf"), "--json"]);
    assert_eq!(code(&o), 1);
    assert!(!dir.path("tile.3mf").exists());
    let j = json_of(&o);
    assert_valid(&schema("report.schema.json"), &j["report"], "the report");
    assert_eq!(j["ok"], false);
    assert_eq!(j["exitCode"], 1);
    assert_eq!(j["exportAllowed"], false);
    assert_eq!(j["outputs"], serde_json::json!([]));
    let findings = j["report"]["findings"].as_array().expect("findings");
    assert!(findings
        .iter()
        .any(|f| f["rule"] == "FIT.MISSING" && f["severity"] == "blocker"));
}

#[test]
fn input_errors_exit_2_and_write_nothing() {
    let dir = Scratch::new("input");
    let out = dir.arg("tile.3mf");
    std::fs::write(dir.path("not-a-fit.json"), "{\"hello\": 1}").expect("write");
    let not_a_fit = dir.arg("not-a-fit.json");
    let folder_as_logo = format!("logo={}", dir.0.to_string_lossy());
    // One byte over the evaluator's SVG limit.
    let big = vec![b' '; openbrick_part::eval::MAX_SVG_BYTES + 1];
    std::fs::write(dir.path("big.svg"), big).expect("write");
    let big_logo = format!("logo={}", dir.arg("big.svg"));
    // A part document may only name assets inside its own folder.
    let escaping = std::fs::read_to_string(repo().join(LOGO_TILE))
        .expect("the logo tile")
        .replace("\"assets/mark.svg\"", "\"../mark.svg\"");
    std::fs::write(dir.path("escaping.obpart.json"), escaping).expect("write");
    let escaping = dir.arg("escaping.obpart.json");
    let cases: Vec<(&str, Vec<&str>, &str)> = vec![
        (
            "a missing part file",
            vec![
                "build",
                "parts/no-such-part.obpart.json",
                "--fit",
                "example",
                "--out",
                &out,
            ],
            "cannot read part document",
        ),
        (
            "a bad footprint",
            vec![
                "build", LOGO_TILE, "--fit", "example", "--param", "size=0x4", "--out", &out,
            ],
            "parameter `size`",
        ),
        (
            "an unknown parameter",
            vec![
                "build",
                LOGO_TILE,
                "--fit",
                "example",
                "--param",
                "colour=red",
                "--out",
                &out,
            ],
            "no parameter `colour`",
        ),
        (
            "not NAME=VALUE",
            vec![
                "build", LOGO_TILE, "--fit", "example", "--param", "size", "--out", &out,
            ],
            "NAME=VALUE",
        ),
        (
            "an enum value that is no option",
            vec![
                "build",
                LOGO_TILE,
                "--fit",
                "example",
                "--param",
                "relief=sparkle",
                "--out",
                &out,
            ],
            "is not one of the options",
        ),
        (
            "a missing SVG",
            vec![
                "build",
                LOGO_TILE,
                "--fit",
                "example",
                "--param",
                "logo=no-such-logo.svg",
                "--out",
                &out,
            ],
            "cannot read asset",
        ),
        (
            "a folder as SVG",
            vec![
                "build",
                LOGO_TILE,
                "--fit",
                "example",
                "--param",
                &folder_as_logo,
                "--out",
                &out,
            ],
            "not a regular file",
        ),
        (
            "an SVG over the size limit",
            vec![
                "build", LOGO_TILE, "--fit", "example", "--param", &big_logo, "--out", &out,
            ],
            "the limit is",
        ),
        (
            "a document asset outside the part's folder",
            vec!["build", &escaping, "--fit", "example", "--out", &out],
            "outside the part's folder",
        ),
        (
            "an unknown output format",
            vec!["build", LOGO_TILE, "--fit", "example", "--out", "tile.obj"],
            "follows the file extension",
        ),
        (
            "an unknown process",
            vec![
                "build",
                LOGO_TILE,
                "--fit",
                "example",
                "--process",
                "fdm-9.9",
                "--out",
                &out,
            ],
            "not a bundled process profile",
        ),
        (
            "a file that is no fit profile",
            vec!["build", LOGO_TILE, "--fit", &not_a_fit, "--out", &out],
            "is not a fit profile",
        ),
        (
            "an op the evaluator does not build",
            vec![
                "build",
                "parts/ndn.nameplate-keychain.obpart.json",
                "--fit",
                "example",
                "--out",
                &out,
            ],
            "does not build yet",
        ),
        (
            "validate of a missing file",
            vec!["validate", "parts/no-such-part.obpart.json"],
            "cannot read part document",
        ),
    ];
    for (what, args, says) in &cases {
        let o = openbrick(args);
        assert_eq!(code(&o), 2, "{what}: {}", stderr(&o));
        let err = stderr(&o);
        assert!(
            err.starts_with("error: ") && err.contains(says),
            "{what}: expected `{says}` in\n{err}"
        );
        assert!(!dir.path("tile.3mf").exists(), "{what}: nothing is written");
        // With --json, stdout still carries one object that says so.
        let mut args = args.clone();
        args.push("--json");
        let o = openbrick(&args);
        assert_eq!(code(&o), 2, "{what} --json");
        let j = json_of(&o);
        assert_eq!(j["ok"], false, "{what}");
        assert_eq!(j["exitCode"], 2, "{what}");
        assert!(
            j["error"].as_str().is_some_and(|e| e.contains(says)),
            "{what}: {j}"
        );
    }
    // A usage error (no --out) is exit 2 too, with a JSON object under --json.
    assert_eq!(code(&openbrick(&["build", LOGO_TILE])), 2);
    let o = openbrick(&["build", LOGO_TILE, "--json"]);
    assert_eq!(code(&o), 2);
    assert_eq!(json_of(&o)["exitCode"], 2);
    // The coupon's manifest may not overwrite the coupon.
    let coupon = dir.arg("c.3mf");
    let o = openbrick(&[
        "calibrate",
        "coupon",
        "--out",
        &coupon,
        "--manifest",
        &coupon,
    ]);
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert!(!dir.path("c.3mf").exists());
}

#[test]
fn validate_prints_the_report_and_sets_the_exit_code() {
    let o = openbrick(&["validate", LOGO_TILE]);
    assert_eq!(code(&o), 1, "no fit profile: FIT.MISSING blocks");
    assert!(stdout(&o).contains("blocker FIT.MISSING"));

    let o = openbrick(&["validate", LOGO_TILE, "--fit", "example", "--json"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    assert_valid(&schema("report.schema.json"), &j["report"], "the report");
    assert_eq!(j["command"], "validate");
    assert_eq!(j["exportAllowed"], true);
    assert!(j.get("outputs").is_none(), "validate writes nothing");

    // A fit profile for another process than the build: FIT.PROCESS.
    let o = openbrick(&[
        "validate",
        LOGO_TILE,
        "--fit",
        "example",
        "--process",
        "fdm-0.2",
        "--json",
    ]);
    assert_eq!(code(&o), 1);
    let j = json_of(&o);
    assert!(j["report"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|f| f["rule"] == "FIT.PROCESS"));
}

#[test]
fn reliefs_are_laid_out_the_way_they_print() {
    let dir = Scratch::new("relief");
    let built = |relief: &str| {
        let o = openbrick(&[
            "build",
            LOGO_TILE,
            "--fit",
            "example",
            "--param",
            &format!("relief={relief}"),
            "--out",
            &dir.arg(&format!("{relief}.3mf")),
            "--json",
        ]);
        assert_eq!(code(&o), 0, "{relief}: {}", stderr(&o));
        json_of(&o)
    };
    let bounds = |j: &Value, role: &str, end: &str, axis: usize| -> f64 {
        j["outputs"][0]["objects"]
            .as_array()
            .expect("objects")
            .iter()
            .find(|o| o["role"] == role)
            .and_then(|o| o["boundsMm"][end][axis].as_f64())
            .unwrap_or_else(|| panic!("no {role} object"))
    };
    let close = |a: f64, b: f64| (a - b).abs() < 1e-5;

    // Emboss prints face up as built: the relief stands on the body top.
    let j = built("emboss");
    assert_eq!(j["orientation"]["turnedFaceDown"], false);
    let top = j["artwork"]["faceZMm"].as_f64().expect("face height");
    assert!(close(bounds(&j, "accent", "min", 2), top));
    assert!(close(bounds(&j, "body", "min", 2), 0.0));
    assert!(!j["colourChanges"].as_array().expect("hints").is_empty());

    // Inlay prints face down: the CLI turns it over, so the accent face slab
    // lies on the bed, as thick as the inlay depth, and the body above it.
    let j = built("inlay");
    assert_eq!(j["orientation"]["turnedFaceDown"], true);
    assert_eq!(j["artwork"]["faceOnBed"], "top");
    let slab = j["artwork"]["heightMm"].as_f64().expect("depth");
    assert!(close(bounds(&j, "accent", "min", 2), 0.0));
    assert!(close(bounds(&j, "accent", "max", 2), slab));
    assert!(close(bounds(&j, "body", "min", 2), slab));
    let height = j["summary"]["sizeMm"][2].as_f64().expect("height");
    assert!(close(bounds(&j, "body", "max", 2), height));
    // The filament change goes at the first layer above the slab.
    for c in j["colourChanges"].as_array().expect("hints") {
        let starts = c["startsAtMm"].as_f64().expect("start");
        let layer = c["layerHeightMm"].as_f64().expect("layer");
        assert!(starts <= slab + 1e-9 && starts + layer / 2.0 > slab, "{c}");
    }
}

#[test]
fn coupon_writes_the_file_and_the_cell_map() {
    let dir = Scratch::new("coupon");
    let o = openbrick(&[
        "calibrate",
        "coupon",
        "--out",
        &dir.arg("coupon.3mf"),
        "--manifest",
        &dir.arg("coupon.json"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let threemf = std::fs::read(dir.path("coupon.3mf")).expect("the coupon is written");
    assert!(threemf.starts_with(b"PK"));
    let j = json_of(&o);
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path("coupon.json")).expect("the manifest is written"),
    )
    .expect("the manifest is JSON");
    assert_eq!(manifest["cells"], j["cells"]);
    assert_eq!(
        manifest["outputs"][0]["path"], "coupon.3mf",
        "the manifest finds the coupon next to itself"
    );
    assert_eq!(j["outputs"][1]["format"], "json");
    assert_eq!(j["summary"]["process"], "fdm-0.4");
    assert_eq!(
        j["outputs"][0]["metadata"]["openbrick:fitProfile"], "none",
        "fit offsets never apply to the coupon"
    );
    // Every cell is in the table, by the values its labels show.
    let table = stderr(&o);
    let spec = coupon_v1();
    for id in ["stud", "tube"] {
        let cell = spec.cell(id).expect("cell");
        assert!(table.contains(&cell.label), "{table}");
        for v in cell.values() {
            assert!(table.contains(&format!("{v:.2}")), "{v} in\n{table}");
        }
    }
    for (od, rib) in spec.printed_pockets("cross").expect("cross") {
        assert!(
            table.contains(&format!("{od:.2}/{rib:.2}")),
            "{od}/{rib} in\n{table}"
        );
    }
    // The default coupon fits the default bed (a Prusa MINI+ is 180 x 180 mm)
    // inside its margins.
    let opts = CouponOptions::default();
    let size = &j["summary"]["footprintMm"];
    for k in 0..2 {
        let room = opts.bed_mm[k] - 2.0 * opts.bed_margin_mm;
        assert!(
            size[k].as_f64().is_some_and(|v| v <= room),
            "{size} on {:?}",
            opts.bed_mm
        );
    }
}

#[test]
fn record_writes_a_fit_profile_that_builds() {
    let dir = Scratch::new("record");
    let spec = coupon_v1();
    let stud = spec.cell("stud").expect("stud").values()[4];
    let tube = spec.cell("tube").expect("tube").values()[3];
    let pockets = spec.printed_pockets("cross").expect("cross");
    let (od, rib) = *pockets.last().expect("a pocket");
    let o = openbrick(&[
        "calibrate",
        "record",
        "--process",
        "fdm-0.4",
        "--printer",
        "Prusa MINI+",
        "--material",
        "PLA",
        "--slicer",
        "PrusaSlicer 2.9",
        "--date",
        "2026-10-28",
        "--stud",
        &format!("{stud:.2}"),
        "--tube",
        &format!("{tube:.2}"),
        "--pocket",
        &format!("{od:.2}/{rib:.2}"),
        "--out",
        &dir.arg("fit.json"),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let text = std::fs::read_to_string(dir.path("fit.json")).expect("written");
    assert!(text.ends_with("}\n"), "pretty JSON with a trailing newline");
    let raw: Value = serde_json::from_str(&text).expect("JSON");
    assert_valid(&schema("fit-profile.schema.json"), &raw, "the fit profile");
    let fit: FitProfile = serde_json::from_value(raw).expect("loads with the print crate");
    assert_eq!(fit.id, "prusa-mini-pla-fdm-0.4");
    assert_eq!(fit.process, "fdm-0.4");
    let cal = fit.calibration.as_ref().expect("calibration");
    assert_eq!(cal.pocket_set.as_deref(), Some("cross"));
    assert_eq!(cal.date, "2026-10-28");
    // Offset = chosen cell minus the standard nominal of its row.
    let std = STANDARD.get();
    for (id, chosen) in [
        ("stud", stud),
        ("tube", tube),
        ("pocketTubeOD", od),
        ("pocketRib", rib),
    ] {
        let cell = spec.cell(id).expect("cell");
        let field = cell.offset_field.as_deref().expect("recordable");
        let want = chosen - std.mm(&cell.standard_id);
        let got = fit.offsets_mm.get(field).expect("offset");
        assert!((got - want).abs() < 5e-4, "{id}: {got} vs {want}");
        assert!((cal.chosen_cells[id] - chosen).abs() < 1e-9, "{id}");
    }
    assert!(stdout(&o).contains("tubeInnerDiameter"), "{}", stdout(&o));

    // The recorded profile builds the fit-critical logo tile.
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        &dir.arg("fit.json"),
        "--out",
        &dir.arg("tile.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    assert_eq!(j["fitProfile"]["id"], "prusa-mini-pla-fdm-0.4");
    assert_eq!(j["fitProfile"]["source"], "file");
    assert_eq!(
        j["outputs"][0]["metadata"]["openbrick:fitProfile"],
        "prusa-mini-pla-fdm-0.4"
    );
    assert!(dir.path("tile.3mf").is_file());

    // Values the coupon did not print are refused, and nothing is written.
    let step = spec.cell("stud").expect("stud").step_mm;
    let off_grid = format!("{:.3}", stud + step / 2.0);
    let o = openbrick(&[
        "calibrate",
        "record",
        "--process",
        "fdm-0.4",
        "--printer",
        "P",
        "--material",
        "PLA",
        "--stud",
        &off_grid,
        "--out",
        &dir.arg("bad.json"),
    ]);
    assert_eq!(code(&o), 2);
    assert!(stderr(&o).contains("is not a cell"), "{}", stderr(&o));
    assert!(!dir.path("bad.json").exists());
    // Half a pocket is refused: both numbers come from one cell.
    let o = openbrick(&[
        "calibrate",
        "record",
        "--process",
        "fdm-0.4",
        "--printer",
        "P",
        "--material",
        "PLA",
        "--pocket-tube-od",
        &format!("{od:.2}"),
        "--out",
        &dir.arg("bad.json"),
    ]);
    assert_eq!(code(&o), 2);
    assert!(!dir.path("bad.json").exists());
}

#[test]
fn existing_commands_are_unchanged() {
    let o = openbrick(&["about"]);
    assert_eq!(code(&o), 0);
    assert!(stdout(&o).contains("not affiliated with"));
    let o = openbrick(&["profiles"]);
    assert!(stdout(&o).contains("fdm-0.4"));
    let o = openbrick(&["check", LOGO_TILE]);
    assert_eq!(code(&o), 0);
    assert!(stdout(&o).starts_with("ok: ndn.logo-tile"));
    let o = openbrick(&["standard", "--json"]);
    assert_eq!(code(&o), 0);
    json_of(&o);
    // Still to come: exit 3 with a pointer to the roadmap.
    assert_eq!(code(&openbrick(&["serve"])), 3);
}
