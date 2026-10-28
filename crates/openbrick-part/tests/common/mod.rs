//! Helpers shared by the evaluator tests: documents, profiles, an independent
//! analytic volume model and a mesh cross-section measure.
#![allow(dead_code)]

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::Mesh;
use openbrick_part::{evaluate, Built, EvalError, EvalInput, MemoryAssets, ParamValues, PartDoc};
use openbrick_print::{default_process_profiles, FitProfile, Offsets, ProcessProfile};
use openbrick_units::STANDARD;

pub fn mm(id: &str) -> f64 {
    STANDARD.get().mm(id)
}

pub fn process(id: &str) -> ProcessProfile {
    default_process_profiles()
        .into_iter()
        .find(|p| p.id == id)
        .expect("bundled process profile")
}

/// A part document with one body, optional studs on top and an optional tube field.
pub fn part(
    body_op: &str,
    size: [u32; 2],
    height: Option<&str>,
    studs: bool,
    tubes: bool,
) -> PartDoc {
    let height = height
        .map(|h| format!(r#", "height": "{h}""#))
        .unwrap_or_default();
    let mut features = vec![format!(
        r#"{{ "id": "body", "op": "{body_op}", "size": [{}, {}]{height} }}"#,
        size[0], size[1]
    )];
    if studs {
        features.push(r#"{ "id": "studs", "op": "stud.field", "on": "body.top" }"#.into());
    }
    if tubes {
        features.push(
            r#"{ "id": "bottom", "op": "tube.field", "on": "body.bottom", "fitCritical": true }"#
                .into(),
        );
    }
    let text = format!(
        r#"{{ "openbrick": "1.0", "id": "test.part", "version": "0.1.0", "name": "test",
              "license": "CC0-1.0", "credit": {{ "author": "OpenBrick3D contributors" }},
              "features": [{}] }}"#,
        features.join(",")
    );
    PartDoc::parse(&text).expect("test document parses")
}

pub fn run(
    doc: &PartDoc,
    params: &ParamValues,
    process: &ProcessProfile,
    fit: Option<&FitProfile>,
    skip_unsupported: bool,
) -> Result<Built, EvalError> {
    let assets = MemoryAssets::new();
    evaluate(
        &ManifoldRust,
        &EvalInput {
            doc,
            params,
            process,
            fit,
            assets: &assets,
            skip_unsupported,
        },
    )
}

pub fn build(doc: &PartDoc, process: &ProcessProfile, fit: Option<&FitProfile>) -> Built {
    run(doc, &ParamValues::new(), process, fit, false).expect("part evaluates")
}

/// Area of a regular n-gon with circumradius 1: the cross-section of every
/// stud, tube and pin is such a polygon (vertices on the nominal circle).
pub fn ngon(n: u32) -> f64 {
    let n = f64::from(n);
    n / 2.0 * (std::f64::consts::TAU / n).sin()
}

/// Volume of a frustum of a regular n-gon pyramid between circumradii a and b
/// over height h, per unit of `ngon(n)`.
pub fn frustum(a: f64, b: f64, h: f64) -> f64 {
    h * (a * a + a * b + b * b) / 3.0
}

/// Volume removed from a w x d box by a 45 degree chamfer of size c along the
/// four edges of one face: the integral over u in [0, c] of wd - (w - 2u)(d - 2u).
pub fn edge_chamfer(w: f64, d: f64, c: f64) -> f64 {
    (w + d) * c * c - 4.0 / 3.0 * c * c * c
}

/// What a test part is, for the analytic model.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub size: [u32; 2],
    pub plates: u32,
    pub studs: bool,
    pub tubes: bool,
    /// Top-edge chamfer from `round` (0 for none).
    pub top_chamfer: f64,
}

/// The exact volume of the polyhedral part the evaluator should build,
/// derived from the rules of docs/plan/04 to 06 independently of the evaluator
/// code. Valid while the wall-to-roof chamfer clears tubes, pins and the ends of
/// ribs (true for the golden parts; each test says why).
pub fn analytic_volume(s: Shape, p: &ProcessProfile, o: &Offsets) -> f64 {
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let [nx, ny] = s.size;
    let (w, d) = (
        f64::from(nx) * pitch - 2.0 * gap,
        f64::from(ny) * pitch - 2.0 * gap,
    );
    let brick_plates = (openbrick_units::BRICK_LDU / openbrick_units::PLATE_LDU) as u32;
    let h = f64::from(s.plates / brick_plates) * mm("brick_height")
        + f64::from(s.plates % brick_plates) * mm("plate_height");
    let t = p.quantise_wall(mm("side_wall"));
    let roof = mm("roof").max(p.min_roof_mm);
    let cd = h - roof;
    let c = p.first_layer_chamfer_mm.max(o.first_layer_chamfer);
    let f = p.wall_roof_fillet(s.plates >= brick_plates);
    let (wi, di) = (w - 2.0 * t, d - 2.0 * t);

    let outer = w * d * h - edge_chamfer(w, d, c) - edge_chamfer(w, d, s.top_chamfer);
    // Cavity: rim widened by c at the bed, straight walls, then the wall-to-roof
    // chamfer narrowing it by f at the roof.
    let rim = wi * di * c + (wi + di) * c * c + 4.0 / 3.0 * c * c * c;
    let straight = wi * di * (cd - f - c);
    let top = wi * di * f - edge_chamfer(wi, di, f);
    let mut v = outer - (rim + straight + top);

    let k = ngon(p.stud_segments());
    if s.studs {
        let r = (mm("stud_diameter") + o.stud_diameter) / 2.0;
        let (sh, ch) = (mm("stud_height"), p.stud_top_chamfer_mm);
        let stud = k * (r * r * (sh - ch) + frustum(r, r - ch, ch));
        v += f64::from(nx * ny) * stud;
    }
    if s.tubes {
        if nx >= 2 && ny >= 2 {
            let t = o.tube(
                mm("tube_outer_diameter"),
                mm("tube_inner_diameter"),
                p.min_tube_wall_mm,
            );
            let (ro, ri) = (t.outer_mm / 2.0, t.inner_mm / 2.0);
            let ct = c.min(t.wall_mm() / 3.0);
            let tube = k
                * (frustum(ro - ct, ro, ct) + ro * ro * (cd - ct)
                    - frustum(ri + ct, ri, ct)
                    - ri * ri * (cd - ct));
            v += f64::from((nx - 1) * (ny - 1)) * tube;
        } else {
            let dp = mm("pin_1wide_diameter") + o.pin_diameter;
            let (rp, cp) = (dp / 2.0, c.min(dp / 4.0));
            let pin = k * (frustum(rp - cp, rp, cp) + rp * rp * (cd - cp));
            v += f64::from(nx.max(ny) - 1) * pin;
        }
        let rib_d = mm("side_wall") + mm("rib_depth") - t + o.rib_depth;
        let rib_w = p.min_feature_mm;
        let m = rib_d.min(f);
        // Each rib runs from the first-layer chamfer to the roof; the part of it
        // inside the wall-to-roof chamfer wedge (height f - s at distance s from
        // the wall) is already solid.
        let rib = rib_w * rib_d * (cd - c) - rib_w * (f * m - m * m / 2.0);
        v += f64::from(2 * (nx + ny)) * rib;
    }
    v
}

/// Area of the cross-section of a closed, outward-oriented mesh by the plane
/// z = `z`, counting only the boundary segments whose midpoint `keep` accepts.
/// Outer boundaries count positive and holes negative (Green's theorem on the
/// oriented segments), so `keep` must select whole loops. Choose `z` away from
/// vertex heights.
pub fn section_area(mesh: &Mesh, z: f64, keep: impl Fn([f64; 2]) -> bool) -> f64 {
    let mut area = 0.0;
    for tri in mesh.indices.as_chunks::<3>().0 {
        let p = tri.map(|i| mesh.positions[i as usize].map(f64::from));
        let mut cut = Vec::new();
        for e in 0..3 {
            let (a, b) = (p[e], p[(e + 1) % 3]);
            if (a[2] - z) * (b[2] - z) < 0.0 {
                let t = (z - a[2]) / (b[2] - a[2]);
                cut.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
        }
        if cut.len() != 2 {
            continue;
        }
        let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2]];
        let (mut s, mut e) = (cut[0], cut[1]);
        // The outward normal must lie to the right of the travel direction.
        if (e[0] - s[0]) * -n[1] + (e[1] - s[1]) * n[0] < 0.0 {
            std::mem::swap(&mut s, &mut e);
        }
        if keep([(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0]) {
            area += (s[0] * e[1] - e[0] * s[1]) / 2.0;
        }
    }
    area
}

/// The circumradius of the regular n-gon loops around `centre` at height `z`:
/// returns (outer, inner) from the loops farther and nearer than `split` mm.
pub fn ring_radii(
    mesh: &Mesh,
    z: f64,
    centre: [f64; 2],
    split: f64,
    reach: f64,
    n: u32,
) -> (f64, f64) {
    let dist = |q: [f64; 2]| (q[0] - centre[0]).hypot(q[1] - centre[1]);
    let outer = section_area(mesh, z, |q| dist(q) >= split && dist(q) < reach);
    let inner = -section_area(mesh, z, |q| dist(q) < split);
    ((outer / ngon(n)).sqrt(), (inner.max(0.0) / ngon(n)).sqrt())
}
