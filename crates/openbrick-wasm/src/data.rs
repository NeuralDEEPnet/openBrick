//! Bundled data for the page: the process profiles, the example fit profile, the
//! bundled part documents (the logo tile and the Neuraldeep.net tile) and their
//! size limit, the artwork samples (the NEURALDEEP.NET wordmark and
//! the CC0 sample mark), the disclaimer, the display colours of the colour roles
//! and the asset size limit. The page builds its controls from these, so nothing
//! here is repeated in JavaScript.

use openbrick_part::coupon::CouponOptions;
use openbrick_print::{
    default_process_profiles, DEFAULT_NOZZLE_MM, DEFAULT_POCKET_SET, FIT_INTENTS, POCKET_CELLS,
    POCKET_SETS,
};
use serde_json::{json, Value};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

/// `parts/ndn.logo-tile.obpart.json`, the part the playground designs first.
const LOGO_TILE_PART: &str = include_str!("../../../parts/ndn.logo-tile.obpart.json");
/// `parts/ndn.brand-tile.obpart.json`, the Neuraldeep.net tile with its
/// presets, the playground's second bundled part. Its default artwork is the
/// NEURALDEEP.NET wordmark (see the brand drawings below).
const BRAND_TILE_PART: &str = include_str!("../../../parts/ndn.brand-tile.obpart.json");
/// `parts/assets/mark.svg` (CC0), the default `logo` of the logo tile. Asset paths
/// in a part document are relative to the part file, so the document names it
/// `assets/mark.svg`.
const SAMPLE_LOGO: &str = include_str!("../../../parts/assets/mark.svg");

/// The NEURALDEEP.NET wordmark drawings in `parts/assets/brand/` (letters from
/// Inter, SIL Open Font License 1.1, made by `scripts/brand-text.py`):
/// Copyright Neural Deep Network Ltd, all rights reserved, trademarks of
/// Neural Deep Network Ltd and not covered by this repository's licences; they
/// may be used to build, print and show the OpenBrick3D demo tiles
/// (`parts/assets/brand/NOTICE.md`). The playground is Neural Deep Network's
/// public demo, so the wordmark is its default artwork. Each is named as a
/// part document in `parts/` names it. Because they are bundled here, this
/// module and the playground built from it carry artwork that is not MIT
/// (LICENSE-POLICY.md): a fork or package that may not ship the marks removes
/// these four and their two entries in [`artwork_samples_json`], which leaves
/// the CC0 sample as the default.
const WORDMARK_2X6: (&str, &str) = (
    "assets/brand/neuraldeep-wordmark-2x6.svg",
    include_str!("../../../parts/assets/brand/neuraldeep-wordmark-2x6.svg"),
);
const WORDMARK_2X8: (&str, &str) = (
    "assets/brand/neuraldeep-wordmark-2x8.svg",
    include_str!("../../../parts/assets/brand/neuraldeep-wordmark-2x8.svg"),
);
const WORDMARK_2X10: (&str, &str) = (
    "assets/brand/neuraldeep-wordmark-2x10.svg",
    include_str!("../../../parts/assets/brand/neuraldeep-wordmark-2x10.svg"),
);
const WORDMARK_4X4: (&str, &str) = (
    "assets/brand/neuraldeep-wordmark-4x4.svg",
    include_str!("../../../parts/assets/brand/neuraldeep-wordmark-4x4.svg"),
);

/// The first sentence of the owner's notice in `parts/assets/brand/NOTICE.md`,
/// word for word (the copyright line): the page shows it under the drop zone,
/// with a link to the whole notice ([`BRAND_NOTICE_URL`]), while a
/// Neuraldeep.net drawing is in use.
pub const BRAND_NOTICE: &str = "Copyright Neural Deep Network Ltd. All rights reserved.";

/// Where the page links the notice.
pub const BRAND_NOTICE_URL: &str =
    "https://github.com/NeuralDEEPnet/openBrick/blob/main/parts/assets/brand/NOTICE.md";
/// `profiles/fit/example-mk4s-petg-0.4.json`, as committed (with its notes: the
/// values are illustrative, not measured).
const EXAMPLE_FIT: &str = include_str!("../../../profiles/fit/example-mk4s-petg-0.4.json");

/// Display colours of the colour roles, `#rrggbb`: what the viewer shows and what
/// a 3MF records when the page passes no colours of its own. `removed` is the red
/// overlay of artwork that does not print as drawn; the page always pairs it
/// with a text entry, so colour is never the only cue (docs/plan/06 section 13).
/// The body default is blue so that red stays free for that overlay.
pub const ROLE_COLORS: [(&str, &str); 4] = [
    ("body", "#2f6db5"),
    ("accent", "#f4f1e8"),
    ("serial", "#3a4048"),
    ("removed", "#e5242b"),
];

/// The display colour of a colour role; unknown roles get the body colour.
pub fn role_color(role: &str) -> &'static str {
    ROLE_COLORS
        .iter()
        .find(|(r, _)| *r == role)
        .map_or(ROLE_COLORS[0].1, |(_, c)| c)
}

/// The bundled process profiles (fdm-0.4, fdm-0.2, resin-msla) as a JSON array.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn process_profiles_json() -> String {
    serde_json::to_string(&default_process_profiles()).expect("process profiles serialise")
}

/// The bundled printers (`profiles/printer/*.json`: id, name, maker, build
/// volume in mm, toolheads, filaments and the source of the volume) as a JSON
/// array sorted by id, for the printer menus of the playground (rule
/// `SIZE.BED`, and the coupon's bed).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn printers_json() -> String {
    serde_json::to_string(&openbrick_print::default_printers()).expect("printers serialise")
}

/// The example fit profile (Prusa MK4S, PETG, fdm-0.4) as committed. Its values
/// are illustrative, not measured; its notes say so.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn example_fit_profile_json() -> String {
    EXAMPLE_FIT.to_string()
}

/// The logo tile part document (`parts/ndn.logo-tile.obpart.json`).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn logo_tile_part_json() -> String {
    LOGO_TILE_PART.to_string()
}

/// The part documents the playground offers before any is opened, as a JSON
/// array of `{ id, label, file, text }`, the default first: the logo tile and
/// the Neuraldeep.net tile with its presets. `file` is the name under
/// `parts/`, which the save dialog suggests; `text` the document as committed.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn bundled_parts_json() -> String {
    json!([
        {
            "id": "ndn.logo-tile",
            "label": "Logo tile",
            "file": "ndn.logo-tile.obpart.json",
            "text": LOGO_TILE_PART,
        },
        {
            "id": "ndn.brand-tile",
            "label": "Neuraldeep.net tile (with presets)",
            "file": "ndn.brand-tile.obpart.json",
            "text": BRAND_TILE_PART,
        },
    ])
    .to_string()
}

/// The largest part document, in bytes, the page opens: the CLI's limit
/// (`openbrick_part::save::MAX_DOCUMENT_BYTES`, 4 MiB).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn max_part_bytes() -> u32 {
    u32::try_from(openbrick_part::save::MAX_DOCUMENT_BYTES).expect("the limit fits u32")
}

/// The sample artwork, `parts/assets/mark.svg` (CC0): the logo tile's default
/// `logo`, which the document names `assets/mark.svg`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn sample_logo_svg() -> String {
    SAMPLE_LOGO.to_string()
}

/// One drawing of a sample: its asset name, the tile it was drawn for (none
/// for a drawing meant for any tile) and its text.
fn sample_file((asset, svg): (&str, &str), tile: Option<[u32; 2]>) -> Value {
    json!({ "asset": asset, "tile": tile, "svg": svg })
}

/// The artwork samples the playground offers, as a JSON array; the first is
/// its default. Each sample is `{ id, label, title, suffix, shape, size,
/// notice, noticeUrl, files }`:
///
/// - `label`: the chooser's text; `title`: the sample in a sentence ("the
///   NEURALDEEP.NET wordmark"); `suffix`: what the download names end with,
///   so files made from different samples never share a name;
/// - `shape`: the tiles a sample is drawn for: `"portrait"` (longer than
///   wide), `"square"`, or null for any; choosing a sample on a tile of
///   another shape, or on a tile smaller than its smallest drawing's, sets
///   the tile to `size` (the smallest drawing's tile; the playground starts
///   there with the first sample);
/// - `notice`: the line the page shows under the drop zone while the sample is
///   in use, and `noticeUrl` its link (null for the CC0 sample);
/// - `files`: `{ asset, tile, svg }`, one drawing per tile it was drawn for,
///   smallest first: the page sends the one drawn for the largest tile that
///   fits the tile being designed, else the smallest. `asset` is the name a
///   part document in `parts/` uses for it, so the native tools read the same
///   bytes under the same name and give the same content hash.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn artwork_samples_json() -> String {
    json!([
        {
            "id": "neuraldeep-wordmark",
            "label": "NEURALDEEP.NET wordmark",
            "title": "the NEURALDEEP.NET wordmark",
            "suffix": "neuraldeep",
            "shape": "portrait",
            "size": [2, 6],
            "notice": BRAND_NOTICE,
            "noticeUrl": BRAND_NOTICE_URL,
            "files": [
                sample_file(WORDMARK_2X6, Some([2, 6])),
                sample_file(WORDMARK_2X8, Some([2, 8])),
                sample_file(WORDMARK_2X10, Some([2, 10])),
            ],
        },
        {
            "id": "neuraldeep-square",
            "label": "NEURALDEEP.NET wordmark (square)",
            "title": "the square NEURALDEEP.NET wordmark",
            "suffix": "neuraldeep-square",
            "shape": "square",
            "size": [4, 4],
            "notice": BRAND_NOTICE,
            "noticeUrl": BRAND_NOTICE_URL,
            "files": [sample_file(WORDMARK_4X4, Some([4, 4]))],
        },
        {
            "id": "sample",
            "label": "OpenBrick3D sample mark (CC0)",
            "title": "the sample mark (CC0)",
            "suffix": "sample",
            "shape": null,
            "size": null,
            "notice": null,
            "noticeUrl": null,
            "files": [sample_file(("assets/mark.svg", SAMPLE_LOGO), None)],
        },
    ])
    .to_string()
}

/// The non-affiliation disclaimer (docs/plan/12 section 4), from the same file
/// as the CLI's `openbrick about` (`openbrick_units::DISCLAIMER`).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn disclaimer() -> String {
    openbrick_units::DISCLAIMER.to_string()
}

/// [`ROLE_COLORS`] as a JSON object, role to `#rrggbb`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn role_colors_json() -> String {
    let map: serde_json::Map<String, serde_json::Value> = ROLE_COLORS
        .iter()
        .map(|(r, c)| (r.to_string(), json!(c)))
        .collect();
    serde_json::Value::Object(map).to_string()
}

/// The largest asset, in bytes, the evaluator reads: `MAX_SVG_BYTES` (4 MiB).
/// The page refuses bigger files before sending them, with this number in the
/// message; [`crate::AssetSet::add`] refuses them too.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn max_asset_bytes() -> u32 {
    u32::try_from(openbrick_part::eval::MAX_SVG_BYTES).expect("the limit fits u32")
}

/// What the coupon controls offer: `{ pocketSets, defaultPocketSet, defaults,
/// record }`, where `defaults` is `CouponOptions::default()` in its JSON form
/// (the layout [`crate::build_coupon`] takes) and `record` the defaults of the
/// wizard's record step, the CLI's (`{ nozzleMm, intents, pocketCells }`: the
/// nozzle `openbrick_print::DEFAULT_NOZZLE_MM`, the fit intents, the default
/// first, and the two cell ids of a pocket, tube OD then rib depth).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn coupon_choices_json() -> String {
    json!({
        "pocketSets": POCKET_SETS,
        "defaultPocketSet": DEFAULT_POCKET_SET,
        "defaults": CouponOptions::default(),
        "record": {
            "nozzleMm": DEFAULT_NOZZLE_MM,
            "intents": FIT_INTENTS,
            "pocketCells": POCKET_CELLS,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_part::PartDoc;
    use openbrick_print::FitProfile;

    #[test]
    fn the_bundled_documents_parse() {
        let profiles: Vec<openbrick_print::ProcessProfile> =
            serde_json::from_str(&process_profiles_json()).unwrap();
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["fdm-0.4", "fdm-0.2", "resin-msla"]);
        let fit: FitProfile = serde_json::from_str(&example_fit_profile_json()).unwrap();
        assert_eq!(fit.id, openbrick_print::example_fit_profile().id);
        let doc = PartDoc::parse(&logo_tile_part_json()).unwrap();
        // The sample logo is the document's default artwork.
        assert_eq!(doc.params["logo"].default, json!("assets/mark.svg"));
        // The bundled parts: the logo tile first, then the brand tile, whose
        // default artwork is a bundled sample drawing and which has presets.
        let parts: Vec<Value> = serde_json::from_str(&bundled_parts_json()).unwrap();
        let ids: Vec<&str> = parts.iter().filter_map(|p| p["id"].as_str()).collect();
        assert_eq!(ids, ["ndn.logo-tile", "ndn.brand-tile"]);
        assert_eq!(parts[0]["text"], json!(logo_tile_part_json()));
        let samples: Vec<Value> = serde_json::from_str(&artwork_samples_json()).unwrap();
        let assets: Vec<&str> = samples
            .iter()
            .flat_map(|s| s["files"].as_array().unwrap())
            .filter_map(|f| f["asset"].as_str())
            .collect();
        for p in &parts {
            let text = p["text"].as_str().unwrap();
            let doc = PartDoc::parse(text).unwrap();
            assert_eq!(p["id"], json!(doc.id));
            assert_eq!(p["file"], json!(format!("{}.obpart.json", doc.id)));
            let logo = doc.params["logo"].default.as_str().unwrap();
            assert!(assets.contains(&logo), "{logo} is a bundled drawing");
            assert!(text.len() < max_part_bytes() as usize);
        }
        let brand = PartDoc::parse(parts[1]["text"].as_str().unwrap()).unwrap();
        assert!(brand.presets.len() >= 2, "the brand tile has presets");
        assert!(sample_logo_svg().contains("<svg"));
        assert!(sample_logo_svg().len() < max_asset_bytes() as usize);
    }

    #[test]
    fn the_samples_bundle_the_wordmarks_with_their_notice() {
        let samples: Vec<Value> = serde_json::from_str(&artwork_samples_json()).unwrap();
        let ids: Vec<&str> = samples.iter().filter_map(|s| s["id"].as_str()).collect();
        // The NEURALDEEP.NET wordmark is the playground's default.
        assert_eq!(ids, ["neuraldeep-wordmark", "neuraldeep-square", "sample"]);
        let notice = include_str!("../../../parts/assets/brand/NOTICE.md");
        // The line shown is where the owner's notice starts, word for word.
        assert!(
            notice.contains(&format!(
                "\n\n{BRAND_NOTICE} The Neuraldeep.net marks are trademarks of Neural Deep Network Ltd"
            )),
            "NOTICE.md starts its notice with the line the page shows"
        );
        assert!(BRAND_NOTICE_URL.ends_with("/parts/assets/brand/NOTICE.md"));
        let mut suffixes = Vec::new();
        for s in &samples {
            let id = s["id"].as_str().unwrap();
            suffixes.push(s["suffix"].as_str().unwrap().to_string());
            let files = s["files"].as_array().unwrap();
            assert!(!files.is_empty(), "{id}");
            let brand = id.starts_with("neuraldeep");
            assert_eq!(s["notice"].is_string(), brand, "{id}");
            for f in files {
                let asset = f["asset"].as_str().unwrap();
                let svg = f["svg"].as_str().unwrap();
                assert!(svg.contains("<svg") && svg.len() < max_asset_bytes() as usize);
                if brand {
                    assert!(asset.starts_with("assets/brand/"), "{asset}");
                    assert!(
                        svg.contains("Copyright Neural Deep Network Ltd. All rights reserved."),
                        "{asset}"
                    );
                    let [w, h] = [0, 1].map(|k| f["tile"][k].as_u64().unwrap());
                    assert!(asset.ends_with(&format!("-{w}x{h}.svg")), "{asset}");
                    let name = asset.rsplit('/').next().unwrap();
                    assert!(
                        notice.contains(&format!("`{name}`")),
                        "NOTICE.md lists {name}"
                    );
                } else {
                    assert!(f["tile"].is_null(), "{asset}");
                }
            }
            // Smallest tile first.
            let areas: Vec<u64> = files
                .iter()
                .map(|f| f["tile"][0].as_u64().unwrap_or(0) * f["tile"][1].as_u64().unwrap_or(0))
                .collect();
            assert!(areas.windows(2).all(|a| a[0] < a[1]), "{id}: {areas:?}");
        }
        suffixes.sort();
        suffixes.dedup();
        assert_eq!(suffixes.len(), samples.len(), "unique download suffixes");
        // The CC0 sample is the logo tile's own default artwork.
        let doc = PartDoc::parse(&logo_tile_part_json()).unwrap();
        let sample = &samples[2]["files"][0];
        assert_eq!(doc.params["logo"].default, sample["asset"]);
        assert_eq!(sample["svg"].as_str(), Some(sample_logo_svg().as_str()));
        // The wordmarks set these tiles when chosen on a tile of another
        // shape or a smaller one: each sample's smallest drawing's tile. The
        // whole text does not print on the logo tile's default 2x4, so the
        // playground starts at the first sample's 2x6.
        assert_eq!(samples[0]["size"], json!([2, 6]));
        assert_eq!(samples[1]["size"], json!([4, 4]));
        assert_ne!(samples[0]["size"], doc.params["size"].default);
        for s in samples.iter().filter(|s| s["notice"].is_string()) {
            assert_eq!(s["size"], s["files"][0]["tile"], "{}", s["id"]);
        }
    }

    #[test]
    fn every_brand_drawing_prints_on_the_playground_tile_it_was_drawn_for() {
        // The playground's document (the logo tile) with each NEURALDEEP.NET
        // drawing at its tile, the example fit and fdm-0.4: nothing to report,
        // so no FEATURE.MIN, and export allowed. The brand part documents are
        // checked in every relief by crates/openbrick-part/tests/brand.rs.
        let samples: Vec<Value> = serde_json::from_str(&artwork_samples_json()).unwrap();
        for f in samples
            .iter()
            .filter(|s| s["notice"].is_string())
            .flat_map(|s| s["files"].as_array().unwrap())
        {
            let asset = f["asset"].as_str().unwrap();
            let mut assets = crate::AssetSet::new();
            assets
                .add(asset, f["svg"].as_str().unwrap().as_bytes().to_vec())
                .unwrap();
            let params = json!({ "logo": asset, "size": f["tile"] }).to_string();
            let part = crate::evaluate_part(
                &logo_tile_part_json(),
                &params,
                "fdm-0.4",
                &example_fit_profile_json(),
                &assets,
            )
            .unwrap_or_else(|e| panic!("{asset}: {e}"));
            let report: Value = serde_json::from_str(&part.report_json()).unwrap();
            assert!(part.export_allowed(), "{asset}: {report}");
            assert_eq!(report["findings"], json!([]), "{asset}");
        }
    }

    #[test]
    fn roles_have_distinct_colours_and_the_disclaimer_is_shared() {
        let colors: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&role_colors_json()).unwrap();
        assert_eq!(colors.len(), ROLE_COLORS.len());
        let mut seen: Vec<&str> = ROLE_COLORS.iter().map(|(_, c)| *c).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            ROLE_COLORS.len(),
            "every role has its own colour"
        );
        for (_, c) in ROLE_COLORS {
            assert!(c.len() == 7 && c.starts_with('#'), "{c}");
            assert!(u32::from_str_radix(&c[1..], 16).is_ok(), "{c}");
        }
        assert_eq!(role_color("accent"), "#f4f1e8");
        assert_eq!(role_color("unknown"), role_color("body"));
        assert_eq!(disclaimer(), openbrick_units::DISCLAIMER);
        assert!(disclaimer().contains("not affiliated with"));
    }

    #[test]
    fn coupon_choices_name_the_pocket_sets_and_defaults() {
        let v: serde_json::Value = serde_json::from_str(&coupon_choices_json()).unwrap();
        assert_eq!(
            v["pocketSets"],
            json!(["cross", "crossAndDiagonal", "full"])
        );
        assert_eq!(v["defaultPocketSet"], "cross");
        let opts: CouponOptions = serde_json::from_value(v["defaults"].clone()).unwrap();
        assert_eq!(opts, CouponOptions::default());
        assert_eq!(v["record"]["nozzleMm"], DEFAULT_NOZZLE_MM);
        assert_eq!(v["record"]["intents"][0], "fits-genuine-bricks");
        assert_eq!(
            v["record"]["pocketCells"],
            json!(["pocketTubeOD", "pocketRib"])
        );
    }
}
