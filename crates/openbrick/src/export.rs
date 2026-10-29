//! Writing built parts and the coupon as 3MF or STL (docs/plan/05 section 7,
//! docs/plan/08 section 5) through `openbrick-io`, laid out the way they print,
//! and describing every written file in the `--json` object so
//! `scripts/validate-3mf.py` can compare it with what lib3mf reads.
//!
//! The CLI and the browser playground (`openbrick-wasm`'s `export_part`) share
//! the two decisions that shape a printable file: the 3MF layout, [`LAYOUT`]
//! (one object whose parts are the colour roles), and the face-down transform,
//! [`openbrick_geom::turn_face_down`]. `crates/openbrick/tests/export_parity.rs`
//! checks that both write the same meshes for the same inputs.

use std::path::Path;

use openbrick_geom::Mesh;
use openbrick_io::threemf::{assembly_name, package_uuids_with_layout, stored_fingerprint};
use openbrick_io::{
    write_3mf_with_layout, write_stl_binary, ExportLayout, ExportMeta, ExportObject,
};
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
                "{}: the output format follows the file extension; use .3mf (one object with a part per colour role, with traceability metadata) or .stl (one single-colour solid)",
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
    /// The display colour written for it, `role_colour` of its role unless
    /// `build --no-colour` left the colours out ([`leave_out_colours`]).
    pub colour: Option<[u8; 4]>,
    pub mesh: Mesh,
}

impl Solid {
    /// A solid with its role's display colour.
    pub fn new(name: String, role: String, mesh: Mesh) -> Solid {
        Solid {
            colour: role_colour(&role),
            name,
            role,
            mesh,
        }
    }

    fn object(&self) -> ExportObject<'_> {
        ExportObject::new(
            self.name.clone(),
            self.role.clone(),
            self.colour,
            &self.mesh,
        )
    }
}

/// Write the solids without display colours (`openbrick build --no-colour`):
/// the 3MF then has no colour group and no `pid`. Bambu Studio 2.5 and later
/// read a 3MF's colours into its "Standard 3MF Import color" dialog, which
/// users report paints them onto the surface instead of giving each part a
/// filament (bambulab/BambuStudio#9666); without colours that dialog cannot
/// open (docs/spikes/q18-slicer-multimaterial-import.md, recommendation 2).
/// The slicer assigns a filament per part either way. Colours stay the
/// default, because whether to leave them out for everyone is a decision for a
/// human (Q18) and PrusaSlicer ignores them.
pub fn leave_out_colours(solids: &mut [Solid]) {
    for s in solids {
        s.colour = None;
    }
}

/// Turn the solids over together, face down, with the transform every export
/// shares, [`openbrick_geom::turn_face_down`]: a half turn about the x axis
/// inside their common bounding box, so the top face lands on the bed (z = 0)
/// and the box stays where it was. A rotation, not a mirror: triangle winding
/// and closedness are unchanged, and artwork reads the right way round when the
/// print is turned back over. Used for parts whose artwork prints face down
/// (inlay, `Artwork::face_on_bed` = `top`).
pub fn turn_face_down(solids: &mut [Solid]) {
    let mut meshes: Vec<&mut Mesh> = solids.iter_mut().map(|s| &mut s.mesh).collect();
    openbrick_geom::turn_face_down(&mut meshes);
}

/// The 3MF layout of every file the CLI writes, the part-designer layout
/// (docs/plan/05 section 7, docs/spikes/q18-slicer-multimaterial-import.md):
/// several colour roles become the parts of one components object under a
/// single build item, so Bambu Studio loads one object with its parts in place
/// and PrusaSlicer offers to merge them into one multi-part object. A single
/// solid (a fused build, the coupon) has nothing to group, and its file is
/// exactly the separate-objects one.
pub const LAYOUT: ExportLayout = ExportLayout::PART_DESIGNER;

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
            let bytes =
                write_3mf_with_layout(&objects, meta, LAYOUT).map_err(|e| internal("3MF", e))?;
            let uuids = package_uuids_with_layout(&objects, meta, LAYOUT);
            // The layout the file holds: a single solid is written as the
            // separate layout, so it is reported as one.
            let grouped = LAYOUT.groups(objects.len());
            let layout = if grouped {
                LAYOUT
            } else {
                ExportLayout::SeparateObjects
            };
            let described: Vec<Value> = solids
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (lo, hi) = s.mesh.bounds();
                    json!({
                        "name": s.name,
                        "role": s.role,
                        "color": s.colour.map(hex),
                        "vertices": s.mesh.positions.len(),
                        "triangles": s.mesh.triangle_count(),
                        "volumeMm3": mesh_volume(&s.mesh),
                        // f64 of the exact f32 bounds, as lib3mf reads them.
                        "boundsMm": { "min": lo.map(f64::from), "max": hi.map(f64::from) },
                        "storedFingerprint": format!("{:016x}", stored_fingerprint(&s.mesh)),
                        "uuid": uuids.objects.get(i).map(|u| u.to_string()),
                        // A part of the components object has a component UUID
                        // instead of a build item of its own.
                        "itemUuid": if grouped {
                            None
                        } else {
                            uuids.items.get(i).map(|u| u.to_string())
                        },
                        "componentUuid": uuids.components.get(i).map(|u| u.to_string()),
                    })
                })
                .collect();
            // The components object, or null: its name, UUID and build item,
            // and one identity component per part in object order (the 3MF
            // matrix as 4 rows of 3), as the write_fixtures manifest has it.
            let identity = [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
            ];
            let assembly = uuids.assembly.map(|uuid| {
                let components: Vec<Value> = solids
                    .iter()
                    .zip(&uuids.components)
                    .map(|(s, u)| {
                        json!({ "object": s.name, "uuid": u.to_string(), "transform": identity })
                    })
                    .collect();
                json!({
                    "name": assembly_name(meta),
                    "uuid": uuid.to_string(),
                    "itemUuid": uuids.items.first().map(|u| u.to_string()),
                    "components": components,
                })
            });
            let entry = json!({
                "format": "3mf",
                "bytes": bytes.len(),
                "layout": layout.name(),
                "assembly": assembly,
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
            Solid::new(
                "body".into(),
                "body".into(),
                cuboid([0.0, 0.0, 0.0], [4.0, 8.0, 3.0]),
            ),
            Solid::new(
                "accent".into(),
                "accent".into(),
                cuboid([1.0, 1.0, 3.0], [3.0, 2.0, 3.5]),
            ),
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
        // positions move), so a change to the turn shows up here. They were
        // recorded with the CLI's own copy of the turn, before it became a call
        // to openbrick_geom::turn_face_down: the shared transform gives the same
        // bits.
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
        // Two roles: the part-designer layout, one components object (named
        // after the part id, as the title is blank) whose parts are the solids.
        let objects: Vec<ExportObject> = solids.iter().map(Solid::object).collect();
        assert_eq!(
            bytes,
            write_3mf_with_layout(&objects, &meta, ExportLayout::PART_DESIGNER).expect("3MF")
        );
        assert_eq!(entry["layout"], "oneObjectWithParts");
        let uuids = package_uuids_with_layout(&objects, &meta, ExportLayout::PART_DESIGNER);
        let assembly = &entry["assembly"];
        assert_eq!(assembly["name"], "test.box");
        assert_eq!(
            assembly["uuid"],
            json!(uuids.assembly.map(|u| u.to_string()))
        );
        assert_eq!(assembly["itemUuid"], json!(uuids.items[0].to_string()));
        assert_eq!(uuids.items.len(), 1, "one build item");
        for (i, s) in solids.iter().enumerate() {
            let o = &entry["objects"][i];
            assert_eq!(o["itemUuid"], Value::Null, "a part has no build item");
            assert_eq!(o["componentUuid"], json!(uuids.components[i].to_string()));
            let c = &assembly["components"][i];
            assert_eq!(c["object"], json!(s.name));
            assert_eq!(c["uuid"], o["componentUuid"]);
            assert_eq!(c["transform"][3], json!([0.0, 0.0, 0.0]));
        }
    }

    #[test]
    fn a_single_solid_is_written_as_the_separate_layout() {
        // One solid (a fused build, the coupon) has nothing to group: the same
        // bytes as write_3mf, an item of its own and no components object.
        let solids = [Solid::new(
            "tile".into(),
            "body".into(),
            cuboid([0.0, 0.0, 0.0], [4.0, 8.0, 3.0]),
        )];
        let meta = ExportMeta {
            part_id: "test.box".into(),
            version: "0.0.1".into(),
            content_hash: "test".into(),
            fit_profile_id: "none".into(),
            process_id: "fdm-0.4".into(),
            generator: crate::ui::GENERATOR.into(),
            kernel_id: "none".into(),
            title: "Box".into(),
            serial: None,
        };
        let (bytes, entry) = encode(&solids, &meta, Format::ThreeMf).expect("a closed mesh");
        let objects: Vec<ExportObject> = solids.iter().map(Solid::object).collect();
        assert_eq!(
            bytes,
            openbrick_io::write_3mf(&objects, &meta).expect("3MF")
        );
        assert_eq!(entry["layout"], "separateObjects");
        assert_eq!(entry["assembly"], Value::Null);
        let uuids = openbrick_io::threemf::package_uuids(&objects, &meta);
        assert_eq!(entry["buildUuid"], json!(uuids.build.to_string()));
        assert_eq!(
            entry["objects"][0]["itemUuid"],
            json!(uuids.items[0].to_string())
        );
        assert_eq!(entry["objects"][0]["componentUuid"], Value::Null);
    }

    #[test]
    fn no_colour_writes_the_same_parts_without_a_colour_group() {
        // build --no-colour: the same layout and meshes, no colour group and no
        // pid, so Bambu Studio's import colour dialog has nothing to read
        // (Q18 recommendation 2). The description says so with a null colour.
        let mut solids = vec![
            Solid::new(
                "body".into(),
                "body".into(),
                cuboid([0.0, 0.0, 0.0], [4.0, 8.0, 3.0]),
            ),
            Solid::new(
                "accent".into(),
                "accent".into(),
                cuboid([1.0, 1.0, 3.0], [3.0, 2.0, 3.5]),
            ),
        ];
        let meta = ExportMeta {
            part_id: "test.box".into(),
            version: "0.0.1".into(),
            content_hash: "test".into(),
            fit_profile_id: "none".into(),
            process_id: "fdm-0.4".into(),
            generator: crate::ui::GENERATOR.into(),
            kernel_id: "none".into(),
            title: "Box".into(),
            serial: None,
        };
        let (coloured, with) = encode(&solids, &meta, Format::ThreeMf).expect("3MF");
        assert_eq!(with["objects"][1]["color"], "#1E5AA8FF");
        let xml = openbrick_io::threemf_model_xml_with_layout(
            &solids.iter().map(Solid::object).collect::<Vec<_>>(),
            &meta,
            LAYOUT,
        )
        .expect("model");
        assert!(xml.contains("colorgroup"), "colours are the default");

        leave_out_colours(&mut solids);
        let (bytes, entry) = encode(&solids, &meta, Format::ThreeMf).expect("3MF");
        assert_ne!(bytes, coloured);
        let objects: Vec<ExportObject> = solids.iter().map(Solid::object).collect();
        let xml =
            openbrick_io::threemf_model_xml_with_layout(&objects, &meta, LAYOUT).expect("model");
        assert!(
            !xml.contains("colorgroup") && !xml.contains("pid="),
            "{xml}"
        );
        assert_eq!(xml.matches("<component ").count(), 2, "still two parts");
        assert_eq!(entry["layout"], "oneObjectWithParts");
        for (i, o) in entry["objects"]
            .as_array()
            .expect("objects")
            .iter()
            .enumerate()
        {
            assert_eq!(o["color"], Value::Null, "object {i}");
            // The same parts and meshes. The colour is part of the UUID seed,
            // so another file gets other UUIDs.
            for key in ["name", "role", "storedFingerprint", "boundsMm"] {
                assert_eq!(o[key], with["objects"][i][key], "object {i} {key}");
            }
            assert_ne!(o["uuid"], with["objects"][i]["uuid"], "object {i}");
        }
        assert_eq!(entry["assembly"]["name"], with["assembly"]["name"]);
        let names = |e: &Value| -> Vec<Value> {
            e["assembly"]["components"]
                .as_array()
                .expect("components")
                .iter()
                .map(|c| c["object"].clone())
                .collect()
        };
        assert_eq!(names(&entry), names(&with));
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
