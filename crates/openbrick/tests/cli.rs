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

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use openbrick_part::coupon::{coupon_layout, Arrangement, CouponOptions};
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
    openbrick_in(&repo(), args)
}

/// Run the binary from another folder, with arguments that need not be UTF-8.
fn openbrick_in<A: AsRef<OsStr>>(dir: &Path, args: &[A]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .args(args)
        .current_dir(dir)
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
    // The part-designer layout (docs/spikes/q18-slicer-multimaterial-import.md):
    // one components object, named after the part, whose parts are the role
    // objects; only it has a build item.
    assert_eq!(out["layout"], "oneObjectWithParts");
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join(LOGO_TILE)).expect("the logo tile"),
    )
    .expect("JSON");
    let assembly = &out["assembly"];
    assert_eq!(assembly["name"], doc["name"]);
    assert_eq!(meta["Title"], doc["name"]);
    assert!(assembly["uuid"].is_string() && assembly["itemUuid"].is_string());
    let components = assembly["components"].as_array().expect("components");
    let objects = out["objects"].as_array().expect("objects");
    assert_eq!(components.len(), objects.len());
    for (c, o) in components.iter().zip(objects) {
        assert_eq!(c["object"], o["name"]);
        assert_eq!(c["uuid"], o["componentUuid"]);
        assert!(o["itemUuid"].is_null(), "a part has no build item: {o}");
    }
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
    let stl = dir.arg("tile.stl");
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
            "--fuse for an STL",
            vec![
                "build", LOGO_TILE, "--fit", "example", "--fuse", "--out", &stl,
            ],
            "--fuse is for 3MF output",
        ),
        (
            "--no-colour for an STL",
            vec![
                "build",
                LOGO_TILE,
                "--fit",
                "example",
                "--no-colour",
                "--out",
                &stl,
            ],
            "--no-colour is for 3MF output",
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
        assert!(!dir.path("tile.stl").exists(), "{what}: nothing is written");
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
    // The filament change goes at the first layer whose middle reaches the
    // top of the slab.
    assert_changes_at_the_slab_top(&j, slab);
}

#[test]
fn a_parts_inlay_is_one_object_with_two_parts_and_no_filament_change() {
    // inlayStyle parts: the logo fill is its own part inside the face, for an
    // AMS or a tool changer. The fill and the face around it share every
    // layer, so there is no single-extruder filament change to give.
    let dir = Scratch::new("parts");
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        "example",
        "--param",
        "relief=inlay",
        "--param",
        "inlayStyle=parts",
        "--out",
        &dir.arg("parts.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    assert_eq!(j["artwork"]["inlayStyle"], "parts");
    assert_eq!(j["orientation"]["turnedFaceDown"], true);
    assert_eq!(j["colourChanges"], serde_json::json!([]));
    let human = stderr(&o);
    for part in [
        "give each part its own filament",
        "No filament change is needed",
        "inlayStyle=swap",
    ] {
        assert!(human.contains(part), "summary lacks `{part}`:\n{human}");
    }
    assert!(!human.contains("filament change before layer"), "{human}");
    let out = &j["outputs"][0];
    assert_eq!(out["layout"], "oneObjectWithParts");
    let parts: Vec<&str> = out["assembly"]["components"]
        .as_array()
        .expect("components")
        .iter()
        .filter_map(|c| c["object"].as_str())
        .collect();
    assert_eq!(parts, ["ndn.logo-tile body", "ndn.logo-tile accent"]);
    // Face down: the fill lies on the bed, as deep as the inlay.
    let accent = &out["objects"][1];
    assert_eq!(accent["role"], "accent");
    let depth = j["artwork"]["heightMm"].as_f64().expect("depth");
    assert_eq!(accent["boundsMm"]["min"][2].as_f64(), Some(0.0));
    let top = accent["boundsMm"]["max"][2].as_f64().expect("top");
    assert!((top - depth).abs() < 1e-5, "{top} vs {depth}");

    // The swap inlay (the default style) keeps its filament change, and its
    // file differs: the face slab is the accent part there.
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        "example",
        "--param",
        "relief=inlay",
        "--out",
        &dir.arg("swap.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let swap = json_of(&o);
    assert_eq!(swap["artwork"]["inlayStyle"], "swap");
    assert!(!swap["colourChanges"].as_array().expect("hints").is_empty());
    assert_ne!(swap["contentHash"], j["contentHash"]);
}

#[test]
fn no_colour_writes_the_parts_without_display_colours() {
    // --no-colour (alias --no-color) for Bambu Studio's import colour dialog
    // (docs/spikes/q18-slicer-multimaterial-import.md, recommendation 2): the
    // same tile, layout and parts, with the user's own fit profile and
    // artwork, and no colour in the file. The default keeps the colours.
    let dir = Scratch::new("no-colour");
    let build = |flag: Option<&str>, name: &str| {
        let out = dir.arg(name);
        let mut args = vec![
            "build",
            LOGO_TILE,
            "--fit",
            "example",
            "--params",
            "relief=inlay",
            "inlayStyle=parts",
            "--out",
            &out,
            "--json",
        ];
        args.extend(flag);
        let o = openbrick(&args);
        assert_eq!(code(&o), 0, "{name}: {}", stderr(&o));
        (json_of(&o), stderr(&o))
    };
    let (coloured, human) = build(None, "coloured.3mf");
    assert!(!human.contains("--no-colour"), "{human}");
    for flag in ["--no-colour", "--no-color"] {
        let (j, human) = build(Some(flag), &format!("{flag}.3mf"));
        assert!(
            human.contains("written without display colours"),
            "{flag}: {human}"
        );
        assert_eq!(j["contentHash"], coloured["contentHash"], "{flag}");
        let (out, with) = (&j["outputs"][0], &coloured["outputs"][0]);
        assert_eq!(out["layout"], "oneObjectWithParts", "{flag}");
        let objects = out["objects"].as_array().expect("objects");
        assert_eq!(objects.len(), 2, "{flag}");
        for (o, w) in objects
            .iter()
            .zip(with["objects"].as_array().expect("objects"))
        {
            assert_eq!(o["color"], Value::Null, "{flag}");
            assert!(w["color"].is_string(), "colours are the default");
            assert_eq!(o["name"], w["name"], "{flag}");
            assert_eq!(o["storedFingerprint"], w["storedFingerprint"], "{flag}");
        }
    }
}

/// Every single-extruder hint of an inlay names the first layer whose middle
/// is at or above the top of the face slab: the layer before it lies below.
fn assert_changes_at_the_slab_top(j: &Value, slab: f64) {
    let changes = j["colourChanges"].as_array().expect("hints");
    assert!(!changes.is_empty(), "{j}");
    for c in changes {
        assert_eq!(c["tie"], "above", "{c}");
        let starts = c["startsAtMm"].as_f64().expect("start");
        let first = c["firstLayerMm"].as_f64().expect("first layer");
        let layer = c["layerHeightMm"].as_f64().expect("layer");
        let k = c["layer"].as_u64().expect("layer number");
        assert!(k >= 2, "{c}");
        let tol = 1e-6;
        assert!(starts <= slab + tol, "{c}");
        assert!(starts + layer / 2.0 >= slab - tol, "{c}");
        let before = if k == 2 {
            first / 2.0
        } else {
            starts - layer / 2.0
        };
        assert!(
            before < slab - tol,
            "the layer before is still the slab: {c}"
        );
    }
}

#[test]
fn a_brick_height_inlay_changes_colour_at_the_tie_layer() {
    // A brick-height inlay keeps the default relief height as its face slab.
    // With a 0.16 mm layer height that slab top lies exactly on a layer
    // middle (0.2 + 2.5 x 0.16 = 0.6 mm), and that layer must already print
    // the body colour, or the floor of the logo comes out in the face colour.
    let dir = Scratch::new("inlay-brick");
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        "example",
        "--params",
        "height=brick",
        "relief=inlay",
        "--out",
        &dir.arg("inlay.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    let slab = j["artwork"]["heightMm"].as_f64().expect("depth");
    assert_changes_at_the_slab_top(&j, slab);
    let tied: Vec<&Value> = j["colourChanges"]
        .as_array()
        .expect("hints")
        .iter()
        .filter(|c| !c["boundaryLayer"].is_null())
        .collect();
    assert_eq!(tied.len(), 1, "one layer height ties: {j}");
    assert_eq!(tied[0]["boundaryLayer"], tied[0]["layer"]);
    assert!(
        stderr(&o).contains("keeps the floor of the logo in the body colour"),
        "{}",
        stderr(&o)
    );
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
    // Copies one behind the other: the table runs from the back copy to the
    // front one.
    assert_eq!(j["summary"]["arrangement"], "stacked");
    assert_lines_back_to_front(&j);
    assert!(
        table.contains("1 back") && table.contains("2 front"),
        "{table}"
    );
}

/// The coupon's lines run from the back (largest y) to the front, and left
/// to right at the same depth.
fn assert_lines_back_to_front(j: &Value) {
    let lines = j["lines"].as_array().expect("lines");
    let cells = j["cells"].as_array().expect("cells");
    assert_eq!(
        lines.len(),
        cells
            .iter()
            .filter_map(|c| c["row"].as_u64())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
    for w in lines.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let (ya, yb) = (
            a["centerYMm"].as_f64().expect("y"),
            b["centerYMm"].as_f64().expect("y"),
        );
        assert!(ya >= yb, "{a} before {b}");
        if ya == yb {
            assert!(a["copy"].as_u64() < b["copy"].as_u64(), "{a} before {b}");
        }
    }
}

#[test]
fn coupon_copies_side_by_side_are_marked_left_and_right() {
    // A bed too shallow for one copy behind the other, but wide enough for
    // two side by side; the layout API (no geometry) confirms it.
    let process = openbrick_print::default_process_profiles()
        .into_iter()
        .find(|p| p.id == "fdm-0.4")
        .expect("fdm-0.4");
    let opts = CouponOptions::default();
    let stacked = coupon_layout(&process, &opts).expect("layout").summary;
    let margin = opts.bed_margin_mm;
    // Twice as wide as the stacked coupon, and three fifths of its depth:
    // more than one copy needs, less than two.
    let bed = [
        2.0 * stacked.footprint_mm[0] + 2.0 * margin,
        stacked.footprint_mm[1] * 3.0 / 5.0 + 2.0 * margin,
    ];
    let side = coupon_layout(
        &process,
        &CouponOptions {
            bed_mm: bed,
            ..opts.clone()
        },
    )
    .expect("layout")
    .summary;
    assert_eq!(side.arrangement, Arrangement::SideBySide);

    let dir = Scratch::new("coupon-side");
    let o = openbrick(&[
        "calibrate",
        "coupon",
        "--bed",
        &format!("{:.3}x{:.3}", bed[0], bed[1]),
        "--out",
        &dir.arg("coupon.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    assert_eq!(j["summary"]["arrangement"], "sideBySide");
    assert_lines_back_to_front(&j);
    let table = stderr(&o);
    assert!(table.contains("side by side"), "{table}");
    assert!(
        table.contains("1 left") && table.contains("2 right"),
        "{table}"
    );
    // Each line of copy 1 has its twin from copy 2 right after it, at the
    // same depth.
    let lines = j["lines"].as_array().expect("lines");
    for pair in lines.chunks(2) {
        assert_eq!(pair[0]["place"], "left", "{pair:?}");
        assert_eq!(pair[1]["place"], "right", "{pair:?}");
        assert_eq!(pair[0]["centerYMm"], pair[1]["centerYMm"]);
        assert_eq!(pair[0]["values"], pair[1]["values"]);
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

#[cfg(unix)]
#[test]
fn arguments_that_are_not_utf8_are_usage_errors() {
    use std::os::unix::ffi::OsStrExt;
    // A Linux file name can be any bytes; such an argument is an input or
    // usage error (exit 2), never a panic (exit 101).
    let bad_part = OsStr::from_bytes(b"parts/\xff.obpart.json");
    let bad = OsStr::from_bytes(b"\xff");
    let dir = Scratch::new("utf8");
    let out = dir.arg("tile.3mf");
    let cases: Vec<Vec<&OsStr>> = vec![
        // A usage error (no --out) on a line that holds a non-UTF-8 argument.
        vec!["build".as_ref(), bad_part],
        // A value clap must read as text.
        vec![
            "build".as_ref(),
            LOGO_TILE.as_ref(),
            "--fit".as_ref(),
            bad,
            "--out".as_ref(),
            out.as_ref(),
        ],
        // A path that is fine as an OS string but names no file.
        vec![
            "build".as_ref(),
            bad_part,
            "--fit".as_ref(),
            "example".as_ref(),
            "--out".as_ref(),
            out.as_ref(),
        ],
    ];
    for args in cases {
        let o = openbrick_in(&repo(), &args);
        assert_eq!(code(&o), 2, "{args:?}: {}", stderr(&o));
        assert!(!stderr(&o).contains("panicked"), "{}", stderr(&o));
        let mut args = args.clone();
        args.push("--json".as_ref());
        let o = openbrick_in(&repo(), &args);
        assert_eq!(code(&o), 2, "{args:?} --json: {}", stderr(&o));
        let j = json_of(&o);
        assert_eq!(j["ok"], false);
        assert_eq!(j["exitCode"], 2);
    }
    assert!(!dir.path("tile.3mf").exists());
}

#[cfg(unix)]
#[test]
fn a_document_asset_may_not_link_out_of_the_part_folder() {
    // The part document names assets/mark.svg, which here is a symbolic link
    // to a file outside the part's folder, as an extracted archive can carry.
    let dir = Scratch::new("symlink");
    std::fs::create_dir_all(dir.path("part/assets")).expect("folder");
    std::fs::create_dir_all(dir.path("outside")).expect("folder");
    std::fs::copy(repo().join(LOGO_TILE), dir.path("part/tile.obpart.json")).expect("copy");
    std::fs::copy(
        repo().join("parts/assets/mark.svg"),
        dir.path("outside/secret.svg"),
    )
    .expect("copy");
    std::os::unix::fs::symlink("../../outside/secret.svg", dir.path("part/assets/mark.svg"))
        .expect("symlink");
    let o = openbrick(&[
        "build",
        &dir.arg("part/tile.obpart.json"),
        "--fit",
        "example",
        "--out",
        &dir.arg("tile.3mf"),
    ]);
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert!(
        stderr(&o).contains("symbolic link") && stderr(&o).contains("outside the part's folder"),
        "{}",
        stderr(&o)
    );
    assert!(!dir.path("tile.3mf").exists());
    // The same file passed on the command line is the user's choice: fine.
    let o = openbrick(&[
        "build",
        &dir.arg("part/tile.obpart.json"),
        "--fit",
        "example",
        "--param",
        &format!("logo={}", dir.arg("outside/secret.svg")),
        "--out",
        &dir.arg("tile.3mf"),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
}

#[test]
fn the_content_hash_does_not_depend_on_how_an_asset_path_is_typed() {
    let dir = Scratch::new("asset-names");
    std::fs::copy(
        repo().join("parts/assets/mark.svg"),
        dir.path("my-logo.svg"),
    )
    .expect("copy");
    let part = repo().join(LOGO_TILE).to_string_lossy().into_owned();
    let absolute = dir.arg("my-logo.svg");
    // The same file, typed three ways, from two folders.
    let runs = [
        (dir.0.clone(), "my-logo.svg".to_string()),
        (dir.0.clone(), "./my-logo.svg".to_string()),
        (repo(), absolute.clone()),
    ];
    let mut hashes = Vec::new();
    for (cwd, path) in &runs {
        let o = openbrick_in(
            cwd,
            &[
                "validate",
                &part,
                "--fit",
                "example",
                "--param",
                &format!("logo={path}"),
                "--json",
            ],
        );
        assert_eq!(code(&o), 0, "{path}: {}", stderr(&o));
        let j = json_of(&o);
        assert_eq!(j["params"]["logo"], "my-logo.svg", "{path}");
        assert_eq!(j["assetFiles"][0]["param"], "logo");
        assert_eq!(j["assetFiles"][0]["name"], "my-logo.svg");
        let file = j["assetFiles"][0]["path"].as_str().expect("path");
        assert!(Path::new(file).is_absolute(), "{file}");
        hashes.push(j["contentHash"].clone());
    }
    assert!(hashes.windows(2).all(|w| w[0] == w[1]), "{hashes:?}");
}

#[test]
fn resin_builds_give_no_filament_advice() {
    let dir = Scratch::new("resin");
    let spec = coupon_v1();
    let stud = spec.cell("stud").expect("stud").values()[4];
    let tube = spec.cell("tube").expect("tube").values()[3];
    let (od, rib) = *spec
        .printed_pockets("cross")
        .expect("cross")
        .last()
        .expect("a pocket");
    let o = openbrick(&[
        "calibrate",
        "record",
        "--process",
        "resin-msla",
        "--printer",
        "Resin printer",
        "--material",
        "tough resin",
        "--date",
        "2026-10-28",
        "--stud",
        &format!("{stud:.2}"),
        "--tube",
        &format!("{tube:.2}"),
        "--pocket",
        &format!("{od:.2}/{rib:.2}"),
        "--out",
        &dir.arg("resin-fit.json"),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    // The process follows the fit profile.
    let o = openbrick(&[
        "build",
        LOGO_TILE,
        "--fit",
        &dir.arg("resin-fit.json"),
        "--out",
        &dir.arg("tile.3mf"),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let j = json_of(&o);
    assert_eq!(j["process"], "resin-msla");
    assert_eq!(j["colourChanges"], serde_json::json!([]));
    let human = stderr(&o);
    assert!(human.contains("ml of resin"), "{human}");
    for fdm in ["filament change", "g of PLA", "elephant"] {
        assert!(!human.contains(fdm), "`{fdm}` in a resin summary:\n{human}");
    }
}
