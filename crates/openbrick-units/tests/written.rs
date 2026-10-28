//! The standard written back (`openbrick standard --json`, the WebAssembly
//! `standard_json()`) keeps its attribution.

use openbrick_units::STANDARD;

#[test]
fn written_standard_keeps_its_credit() {
    let written = serde_json::to_value(STANDARD.get()).expect("serialises");
    let author = written["credit"]["author"].as_str().unwrap_or_default();
    assert!(!author.is_empty(), "credit.author is written: {written:#}");
}
