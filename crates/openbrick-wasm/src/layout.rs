//! How an export lays the objects out for printing, and the colours it records.
//!
//! The evaluator builds every part face up in the kernel frame (millimetres, +Z
//! up, body minimum corner at the origin). An inlay prints face down
//! (`Built.artwork.face_on_bed == "top"`, docs/plan/05 section 6), so its export
//! turns every object half a turn about the x axis, inside the box the objects
//! share: (x, y, z) becomes (x, y0 + y1 - y, z0 + z1 - z). That is a proper
//! rotation (determinant +1), so triangle winding and the closed-mesh checks of
//! the writers hold, and the objects keep touching where they touched. Every
//! other part is exported as built.

use std::collections::BTreeMap;

use openbrick_geom::Mesh;

use crate::data::role_color;
use crate::CoreError;

/// The box shared by `meshes`: minimum and maximum corner.
fn shared_bounds<'a>(meshes: impl IntoIterator<Item = &'a Mesh>) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for m in meshes {
        let (a, b) = m.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    (lo, hi)
}

/// The objects as they go on the bed: turned face down when `face_down`, else
/// unchanged copies. The sums of the bounds are formed in f64, where the sum of
/// two f32 values is exact, so a vertex on a bounding plane lands exactly on the
/// opposite one.
pub(crate) fn on_bed(meshes: &[&Mesh], face_down: bool) -> Vec<Mesh> {
    if !face_down {
        return meshes.iter().map(|m| (*m).clone()).collect();
    }
    let (lo, hi) = shared_bounds(meshes.iter().copied());
    let sy = f64::from(lo[1]) + f64::from(hi[1]);
    let sz = f64::from(lo[2]) + f64::from(hi[2]);
    meshes
        .iter()
        .map(|m| Mesh {
            positions: m
                .positions
                .iter()
                .map(|&[x, y, z]| [x, (sy - f64::from(y)) as f32, (sz - f64::from(z)) as f32])
                .collect(),
            indices: m.indices.clone(),
        })
        .collect()
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

    fn signed_volume(m: &Mesh) -> f64 {
        m.indices
            .chunks(3)
            .map(|t| {
                let p = |i: u32| m.positions[i as usize].map(f64::from);
                let [a, b, c] = [p(t[0]), p(t[1]), p(t[2])];
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    #[test]
    fn face_down_is_a_half_turn_inside_the_shared_box() {
        let t = tetra();
        // Volume 1 * 2 * 3 / 6 = 1, positive: the winding faces outward.
        assert!((signed_volume(&t) - 1.0).abs() < 1e-12);
        let same = on_bed(&[&t], false);
        assert_eq!(same[0], t);
        let turned = on_bed(&[&t], true);
        let m = &turned[0];
        assert_eq!(m.indices, t.indices);
        // (x, y, z) -> (x, 2 - y, 3 - z): the apex at z = 3 now touches the bed.
        assert_eq!(m.positions[3], [0.0, 2.0, 0.0]);
        assert_eq!(m.positions[0], [0.0, 2.0, 3.0]);
        assert_eq!(m.bounds(), t.bounds(), "the box is unchanged");
        // A proper rotation keeps the volume positive (winding still outward).
        assert!((signed_volume(m) - 1.0).abs() < 1e-12);
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
