//! Part catalog (docs/plan/07 section 3).
//!
//! Today: the built-in catalog of the assembler alpha ([`builtin`]): standard
//! bricks, plates, tiles and a baseplate built by the part evaluator, each with
//! a render mesh, integer-LDU boxes and connectors derived from its features
//! ([`shape`]). Next (M3a): the packed LDraw subset, our own type-1 parser,
//! connector derivation from primitives and LOD baking.
//!
//! The LDraw Parts Library is downloaded at first run into the user's cache and is
//! never vendored (CC BY attribution travels with derived caches; see LICENSE-POLICY.md).

pub mod builtin;
pub mod palette;
pub mod shape;

use std::sync::OnceLock;

pub use builtin::{BuiltPart, BuiltinSpec, CatalogError, Lod, PartMesh, BUILTIN, CATALOG_ID};
pub use palette::{Colour, Palette};
pub use shape::{Aabb, Conn, ConnKind, PartShape};

/// A connector in the LDCad SNAP vocabulary.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Connector {
    /// studM, studF, tubeF, pinM, pinF, axleM, axleF, barM, clipF, hingeFinger, ball, socket
    pub kind: String,
    pub pos_ldu: [i32; 3],
    /// Unit direction as a lattice axis vector.
    pub dir: [i32; 3],
    /// Sections as (shape R|A|S, radius LDU, length LDU).
    pub sections: Vec<(char, f32, f32)>,
    #[serde(default)]
    pub group: Option<String>,
}

/// Compatibility class per docs/plan/04 section 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompatClass {
    A,
    B,
    C,
    D,
}

/// The LDraw library attribution line every client must show.
pub const LDRAW_ATTRIBUTION: &str =
    "This software uses the LDraw Parts Library (https://www.ldraw.org).";

/// Index of a part in a [`Catalog`].
pub type PartId = u32;

/// One catalog entry.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPart {
    pub id: String,
    pub name: String,
    pub body: builtin::Body,
    /// Footprint in studs along X then Z.
    pub studs: [u32; 2],
    pub height_ldu: i32,
    pub base: bool,
    pub shape: PartShape,
}

/// The parts the engine can place, by [`PartId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub parts: Vec<CatalogPart>,
}

impl Catalog {
    /// The built-in catalog, built once per process with the evaluator at
    /// [`Lod::Low`] (the boxes and connectors do not depend on the level of
    /// detail).
    pub fn builtin() -> Result<&'static Catalog, CatalogError> {
        static CATALOG: OnceLock<Result<Catalog, CatalogError>> = OnceLock::new();
        CATALOG
            .get_or_init(|| {
                let mut parts = Vec::with_capacity(BUILTIN.len());
                for spec in BUILTIN {
                    let built = builtin::build(spec, Lod::Low)?;
                    parts.push(CatalogPart::new(spec, &built));
                }
                Ok(Catalog { parts })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The part with catalog id `id`.
    pub fn find(&self, id: &str) -> Option<PartId> {
        self.parts
            .iter()
            .position(|p| p.id == id)
            .map(|i| i as PartId)
    }

    pub fn get(&self, part: PartId) -> Option<&CatalogPart> {
        self.parts.get(part as usize)
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

impl CatalogPart {
    /// The entry of a built-in part from its build.
    pub fn new(spec: &BuiltinSpec, built: &BuiltPart) -> CatalogPart {
        CatalogPart {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            body: spec.body,
            studs: spec.studs,
            height_ldu: built.height_ldu,
            base: spec.base,
            shape: built.shape.clone(),
        }
    }
}
