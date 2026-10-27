//! WASM facade. In M2 this gains `wasm-bindgen` exports (`evaluate_part`, `validate`,
//! `snap_query`, `apply_command`, `export_3mf`) consumed by `packages/core-wasm`.
//! Until then it exposes plain functions so the crate builds on every target.

/// Parse a part document and return its structural errors as JSON (empty array when valid).
pub fn check_part_json(text: &str) -> String {
    match openbrick_part::PartDoc::parse(text) {
        Ok(_) => "[]".to_string(),
        Err(e) => {
            serde_json::to_string(&vec![e.to_string()]).unwrap_or_else(|_| "[\"error\"]".into())
        }
    }
}

/// The Dimensional Standard as JSON for the UI.
pub fn standard_json() -> String {
    serde_json::to_string(openbrick_units::STANDARD.get()).expect("standard serialises")
}
