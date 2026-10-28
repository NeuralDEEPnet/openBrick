//! 3MF writer (docs/plan/05 section 7, docs/plan/08 section 4, decision D18 in
//! docs/plan/03).
//!
//! The package holds three parts, written in this order into a zip (see
//! `zip_writer`): `[Content_Types].xml`, `_rels/.rels` and `3D/3dmodel.model`.
//! The model follows the 3MF Core specification 1.4.0 (3MF is ISO/IEC
//! 25422:2025), with
//!
//! - unit millimetre; meshes are written as given (millimetres, Z up, one frame
//!   shared by all objects), so every build item has the identity transform;
//! - one `<object type="model">` per [`ExportObject`] and one build `<item>` each;
//! - one `<m:colorgroup>` (Materials and Properties extension 1.2.1) holding one
//!   sRGB colour per role, referenced by each object's `pid` and `pindex`; objects
//!   without a colour carry no property;
//! - `p:UUID` (Production extension 1.1.2) on the build, every item and every
//!   object, derived by [`package_uuids`] instead of drawn at random;
//! - model metadata `Title`, `Application` and `openbrick:*` from [`ExportMeta`],
//!   and an object metadata group with `openbrick:role`.
//!
//! Neither extension is listed in `requiredextensions`: colours and UUIDs are
//! optional information, and a consumer that knows only the core still reads the
//! geometry. lib3mf 2.5.0 makes the same choice for this content.
//!
//! Coordinates are written with Rust's shortest round-trip formatting of `f32`, so
//! a reader that parses them to `f32` gets the kernel's exact bits back (`-0` is
//! written as `0`). The same input gives a byte-identical package.

use std::fmt::Write as _;

use uuid::Uuid;

use crate::export::{check_closed, check_mesh};
use crate::xml::{check_text, escape_into};
use crate::zip_writer::write_zip;
use crate::{ExportMeta, ExportObject, IoError};

/// 3MF Core namespace (unchanged since Core 1.0, still used by Core 1.4.0).
pub const CORE_NAMESPACE: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
/// Materials and Properties extension namespace, bound to the prefix `m`.
pub const MATERIAL_NAMESPACE: &str =
    "http://schemas.microsoft.com/3dmanufacturing/material/2015/02";
/// Production extension namespace, bound to the prefix `p`.
pub const PRODUCTION_NAMESPACE: &str =
    "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";
/// Namespace of the `openbrick:` metadata names. Like the JSON Schema ids
/// (schemas/README.md) it names the vocabulary; nothing fetches it.
pub const OPENBRICK_NAMESPACE: &str = "https://openbrick3d.org/3mf/metadata/1.0";
/// Path of the model part inside the package.
pub const MODEL_PATH: &str = "3D/3dmodel.model";
/// Content type of the model part.
pub const MODEL_CONTENT_TYPE: &str = "application/vnd.ms-package.3dmanufacturing-3dmodel+xml";
/// Relationship type that marks the model part as the 3D payload of the package.
pub const MODEL_RELATIONSHIP_TYPE: &str =
    "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
/// DNS name whose UUIDv5 in the DNS namespace (RFC 9562 section 6.5) is the root of
/// every UUID OpenBrick3D derives: c956d47c-96bf-5bbd-85a6-d3a1672ff0d7.
pub const UUID_ROOT_NAME: &str = "openbrick3d.org";

/// `[Content_Types].xml` (Open Packaging Conventions, ECMA-376 Part 2): default
/// content types for the relationship and model parts.
fn content_types_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n \
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n \
         <Default Extension=\"model\" ContentType=\"{MODEL_CONTENT_TYPE}\"/>\n\
         </Types>\n"
    )
}

/// `_rels/.rels`: the package relationship that names the model part.
fn rels_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n \
         <Relationship Target=\"/{MODEL_PATH}\" Id=\"rel0\" Type=\"{MODEL_RELATIONSHIP_TYPE}\"/>\n\
         </Relationships>\n"
    )
}

/// Write a 3MF package. See the module documentation for the content; nothing is
/// written when any object or metadata field fails the checks listed on
/// [`threemf_model_xml`].
pub fn write_3mf(objects: &[ExportObject<'_>], meta: &ExportMeta) -> Result<Vec<u8>, IoError> {
    let model = threemf_model_xml(objects, meta)?;
    write_zip(&[
        ("[Content_Types].xml", content_types_xml().as_bytes()),
        ("_rels/.rels", rels_xml().as_bytes()),
        (MODEL_PATH, model.as_bytes()),
    ])
}

/// The UUIDs of one package (Production extension 1.1.2, attribute `p:UUID`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUuids {
    pub build: Uuid,
    /// One per object, in input order.
    pub objects: Vec<Uuid>,
    /// One per build item, in input order.
    pub items: Vec<Uuid>,
}

/// Derive the package UUIDs from the metadata alone, so the same export always
/// gets the same UUIDs and exports that differ in any metadata field (content
/// hash, serial, fit profile, ...) get different ones.
///
/// All are name-based version 5 UUIDs (RFC 9562 section 5.5):
///
/// 1. root = v5(DNS namespace, "openbrick3d.org"), see [`UUID_ROOT_NAME`];
/// 2. package = v5(root, seed), where the seed is the line
///    `openbrick 3mf package` followed by one line per [`ExportMeta`] field in
///    declaration order, `name=<byte length>:<value>`, or `name=-` for a missing
///    serial (the length prefix keeps the encoding unambiguous);
/// 3. build = v5(package, "build"), object i = v5(package, "object/i") and
///    item i = v5(package, "item/i"), counting from 0.
pub fn package_uuids(meta: &ExportMeta, object_count: usize) -> PackageUuids {
    let root = Uuid::new_v5(&Uuid::NAMESPACE_DNS, UUID_ROOT_NAME.as_bytes());
    let mut seed = String::from("openbrick 3mf package\n");
    for (name, value) in meta.fields() {
        match value {
            Some(v) => {
                let _ = writeln!(seed, "{name}={}:{v}", v.len());
            }
            None => {
                let _ = writeln!(seed, "{name}=-");
            }
        }
    }
    let package = Uuid::new_v5(&root, seed.as_bytes());
    let derive = |kind: &str, i: usize| Uuid::new_v5(&package, format!("{kind}/{i}").as_bytes());
    PackageUuids {
        build: Uuid::new_v5(&package, b"build"),
        objects: (0..object_count).map(|i| derive("object", i)).collect(),
        items: (0..object_count).map(|i| derive("item", i)).collect(),
    }
}

/// A role and its colour, if it has one.
type RoleColour<'o> = (&'o str, Option<[u8; 4]>);

/// One colour per role, in order of first appearance; `None` for roles without one.
fn role_colours<'o>(objects: &'o [ExportObject<'_>]) -> Result<Vec<RoleColour<'o>>, IoError> {
    let mut roles: Vec<RoleColour<'o>> = Vec::new();
    for o in objects {
        match roles.iter().find(|(role, _)| *role == o.role) {
            Some(&(_, colour)) if colour != o.color_srgb => {
                return Err(IoError::RoleColourConflict {
                    role: o.role.clone(),
                })
            }
            Some(_) => {}
            None => roles.push((&o.role, o.color_srgb)),
        }
    }
    Ok(roles)
}

/// Append `<metadata name="..." [preserve="1"]>value</metadata>` on its own line.
fn metadata(out: &mut String, indent: &str, name: &str, value: &str, preserve: bool) {
    out.push_str(indent);
    out.push_str("<metadata name=\"");
    out.push_str(name);
    out.push_str(if preserve {
        "\" preserve=\"1\">"
    } else {
        "\">"
    });
    escape_into(out, value);
    out.push_str("</metadata>\n");
}

/// The model part `3D/3dmodel.model` as text: what [`write_3mf`] puts in the zip.
///
/// Checks, in this order: at least one object; every metadata field, object name
/// and role holds only characters XML 1.0 allows; roles are non-empty; objects of
/// one role share a colour; every mesh has whole triangles, indices in range,
/// finite coordinates, at least one triangle, three distinct vertices per
/// triangle, and is closed and consistently oriented (each edge used once in each
/// direction), as 3MF requires of objects of type "model".
pub fn threemf_model_xml(
    objects: &[ExportObject<'_>],
    meta: &ExportMeta,
) -> Result<String, IoError> {
    if objects.is_empty() {
        return Err(IoError::NoObjects);
    }
    for (name, value) in meta.fields() {
        check_text(&format!("meta.{name}"), value.unwrap_or_default())?;
    }
    for (i, o) in objects.iter().enumerate() {
        check_text(&format!("objects[{i}].name"), &o.name)?;
        check_text(&format!("objects[{i}].role"), &o.role)?;
        if o.role.is_empty() {
            return Err(IoError::EmptyRole {
                object: o.name.clone(),
            });
        }
    }
    let roles = role_colours(objects)?;
    for o in objects {
        check_mesh(&o.name, o.mesh)?;
        check_closed(&o.name, o.mesh)?;
    }

    // Colour index of each role in the colour group.
    let colours: Vec<(&str, [u8; 4])> = roles
        .iter()
        .filter_map(|&(role, colour)| colour.map(|c| (role, c)))
        .collect();
    let colour_group_id = 1;
    let first_object_id = if colours.is_empty() { 1 } else { 2 };
    let uuids = package_uuids(meta, objects.len());

    // Kernel meshes take about 45 bytes per vertex line and 50 per triangle line.
    // Only a hint: saturating (usize is 32 bits on wasm32), and growth takes over
    // if the reservation fails.
    let estimate = objects.iter().fold(2048usize, |n, o| {
        n.saturating_add(o.mesh.positions.len().saturating_mul(48))
            .saturating_add(o.mesh.triangle_count().saturating_mul(56))
    });
    let mut out = String::new();
    let _ = out.try_reserve(estimate);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE_NAMESPACE}\" xmlns:m=\"{MATERIAL_NAMESPACE}\" xmlns:p=\"{PRODUCTION_NAMESPACE}\" xmlns:openbrick=\"{OPENBRICK_NAMESPACE}\">"
    );
    if !meta.title.is_empty() {
        metadata(&mut out, " ", "Title", &meta.title, false);
    }
    if !meta.generator.is_empty() {
        metadata(&mut out, " ", "Application", &meta.generator, false);
    }
    for (name, value) in [
        ("openbrick:partId", Some(&meta.part_id)),
        ("openbrick:version", Some(&meta.version)),
        ("openbrick:contentHash", Some(&meta.content_hash)),
        ("openbrick:fitProfile", Some(&meta.fit_profile_id)),
        ("openbrick:process", Some(&meta.process_id)),
        ("openbrick:generator", Some(&meta.generator)),
        ("openbrick:kernel", Some(&meta.kernel_id)),
        ("openbrick:serial", meta.serial.as_ref()),
    ] {
        if let Some(value) = value {
            metadata(&mut out, " ", name, value, true);
        }
    }

    out.push_str(" <resources>\n");
    if !colours.is_empty() {
        let _ = writeln!(out, "  <m:colorgroup id=\"{colour_group_id}\">");
        for (_, [r, g, b, a]) in &colours {
            let _ = writeln!(out, "   <m:color color=\"#{r:02X}{g:02X}{b:02X}{a:02X}\"/>");
        }
        out.push_str("  </m:colorgroup>\n");
    }
    for (i, o) in objects.iter().enumerate() {
        let _ = write!(
            out,
            "  <object id=\"{}\" type=\"model\" name=\"",
            first_object_id + i
        );
        escape_into(&mut out, &o.name);
        out.push('"');
        if let Some(index) = colours.iter().position(|(role, _)| *role == o.role) {
            let _ = write!(out, " pid=\"{colour_group_id}\" pindex=\"{index}\"");
        }
        let _ = writeln!(out, " p:UUID=\"{}\">", uuids.objects[i]);
        out.push_str("   <metadatagroup>\n");
        metadata(&mut out, "    ", "openbrick:role", &o.role, true);
        out.push_str("   </metadatagroup>\n");
        out.push_str("   <mesh>\n    <vertices>\n");
        for p in &o.mesh.positions {
            // Adding 0.0 turns -0.0 into 0.0; Display never uses exponents.
            let [x, y, z] = p.map(|c| c + 0.0);
            let _ = writeln!(out, "     <vertex x=\"{x}\" y=\"{y}\" z=\"{z}\"/>");
        }
        out.push_str("    </vertices>\n    <triangles>\n");
        let (triangles, _) = o.mesh.indices.as_chunks::<3>();
        for [a, b, c] in triangles {
            let _ = writeln!(out, "     <triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"/>");
        }
        out.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    out.push_str(" </resources>\n");
    let _ = writeln!(out, " <build p:UUID=\"{}\">", uuids.build);
    for i in 0..objects.len() {
        let _ = writeln!(
            out,
            "  <item objectid=\"{}\" p:UUID=\"{}\"/>",
            first_object_id + i,
            uuids.items[i]
        );
    }
    out.push_str(" </build>\n</model>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_geom::Mesh;

    /// A tetrahedron with outward triangles, shifted along x. Abstract test
    /// geometry in millimetres, not a brick dimension.
    fn tetra(dx: f32) -> Mesh {
        Mesh {
            positions: vec![
                [dx, 0.0, 0.0],
                [dx + 1.0, 0.0, 0.0],
                [dx, 1.0, 0.0],
                [dx, 0.0, 1.0],
            ],
            indices: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2],
        }
    }

    fn meta() -> ExportMeta {
        ExportMeta {
            part_id: "test.tetra".into(),
            version: "1.0.0".into(),
            content_hash: "sha256:00ff".into(),
            fit_profile_id: "example-mk4s-petg-0.4".into(),
            process_id: "fdm-0.4".into(),
            generator: "openbrick-io 0.0.1".into(),
            kernel_id: "none (hand-built mesh)".into(),
            title: "Tetra <A & B>".into(),
            serial: Some("NDN-0001".into()),
        }
    }

    /// The whole model part for two small objects, byte for byte. The UUIDs were
    /// checked independently with Python's uuid module:
    /// `root = uuid5(NAMESPACE_DNS, "openbrick3d.org")`,
    /// `pkg = uuid5(root, seed)` with the seed documented on `package_uuids`, then
    /// `uuid5(pkg, "build")`, `uuid5(pkg, "object/0")` and so on.
    #[test]
    fn model_xml_golden() {
        let (a, b) = (tetra(0.0), tetra(2.5));
        let objects = [
            ExportObject::new("body", "body", Some([0xC9, 0x1A, 0x09, 0xFF]), &a),
            ExportObject::new("accent \"x\"", "accent", None, &b),
        ];
        let xml = threemf_model_xml(&objects, &meta()).unwrap();
        let expected = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" xmlns:openbrick="https://openbrick3d.org/3mf/metadata/1.0">
 <metadata name="Title">Tetra &lt;A &amp; B&gt;</metadata>
 <metadata name="Application">openbrick-io 0.0.1</metadata>
 <metadata name="openbrick:partId" preserve="1">test.tetra</metadata>
 <metadata name="openbrick:version" preserve="1">1.0.0</metadata>
 <metadata name="openbrick:contentHash" preserve="1">sha256:00ff</metadata>
 <metadata name="openbrick:fitProfile" preserve="1">example-mk4s-petg-0.4</metadata>
 <metadata name="openbrick:process" preserve="1">fdm-0.4</metadata>
 <metadata name="openbrick:generator" preserve="1">openbrick-io 0.0.1</metadata>
 <metadata name="openbrick:kernel" preserve="1">none (hand-built mesh)</metadata>
 <metadata name="openbrick:serial" preserve="1">NDN-0001</metadata>
 <resources>
  <m:colorgroup id="1">
   <m:color color="#C91A09FF"/>
  </m:colorgroup>
  <object id="2" type="model" name="body" pid="1" pindex="0" p:UUID="3a77f527-a7c4-570f-8748-b9e5fe06bf06">
   <metadatagroup>
    <metadata name="openbrick:role" preserve="1">body</metadata>
   </metadatagroup>
   <mesh>
    <vertices>
     <vertex x="0" y="0" z="0"/>
     <vertex x="1" y="0" z="0"/>
     <vertex x="0" y="1" z="0"/>
     <vertex x="0" y="0" z="1"/>
    </vertices>
    <triangles>
     <triangle v1="0" v2="2" v3="1"/>
     <triangle v1="0" v2="1" v3="3"/>
     <triangle v1="1" v2="2" v3="3"/>
     <triangle v1="0" v2="3" v3="2"/>
    </triangles>
   </mesh>
  </object>
  <object id="3" type="model" name="accent &quot;x&quot;" p:UUID="c5434001-ccdb-5317-b63f-b3ae227c2e59">
   <metadatagroup>
    <metadata name="openbrick:role" preserve="1">accent</metadata>
   </metadatagroup>
   <mesh>
    <vertices>
     <vertex x="2.5" y="0" z="0"/>
     <vertex x="3.5" y="0" z="0"/>
     <vertex x="2.5" y="1" z="0"/>
     <vertex x="2.5" y="0" z="1"/>
    </vertices>
    <triangles>
     <triangle v1="0" v2="2" v3="1"/>
     <triangle v1="0" v2="1" v3="3"/>
     <triangle v1="1" v2="2" v3="3"/>
     <triangle v1="0" v2="3" v3="2"/>
    </triangles>
   </mesh>
  </object>
 </resources>
 <build p:UUID="9a123626-b1b5-5035-b389-af6a1de6e778">
  <item objectid="2" p:UUID="33b2be0a-b6eb-5aff-9fc0-c7ae93078cca"/>
  <item objectid="3" p:UUID="42aa74e7-8547-5788-8ccf-d59a10ba5554"/>
 </build>
</model>
"##;
        assert_eq!(xml, expected);
    }

    #[test]
    fn uuid_root_is_the_documented_value() {
        // Python: uuid.uuid5(uuid.NAMESPACE_DNS, "openbrick3d.org")
        let root = Uuid::new_v5(&Uuid::NAMESPACE_DNS, UUID_ROOT_NAME.as_bytes());
        assert_eq!(root.to_string(), "c956d47c-96bf-5bbd-85a6-d3a1672ff0d7");
    }

    #[test]
    fn uuids_are_unique_in_a_package_and_follow_the_metadata() {
        let u = package_uuids(&meta(), 3);
        let mut all = vec![u.build];
        all.extend(&u.objects);
        all.extend(&u.items);
        let count = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), count, "duplicate UUID in one package");
        assert!(all.iter().all(|x| x.get_version_num() == 5));

        assert_eq!(package_uuids(&meta(), 3), u, "not deterministic");
        let other_serial = ExportMeta {
            serial: Some("NDN-0002".into()),
            ..meta()
        };
        assert_ne!(package_uuids(&other_serial, 3).build, u.build);
        // A missing serial and an empty one are different exports.
        let none = ExportMeta {
            serial: None,
            ..meta()
        };
        let empty = ExportMeta {
            serial: Some(String::new()),
            ..meta()
        };
        assert_ne!(
            package_uuids(&none, 1).build,
            package_uuids(&empty, 1).build
        );
        // Moving text between fields changes the seed (the length prefixes).
        let shifted = ExportMeta {
            part_id: "test.tetra1".into(),
            version: ".0.0".into(),
            ..meta()
        };
        assert_ne!(package_uuids(&shifted, 1).build, u.build);
    }

    #[test]
    fn coordinates_round_trip_through_their_decimal_text() {
        let values = [
            0.1f32,
            15.8,
            -3.25,
            1.0e-7,
            123_456.79,
            f32::MIN_POSITIVE,
            f32::MAX,
        ];
        for v in values {
            let text = format!("{}", v + 0.0);
            assert!(!text.contains(['e', 'E']), "{text}");
            assert_eq!(
                text.parse::<f32>().unwrap().to_bits(),
                v.to_bits(),
                "{text}"
            );
        }
        // Negative zero is written as plain 0.
        let mut m = tetra(0.0);
        m.positions[0] = [-0.0, -0.0, -0.0];
        let objects = [ExportObject::new("t", "body", None, &m)];
        let xml = threemf_model_xml(&objects, &meta()).unwrap();
        assert!(xml.contains("<vertex x=\"0\" y=\"0\" z=\"0\"/>"));
        assert!(!xml.contains("-0\""));
    }

    #[test]
    fn objects_of_one_role_share_one_colour_entry() {
        let (a, b, c) = (tetra(0.0), tetra(2.0), tetra(4.0));
        let red = Some([255, 0, 0, 255]);
        let objects = [
            ExportObject::new("a", "body", red, &a),
            ExportObject::new("b", "accent", Some([0, 0, 255, 128]), &b),
            ExportObject::new("c", "body", red, &c),
        ];
        let xml = threemf_model_xml(&objects, &meta()).unwrap();
        assert_eq!(xml.matches("<m:color ").count(), 2);
        assert!(xml.contains("<m:color color=\"#0000FF80\"/>"));
        assert_eq!(xml.matches("pid=\"1\" pindex=\"0\"").count(), 2);
        assert_eq!(xml.matches("pid=\"1\" pindex=\"1\"").count(), 1);
    }

    #[test]
    fn without_colours_there_is_no_colour_group() {
        let a = tetra(0.0);
        let objects = [ExportObject::new("a", "body", None, &a)];
        let xml = threemf_model_xml(&objects, &meta()).unwrap();
        assert!(!xml.contains("colorgroup"));
        assert!(!xml.contains("pid="));
        assert!(xml.contains("<object id=\"1\" type=\"model\" name=\"a\" p:UUID="));
        assert!(xml.contains("<item objectid=\"1\" "));
    }

    #[test]
    fn invalid_input_is_refused_with_a_reason() {
        let a = tetra(0.0);
        assert_eq!(write_3mf(&[], &meta()), Err(IoError::NoObjects));

        let objects = [ExportObject::new("a", "", None, &a)];
        assert_eq!(
            write_3mf(&objects, &meta()),
            Err(IoError::EmptyRole { object: "a".into() })
        );

        let objects = [
            ExportObject::new("a", "body", Some([1, 2, 3, 255]), &a),
            ExportObject::new("b", "body", None, &a),
        ];
        assert_eq!(
            write_3mf(&objects, &meta()),
            Err(IoError::RoleColourConflict {
                role: "body".into()
            })
        );

        let objects = [ExportObject::new("a", "body", None, &a)];
        let bad = ExportMeta {
            serial: Some("NDN\u{7}".into()),
            ..meta()
        };
        assert_eq!(
            write_3mf(&objects, &bad),
            Err(IoError::InvalidText {
                field: "meta.serial".into(),
                code: 7
            })
        );

        let mut open = tetra(0.0);
        open.indices.truncate(9);
        let objects = [ExportObject::new("open", "body", None, &open)];
        let err = write_3mf(&objects, &meta()).unwrap_err();
        assert!(matches!(err, IoError::NotClosed { ref object, .. } if object == "open"));
        // The message says what is wrong and where to look.
        let message = err.to_string();
        assert!(message.contains("open"), "{message}");
        assert!(message.contains("docs/plan/05"), "{message}");
    }

    #[test]
    fn packages_are_byte_identical_for_identical_input() {
        let (a, b) = (tetra(0.0), tetra(2.0));
        let objects = [
            ExportObject::new("a", "body", Some([9, 9, 9, 255]), &a),
            ExportObject::new("b", "accent", Some([200, 9, 9, 255]), &b),
        ];
        let first = write_3mf(&objects, &meta()).unwrap();
        let second = write_3mf(&objects, &meta()).unwrap();
        assert_eq!(first, second);
        let other = write_3mf(
            &objects,
            &ExportMeta {
                serial: None,
                ..meta()
            },
        )
        .unwrap();
        assert_ne!(first, other);
    }
}
