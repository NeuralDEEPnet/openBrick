//! 3MF writer (docs/plan/05 section 7, docs/plan/08 section 4, decision D18 in
//! docs/plan/03).
//!
//! The package holds three parts, written in this order into a zip (see
//! `zip_writer`): `[Content_Types].xml`, `_rels/.rels` and `3D/3dmodel.model`.
//! The model follows the 3MF Core specification 1.4.0 (3MF is ISO/IEC
//! 25422:2025), with
//!
//! - unit millimetre; meshes are written as given (millimetres, Z up, one frame
//!   shared by all objects), so every build item and every component has the
//!   identity transform (none is written);
//! - one mesh `<object type="model">` per [`ExportObject`], laid out by an
//!   [`ExportLayout`]: [`ExportLayout::SeparateObjects`] (what [`write_3mf`]
//!   writes) gives each object its own build `<item>`;
//!   [`ExportLayout::OneObjectWithParts`] adds one `<components>` object that
//!   references every mesh object in input order and gives only that object a
//!   build item, so slicers that read components as parts load one object with
//!   a part per [`ExportObject`], in place (docs/plan/05 section 7,
//!   docs/spikes/q18-slicer-multimaterial-import.md);
//! - one `<m:colorgroup>` (Materials and Properties extension 1.2.1) holding one
//!   sRGB colour per role, referenced by each mesh object's `pid` and `pindex`;
//!   objects without a colour carry no property, and neither does the
//!   components object (Core 1.4.0 chapter 4: producers must not put `pid` on
//!   an object with components);
//! - `p:UUID` (Production extension 1.1.2) on the build, every item, every
//!   object and every component, derived by [`package_uuids_with_layout`] from
//!   the metadata, the objects and the layout instead of drawn at random;
//! - model metadata `Title`, `Application` and `openbrick:*` from [`ExportMeta`],
//!   and an object metadata group with `openbrick:role` on every mesh object.
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

/// How a package arranges its objects. Both layouts write one mesh object per
/// [`ExportObject`] in the same shared frame; they differ in what the build
/// places. Findings from the slicers' import code and issue trackers, with
/// links and what could not be confirmed, are in
/// docs/spikes/q18-slicer-multimaterial-import.md.
///
/// - The 3MF Core specification 1.4.0 lets consumers rearrange build items
///   ("consumers MAY rearrange the items", section 3.4.3.1) but requires them
///   to respect the relative positions of components (section 4.2).
/// - Bambu Studio (2.x) builds one object per build item, and an object with
///   components becomes one object with a part per component, each keeping its
///   transform. Several build items load as separate objects, which users
///   report being dropped to the bed one by one.
/// - PrusaSlicer 2.9.x turns each component into an object of its own, as it
///   does with build items, so both layouts load alike there: objects whose
///   boxes touch or overlap make it ask "Multi-part object detected"; Yes
///   merges them into one object with parts in place and assigns extruders
///   1, 2, ... in object order.
///
/// So [`ExportLayout::PART_DESIGNER`], the layout for part-designer exports
/// with more than one colour role, is [`ExportLayout::OneObjectWithParts`].
/// `Default` is [`ExportLayout::SeparateObjects`], what [`write_3mf`] has
/// always written, so existing callers and fixtures keep their bytes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum ExportLayout {
    /// One mesh object and one build item per [`ExportObject`].
    #[default]
    SeparateObjects,
    /// One mesh object per [`ExportObject`], one components object (named
    /// after [`ExportMeta::title`], or the part id when the title is blank)
    /// that references them all in input order with no transform, and a single
    /// build item for the components object. With one [`ExportObject`] there is
    /// nothing to group, and the package is exactly the
    /// [`ExportLayout::SeparateObjects`] one (same bytes, same UUIDs).
    OneObjectWithParts,
}

impl ExportLayout {
    /// The layout of part-designer exports (docs/plan/05 section 7): one object
    /// whose parts are the colour roles, so a multi-material printer gets one
    /// object in place and the user assigns a filament to each part.
    pub const PART_DESIGNER: ExportLayout = ExportLayout::OneObjectWithParts;
    /// Every layout, in declaration order.
    pub const ALL: [ExportLayout; 2] = [
        ExportLayout::SeparateObjects,
        ExportLayout::OneObjectWithParts,
    ];

    /// The camelCase name used in JSON, in the UUID seed and in fixtures.
    pub fn name(self) -> &'static str {
        match self {
            ExportLayout::SeparateObjects => "separateObjects",
            ExportLayout::OneObjectWithParts => "oneObjectWithParts",
        }
    }

    /// The layout named `name` (see [`ExportLayout::name`]).
    pub fn from_name(name: &str) -> Option<ExportLayout> {
        ExportLayout::ALL.into_iter().find(|l| l.name() == name)
    }

    /// Whether this layout writes a components object for `objects` objects:
    /// only [`ExportLayout::OneObjectWithParts`] with two or more.
    pub fn groups(self, objects: usize) -> bool {
        self == ExportLayout::OneObjectWithParts && objects >= 2
    }
}

/// Write a 3MF package with one build item per object
/// ([`ExportLayout::SeparateObjects`]). See the module documentation for the
/// content; nothing is written when any object or metadata field fails the
/// checks listed on [`threemf_model_xml`].
pub fn write_3mf(objects: &[ExportObject<'_>], meta: &ExportMeta) -> Result<Vec<u8>, IoError> {
    write_3mf_with_layout(objects, meta, ExportLayout::SeparateObjects)
}

/// Write a 3MF package in the given layout; [`write_3mf`] is this with
/// [`ExportLayout::SeparateObjects`]. Part-designer exports use
/// [`ExportLayout::PART_DESIGNER`].
pub fn write_3mf_with_layout(
    objects: &[ExportObject<'_>],
    meta: &ExportMeta,
    layout: ExportLayout,
) -> Result<Vec<u8>, IoError> {
    let model = threemf_model_xml_with_layout(objects, meta, layout)?;
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
    /// One per mesh object, in input order.
    pub objects: Vec<Uuid>,
    /// One per build item, in build order: one per object, or the single item
    /// of the components object.
    pub items: Vec<Uuid>,
    /// The components object, when the layout writes one.
    pub assembly: Option<Uuid>,
    /// One per component (so per mesh object, in input order) when the layout
    /// writes a components object; empty otherwise.
    pub components: Vec<Uuid>,
}

/// The name of the components object: the title, or the part id when the
/// title is blank (`write_3mf` requires a non-blank part id).
pub fn assembly_name(meta: &ExportMeta) -> &str {
    if meta.title.trim().is_empty() {
        &meta.part_id
    } else {
        &meta.title
    }
}

/// FNV-1a (64 bit) of a mesh as the 3MF stores it: the little-endian `f32` bits
/// of every coordinate, with `-0` taken as `0` because it is written as `0`,
/// then every index as a little-endian `u32`. A reader can recompute it from the
/// file alone (`scripts/validate-3mf.py` does, from what lib3mf reads). It
/// equals [`Mesh::fingerprint`](openbrick_geom::Mesh::fingerprint) for a mesh
/// without negative zeros.
pub fn stored_fingerprint(mesh: &openbrick_geom::Mesh) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: [u8; 4]| {
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for p in &mesh.positions {
        for c in p {
            eat((c + 0.0).to_le_bytes());
        }
    }
    for i in &mesh.indices {
        eat(i.to_le_bytes());
    }
    h
}

/// Derive the package UUIDs from everything the model part holds, so the same
/// export always gets the same UUIDs, and exports that differ in a metadata
/// field (content hash, serial, fit profile, ...) or in any object (geometry,
/// name, role, colour, order, count) get different ones. The geometry enters
/// through [`stored_fingerprint`], which hashes the vertex and triangle lists
/// exactly as they are written, so no export option the metadata does not
/// record (fused or not, another tessellation) can reuse another file's UUIDs.
///
/// All are name-based version 5 UUIDs (RFC 9562 section 5.5):
///
/// 1. root = v5(DNS namespace, "openbrick3d.org"), see [`UUID_ROOT_NAME`];
/// 2. package = v5(root, seed). The seed is UTF-8 text, one line each, ending
///    in `\n`:
///    - `openbrick 3mf package`;
///    - one line per [`ExportMeta`] field in declaration order (`part_id`,
///      `version`, `content_hash`, `fit_profile_id`, `process_id`, `generator`,
///      `kernel_id`, `title`, `serial`): `name=<byte length>:<value>`, or
///      `serial=-` when there is no serial;
///    - `objects=<count>`;
///    - per object i, counting from 0: `object/i/name=<byte length>:<name>`,
///      `object/i/role=<byte length>:<role>`, `object/i/color=#RRGGBBAA`
///      (upper-case hex, as written) or `object/i/color=-`, and
///      `object/i/mesh=<vertices>:<triangles>:<stored fingerprint as 16
///      lower-case hex digits>`.
///
///    The length prefixes keep the encoding unambiguous.
/// 3. build = v5(package, "build"), object i = v5(package, "object/i") and
///    item i = v5(package, "item/i").
///
/// These are the UUIDs of [`ExportLayout::SeparateObjects`]; see
/// [`package_uuids_with_layout`] for the components layout.
pub fn package_uuids(objects: &[ExportObject<'_>], meta: &ExportMeta) -> PackageUuids {
    package_uuids_with_layout(objects, meta, ExportLayout::SeparateObjects)
}

/// [`package_uuids`] for any layout. A layout that writes a components object
/// ([`ExportLayout::groups`]) appends two lines to the seed, after the objects:
///
/// - `layout=oneObjectWithParts` ([`ExportLayout::name`]);
/// - `assembly=<byte length>:<name>`, the components object's name
///   ([`assembly_name`]),
///
/// and derives the components object = v5(package, "assembly"), component i =
/// v5(package, "component/i") and its single build item = v5(package,
/// "item/0"). Other packages keep the seed and UUIDs of [`package_uuids`]: a
/// [`ExportLayout::OneObjectWithParts`] export of one object is the same file
/// as the separate layout, so it has the same UUIDs, and a grouped export never
/// shares UUIDs with an ungrouped one.
pub fn package_uuids_with_layout(
    objects: &[ExportObject<'_>],
    meta: &ExportMeta,
    layout: ExportLayout,
) -> PackageUuids {
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
    let _ = writeln!(seed, "objects={}", objects.len());
    for (i, o) in objects.iter().enumerate() {
        let _ = writeln!(seed, "object/{i}/name={}:{}", o.name.len(), o.name);
        let _ = writeln!(seed, "object/{i}/role={}:{}", o.role.len(), o.role);
        match o.color_srgb {
            Some(c) => {
                let _ = writeln!(seed, "object/{i}/color={}", colour_hex(c));
            }
            None => {
                let _ = writeln!(seed, "object/{i}/color=-");
            }
        }
        let _ = writeln!(
            seed,
            "object/{i}/mesh={}:{}:{:016x}",
            o.mesh.positions.len(),
            o.mesh.triangle_count(),
            stored_fingerprint(o.mesh)
        );
    }
    let grouped = layout.groups(objects.len());
    if grouped {
        let name = assembly_name(meta);
        let _ = writeln!(seed, "layout={}", layout.name());
        let _ = writeln!(seed, "assembly={}:{name}", name.len());
    }
    let package = Uuid::new_v5(&root, seed.as_bytes());
    let derive = |kind: &str, i: usize| Uuid::new_v5(&package, format!("{kind}/{i}").as_bytes());
    let n = objects.len();
    PackageUuids {
        build: Uuid::new_v5(&package, b"build"),
        objects: (0..n).map(|i| derive("object", i)).collect(),
        items: (0..if grouped { 1 } else { n })
            .map(|i| derive("item", i))
            .collect(),
        assembly: grouped.then(|| Uuid::new_v5(&package, b"assembly")),
        components: if grouped {
            (0..n).map(|i| derive("component", i)).collect()
        } else {
            Vec::new()
        },
    }
}

/// `#RRGGBBAA` in upper-case hex, the form `m:color` uses.
fn colour_hex([r, g, b, a]: [u8; 4]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
}

/// Every [`ExportMeta`] field must be text XML can carry, and every field but
/// `title` must hold more than white space (`serial` only when present): a 3MF
/// without its traceability metadata cannot be traced to its document.
fn check_meta(meta: &ExportMeta) -> Result<(), IoError> {
    for (name, value) in meta.fields() {
        let field = format!("meta.{name}");
        check_text(&field, value.unwrap_or_default())?;
        if name != "title" && value.is_some_and(|v| v.trim().is_empty()) {
            return Err(IoError::MissingMeta { field });
        }
    }
    Ok(())
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
/// Checks, in this order: at least one object; every metadata field holds only
/// characters XML 1.0 allows, and every field but `title` is non-blank (`serial`
/// when present); object names and roles hold only characters XML 1.0 allows;
/// roles are non-empty; objects of one role share a colour; every mesh has whole
/// triangles, indices in range, finite coordinates, at least one triangle, three
/// distinct vertices per triangle, and is closed and consistently oriented (each
/// edge used once in each direction), as 3MF requires of objects of type "model".
pub fn threemf_model_xml(
    objects: &[ExportObject<'_>],
    meta: &ExportMeta,
) -> Result<String, IoError> {
    threemf_model_xml_with_layout(objects, meta, ExportLayout::SeparateObjects)
}

/// [`threemf_model_xml`] in the given layout: what [`write_3mf_with_layout`]
/// puts in the zip. The same checks apply. The writers do not test whether
/// objects overlap; the evaluator builds touching, non-overlapping objects, and
/// `scripts/validate-3mf.py` samples every package for overlaps.
pub fn threemf_model_xml_with_layout(
    objects: &[ExportObject<'_>],
    meta: &ExportMeta,
    layout: ExportLayout,
) -> Result<String, IoError> {
    if objects.is_empty() {
        return Err(IoError::NoObjects);
    }
    check_meta(meta)?;
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
    let uuids = package_uuids_with_layout(objects, meta, layout);

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
    metadata(&mut out, " ", "Application", &meta.generator, false);
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
        for &(_, colour) in &colours {
            let _ = writeln!(out, "   <m:color color=\"{}\"/>", colour_hex(colour));
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
    // The components object comes after the mesh objects it references (Core
    // 1.4.0 section 4.2.1: objects are defined before they are referenced) and
    // carries no pid (chapter 4: not on objects with components). The
    // components have no transform: every mesh is already in the shared frame.
    let assembly = uuids
        .assembly
        .map(|uuid| (first_object_id + objects.len(), uuid));
    if let Some((id, uuid)) = assembly {
        let _ = write!(out, "  <object id=\"{id}\" type=\"model\" name=\"");
        escape_into(&mut out, assembly_name(meta));
        let _ = writeln!(out, "\" p:UUID=\"{uuid}\">");
        out.push_str("   <components>\n");
        for (i, component) in uuids.components.iter().enumerate() {
            let _ = writeln!(
                out,
                "    <component objectid=\"{}\" p:UUID=\"{component}\"/>",
                first_object_id + i
            );
        }
        out.push_str("   </components>\n  </object>\n");
    }
    out.push_str(" </resources>\n");
    let _ = writeln!(out, " <build p:UUID=\"{}\">", uuids.build);
    let placed: Vec<usize> = match assembly {
        Some((id, _)) => vec![id],
        None => (0..objects.len()).map(|i| first_object_id + i).collect(),
    };
    for (id, uuid) in placed.iter().zip(&uuids.items) {
        let _ = writeln!(out, "  <item objectid=\"{id}\" p:UUID=\"{uuid}\"/>");
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
    /// computed independently in Python (the uuid and struct modules, no Rust):
    /// the stored fingerprints by FNV-1a over `struct.pack("<f", c)` of every
    /// coordinate and `struct.pack("<I", i)` of every index (94c82c300515cbf8 and
    /// 31830281756abe55), the seed as documented on `package_uuids`, then
    /// `root = uuid5(NAMESPACE_DNS, "openbrick3d.org")`, `pkg = uuid5(root, seed)`,
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
  <object id="2" type="model" name="body" pid="1" pindex="0" p:UUID="f25cd000-d00b-5792-9349-2681f941d8be">
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
  <object id="3" type="model" name="accent &quot;x&quot;" p:UUID="1c61f2e9-fce3-5a65-bb38-114fcdac3e41">
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
 <build p:UUID="54994c9d-067a-5155-8314-8ee9e19c9778">
  <item objectid="2" p:UUID="e58aa277-4798-593c-aeba-81e49c9ebe3a"/>
  <item objectid="3" p:UUID="be8efcbf-65d1-5149-b7e5-0d534df2a581"/>
 </build>
</model>
"##;
        assert_eq!(xml, expected);
        assert_eq!(stored_fingerprint(&a), 0x94c8_2c30_0515_cbf8);
        assert_eq!(stored_fingerprint(&b), 0x3183_0281_756a_be55);
    }

    /// The components layout for the same two objects, byte for byte: the mesh
    /// objects as before (new UUIDs), then the components object named after
    /// the title, with no pid and no transforms, and a single build item. The
    /// UUIDs were computed independently in Python as for `model_xml_golden`,
    /// with the two seed lines `layout=oneObjectWithParts` and
    /// `assembly=13:Tetra <A & B>` appended, then `uuid5(pkg, "assembly")`,
    /// `uuid5(pkg, "component/0")`, ... and `uuid5(pkg, "item/0")`.
    #[test]
    fn model_xml_golden_one_object_with_parts() {
        let (a, b) = (tetra(0.0), tetra(2.5));
        let objects = [
            ExportObject::new("body", "body", Some([0xC9, 0x1A, 0x09, 0xFF]), &a),
            ExportObject::new("accent \"x\"", "accent", None, &b),
        ];
        let xml =
            threemf_model_xml_with_layout(&objects, &meta(), ExportLayout::OneObjectWithParts)
                .unwrap();
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
  <object id="2" type="model" name="body" pid="1" pindex="0" p:UUID="282f3785-76aa-5b46-9e09-a8af49fa4fc8">
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
  <object id="3" type="model" name="accent &quot;x&quot;" p:UUID="ee1f433c-92ac-57c5-9bf6-9f44f8ac7bf6">
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
  <object id="4" type="model" name="Tetra &lt;A &amp; B&gt;" p:UUID="987b4428-acfa-5183-afdf-648dc5de7634">
   <components>
    <component objectid="2" p:UUID="33b23fe3-534e-5646-97d4-0f2cbb9b8166"/>
    <component objectid="3" p:UUID="236ad737-9648-5e74-85d4-34c1a6782d71"/>
   </components>
  </object>
 </resources>
 <build p:UUID="ec7e4aa9-88cc-572d-b45f-4a5d2f7c4558">
  <item objectid="4" p:UUID="6d42a29b-0ff1-5531-8213-53a22a32f4a4"/>
 </build>
</model>
"##;
        assert_eq!(xml, expected);
        // The same package through the UUID function.
        let u = package_uuids_with_layout(&objects, &meta(), ExportLayout::OneObjectWithParts);
        assert_eq!(u.items.len(), 1);
        assert_eq!(u.components.len(), 2);
        assert!(u.assembly.is_some());
        // The default layout is the one write_3mf has always written.
        assert_eq!(
            threemf_model_xml_with_layout(&objects, &meta(), ExportLayout::default()),
            threemf_model_xml(&objects, &meta())
        );
    }

    #[test]
    fn the_components_object_is_named_after_the_part_without_a_title() {
        let (a, b) = (tetra(0.0), tetra(2.0));
        let objects = [
            ExportObject::new("a", "body", None, &a),
            ExportObject::new("b", "accent", None, &b),
        ];
        let untitled = ExportMeta {
            title: " ".into(),
            ..meta()
        };
        assert_eq!(assembly_name(&untitled), "test.tetra");
        assert_eq!(assembly_name(&meta()), "Tetra <A & B>");
        let xml =
            threemf_model_xml_with_layout(&objects, &untitled, ExportLayout::OneObjectWithParts)
                .unwrap();
        assert!(xml.contains("<object id=\"3\" type=\"model\" name=\"test.tetra\" p:UUID="));
        // The name is part of the seed: another title, other UUIDs.
        let grouped = |m: &ExportMeta| {
            package_uuids_with_layout(&objects, m, ExportLayout::OneObjectWithParts)
        };
        assert_ne!(grouped(&untitled).assembly, grouped(&meta()).assembly);
    }

    #[test]
    fn layouts_never_share_uuids_and_one_object_is_not_grouped() {
        let (a, b) = (tetra(0.0), tetra(2.0));
        let red = Some([255, 0, 0, 255]);
        let objects = [
            ExportObject::new("a", "body", red, &a),
            ExportObject::new("b", "accent", None, &b),
        ];
        let separate = package_uuids(&objects, &meta());
        let grouped =
            package_uuids_with_layout(&objects, &meta(), ExportLayout::OneObjectWithParts);
        assert!(separate.assembly.is_none() && separate.components.is_empty());
        assert_eq!(separate.items.len(), 2);
        let mut all = vec![separate.build, grouped.build];
        all.extend(&separate.objects);
        all.extend(&separate.items);
        all.extend(&grouped.objects);
        all.extend(&grouped.items);
        all.extend(&grouped.components);
        all.extend(grouped.assembly);
        let count = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), count, "a UUID repeats across the two layouts");
        assert!(all.iter().all(|x| x.get_version_num() == 5));

        // One object: nothing to group, so the very same file and UUIDs.
        let one = [ExportObject::new("a", "body", red, &a)];
        for layout in ExportLayout::ALL {
            assert!(!layout.groups(1));
            assert_eq!(
                write_3mf_with_layout(&one, &meta(), layout).unwrap(),
                write_3mf(&one, &meta()).unwrap(),
                "{}",
                layout.name()
            );
        }
        assert!(ExportLayout::OneObjectWithParts.groups(2));
        assert!(!ExportLayout::SeparateObjects.groups(2));
        assert_eq!(
            ExportLayout::PART_DESIGNER,
            ExportLayout::OneObjectWithParts
        );
        assert_eq!(ExportLayout::default(), ExportLayout::SeparateObjects);
        for layout in ExportLayout::ALL {
            assert_eq!(ExportLayout::from_name(layout.name()), Some(layout));
            let json = serde_json::to_string(&layout).unwrap();
            assert_eq!(json, format!("\"{}\"", layout.name()));
        }
        assert_eq!(ExportLayout::from_name("parts"), None);
    }

    #[test]
    fn grouped_packages_are_byte_identical_and_refuse_bad_input_alike() {
        let (a, b, c) = (tetra(0.0), tetra(2.0), tetra(4.0));
        let objects = [
            ExportObject::new("a", "body", Some([9, 9, 9, 255]), &a),
            ExportObject::new("b", "accent", Some([200, 9, 9, 255]), &b),
            ExportObject::new("c", "serial", None, &c),
        ];
        let layout = ExportLayout::OneObjectWithParts;
        let first = write_3mf_with_layout(&objects, &meta(), layout).unwrap();
        assert_eq!(
            first,
            write_3mf_with_layout(&objects, &meta(), layout).unwrap()
        );
        assert_ne!(first, write_3mf(&objects, &meta()).unwrap());
        let xml = threemf_model_xml_with_layout(&objects, &meta(), layout).unwrap();
        assert_eq!(xml.matches("<component ").count(), 3);
        assert_eq!(xml.matches("<item ").count(), 1);
        assert!(!xml.contains("transform="));
        // The components object is the last resource, after its parts, and the
        // only object without a colour property or a role.
        let assembly = xml.find("<components>").unwrap();
        assert!(xml.rfind("<mesh>").unwrap() < assembly);
        assert_eq!(xml.matches("openbrick:role").count(), 3);

        let mut open = tetra(0.0);
        open.indices.truncate(9);
        let broken = [
            ExportObject::new("open", "body", None, &open),
            ExportObject::new("b", "accent", None, &b),
        ];
        assert!(matches!(
            write_3mf_with_layout(&broken, &meta(), layout),
            Err(IoError::NotClosed { ref object, .. }) if object == "open"
        ));
        assert_eq!(
            write_3mf_with_layout(&[], &meta(), layout),
            Err(IoError::NoObjects)
        );
    }

    #[test]
    fn uuid_root_is_the_documented_value() {
        // Python: uuid.uuid5(uuid.NAMESPACE_DNS, "openbrick3d.org")
        let root = Uuid::new_v5(&Uuid::NAMESPACE_DNS, UUID_ROOT_NAME.as_bytes());
        assert_eq!(root.to_string(), "c956d47c-96bf-5bbd-85a6-d3a1672ff0d7");
    }

    #[test]
    fn uuids_are_unique_in_a_package_and_follow_the_metadata() {
        let (a, b, c) = (tetra(0.0), tetra(2.0), tetra(4.0));
        let objects = [
            ExportObject::new("a", "body", None, &a),
            ExportObject::new("b", "accent", None, &b),
            ExportObject::new("c", "serial", None, &c),
        ];
        let u = package_uuids(&objects, &meta());
        let mut all = vec![u.build];
        all.extend(&u.objects);
        all.extend(&u.items);
        let count = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), count, "duplicate UUID in one package");
        assert!(all.iter().all(|x| x.get_version_num() == 5));

        assert_eq!(package_uuids(&objects, &meta()), u, "not deterministic");
        let other_serial = ExportMeta {
            serial: Some("NDN-0002".into()),
            ..meta()
        };
        assert_ne!(package_uuids(&objects, &other_serial).build, u.build);
        // A missing serial and an empty one are different seeds (write_3mf
        // refuses the empty one, but the derivation itself keeps them apart).
        let none = ExportMeta {
            serial: None,
            ..meta()
        };
        let empty = ExportMeta {
            serial: Some(String::new()),
            ..meta()
        };
        assert_ne!(
            package_uuids(&objects, &none).build,
            package_uuids(&objects, &empty).build
        );
        // Moving text between fields changes the seed (the length prefixes).
        let shifted = ExportMeta {
            part_id: "test.tetra1".into(),
            version: ".0.0".into(),
            ..meta()
        };
        assert_ne!(package_uuids(&objects, &shifted).build, u.build);
    }

    /// Equal metadata is not enough for equal UUIDs: the objects take part too,
    /// so a fused and an unfused export of one document, or a re-tessellated
    /// one, cannot share UUIDs (review of backlog item 12).
    #[test]
    fn uuids_follow_the_objects() {
        let (a, b) = (tetra(0.0), tetra(2.0));
        let red = Some([255, 0, 0, 255]);
        let base = [
            ExportObject::new("a", "body", red, &a),
            ExportObject::new("b", "accent", None, &b),
        ];
        let u = package_uuids(&base, &meta());
        let differs = |objects: &[ExportObject<'_>], what: &str| {
            let v = package_uuids(objects, &meta());
            assert_ne!(v.build, u.build, "{what}: build");
            assert_ne!(v.objects[0], u.objects[0], "{what}: object/0");
            assert_ne!(v.items[0], u.items[0], "{what}: item/0");
        };
        // Fewer objects (the reviewer's case: [a] against [b, a]).
        differs(&base[..1], "one object fewer");
        differs(&[base[1].clone(), base[0].clone()], "order");
        let moved = tetra(0.5);
        differs(
            &[ExportObject::new("a", "body", red, &moved), base[1].clone()],
            "geometry",
        );
        let mut renumbered = a.clone();
        renumbered.indices.rotate_left(3);
        differs(
            &[
                ExportObject::new("a", "body", red, &renumbered),
                base[1].clone(),
            ],
            "triangle order",
        );
        differs(
            &[ExportObject::new("a2", "body", red, &a), base[1].clone()],
            "name",
        );
        differs(
            &[ExportObject::new("a", "serial", red, &a), base[1].clone()],
            "role",
        );
        differs(
            &[
                ExportObject::new("a", "body", Some([255, 0, 0, 254]), &a),
                base[1].clone(),
            ],
            "colour",
        );
        differs(
            &[ExportObject::new("a", "body", None, &a), base[1].clone()],
            "no colour",
        );

        // -0 is written as 0, so a mesh that differs only there writes the same
        // file and gets the same UUIDs.
        let mut negative_zero = a.clone();
        negative_zero.positions[0] = [-0.0, -0.0, -0.0];
        assert_ne!(negative_zero.fingerprint(), a.fingerprint());
        assert_eq!(stored_fingerprint(&negative_zero), stored_fingerprint(&a));
        let same = [
            ExportObject::new("a", "body", red, &negative_zero),
            base[1].clone(),
        ];
        assert_eq!(package_uuids(&same, &meta()), u);
        assert_eq!(
            threemf_model_xml(&same, &meta()),
            threemf_model_xml(&base, &meta())
        );
    }

    #[test]
    fn stored_fingerprint_is_the_mesh_fingerprint_without_negative_zeros() {
        let a = tetra(2.5);
        assert_eq!(stored_fingerprint(&a), a.fingerprint());
        assert_ne!(stored_fingerprint(&a), stored_fingerprint(&tetra(2.0)));
    }

    #[test]
    fn coordinates_round_trip_through_their_decimal_text() {
        let values = [
            0.1f32,
            12.3,
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
        // Traceability metadata must be present (docs/plan/05 section 3).
        assert_eq!(
            write_3mf(&objects, &ExportMeta::default()),
            Err(IoError::MissingMeta {
                field: "meta.part_id".into()
            })
        );
        for (blank, field) in [
            (
                ExportMeta {
                    content_hash: " \t".into(),
                    ..meta()
                },
                "meta.content_hash",
            ),
            (
                ExportMeta {
                    fit_profile_id: String::new(),
                    ..meta()
                },
                "meta.fit_profile_id",
            ),
            (
                ExportMeta {
                    kernel_id: String::new(),
                    ..meta()
                },
                "meta.kernel_id",
            ),
            (
                ExportMeta {
                    serial: Some(String::new()),
                    ..meta()
                },
                "meta.serial",
            ),
        ] {
            let err = write_3mf(&objects, &blank).unwrap_err();
            assert_eq!(
                err,
                IoError::MissingMeta {
                    field: field.into()
                }
            );
            assert!(err.to_string().contains("docs/plan/05"), "{err}");
        }
        // The title is optional, and so is the serial number.
        let optional = ExportMeta {
            title: String::new(),
            serial: None,
            ..meta()
        };
        let xml = threemf_model_xml(&objects, &optional).unwrap();
        assert!(!xml.contains("\"Title\"") && !xml.contains("openbrick:serial"));

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
