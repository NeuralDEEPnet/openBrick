//! Geometry kernel abstraction (decision D02 in docs/plan/03).
//!
//! The `Kernel` trait is implemented in M1 by `manifold-rust` (default) and,
//! behind a feature flag, by `manifold-csg`. Web and mobile may use `manifold-3d`
//! from the worker through the same interface. This crate holds the trait and the
//! plain mesh and polygon types so that no other crate depends on a kernel directly.

/// A closed polygon in millimetres (counter-clockwise outer, clockwise holes).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Polygon {
    pub points: Vec<[f64; 2]>,
}

/// A set of polygons forming a 2D region (fill rule: NonZero).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrossSection {
    pub polygons: Vec<Polygon>,
}

/// An indexed triangle mesh in millimetres.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Tessellation settings applied per export profile.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tessellation {
    /// Segments for stud and tube cylinders (fixed so stud-tube booleans stay stable).
    pub stud_segments: u32,
    pub min_edge_mm: f64,
}

impl Default for Tessellation {
    fn default() -> Self {
        Tessellation {
            stud_segments: 64,
            min_edge_mm: 0.05,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeomError {
    #[error("kernel reported a non-manifold result: {0}")]
    NonManifold(String),
    #[error("operation not supported by this kernel: {0}")]
    Unsupported(&'static str),
}

/// A solid handle owned by a kernel.
pub trait Solid: Send + Sync {
    fn to_mesh(&self) -> Mesh;
    fn volume_mm3(&self) -> f64;
    fn is_manifold(&self) -> bool;
}

/// The kernel interface every geometry backend implements.
pub trait Kernel: Send + Sync {
    type Solid: Solid;

    /// Stable identifier such as "manifold-rust 0.19.1", included in content hashes.
    fn id(&self) -> String;
    fn extrude(
        &self,
        section: &CrossSection,
        height_mm: f64,
        scale_top: f64,
    ) -> Result<Self::Solid, GeomError>;
    fn revolve(&self, profile: &Polygon, segments: u32) -> Result<Self::Solid, GeomError>;
    fn union(&self, a: &Self::Solid, b: &Self::Solid) -> Result<Self::Solid, GeomError>;
    fn difference(&self, a: &Self::Solid, b: &Self::Solid) -> Result<Self::Solid, GeomError>;
    fn intersection(&self, a: &Self::Solid, b: &Self::Solid) -> Result<Self::Solid, GeomError>;
    /// 2D offset (positive outset, negative inset) with round joins; used for fit
    /// compensation and the morphological opening of artwork.
    fn offset2d(&self, section: &CrossSection, delta_mm: f64) -> Result<CrossSection, GeomError>;
}
