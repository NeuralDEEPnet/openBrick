//! Geometry kernel abstraction (decision D02 in docs/plan/03).
//!
//! The `Kernel` trait is implemented by `manifold-rust` (default, feature
//! `manifold-rust`, module `manifold_rust`) and by `manifold-csg`, the C++ Manifold
//! library through FFI, for CLI and desktop builds (opt-in feature `manifold-csg`,
//! module `manifold_csg`). A `manifold-3d` WASM backend for web and mobile follows
//! behind the same interface. This crate holds the trait and the plain mesh and
//! polygon types so that no other crate depends on a kernel directly.
//!
//! Backends agree on volume and topology but may tessellate differently, so a mesh
//! fingerprint is only meaningful together with its kernel id: never mix content
//! hashes from two backends in one registry
//! (docs/spikes/q01b-manifold-csg-comparison.md).

#[cfg(feature = "manifold-rust")]
pub mod manifold_rust;

#[cfg(feature = "manifold-csg")]
pub mod manifold_csg;

/// A closed polygon in millimetres (counter-clockwise outer, clockwise holes).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Polygon {
    pub points: Vec<[f64; 2]>,
}

impl Polygon {
    /// Axis-aligned rectangle, counter-clockwise, with its minimum corner at `min`.
    pub fn rect(min: [f64; 2], size: [f64; 2]) -> Polygon {
        let [x, y] = min;
        let [w, h] = size;
        Polygon {
            points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
        }
    }

    /// Regular polygon approximating a circle, counter-clockwise, vertices on the circle.
    pub fn circle(center: [f64; 2], radius: f64, segments: u32) -> Polygon {
        let n = segments.max(3);
        let points = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
            })
            .collect();
        Polygon { points }
    }

    /// The same polygon with reversed winding (a hole under the Positive fill rule).
    pub fn reversed(mut self) -> Polygon {
        self.points.reverse();
        self
    }

    /// Signed area (positive for counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        let n = self.points.len();
        (0..n)
            .map(|i| {
                let [x0, y0] = self.points[i];
                let [x1, y1] = self.points[(i + 1) % n];
                x0 * y1 - x1 * y0
            })
            .sum::<f64>()
            / 2.0
    }
}

/// A set of polygons forming a 2D region (fill rule: NonZero).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrossSection {
    pub polygons: Vec<Polygon>,
}

impl CrossSection {
    pub fn from(polygons: Vec<Polygon>) -> CrossSection {
        CrossSection { polygons }
    }

    /// Net area under the Positive fill rule (holes are clockwise and subtract).
    pub fn area(&self) -> f64 {
        self.polygons.iter().map(Polygon::signed_area).sum()
    }
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

    /// Axis-aligned bounding box as (min, max).
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in &self.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    /// FNV-1a hash of positions and indices, used to check determinism and as a
    /// golden-mesh fingerprint. Combine with the kernel id for content hashes.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        for p in &self.positions {
            for c in p {
                eat(&c.to_le_bytes());
            }
        }
        for i in &self.indices {
            eat(&i.to_le_bytes());
        }
        h
    }

    /// FNV-1a hash of the triangles as position triples, independent of vertex
    /// numbering and triangle order. Each triangle is rotated to start at its
    /// smallest corner (keeping its winding, so a flipped triangle hashes
    /// differently) and the list is sorted.
    ///
    /// [`Mesh::fingerprint`] changes when the same solid is built from inputs created
    /// in a different order, because the kernel numbers vertices and groups
    /// triangles by input; this one does not. It still differs between kernels that
    /// triangulate a face differently (docs/spikes/q01b-manifold-csg-comparison.md).
    pub fn canonical_fingerprint(&self) -> u64 {
        // -0.0 and 0.0 are the same point.
        let key = |i: u32| self.positions[i as usize].map(|c| (c + 0.0).to_bits());
        let mut tris: Vec<[[u32; 3]; 3]> = self
            .indices
            .chunks_exact(3)
            .map(|t| {
                let c = [key(t[0]), key(t[1]), key(t[2])];
                // The smallest of the three rotations, so a tie between two corners
                // with the same position cannot depend on the starting corner.
                (0..3)
                    .map(|r| [c[r], c[(r + 1) % 3], c[(r + 2) % 3]])
                    .min()
                    .unwrap_or(c)
            })
            .collect();
        tris.sort_unstable();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for word in tris.iter().flatten().flatten() {
            for b in word.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
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
    /// True when the solid is non-empty, watertight and the kernel reports no error.
    fn is_manifold(&self) -> bool;
    /// Topological genus (0 for a ball, 1 for a ring).
    fn genus(&self) -> i32;
    /// Number of disconnected components.
    fn component_count(&self) -> usize;
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
    /// Union of many solids in one batch (much faster than pairwise for stud fields).
    fn union_all(&self, solids: &[Self::Solid]) -> Result<Self::Solid, GeomError>;
    fn translate(&self, solid: &Self::Solid, v_mm: [f64; 3]) -> Self::Solid;
    /// 2D offset (positive outset, negative inset) with round joins; used for fit
    /// compensation and the morphological opening of artwork.
    fn offset2d(&self, section: &CrossSection, delta_mm: f64) -> Result<CrossSection, GeomError>;
}

#[cfg(test)]
mod tests {
    use super::Mesh;

    /// A unit square as two triangles (an abstract mesh, not a brick dimension).
    fn square() -> Mesh {
        Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    #[test]
    fn canonical_fingerprint_ignores_numbering_and_order_but_not_winding() {
        let a = square();
        // The same two triangles with the vertices renumbered, the triangles
        // swapped and each triangle starting at a different corner.
        let b = Mesh {
            positions: vec![
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
            ],
            indices: vec![1, 2, 0, 3, 0, 2],
        };
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.canonical_fingerprint(), b.canonical_fingerprint());

        let flipped = Mesh {
            indices: vec![0, 2, 1, 0, 2, 3],
            ..square()
        };
        assert_ne!(a.canonical_fingerprint(), flipped.canonical_fingerprint());

        let mut negative_zero = square();
        negative_zero.positions[0] = [-0.0, -0.0, -0.0];
        assert_eq!(
            a.canonical_fingerprint(),
            negative_zero.canonical_fingerprint()
        );
    }

    #[test]
    fn canonical_fingerprint_handles_triangles_with_tied_corners() {
        // Two distinct vertices can share a position after f32 narrowing. The same
        // cyclic triangle must hash the same whichever corner the kernel starts at.
        let positions = vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let a = Mesh {
            positions: positions.clone(),
            indices: vec![0, 2, 1, 0, 2, 3],
        };
        let b = Mesh {
            positions,
            indices: vec![1, 0, 2, 0, 2, 3],
        };
        assert_eq!(a.canonical_fingerprint(), b.canonical_fingerprint());
    }
}
