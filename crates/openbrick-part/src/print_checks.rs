//! The print checks of the final meshes (docs/plan/05 section 5): the
//! measurements of rules `MESH.SELF_INTERSECT`, `ROOF.BRIDGE`, `OVERHANG.MAX`
//! and `OVERHANG.BRIDGED`, taken on the meshes the exporters write, in print
//! orientation. The evaluator and the coupon builder fill them the same way.

use openbrick_geom::{downward_regions, self_intersections, turn_face_down, Mesh};
use openbrick_print::{Overhang, ProcessProfile};

/// A downward region lies in the plane of the cavity ceiling when its lowest
/// and highest corner are this close to it, in millimetres: far below a
/// layer, far above f32 rounding of a ceiling height.
const CEILING_TOLERANCE_MM: f64 = 1e-4;

/// What the print checks measured.
pub(crate) struct PrintChecks {
    /// Pairs of triangles that pass through each other, over every mesh
    /// tested; `None` when a mesh lay outside the test's range.
    pub self_intersections: Option<usize>,
    /// The roof's longest bridge, when the roof hangs down.
    pub roof_span_mm: Option<f64>,
    /// The other steep downward regions.
    pub overhangs: Vec<Overhang>,
}

/// Measure the print checks. `exported` are the meshes the exporters write
/// (each object, and the fused single-colour mesh when there is one);
/// `print_mesh` is the single-colour solid in the design frame, turned face
/// down here when `face_down`; `ceiling_z` is the height of the cavity ceiling
/// in the design frame, if the part has one. Faces no higher than the
/// first-layer chamfer `on_bed_mm` above the bed rest on the first layer
/// (`openbrick_geom::overhang`).
pub(crate) fn measure(
    exported: &[&Mesh],
    print_mesh: &Mesh,
    face_down: bool,
    ceiling_z: Option<f64>,
    process: &ProcessProfile,
    on_bed_mm: f64,
) -> PrintChecks {
    let mut pairs = Some(0usize);
    for m in exported {
        let r = self_intersections(m);
        pairs = match (pairs, r.out_of_range) {
            (Some(n), false) => Some(n + r.pairs),
            _ => None,
        };
    }
    let turned;
    let mesh = if face_down {
        let mut copy = print_mesh.clone();
        turn_face_down(&mut [&mut copy]);
        turned = copy;
        &turned
    } else {
        print_mesh
    };
    let bed = f64::from(mesh.bounds().0[2]);
    let regions = downward_regions(mesh, process.max_overhang_deg, on_bed_mm);
    // Face down, the ceiling faces up: there is no hanging roof.
    let ceiling = ceiling_z.filter(|_| !face_down);
    let on_ceiling = |z: f64| ceiling.is_some_and(|c| (z - c).abs() <= CEILING_TOLERANCE_MM);
    let mut roof_span: Option<f64> = None;
    let mut overhangs = Vec::new();
    for r in regions {
        match r.span_mm {
            Some(span) if on_ceiling(r.z_min_mm) && on_ceiling(r.z_max_mm) => {
                roof_span = Some(roof_span.map_or(span, |s| s.max(span)));
            }
            span => overhangs.push(Overhang {
                angle_deg: r.max_angle_deg,
                span_mm: span,
                height_mm: r.z_min_mm - bed,
                area_mm2: r.area_mm2,
                centre_mm: r.centre_mm,
            }),
        }
    }
    PrintChecks {
        self_intersections: pairs,
        roof_span_mm: roof_span,
        overhangs,
    }
}
