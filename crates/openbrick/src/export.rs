//! Writing built parts and the coupon as 3MF or STL (docs/plan/05 section 7,
//! docs/plan/08 section 5) through `openbrick-io`, laid out the way they print,
//! and describing every written file in the `--json` object so
//! `scripts/validate-3mf.py` can compare it with what lib3mf reads.

use std::path::Path;

use openbrick_geom::Mesh;
use openbrick_io::threemf::{package_uuids, stored_fingerprint};
use openbrick_io::{write_3mf, write_stl_binary, ExportMeta, ExportObject};
use serde_json::{json, Value};

use crate::files::{absolute, write_file};
use crate::ui::Fail;

/// Display colours (sRGB plus alpha) per colour role. Slicers show them; the
/// filament each role prints with is chosen in the slicer.
pub fn role_colour(role: &str) -> Option<[u8; 4]> {
    match role {
        "body" => Some([0xE6, 0xE6, 0xE6, 0xFF]),
        "accent" => Some([0x1E, 0x5A, 0xA8, 0xFF]),
        "serial" => Some([0x40, 0x40, 0x40, 0xFF]),
        _ => None,
    }
}

/// The output format, from the file extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    ThreeMf,
    Stl,
}

impl Format {
    pub fn of(path: &Path) -> Result<Format, Fail> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("3mf") => Ok(Format::ThreeMf),
            Some("stl") => Ok(Format::Stl),
            _ => Err(Fail::Input(format!(
                "{}: the output format follows the file extension; use .3mf (one object per colour role, with traceability metadata) or .stl (one single-colour solid)",
                path.display()
            ))),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::ThreeMf => "3mf",
            Format::Stl => "stl",
        }
    }
}

/// One solid to write, owning its mesh (it may have been turned face down).
pub struct Solid {
    pub name: String,
    pub role: String,
    pub mesh: Mesh,
}

impl Solid {
    fn object(&self) -> ExportObject<'_> {
        ExportObject::new(
            self.name.clone(),
            self.role.clone(),
            role_colour(&self.role),
            &self.mesh,
        )
    }
}

/// Turn the solids over together, face down: a half turn about the x axis
/// through the centre of their common bounding box, so the top face lands on
/// the bed (z = 0) and the box stays where it was. A rotation, not a mirror:
/// triangle winding and closedness are unchanged, and artwork reads the right
/// way round when the print is turned back over. Used for parts whose artwork
/// prints face down (inlay, `Artwork::face_on_bed` = `top`).
pub fn turn_face_down(solids: &mut [Solid]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for s in solids.iter() {
        for p in &s.mesh.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(f64::from(p[k]));
                hi[k] = hi[k].max(f64::from(p[k]));
            }
        }
    }
    if !lo[1].is_finite() || !lo[2].is_finite() {
        return;
    }
    // y -> (lo + hi) - y and z -> (lo + hi) - z: the box maps onto itself.
    let (sy, sz) = (lo[1] + hi[1], lo[2] + hi[2]);
    for s in solids.iter_mut() {
        for p in &mut s.mesh.positions {
            p[1] = (sy - f64::from(p[1])) as f32;
            p[2] = (sz - f64::from(p[2])) as f32;
        }
    }
}

/// Volume of a closed mesh (divergence theorem in f64 over the stored f32
/// coordinates, as `scripts/validate-3mf.py` computes it from lib3mf).
pub fn mesh_volume(m: &Mesh) -> f64 {
    let p = |i: u32| {
        m.positions
            .get(i as usize)
            .map(|v| v.map(f64::from))
            .unwrap_or([0.0; 3])
    };
    m.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[i, j, k]| {
            let (a, b, c) = (p(i), p(j), p(k));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
}

/// The model metadata a 3MF written with `meta` carries, keyed as
/// `scripts/validate-3mf.py` reads it.
fn expected_metadata(meta: &ExportMeta) -> Value {
    let mut m = serde_json::Map::new();
    if !meta.title.is_empty() {
        m.insert("Title".into(), json!(meta.title));
    }
    m.insert("Application".into(), json!(meta.generator));
    m.insert("openbrick:partId".into(), json!(meta.part_id));
    m.insert("openbrick:version".into(), json!(meta.version));
    m.insert("openbrick:contentHash".into(), json!(meta.content_hash));
    m.insert("openbrick:fitProfile".into(), json!(meta.fit_profile_id));
    m.insert("openbrick:process".into(), json!(meta.process_id));
    m.insert("openbrick:generator".into(), json!(meta.generator));
    m.insert("openbrick:kernel".into(), json!(meta.kernel_id));
    if let Some(serial) = &meta.serial {
        m.insert("openbrick:serial".into(), json!(serial));
    }
    Value::Object(m)
}

/// Encode the solids in `format`; returns the bytes and the description of the
/// file for the `--json` object (without its path).
pub fn encode(
    solids: &[Solid],
    meta: &ExportMeta,
    format: Format,
) -> Result<(Vec<u8>, Value), Fail> {
    let objects: Vec<ExportObject> = solids.iter().map(Solid::object).collect();
    let internal = |what: &str, e: openbrick_io::IoError| {
        match e {
        // Blank or unprintable metadata comes from the part document.
        openbrick_io::IoError::MissingMeta { .. } | openbrick_io::IoError::InvalidText { .. } => {
            Fail::Input(format!("cannot write the {what}: {e}"))
        }
        _ => Fail::Internal(format!(
            "the {what} writer refused the built part: {e}. This is a bug; please report it with the part file and parameters"
        )),
    }
    };
    match format {
        Format::ThreeMf => {
            let bytes = write_3mf(&objects, meta).map_err(|e| internal("3MF", e))?;
            let uuids = package_uuids(&objects, meta);
            let described: Vec<Value> = solids
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (lo, hi) = s.mesh.bounds();
                    json!({
                        "name": s.name,
                        "role": s.role,
                        "color": role_colour(&s.role).map(hex),
                        "vertices": s.mesh.positions.len(),
                        "triangles": s.mesh.triangle_count(),
                        "volumeMm3": mesh_volume(&s.mesh),
                        // f64 of the exact f32 bounds, as lib3mf reads them.
                        "boundsMm": { "min": lo.map(f64::from), "max": hi.map(f64::from) },
                        "storedFingerprint": format!("{:016x}", stored_fingerprint(&s.mesh)),
                        "uuid": uuids.objects.get(i).map(|u| u.to_string()),
                        "itemUuid": uuids.items.get(i).map(|u| u.to_string()),
                    })
                })
                .collect();
            let entry = json!({
                "format": "3mf",
                "bytes": bytes.len(),
                "buildUuid": uuids.build.to_string(),
                "metadata": expected_metadata(meta),
                "objects": described,
            });
            Ok((bytes, entry))
        }
        Format::Stl => {
            let bytes = write_stl_binary(&objects).map_err(|e| internal("STL", e))?;
            let entry = json!({
                "format": "stl",
                "bytes": bytes.len(),
                "triangles": solids.iter().map(|s| s.mesh.triangle_count()).sum::<usize>(),
                "volumeMm3": solids.iter().map(|s| mesh_volume(&s.mesh)).sum::<f64>(),
            });
            Ok((bytes, entry))
        }
    }
}

/// Write an encoded file and complete its description with the path:
/// absolute in `path`, so a saved `--json` object finds it from anywhere.
pub fn write(path: &Path, bytes: &[u8], mut entry: Value) -> Result<Value, Fail> {
    write_file(path, bytes).map_err(Fail::Input)?;
    if let Value::Object(m) = &mut entry {
        m.insert("path".into(), json!(absolute(path).to_string_lossy()));
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed box from `lo` to `hi`, outward winding.
    fn cuboid(lo: [f32; 3], hi: [f32; 3]) -> Mesh {
        let mut positions = Vec::new();
        for i in 0..8u32 {
            positions.push([
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]);
        }
        let quads = [
            [0, 2, 3, 1], // z = lo
            [4, 5, 7, 6], // z = hi
            [0, 1, 5, 4], // y = lo
            [2, 6, 7, 3], // y = hi
            [0, 4, 6, 2], // x = lo
            [1, 3, 7, 5], // x = hi
        ];
        let indices = quads
            .iter()
            .flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]])
            .collect();
        Mesh { positions, indices }
    }

    #[test]
    fn turning_face_down_keeps_the_box_volume_and_winding() {
        // A slab with a thin plate on top: after the half turn the plate is on
        // the bed and the slab above it, in the same bounding box.
        let mut solids = vec![
            Solid {
                name: "body".into(),
                role: "body".into(),
                mesh: cuboid([0.0, 0.0, 0.0], [4.0, 8.0, 3.0]),
            },
            Solid {
                name: "accent".into(),
                role: "accent".into(),
                mesh: cuboid([1.0, 1.0, 3.0], [3.0, 2.0, 3.5]),
            },
        ];
        let before: Vec<f64> = solids.iter().map(|s| mesh_volume(&s.mesh)).collect();
        assert_eq!(before, vec![96.0, 1.0]);
        turn_face_down(&mut solids);
        // A rotation keeps the volume and its sign. The divergence sum runs
        // over other coordinates now (the plate moved from y = 1..2 to 6..7),
        // so the last bit may differ: 1.0000000000000002 is 1 + 2^-52.
        for (a, b) in solids.iter().map(|s| mesh_volume(&s.mesh)).zip(&before) {
            assert!((a - b).abs() <= 1e-12 * b, "{a} vs {b}");
        }
        assert_eq!(
            solids[1].mesh.bounds(),
            ([1.0, 6.0, 0.0], [3.0, 7.0, 0.5]),
            "the plate is on the bed, at y mirrored through the centre"
        );
        assert_eq!(solids[0].mesh.bounds(), ([0.0, 0.0, 0.5], [4.0, 8.0, 3.5]));
        // Golden fingerprints of the turned meshes (indices unchanged, only
        // positions move), so a change to the turn shows up here.
        let prints: Vec<String> = solids
            .iter()
            .map(|s| format!("{:016x}", s.mesh.fingerprint()))
            .collect();
        assert_eq!(prints, ["5a676f4ea10a0d15", "143c461575771195"]);
        // Still closed and outward: the 3MF writer checks every edge.
        let meta = ExportMeta {
            part_id: "test.box".into(),
            version: "0.0.1".into(),
            content_hash: "test".into(),
            fit_profile_id: "none".into(),
            process_id: "fdm-0.4".into(),
            generator: crate::ui::GENERATOR.into(),
            kernel_id: "none".into(),
            title: String::new(),
            serial: None,
        };
        let (bytes, entry) = encode(&solids, &meta, Format::ThreeMf).expect("closed meshes");
        assert!(bytes.starts_with(b"PK"));
        assert_eq!(entry["objects"][1]["boundsMm"]["min"][2], 0.0);
        assert_eq!(entry["metadata"]["openbrick:partId"], "test.box");
    }

    #[test]
    fn formats_follow_the_extension() {
        assert_eq!(
            Format::of(Path::new("a/tile.3MF")).ok(),
            Some(Format::ThreeMf)
        );
        assert_eq!(Format::of(Path::new("tile.stl")).ok(), Some(Format::Stl));
        assert!(Format::of(Path::new("tile.obj")).is_err());
        assert!(Format::of(Path::new("tile")).is_err());
    }
}
