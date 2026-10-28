//! How an export lays the objects out for printing, and the colours it records.
//!
//! The evaluator builds every part face up in the kernel frame (millimetres, +Z
//! up, body minimum corner at the origin). An inlay prints face down
//! (`Built.artwork.face_on_bed == "top"`, docs/plan/05 section 6), so its export
//! turns every object over together with [`openbrick_geom::turn_face_down`], the
//! shared face-down transform (for every export of such a part, so the CLI's
//! export should call it too): a half turn about the x axis inside the box the
//! objects share. That is a proper rotation (determinant +1),
//! so triangle winding and the closed-mesh checks of the writers hold, and the
//! objects keep touching where they touched. Every other part is exported as
//! built.

use std::collections::BTreeMap;

use openbrick_geom::{turn_face_down, Mesh};

use crate::data::role_color;
use crate::CoreError;

/// The objects as they go on the bed: copies, turned face down together when
/// `face_down`.
pub(crate) fn on_bed(meshes: &[&Mesh], face_down: bool) -> Vec<Mesh> {
    let mut out: Vec<Mesh> = meshes.iter().map(|m| (*m).clone()).collect();
    if face_down {
        let mut all: Vec<&mut Mesh> = out.iter_mut().collect();
        turn_face_down(&mut all);
    }
    out
}

/// Colours to record per role: the role colours of [`crate::data::ROLE_COLORS`],
/// overridden by `colors_json`, a JSON object of role to `#rrggbb` (empty text
/// for none). Returns sRGB plus an opaque alpha.
pub(crate) fn role_colors(colors_json: &str) -> Result<BTreeMap<String, [u8; 4]>, CoreError> {
    let mut out = BTreeMap::new();
    if colors_json.trim().is_empty() {
        return Ok(out);
    }
    let map: BTreeMap<String, String> = serde_json::from_str(colors_json).map_err(|e| {
        CoreError(format!(
            "the colours must be a JSON object of role to \"#rrggbb\": {e}"
        ))
    })?;
    for (role, text) in map {
        let rgb = parse_hex(&text).ok_or_else(|| {
            CoreError(format!(
                "the colour of `{role}` must be written #rrggbb, got {text:?}"
            ))
        })?;
        out.insert(role, rgb);
    }
    Ok(out)
}

/// The colour to record for `role`: the override, else the role's display colour.
pub(crate) fn color_of(overrides: &BTreeMap<String, [u8; 4]>, role: &str) -> [u8; 4] {
    overrides
        .get(role)
        .copied()
        .unwrap_or_else(|| parse_hex(role_color(role)).expect("ROLE_COLORS are #rrggbb"))
}

/// `#rrggbb` as sRGB plus an opaque alpha.
fn parse_hex(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> Mesh {
        Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 2.0, 0.0],
                [0.0, 0.0, 3.0],
            ],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        }
    }

    #[test]
    fn on_bed_copies_and_turns_with_the_shared_transform() {
        // The transform itself is tested in openbrick-geom; here: copies, the
        // flag, and every object turned in one shared box.
        let t = tetra();
        assert_eq!(on_bed(&[&t], false), vec![t.clone()]);
        let mut lifted = t.clone();
        for p in &mut lifted.positions {
            p[2] += 3.0;
        }
        let turned = on_bed(&[&t, &lifted], true);
        let (mut a, mut b) = (t.clone(), lifted.clone());
        turn_face_down(&mut [&mut a, &mut b]);
        assert_eq!(turned, vec![a, b]);
        assert_eq!(
            turned[1].bounds().0[2],
            0.0,
            "the upper object is now on the bed"
        );
    }

    #[test]
    fn colours_parse_and_fall_back_to_the_role_colours() {
        let none = role_colors("").unwrap();
        assert!(none.is_empty());
        assert_eq!(color_of(&none, "accent"), [0xf4, 0xf1, 0xe8, 255]);
        let some = role_colors(r##"{"accent":"#FF8000"}"##).unwrap();
        assert_eq!(color_of(&some, "accent"), [255, 128, 0, 255]);
        assert_eq!(color_of(&some, "body"), [0x2f, 0x6d, 0xb5, 255]);
        for bad in [
            r#"{"accent":"red"}"#,
            r##"{"accent":"#12345"}"##,
            "[1]",
            "{",
        ] {
            assert!(role_colors(bad).is_err(), "{bad}");
        }
    }
}
