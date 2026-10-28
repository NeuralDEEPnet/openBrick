//! `Kernel` backend on `manifold-csg` 0.4.1 (Apache-2.0 OR MIT): safe bindings over
//! the C API of the C++ Manifold library, which `manifold-csg-sys` 3.5.105 clones
//! (tag v3.5.3) and compiles with cmake at build time. This is the CLI and desktop
//! hedge in decision D02. On the conformance corpus its volumes and topology equal
//! those of the `manifold_rust` backend exactly, but its meshes can differ (the 2x4
//! brick does), and the kernel id differs, so content hashes from the two backends
//! must never be mixed in one registry (docs/spikes/q01b-manifold-csg-comparison.md).
//!
//! The crate is built with `default-features = false`, which turns off its
//! `parallel` feature. That feature makes cmake fetch and build oneTBB as a second
//! upstream download and runs booleans on a thread pool; without it the backend is
//! single-threaded like `manifold-rust`, so the two can be compared like for like.

use manifold_csg::{CrossSection as CsgCrossSection, JoinType, Manifold, OpType};
// A direct dependency only to pin the exact version that `UPSTREAM` describes.
use manifold_csg_sys as _;

use crate::{CrossSection, GeomError, Kernel, Mesh, Polygon, Solid};

/// Upstream Manifold tag that `manifold-csg-sys` 3.5.105 pins (its `MANIFOLD_VERSION`).
const UPSTREAM: &str = "v3.5.3";

/// The C++ Manifold kernel through `manifold-csg`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ManifoldCsg;

/// A solid owned by [`ManifoldCsg`].
///
/// `manifold_csg::Manifold` is `Send + Sync` (the C++ object synchronises its lazy
/// CSG tree evaluation), so the trait bounds hold without relaxing them.
#[derive(Clone)]
pub struct CsgSolid(pub Manifold);

impl Solid for CsgSolid {
    fn to_mesh(&self) -> Mesh {
        // The f32 MeshGL, as in the manifold-rust backend, so fingerprints compare
        // the same representation.
        let (props, num_prop, indices) = self.0.to_mesh_f32();
        let stride = num_prop.max(3);
        let positions = props
            .chunks_exact(stride)
            .map(|v| [v[0], v[1], v[2]])
            .collect();
        Mesh { positions, indices }
    }

    fn volume_mm3(&self) -> f64 {
        self.0.volume()
    }

    fn is_manifold(&self) -> bool {
        !self.0.is_empty() && self.0.status().is_ok()
    }

    fn genus(&self) -> i32 {
        self.0.genus()
    }

    fn component_count(&self) -> usize {
        self.0.decompose().len()
    }
}

fn to_csg(section: &CrossSection) -> CsgCrossSection {
    let polys: Vec<Vec<[f64; 2]>> = section.polygons.iter().map(|p| p.points.clone()).collect();
    // Positive fill rule (the upstream default), which unions overlapping contours
    // and resolves self-intersections exactly as the manifold-rust backend does.
    CsgCrossSection::from_polygons(&polys)
}

fn from_csg(cs: &CsgCrossSection) -> CrossSection {
    CrossSection {
        polygons: cs
            .to_polygons()
            .into_iter()
            .map(|points| Polygon { points })
            .collect(),
    }
}

fn checked(m: Manifold, op: &'static str) -> Result<CsgSolid, GeomError> {
    match m.status() {
        Ok(()) => Ok(CsgSolid(m)),
        Err(e) => Err(GeomError::NonManifold(format!("{op}: {e}"))),
    }
}

impl Kernel for ManifoldCsg {
    type Solid = CsgSolid;

    fn id(&self) -> String {
        // manifold-csg-sys skips its pinned clone and links whatever library this
        // directory holds when MANIFOLD_CSG_LIB_DIR is set at build time, so the
        // id cannot claim the pinned upstream version in that case.
        match option_env!("MANIFOLD_CSG_LIB_DIR") {
            None => format!("manifold-csg 0.4.1 (Manifold {UPSTREAM})"),
            Some(dir) => {
                format!("manifold-csg 0.4.1 (Manifold from MANIFOLD_CSG_LIB_DIR={dir}, unverified)")
            }
        }
    }

    fn extrude(
        &self,
        section: &CrossSection,
        height_mm: f64,
        scale_top: f64,
    ) -> Result<CsgSolid, GeomError> {
        checked(
            Manifold::extrude_with_options(
                &to_csg(section),
                height_mm,
                0,
                0.0,
                scale_top,
                scale_top,
            ),
            "extrude",
        )
    }

    fn revolve(&self, profile: &Polygon, segments: u32) -> Result<CsgSolid, GeomError> {
        let section = to_csg(&CrossSection {
            polygons: vec![profile.clone()],
        });
        let segments = i32::try_from(segments)
            .map_err(|_| GeomError::Unsupported("revolve: more than i32::MAX segments"))?;
        checked(Manifold::revolve(&section, segments, 360.0), "revolve")
    }

    fn union(&self, a: &CsgSolid, b: &CsgSolid) -> Result<CsgSolid, GeomError> {
        checked(a.0.union(&b.0), "union")
    }

    fn difference(&self, a: &CsgSolid, b: &CsgSolid) -> Result<CsgSolid, GeomError> {
        checked(a.0.difference(&b.0), "difference")
    }

    fn intersection(&self, a: &CsgSolid, b: &CsgSolid) -> Result<CsgSolid, GeomError> {
        checked(a.0.intersection(&b.0), "intersection")
    }

    fn union_all(&self, solids: &[CsgSolid]) -> Result<CsgSolid, GeomError> {
        let ms: Vec<Manifold> = solids.iter().map(|s| s.0.clone()).collect();
        checked(Manifold::batch_boolean(&ms, OpType::Add), "union_all")
    }

    fn translate(&self, solid: &CsgSolid, v_mm: [f64; 3]) -> CsgSolid {
        CsgSolid(solid.0.translate(v_mm[0], v_mm[1], v_mm[2]))
    }

    fn offset2d(&self, section: &CrossSection, delta_mm: f64) -> Result<CrossSection, GeomError> {
        // Same parameters as the manifold-rust backend: round joins, miter limit 2,
        // and 0 circular segments, which means the global default quality.
        let cs = to_csg(section).offset(delta_mm, JoinType::Round, 2.0, 0);
        Ok(from_csg(&cs))
    }
}
