//! The CLI and the browser playground write the same meshes (docs/plan/05
//! section 7, docs/spikes/q18-slicer-multimaterial-import.md).
//!
//! `openbrick build` (run as a user runs it, from the repository root) and
//! `openbrick-wasm`'s export path (`export_solids` and `export_part`, the
//! functions the playground's worker calls, run natively here) evaluate the
//! same part document with the same parameters, process and fit profile. Both
//! lay an inlay face down with `openbrick_geom::turn_face_down` and write a 3MF
//! in the part-designer layout (`ExportLayout::PART_DESIGNER`). The files are
//! not compared as files, because each front end records its own generator,
//! title and display colours; their meshes are:
//!
//! - every object the CLI's `--json` reports (name, role, vertex and triangle
//!   counts, stored fingerprint, bounds) equals the playground's solid;
//! - writing the playground's meshes with the CLI's own metadata and colours
//!   gives the CLI's 3MF byte for byte, so the meshes in the file are exactly
//!   the playground's;
//! - the STL, which records no metadata, is byte-identical.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use openbrick_io::threemf::stored_fingerprint;
use openbrick_io::{write_3mf_with_layout, ExportLayout, ExportMeta, ExportObject};
use openbrick_wasm::{example_fit_profile_json, export_part, export_solids, AssetSet, PartExport};
use serde_json::Value;

const LOGO_TILE: &str = "parts/ndn.logo-tile.obpart.json";
/// The logo tile's default artwork, as the document names it (relative to the
/// part file), and where it is in the repository.
const LOGO_ASSET: &str = "assets/mark.svg";
const LOGO_FILE: &str = "parts/assets/mark.svg";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "openbrick-parity-{}-{}-{name}",
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One way to build the logo tile: the CLI's `--param` values and the same
/// overrides as the playground's JSON object.
struct Case {
    label: &'static str,
    cli: &'static [&'static str],
    json: &'static str,
}

const CASES: [Case; 3] = [
    Case {
        label: "emboss",
        cli: &[],
        json: "{}",
    },
    Case {
        label: "inlay-swap",
        cli: &["relief=inlay"],
        json: r#"{"relief":"inlay"}"#,
    },
    Case {
        label: "inlay-parts",
        cli: &["relief=inlay", "inlayStyle=parts"],
        json: r#"{"relief":"inlay","inlayStyle":"parts"}"#,
    },
];

/// `openbrick build` of the logo tile with the example fit profile into
/// `out`; returns the `--json` object.
fn cli_build(case: &Case, out: &Path) -> Value {
    let mut args: Vec<String> = vec![
        "build".into(),
        LOGO_TILE.into(),
        "--fit".into(),
        "example".into(),
    ];
    for p in case.cli {
        args.push("--param".into());
        args.push((*p).into());
    }
    args.push("--out".into());
    args.push(out.to_string_lossy().into_owned());
    args.push("--json".into());
    let o = Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .args(&args)
        .current_dir(repo())
        .output()
        .expect("the openbrick binary runs");
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}: {}",
        case.label,
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).expect("one JSON object on stdout")
}

/// The playground's inputs: the same part file and artwork bytes the CLI
/// reads, the bundled example fit profile, process fdm-0.4.
struct Playground {
    doc: String,
    assets: AssetSet,
    fit: String,
}

impl Playground {
    fn new() -> Playground {
        let doc = std::fs::read_to_string(repo().join(LOGO_TILE)).expect("the logo tile");
        let mut assets = AssetSet::new();
        assets
            .add(
                LOGO_ASSET,
                std::fs::read(repo().join(LOGO_FILE)).expect("the sample artwork"),
            )
            .expect("the sample is under the asset limit");
        Playground {
            doc,
            assets,
            fit: example_fit_profile_json(),
        }
    }

    fn solids(&self, format: &str, case: &Case) -> PartExport {
        export_solids(
            format,
            &self.doc,
            case.json,
            "fdm-0.4",
            &self.fit,
            &self.assets,
            "",
        )
        .unwrap_or_else(|e| panic!("{} {format}: {e}", case.label))
    }

    fn file(&self, format: &str, case: &Case) -> Vec<u8> {
        export_part(
            format,
            &self.doc,
            case.json,
            "fdm-0.4",
            &self.fit,
            &self.assets,
            "",
        )
        .unwrap_or_else(|e| panic!("{} {format}: {e}", case.label))
    }
}

/// The metadata a CLI 3MF records, read back from its `--json` description.
fn cli_meta(entry: &Value) -> ExportMeta {
    let m = &entry["metadata"];
    let text = |key: &str| m[key].as_str().unwrap_or_default().to_string();
    ExportMeta {
        part_id: text("openbrick:partId"),
        version: text("openbrick:version"),
        content_hash: text("openbrick:contentHash"),
        fit_profile_id: text("openbrick:fitProfile"),
        process_id: text("openbrick:process"),
        generator: text("openbrick:generator"),
        kernel_id: text("openbrick:kernel"),
        title: text("Title"),
        serial: m["openbrick:serial"].as_str().map(str::to_string),
    }
}

/// `#RRGGBBAA` as sRGB plus alpha.
fn rgba(text: &str) -> [u8; 4] {
    let hex = text.strip_prefix('#').expect("a #RRGGBBAA colour");
    let byte = |i: usize| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex");
    [byte(0), byte(1), byte(2), byte(3)]
}

#[test]
fn cli_3mf_meshes_equal_the_playground_export() {
    let dir = Scratch::new("3mf");
    let web = Playground::new();
    for case in &CASES {
        let label = case.label;
        let out = dir.path(&format!("{label}.3mf"));
        let j = cli_build(case, &out);
        let ours = web.solids("3mf", case);
        assert_eq!(j["process"], "fdm-0.4", "{label}");
        assert_eq!(
            j["contentHash"].as_str(),
            Some(ours.meta.content_hash.as_str()),
            "{label}: the same inputs"
        );
        let entry = &j["outputs"][0];
        assert_eq!(entry["layout"], "oneObjectWithParts", "{label}");
        assert_eq!(
            entry["metadata"]["openbrick:fitProfile"].as_str(),
            Some(ours.meta.fit_profile_id.as_str()),
            "{label}: the same fit profile"
        );
        let face_down = j["orientation"]["turnedFaceDown"] == true;
        assert_eq!(face_down, label.starts_with("inlay"), "{label}");

        // Object by object: the CLI's description of what it wrote.
        let objects = entry["objects"].as_array().expect("objects");
        assert_eq!(objects.len(), ours.solids.len(), "{label}");
        for (o, s) in objects.iter().zip(&ours.solids) {
            let what = format!("{label} {}", s.role);
            assert_eq!(o["name"].as_str(), Some(s.name.as_str()), "{what}");
            assert_eq!(o["role"].as_str(), Some(s.role.as_str()), "{what}");
            assert_eq!(
                o["vertices"].as_u64(),
                Some(s.mesh.positions.len() as u64),
                "{what}"
            );
            assert_eq!(
                o["triangles"].as_u64(),
                Some(s.mesh.triangle_count() as u64),
                "{what}"
            );
            assert_eq!(
                o["storedFingerprint"].as_str(),
                Some(format!("{:016x}", stored_fingerprint(&s.mesh)).as_str()),
                "{what}: the mesh differs"
            );
            let (lo, hi) = s.mesh.bounds();
            assert_eq!(o["boundsMm"]["min"], serde_json::json!(lo.map(f64::from)));
            assert_eq!(o["boundsMm"]["max"], serde_json::json!(hi.map(f64::from)));
        }
        // An inlay prints face down in both: its fill or face slab is on the bed.
        if face_down {
            let accent = ours.solids.iter().find(|s| s.role == "accent");
            let accent = accent.expect("an accent part");
            assert_eq!(accent.mesh.bounds().0[2], 0.0, "{label}");
        }

        // The file: the playground's meshes, written with the CLI's metadata
        // and colours in the part-designer layout, are the CLI's 3MF exactly.
        let meta = cli_meta(entry);
        let rebuilt: Vec<ExportObject> = objects
            .iter()
            .zip(&ours.solids)
            .map(|(o, s)| {
                ExportObject::new(
                    s.name.clone(),
                    s.role.clone(),
                    o["color"].as_str().map(rgba),
                    &s.mesh,
                )
            })
            .collect();
        let expected =
            write_3mf_with_layout(&rebuilt, &meta, ExportLayout::PART_DESIGNER).expect("3MF");
        let written = std::fs::read(&out).expect("the CLI wrote the 3MF");
        assert!(
            written == expected,
            "{label}: the CLI's 3MF ({} bytes) is not the playground's meshes in the part-designer layout ({} bytes)",
            written.len(),
            expected.len()
        );
    }
}

#[test]
fn cli_stl_equals_the_playground_stl() {
    // An STL records no metadata, so the whole file must match: the fused
    // single-colour solid, turned face down for an inlay.
    let dir = Scratch::new("stl");
    let web = Playground::new();
    for case in &CASES {
        let out = dir.path(&format!("{}.stl", case.label));
        let j = cli_build(case, &out);
        let written = std::fs::read(&out).expect("the CLI wrote the STL");
        assert_eq!(j["outputs"][0]["format"], "stl");
        let ours = web.file("stl", case);
        assert!(
            written == ours,
            "{}: the CLI's STL ({} bytes) differs from the playground's ({} bytes)",
            case.label,
            written.len(),
            ours.len()
        );
        let solids = web.solids("stl", case);
        assert_eq!(solids.solids.len(), 1, "{}: one fused solid", case.label);
    }
}
