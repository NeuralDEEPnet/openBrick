//! The browser playground's numbers, computed natively by the same Rust functions
//! the WebAssembly core exports (`evaluate_part`, `build_coupon`), for the smoke
//! test to compare with what the page reports (apps/web/smoke.mjs):
//!
//! ```text
//! cargo run -q --release -p openbrick-wasm --example native_eval
//! ```
//!
//! Prints one JSON object: the kernel, then for each case of the logo tile with
//! its sample artwork on fdm-0.4 (emboss, deboss and inlay with the example fit
//! profile, emboss without a fit profile) the parameter overrides, the content
//! hash, the triangle count, each object's role, triangles and fingerprint, and
//! the blocking rules; then the default coupon's content hash, triangles and
//! fingerprint. Content hashes are the same on every target; triangle counts and
//! fingerprints are bit-exact on x86-64 Linux, where CI runs the smoke test.

use std::time::Instant;

use openbrick_wasm::{
    build_coupon, evaluate_part, example_fit_profile_json, logo_tile_part_json, sample_logo_svg,
    AssetSet,
};
use serde_json::{json, Value};

fn main() {
    let doc = logo_tile_part_json();
    let fit = example_fit_profile_json();
    let mut assets = AssetSet::new();
    assets
        .add("assets/mark.svg", sample_logo_svg().into_bytes())
        .expect("the sample fits the limit");
    let cases = [
        ("emboss", json!({}), true),
        ("deboss", json!({ "relief": "deboss" }), true),
        ("inlay", json!({ "relief": "inlay" }), true),
        ("emboss-no-fit", json!({}), false),
    ];
    let mut out = serde_json::Map::new();
    for (name, params, with_fit) in cases {
        let started = Instant::now();
        let mut part = evaluate_part(
            &doc,
            &params.to_string(),
            "fdm-0.4",
            if with_fit { &fit } else { "" },
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
        let blockers: Vec<Value> = report["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .filter(|f| f["severity"] == "blocker")
            .map(|f| f["rule"].clone())
            .collect();
        out.insert(
            name.to_string(),
            json!({
                "params": params,
                "fit": if with_fit { "example" } else { "none" },
                "contentHash": part.content_hash(),
                "triangles": part.triangle_count(),
                "meshes": meshes,
                "exportAllowed": part.export_allowed(),
                "blockers": blockers,
                "removedOverlay": part.take_removed().is_some(),
                "ms": (ms * 10.0).round() / 10.0,
            }),
        );
    }
    let started = Instant::now();
    let mut coupon = build_coupon("fdm-0.4", "").expect("the default coupon builds");
    let ms = started.elapsed().as_secs_f64() * 1e3;
    let mesh = coupon.take_mesh().expect("mesh");
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
        },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("serialises")
    );
}
