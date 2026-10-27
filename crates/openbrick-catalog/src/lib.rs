//! Part catalog (docs/plan/07 section 3): packed LDraw subset, own type-1 parser,
//! connector derivation from primitives, LOD baking. Lands in M3a.
//!
//! The LDraw Parts Library is downloaded at first run into the user's cache and is
//! never vendored (CC BY attribution travels with derived caches; see LICENSE-POLICY.md).

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
