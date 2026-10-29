//! Geometry kernel abstraction (decision D02 in docs/plan/03).
//!
//! The `Kernel` trait is implemented by `manifold-rust` (default, feature
//! `manifold-rust`, module `manifold_rust`) and by `manifold-csg`, the C++ Manifold
//! library through FFI, for CLI and desktop builds (opt-in feature `manifold-csg`,
//! module `manifold_csg`). A `manifold-3d` WASM backend for web and mobile follows
//! behind the same interface. This crate holds the trait and the plain mesh and
//! polygon types so that no other crate depends on a kernel directly. Module
//! `nominal` builds nominal bricks from the Dimensional Standard over any kernel,
//! for the render harness and the browser pipeline until the part evaluator exists.
//! Module `region` resolves fill rules and booleans of plain 2D regions (artwork)
//! through Clipper2, the same way for every backend.
//!
//! Backends agree on volume and topology but may tessellate differently, so a mesh
//! fingerprint is only meaningful together with its kernel id: never mix content
//! hashes from two backends in one registry
//! (docs/spikes/q01b-manifold-csg-comparison.md).

#[cfg(feature = "manifold-rust")]
pub mod manifold_rust;

#[cfg(feature = "manifold-csg")]
pub mod manifold_csg;

pub mod nominal;
pub mod region;

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
            .as_chunks::<3>()
            .0
            .iter()
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

/// Turn meshes over together, face down: a half turn about the x axis inside
/// the box they share, (x, y, z) to (x, y0 + y1 - y, z0 + z1 - z), so the top
/// face lands on the bed plane and the box stays where it was. A proper rotation
/// (determinant +1), not a mirror: triangle winding, closedness and indices are
/// unchanged, artwork reads the right way round once the print is turned back
/// over, and meshes that touched still touch.
///
/// This is the one layout for parts whose artwork prints face down (an inlay,
/// `Artwork::face_on_bed` = `top` in openbrick-part): every export of such a
/// part is meant to call it, so the CLI and the browser write the same bytes
/// (openbrick-wasm's `export_part` does). The sums of
/// the bounds are formed in f64, where the sum of two f32 values is exact, so a
/// vertex on a bounding plane lands exactly on the opposite one. Empty meshes
/// are left as they are.
pub fn turn_face_down(meshes: &mut [&mut Mesh]) {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for m in meshes.iter() {
        let (a, b) = m.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    if !(lo[1].is_finite() && lo[2].is_finite() && hi[1].is_finite() && hi[2].is_finite()) {
        return;
    }
    let sy = f64::from(lo[1]) + f64::from(hi[1]);
    let sz = f64::from(lo[2]) + f64::from(hi[2]);
    for m in meshes.iter_mut() {
        for p in &mut m.positions {
            p[1] = (sy - f64::from(p[1])) as f32;
            p[2] = (sz - f64::from(p[2])) as f32;
        }
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
    #[error("invalid input: {0}")]
    InvalidInput(String),
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
    /// 2D offset (positive outset, negative inset) with round joins drawn at the
    /// backend's default circle quality, which is coarse at small radii (on
    /// manifold-rust 0.19.1 a quarter turn of radius 0.4 mm is one chord); use
    /// [`Kernel::offset2d_round`] where the joins must follow the arc.
    fn offset2d(&self, section: &CrossSection, delta_mm: f64) -> Result<CrossSection, GeomError> {
        self.offset2d_round(section, delta_mm, 0)
    }
    /// 2D offset with round joins drawn as `circle_segments` chords per full turn
    /// (0: the backend's default quality); [`round_segments`] gives the count for
    /// a chord tolerance. Used for the morphological opening of artwork.
    fn offset2d_round(
        &self,
        section: &CrossSection,
        delta_mm: f64,
        circle_segments: u32,
    ) -> Result<CrossSection, GeomError>;
}

/// Chords per full turn so that no point of a circle of `radius_mm` lies farther
/// than `tolerance_mm` from its chords: the sagitta r (1 - cos(pi / n)) is at most
/// the tolerance when n >= pi / acos(1 - t / r). Rounded up to a multiple of 4,
/// so a quarter turn is whole chords (as Manifold's own circle quality does), at
/// least 4 and at most 4096. Radius 0.4 mm at 0.05 mm gives 8.
pub fn round_segments(radius_mm: f64, tolerance_mm: f64) -> u32 {
    let r = radius_mm.abs();
    if !(tolerance_mm > 0.0 && r > tolerance_mm) {
        return 4;
    }
    let n = (std::f64::consts::PI / (1.0 - tolerance_mm / r).acos()).ceil();
    if !n.is_finite() {
        return 4096;
    }
    // In range, so the cast is exact.
    let n = n.clamp(4.0, 4096.0) as u32;
    n.div_ceil(4) * 4
}

#[cfg(test)]
mod tests {
    use super::{round_segments, turn_face_down, Mesh};

    /// A tetrahedron with outward winding (abstract numbers, not a brick
    /// dimension): legs 1, 2 and 3 along x, y and z.
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
        let mut m = t.clone();
        turn_face_down(&mut [&mut m]);
        assert_eq!(m.indices, t.indices);
        // (x, y, z) -> (x, 2 - y, 3 - z): the apex at z = 3 now touches the bed.
        assert_eq!(m.positions[3], [0.0, 2.0, 0.0]);
        assert_eq!(m.positions[0], [0.0, 2.0, 3.0]);
        assert_eq!(m.bounds(), t.bounds(), "the box is unchanged");
        // A proper rotation keeps the volume positive (winding still outward).
        assert!((signed_volume(&m) - 1.0).abs() < 1e-12);
        // Twice is the identity.
        turn_face_down(&mut [&mut m]);
        assert_eq!(m, t);
    }

    #[test]
    fn meshes_turn_together_in_the_box_they_share() {
        // A second tetrahedron sitting on top of the first (z 3 to 6): turned
        // together, it lands on the bed and the first goes on top of it, still
        // touching at z = 3.
        let low = tetra();
        let mut high = tetra();
        for p in &mut high.positions {
            p[2] += 3.0;
        }
        let (mut a, mut b) = (low.clone(), high.clone());
        turn_face_down(&mut [&mut a, &mut b]);
        assert_eq!(a.bounds().0[2], 3.0);
        assert_eq!(a.bounds().1[2], 6.0);
        assert_eq!(b.bounds().0[2], 0.0);
        assert_eq!(b.bounds().1[2], 3.0);
        // Alone, each would have turned inside its own box instead.
        let mut alone = high.clone();
        turn_face_down(&mut [&mut alone]);
        assert_eq!(alone.bounds(), high.bounds());
        assert_ne!(alone, b);
        // Nothing to turn: unchanged, no NaN.
        let mut empty = Mesh::default();
        turn_face_down(&mut [&mut empty]);
        assert_eq!(empty, Mesh::default());
        turn_face_down(&mut []);
    }

    #[test]
    fn round_segments_meet_the_tolerance() {
        // Unit-free radii and tolerances (no brick dimension is involved).
        for (r, t) in [(0.4, 0.05), (1.0, 0.05), (6.6, 0.05), (10.0, 0.001)] {
            let n = round_segments(r, t);
            assert_eq!(n % 4, 0);
            let sagitta = r * (1.0 - (std::f64::consts::PI / f64::from(n)).cos());
            assert!(
                sagitta <= t * (1.0 + 1e-12),
                "r {r}: {n} chords, sagitta {sagitta}"
            );
            // The next multiple of 4 down would not do (unless at the minimum).
            if n > 4 {
                let fewer = r * (1.0 - (std::f64::consts::PI / f64::from(n - 4)).cos());
                assert!(fewer > t, "r {r}: {} chords would do", n - 4);
            }
        }
        assert_eq!(round_segments(0.4, 0.05), 8);
        assert_eq!(round_segments(0.03, 0.05), 4, "smaller than the tolerance");
        assert_eq!(round_segments(0.4, 0.0), 4, "no tolerance: the minimum");
    }

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
