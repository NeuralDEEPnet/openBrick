//! `Kernel` backend on `manifold-rust` 0.19.x, a pure-Rust port of Manifold 3.5.0
//! (Apache-2.0). Pure Rust means the same backend builds for native, wasm32 and
//! mobile targets without a C++ toolchain (decision D02).

use manifold_rust::cross_section::CrossSection as MrCrossSection;
use manifold_rust::linalg::{Vec2, Vec3};
use manifold_rust::manifold::Manifold;
use manifold_rust::types::{Error as MrError, OpType, Polygons};

use crate::{CrossSection, GeomError, Kernel, Mesh, Polygon, Solid};

/// Clipper2 join type code for round joins in `offset_with_params`.
const JOIN_ROUND: i32 = 1;

/// The pure-Rust Manifold kernel.
#[derive(Clone, Copy, Debug, Default)]
pub struct ManifoldRust;

/// A solid owned by [`ManifoldRust`].
#[derive(Clone)]
pub struct MrSolid(pub Manifold);

impl Solid for MrSolid {
    fn to_mesh(&self) -> Mesh {
        let gl = self.0.get_mesh_gl(-1);
        let stride = (gl.num_prop as usize).max(3);
        let positions = gl
            .vert_properties
            .chunks_exact(stride)
            .map(|v| [v[0], v[1], v[2]])
            .collect();
        Mesh {
            positions,
            indices: gl.tri_verts.clone(),
        }
    }

    fn volume_mm3(&self) -> f64 {
        self.0.volume()
    }

    fn is_manifold(&self) -> bool {
        !self.0.is_empty() && self.0.status() == MrError::NoError
    }

    fn genus(&self) -> i32 {
        self.0.genus()
    }

    fn component_count(&self) -> usize {
        self.0.decompose().len()
    }
}

fn to_mr(section: &CrossSection) -> Polygons {
    section
        .polygons
        .iter()
        .map(|p| p.points.iter().map(|[x, y]| Vec2::new(*x, *y)).collect())
        .collect()
}

fn from_mr(polys: Polygons) -> CrossSection {
    CrossSection {
        polygons: polys
            .into_iter()
            .map(|p| Polygon {
                points: p.into_iter().map(|v| [v.x, v.y]).collect(),
            })
            .collect(),
    }
}

/// Normalise contours through Clipper2 (union under the Positive fill rule), which
/// resolves overlaps and self-intersections before extrusion.
fn normalised(section: &CrossSection) -> Polygons {
    MrCrossSection::new(to_mr(section)).to_polygons()
}

fn checked(m: Manifold, op: &'static str) -> Result<MrSolid, GeomError> {
    match m.status() {
        MrError::NoError => Ok(MrSolid(m)),
        e => Err(GeomError::NonManifold(format!("{op}: {e:?}"))),
    }
}

impl Kernel for ManifoldRust {
    type Solid = MrSolid;

    fn id(&self) -> String {
        "manifold-rust 0.19".to_string()
    }

    fn extrude(
        &self,
        section: &CrossSection,
        height_mm: f64,
        scale_top: f64,
    ) -> Result<MrSolid, GeomError> {
        let polys = normalised(section);
        checked(
            Manifold::extrude(&polys, height_mm, 0, 0.0, Vec2::new(scale_top, scale_top)),
            "extrude",
        )
    }

    fn revolve(&self, profile: &Polygon, segments: u32) -> Result<MrSolid, GeomError> {
        let polys = normalised(&CrossSection {
            polygons: vec![profile.clone()],
        });
        checked(Manifold::revolve(&polys, segments as i32, 360.0), "revolve")
    }

    fn union(&self, a: &MrSolid, b: &MrSolid) -> Result<MrSolid, GeomError> {
        checked(a.0.union(&b.0), "union")
    }

    fn difference(&self, a: &MrSolid, b: &MrSolid) -> Result<MrSolid, GeomError> {
        checked(a.0.difference(&b.0), "difference")
    }

    fn intersection(&self, a: &MrSolid, b: &MrSolid) -> Result<MrSolid, GeomError> {
        checked(a.0.intersection(&b.0), "intersection")
    }

    fn union_all(&self, solids: &[MrSolid]) -> Result<MrSolid, GeomError> {
        let ms: Vec<Manifold> = solids.iter().map(|s| s.0.clone()).collect();
        checked(Manifold::batch_boolean(&ms, OpType::Add), "union_all")
    }

    fn translate(&self, solid: &MrSolid, v_mm: [f64; 3]) -> MrSolid {
        MrSolid(solid.0.translate(Vec3::new(v_mm[0], v_mm[1], v_mm[2])))
    }

    fn offset2d(&self, section: &CrossSection, delta_mm: f64) -> Result<CrossSection, GeomError> {
        let cs =
            MrCrossSection::new(to_mr(section)).offset_with_params(delta_mm, JOIN_ROUND, 2.0, 0);
        Ok(from_mr(cs.to_polygons()))
    }
}
