//! Bundled data for the page: the process profiles, the example fit profile, the
//! logo tile part document and its sample artwork, the disclaimer, the display
//! colours of the colour roles and the asset size limit. The page builds its
//! controls from these, so nothing here is repeated in JavaScript.

use openbrick_part::coupon::CouponOptions;
use openbrick_print::{default_process_profiles, DEFAULT_POCKET_SET, POCKET_SETS};
use serde_json::json;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

/// `parts/ndn.logo-tile.obpart.json`, the part the playground designs.
const LOGO_TILE_PART: &str = include_str!("../../../parts/ndn.logo-tile.obpart.json");
/// `parts/assets/mark.svg` (CC0), the default `logo` of the logo tile. Asset paths
/// in a part document are relative to the part file, so the document names it
/// `assets/mark.svg`.
const SAMPLE_LOGO: &str = include_str!("../../../parts/assets/mark.svg");
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

/// The sample artwork, `parts/assets/mark.svg` (CC0): the logo tile's default
/// `logo`, which the document names `assets/mark.svg`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn sample_logo_svg() -> String {
    SAMPLE_LOGO.to_string()
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

/// What the coupon controls offer: `{ pocketSets, defaultPocketSet, defaults }`,
/// where `defaults` is `CouponOptions::default()` in its JSON form (the layout
/// [`crate::build_coupon`] takes).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn coupon_choices_json() -> String {
    json!({
        "pocketSets": POCKET_SETS,
        "defaultPocketSet": DEFAULT_POCKET_SET,
        "defaults": CouponOptions::default(),
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
        assert!(sample_logo_svg().contains("<svg"));
        assert!(sample_logo_svg().len() < max_asset_bytes() as usize);
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
    }
}
