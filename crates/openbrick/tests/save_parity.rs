//! A part saved by the designer (the playground and the desktop app) builds in
//! the CLI as it built in the designer (docs/plan/06 sections 3, 10 and 11):
//! open `parts/ndn.brand-tile.obpart.json`, edit it, keep the values as a
//! preset and save it with `openbrick_wasm::add_preset_json` and
//! `save_part_json` (the functions the page calls), write the file next to
//! its artwork, and `openbrick build --json` reports the content hash the
//! designer showed for the saved document. Saving without edits changes
//! nothing: the CLI's hash for the saved file is the hash of the original.

use std::path::{Path, PathBuf};
use std::process::Command;

use openbrick_wasm::{
    add_preset_json, evaluate_part, example_fit_profile_json, save_part_json, AssetSet,
};
use serde_json::Value;

const BRAND_TILE: &str = include_str!("../../../parts/ndn.brand-tile.obpart.json");
const MARK: &str = "assets/brand/neuraldeep-mark-2x4.svg";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh folder holding the part's artwork at the path the part names,
/// removed on drop.
struct PartFolder(PathBuf);

impl PartFolder {
    fn new(name: &str) -> PartFolder {
        let dir =
            std::env::temp_dir().join(format!("openbrick-save-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets/brand")).expect("create the folder");
        std::fs::copy(repo().join("parts").join(MARK), dir.join(MARK)).expect("copy the mark");
        PartFolder(dir)
    }

    /// Write `text` as the part file and build it with the example fit
    /// profile; the CLI's JSON answer.
    fn build(&self, text: &str) -> Value {
        let part = self.0.join("ndn.brand-tile.obpart.json");
        std::fs::write(&part, text).expect("write the part");
        let out = Command::new(env!("CARGO_BIN_EXE_openbrick"))
            .arg("build")
            .arg(&part)
            .args(["--fit", "example", "--json", "--out"])
            .arg(self.0.join("tile.3mf"))
            .output()
            .expect("the openbrick binary runs");
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("--json prints one JSON object")
    }
}

impl Drop for PartFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The content hash the designer shows for `doc` with `params`.
fn designer_hash(doc: &str, params: &str) -> String {
    let mut assets = AssetSet::new();
    let svg = std::fs::read(repo().join("parts").join(MARK)).expect("the mark");
    assets.add(MARK, svg).expect("the mark fits");
    evaluate_part(doc, params, "fdm-0.4", &example_fit_profile_json(), &assets)
        .expect("the brand tile builds")
        .content_hash()
}

#[test]
fn open_edit_preset_save_then_build_gives_the_designers_hash() {
    let folder = PartFolder::new("edited");
    // Edited in the panel: a longer tile, then the "Pressed-in mark" preset.
    let params = r#"{"size":[2,6],"relief":"deboss","reliefHeight":0.6,"logo":"assets/brand/neuraldeep-mark-2x4.svg"}"#;
    let with_preset = add_preset_json(BRAND_TILE, "Long and pressed in", params, "fdm-0.4")
        .expect("the values are a valid preset");
    let saved = save_part_json(&with_preset, params, "fdm-0.4").expect("the values save");
    let cli = folder.build(&saved);
    // The designer shows the saved document's hash once it has saved it; the
    // values it sends then are the saved defaults.
    assert_eq!(
        cli["contentHash"].as_str(),
        Some(designer_hash(&saved, "").as_str())
    );
    assert_eq!(
        cli["contentHash"].as_str(),
        Some(designer_hash(&saved, params).as_str())
    );
    assert_eq!(cli["params"]["relief"], "deboss");
    assert_eq!(cli["params"]["size"], serde_json::json!([2, 6]));
    // The saved file carries the preset, and the CLI reads it back unchanged.
    assert!(
        saved.contains("\"name\": \"Long and pressed in\""),
        "{saved}"
    );
    let reread = std::fs::read_to_string(folder.0.join("ndn.brand-tile.obpart.json")).unwrap();
    assert_eq!(save_part_json(&reread, "", "fdm-0.4").unwrap(), saved);
}

#[test]
fn saving_without_edits_keeps_the_original_hash() {
    let folder = PartFolder::new("unchanged");
    let original = folder.build(BRAND_TILE);
    let saved = save_part_json(BRAND_TILE, "", "fdm-0.4").expect("saves");
    let resaved = folder.build(&saved);
    assert_eq!(original["contentHash"], resaved["contentHash"]);
    assert_eq!(
        original["contentHash"].as_str(),
        Some(designer_hash(BRAND_TILE, "").as_str())
    );
}
