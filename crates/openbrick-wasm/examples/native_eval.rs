//! The browser playground's numbers, computed natively by the same Rust functions
//! the WebAssembly core exports (`evaluate_part`, `build_coupon`), for the smoke
//! test to compare with what the page reports (apps/web/smoke.mjs):
//!
//! ```text
//! cargo run -q --release -p openbrick-wasm --example native_eval
//! ```
//!
//! Prints one JSON object: the kernel, then for each case of the logo tile on
//! fdm-0.4 the parameter overrides, the content hash, the triangle count, each
//! object's role, triangles and fingerprint, and the blocking rules; then the
//! default coupon's content hash, triangles, fingerprint and cell labels. The
//! cases are the playground's default artwork, the Neuraldeep.net mark drawn
//! for 2x4 (emboss, deboss, the filament-swap inlay and the multi-material
//! parts inlay with the example fit profile, emboss without a fit profile and
//! emboss with the fit profile the wizard records), the Neuraldeep.net badge
//! on a 4x4 tile and the CC0 sample mark (both emboss with the example fit).
//! The artwork is the core's own bundle (`artwork_samples_json`), under the
//! asset names the page sends, so the content hashes agree. Under `exports`:
//! the files `export_part` and `export_coupon` write for the downloads the
//! smoke test makes (the emboss 3MF and STL, the face-down swap and parts
//! inlay 3MFs and the badge 3MF, all with the example fit, the default
//! coupon 3MF, the fit profile the calibration wizard records and the emboss
//! 3MF built with that profile), each with its download name (the page's:
//! `{id}-{w}x{h}-{relief}[-{inlayStyle}]-{sample}`), byte count and SHA-256,
//! so the downloaded files must be byte-identical; the STL also with its
//! triangle count, and each 3MF with its number of build items and components
//! (the part-designer layout: one item, a component per colour role). Under
//! `record`: the wizard's choices the smoke test makes (process, pocket set,
//! printer, material, nozzle, date and the stud, bore and pocket cells, every
//! value from the coupon data), and the fit profile `record_fit` writes for
//! them, which `crates/openbrick/tests/record_parity.rs` checks is the file
//! `openbrick calibrate record` writes, byte for byte; the part case
//! `emboss-recorded-fit` is the default tile with that profile. Content
//! hashes are the same on every target; triangle counts, fingerprints and file
//! hashes are bit-exact on x86-64 Linux, where CI runs the smoke test.

use std::time::Instant;

use openbrick_io::threemf_model_xml_with_layout;
use openbrick_io::ExportObject;
use openbrick_print::coupon_v1;
use openbrick_wasm::part::PART_EXPORT_LAYOUT;
use openbrick_wasm::{
    artwork_samples_json, build_coupon, evaluate_part, example_fit_profile_json, export_coupon,
    export_part, export_solids, logo_tile_part_json, record_fit, AssetSet,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Byte count and SHA-256 (hex) of a file, under its download name.
fn file(name: &str, bytes: &[u8]) -> Value {
    let hash: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({ "name": name, "bytes": bytes.len(), "sha256": hash })
}

/// Build items and components of the model part `export_part` writes for a
/// 3MF of the logo tile with `params` (example fit), from the same solids.
fn layout_counts(doc: &str, params: &str, fit: &str, assets: &AssetSet) -> Value {
    let export = export_solids("3mf", doc, params, "fdm-0.4", fit, assets, "")
        .unwrap_or_else(|e| panic!("3mf {params}: {e}"));
    let objects: Vec<ExportObject> = export
        .solids
        .iter()
        .map(|s| ExportObject::new(s.name.clone(), s.role.clone(), Some(s.color), &s.mesh))
        .collect();
    let xml = threemf_model_xml_with_layout(&objects, &export.meta, PART_EXPORT_LAYOUT)
        .expect("the model part");
    json!({
        "items": xml.matches("<item ").count(),
        "components": xml.matches("<component ").count(),
    })
}

/// The asset name of sample `id`'s drawing for `tile` (`None`: a drawing for
/// any tile).
fn sample_asset(samples: &[Value], id: &str, tile: Option<[u32; 2]>) -> String {
    let sample = samples
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("no sample {id}"));
    sample["files"]
        .as_array()
        .expect("files")
        .iter()
        .find(|f| tile.map_or(f["tile"].is_null(), |t| f["tile"] == json!(t)))
        .and_then(|f| f["asset"].as_str())
        .unwrap_or_else(|| panic!("{id} has no drawing for {tile:?}"))
        .to_string()
}

fn main() {
    let doc = logo_tile_part_json();
    let fit = example_fit_profile_json();
    let samples: Vec<Value> =
        serde_json::from_str(&artwork_samples_json()).expect("the bundled samples");
    // Every drawing the core bundles, under the names the page sends; the
    // content hash covers only the assets an evaluation reads.
    let mut assets = AssetSet::new();
    for f in samples
        .iter()
        .flat_map(|s| s["files"].as_array().expect("files"))
    {
        let (asset, svg) = (f["asset"].as_str(), f["svg"].as_str());
        assets
            .add(asset.expect("asset"), svg.expect("svg").as_bytes().to_vec())
            .expect("the samples fit the limit");
    }
    // The playground's default: the Neuraldeep.net mark drawn for 2x4, on the
    // logo tile's default 2x4.
    let mark = sample_asset(&samples, "neuraldeep-mark", Some([2, 4]));
    let badge = sample_asset(&samples, "neuraldeep-badge", Some([4, 4]));
    let cc0 = sample_asset(&samples, "sample", None);

    // The calibration wizard's record step as the smoke test drives it: the
    // default coupon (fdm-0.4, cross pockets), the first printer suggestion,
    // PLA, the nozzle field's default, a fixed date, and a stud, a bore and a
    // pocket cell of that coupon.
    let spec = coupon_v1();
    let (od, rib) = *spec
        .printed_pockets("cross")
        .expect("the cross")
        .last()
        .expect("a pocket");
    let record_input = json!({
        "process": "fdm-0.4",
        "pocketSet": "cross",
        "printer": "Prusa MINI+",
        "material": "PLA",
        "nozzleMm": openbrick_print::DEFAULT_NOZZLE_MM,
        "date": "2026-11-12",
        "cells": {
            "stud": spec.cell("stud").expect("stud").values()[4],
            "tube": spec.cell("tube").expect("tube").values()[3],
            "pocketTubeOD": od,
            "pocketRib": rib,
        },
    });
    let recorded: Value = serde_json::from_str(
        &record_fit(&record_input.to_string()).expect("the record input is valid"),
    )
    .expect("record_fit answers JSON");
    assert_eq!(recorded["ok"], true, "the wizard's choices: {recorded}");
    let recorded_fit = recorded["text"].as_str().expect("the file").to_string();

    let cases = [
        ("emboss", json!({ "logo": mark }), "example"),
        (
            "deboss",
            json!({ "logo": mark, "relief": "deboss" }),
            "example",
        ),
        (
            "inlay",
            json!({ "logo": mark, "relief": "inlay" }),
            "example",
        ),
        (
            "inlay-parts",
            json!({ "logo": mark, "relief": "inlay", "inlayStyle": "parts" }),
            "example",
        ),
        ("emboss-no-fit", json!({ "logo": mark }), "none"),
        (
            "badge-emboss",
            json!({ "logo": badge, "size": [4, 4] }),
            "example",
        ),
        ("sample-emboss", json!({ "logo": cc0 }), "example"),
        ("emboss-recorded-fit", json!({ "logo": mark }), "recorded"),
    ];
    let fit_text = |which: &str| -> String {
        match which {
            "example" => fit.clone(),
            "recorded" => recorded_fit.clone(),
            _ => String::new(),
        }
    };
    let mut out = serde_json::Map::new();
    for (name, params, which_fit) in cases {
        let started = Instant::now();
        let mut part = evaluate_part(
            &doc,
            &params.to_string(),
            "fdm-0.4",
            &fit_text(which_fit),
            &assets,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let ms = started.elapsed().as_secs_f64() * 1e3;
        let meshes: Vec<Value> = (0..part.mesh_count())
            .filter_map(|i| part.take_mesh(i))
            .map(|m| {
                json!({
                    "role": m.role(),
                    "triangles": m.triangle_count(),
                    "fingerprint": m.fingerprint(),
                    "canonicalFingerprint": m.canonical_fingerprint(),
                })
            })
            .collect();
        let report: Value = serde_json::from_str(&part.report_json()).expect("report");
        let findings = report["findings"].as_array().expect("findings");
        let rules = |blocker: bool| -> Vec<Value> {
            findings
                .iter()
                .filter(|f| (f["severity"] == "blocker") == blocker)
                .map(|f| f["rule"].clone())
                .collect()
        };
        out.insert(
            name.to_string(),
            json!({
                "params": params,
                "fit": which_fit,
                "contentHash": part.content_hash(),
                "triangles": part.triangle_count(),
                "meshes": meshes,
                "exportAllowed": part.export_allowed(),
                "blockers": rules(true),
                "warnings": rules(false),
                "removedOverlay": part.take_removed().is_some(),
                "ms": (ms * 10.0).round() / 10.0,
            }),
        );
    }
    // The files the smoke test downloads, written natively by the same functions.
    let export = |format: &str, params: &str| {
        export_part(format, &doc, params, "fdm-0.4", &fit, &assets, "")
            .unwrap_or_else(|e| panic!("{format} {params}: {e}"))
    };
    // A 3MF download with its layout counts.
    let threemf = |name: &str, params: &str| {
        let mut f = file(name, &export("3mf", params));
        f["layout"] = layout_counts(&doc, params, &fit, &assets);
        f
    };
    let params = |case: &str| out[case]["params"].to_string();
    let [emboss, inlay, inlay_parts, badge] =
        ["emboss", "inlay", "inlay-parts", "badge-emboss"].map(params);
    let stl = export("stl", &emboss);
    let stl_triangles = u32::from_le_bytes(stl[80..84].try_into().expect("an STL header"));
    let mut stl_file = file("ndn.logo-tile-2x4-emboss-neuraldeep.stl", &stl);
    stl_file["triangles"] = json!(stl_triangles);

    let started = Instant::now();
    let mut coupon = build_coupon("fdm-0.4", "").expect("the default coupon builds");
    let ms = started.elapsed().as_secs_f64() * 1e3;
    let mesh = coupon.take_mesh().expect("mesh");
    let coupon_cells: Vec<Value> =
        serde_json::from_str(&coupon.cells_json()).expect("the coupon cells");
    let coupon_labels: Vec<Value> = coupon_cells.iter().map(|c| c["label"].clone()).collect();
    let coupon_3mf = export_coupon("3mf", "fdm-0.4", "", "").expect("the coupon exports");
    let mut fit_file = file(
        recorded["fileName"].as_str().expect("a file name"),
        recorded_fit.as_bytes(),
    );
    fit_file["text"] = json!(recorded_fit);
    let recorded_3mf = export_part("3mf", &doc, &emboss, "fdm-0.4", &recorded_fit, &assets, "")
        .unwrap_or_else(|e| panic!("3mf with the recorded fit: {e}"));
    let exports = json!({
        "emboss3mf": threemf("ndn.logo-tile-2x4-emboss-neuraldeep.3mf", &emboss),
        "embossStl": stl_file,
        "inlay3mf": threemf("ndn.logo-tile-2x4-inlay-swap-neuraldeep.3mf", &inlay),
        "inlayParts3mf": threemf("ndn.logo-tile-2x4-inlay-parts-neuraldeep.3mf", &inlay_parts),
        "badge3mf": threemf("ndn.logo-tile-4x4-emboss-neuraldeep-badge.3mf", &badge),
        "coupon3mf": file(&format!("{}.3mf", coupon.file_stem()), &coupon_3mf),
        "fitProfile": fit_file,
        "recorded3mf": file("ndn.logo-tile-2x4-emboss-neuraldeep.3mf", &recorded_3mf),
    });
    let result = json!({
        "kernel": openbrick_wasm::kernel_id(),
        "part": out,
        "coupon": {
            "process": "fdm-0.4",
            "fileStem": coupon.file_stem(),
            "contentHash": coupon.content_hash(),
            "triangles": coupon.triangle_count(),
            "fingerprint": mesh.fingerprint(),
            "ms": (ms * 10.0).round() / 10.0,
            // The embossed text of every cell, in layout order: the wizard's
            // map shows these labels.
            "labels": coupon_labels,
        },
        "exports": exports,
        "record": {
            "input": record_input,
            "id": recorded["fitProfile"]["id"],
            "fileName": recorded["fileName"],
            "offsets": recorded["offsets"],
        },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("serialises")
    );
}
