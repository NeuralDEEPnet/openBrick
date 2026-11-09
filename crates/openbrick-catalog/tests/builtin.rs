//! The built-in catalog: boxes and connectors derived from the evaluator, the
//! mesh in the part's LDraw frame, and golden meshes (CLAUDE.md: geometry
//! changes need golden-mesh tests).

use openbrick_catalog::builtin::{self, Body};
use openbrick_catalog::{Catalog, ConnKind, Lod, BUILTIN};
use openbrick_geom::Mesh;
use openbrick_units::{BRICK_LDU, HALF_STUD_LDU, PLATE_LDU, STANDARD, STUD_PITCH_LDU};

fn std_ldu(id: &str) -> f64 {
    STANDARD
        .get()
        .feature(id)
        .and_then(|f| f.ldraw_ldu)
        .unwrap()
}

#[test]
fn boxes_and_connectors_follow_the_features() {
    let catalog = Catalog::builtin().expect("builds");
    assert_eq!(catalog.len(), BUILTIN.len());
    let stud_height = std_ldu("stud_height").round() as i32;
    for (spec, part) in BUILTIN.iter().zip(&catalog.parts) {
        let [nx, nz] = spec.studs.map(|n| n as i32);
        let height = match spec.body {
            Body::Brick => BRICK_LDU,
            Body::Plate | Body::Tile => PLATE_LDU,
        };
        assert_eq!(part.height_ldu, height, "{}", spec.id);
        let s = &part.shape;
        // The body box is the lattice cell: n studs centred on the origin,
        // from the stud bases (Y = 0) down to the bottom.
        assert_eq!(
            s.body.min,
            [-HALF_STUD_LDU * nx, 0, -HALF_STUD_LDU * nz],
            "{}",
            spec.id
        );
        assert_eq!(
            s.body.max,
            [HALF_STUD_LDU * nx, height, HALF_STUD_LDU * nz],
            "{}",
            spec.id
        );
        let top = if spec.body == Body::Tile {
            0
        } else {
            -stud_height
        };
        assert_eq!(s.pick.min[1], top, "{}: studs are in the pick box", spec.id);
        assert_eq!((s.pick.min[0], s.pick.max), (s.body.min[0], s.body.max));

        let studs: Vec<_> = s
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::StudM)
            .collect();
        let tubes: Vec<_> = s
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::TubeF)
            .collect();
        let n = (nx * nz) as usize;
        assert_eq!(
            studs.len(),
            if spec.body == Body::Tile { 0 } else { n },
            "{}",
            spec.id
        );
        assert_eq!(tubes.len(), n, "{}", spec.id);
        for c in &studs {
            assert_eq!((c.pos[1], c.dir), (0, [0, -1, 0]));
        }
        for c in &tubes {
            assert_eq!((c.pos[1], c.dir), (height, [0, 1, 0]));
        }
        // On the stud grid of the part: x = 20 i + 10 - 10 nx.
        for c in &s.connectors {
            for (v, n) in [(c.pos[0], nx), (c.pos[2], nz)] {
                assert_eq!(
                    (v - HALF_STUD_LDU + HALF_STUD_LDU * n).rem_euclid(STUD_PITCH_LDU),
                    0,
                    "{}: {c:?}",
                    spec.id
                );
            }
        }
    }
}

#[test]
fn shapes_do_not_depend_on_the_level_of_detail() {
    let catalog = Catalog::builtin().expect("builds");
    // The baseplate takes about a second at full detail in release; the small
    // parts cover the construction.
    for (spec, part) in BUILTIN.iter().zip(&catalog.parts).filter(|(s, _)| !s.base) {
        let detail = builtin::build(spec, Lod::Detail).expect("builds");
        assert_eq!(detail.shape, part.shape, "{}", spec.id);
        let low = builtin::build(spec, Lod::Low).expect("builds");
        assert!(
            low.mesh.triangle_count() < detail.mesh.triangle_count(),
            "{}",
            spec.id
        );
    }
}

/// The render levels (docs/plan/07 sections 3 and 6): LOD changes what is drawn, never what
/// collides or connects ("never derive collision from the render LOD").
#[test]
fn every_render_level_has_the_same_shape_and_fewer_triangles() {
    let catalog = Catalog::builtin().expect("builds");
    for (spec, part) in BUILTIN.iter().zip(&catalog.parts) {
        let low = builtin::build(spec, Lod::Low).expect("builds");
        let simple = builtin::build(spec, Lod::Simple).expect("builds");
        let hull = builtin::build(spec, Lod::Hull).expect("builds");
        // Same collision box, pick box and connectors as the catalog holds, whichever level is drawn.
        for built in [&low, &simple, &hull] {
            assert_eq!(built.shape, part.shape, "{}", spec.id);
            assert_eq!(built.height_ldu, low.height_ldu, "{}", spec.id);
        }
        let (l, s, h) = (
            low.mesh.triangle_count(),
            simple.mesh.triangle_count(),
            hull.mesh.triangle_count(),
        );
        assert!(h == 12 && s < l && h < s, "{}: {l} {s} {h}", spec.id);
        if !spec.base {
            let detail = builtin::build(spec, Lod::Detail).expect("builds");
            assert!(l < detail.mesh.triangle_count(), "{}", spec.id);
        }
        // The hull is the body's box, from the mesh: it sits inside the pick box and, apart from the
        // undersize gap, fills the body box; it has no stud.
        let shape = &part.shape;
        let (lo, hi) = hull.mesh.positions.iter().fold(
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
            |(mut lo, mut hi), p| {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
                (lo, hi)
            },
        );
        for k in 0..3 {
            assert!(
                f64::from(lo[k]) >= f64::from(shape.pick.min[k]) - 1.0,
                "{} {k}",
                spec.id
            );
            assert!(
                f64::from(hi[k]) <= f64::from(shape.pick.max[k]) + 1.0,
                "{} {k}",
                spec.id
            );
        }
        assert!(
            f64::from(lo[1]) >= f64::from(shape.body.min[1]) - 0.5,
            "{}: no studs on the hull",
            spec.id
        );
        for k in [0, 2] {
            assert!(
                (f64::from(lo[k]) - f64::from(shape.body.min[k])).abs() < 1.0,
                "{}",
                spec.id
            );
            assert!(
                (f64::from(hi[k]) - f64::from(shape.body.max[k])).abs() < 1.0,
                "{}",
                spec.id
            );
        }
        // Counter-clockwise from outside: every triangle's normal points away from the centre.
        let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        for t in hull.mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| hull.mesh.positions[i as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let out = [a[0] - centre[0], a[1] - centre[1], a[2] - centre[2]];
            assert!(
                n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0,
                "{}",
                spec.id
            );
        }
    }
}

#[test]
fn meshes_sit_in_their_boxes_with_studs_over_the_connectors() {
    let spec = BUILTIN.iter().find(|s| s.id == "ob.brick-2x4").unwrap();
    let b = builtin::build(spec, Lod::Detail).expect("builds");
    let gap = STANDARD.get().mm("undersize_per_side") / STANDARD.get().ldu_mm;
    let (lo, hi) = b.mesh.positions.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(mut lo, mut hi), p| {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            (lo, hi)
        },
    );
    // Inset by the undersize from the lattice cell on the four sides.
    assert!((f64::from(lo[0]) - (f64::from(b.shape.body.min[0]) + gap)).abs() < 1e-3);
    assert!((f64::from(hi[2]) - (f64::from(b.shape.body.max[2]) - gap)).abs() < 1e-3);
    assert!(
        (f64::from(hi[1]) - f64::from(b.height_ldu)).abs() < 1e-3,
        "flat on its bottom"
    );
    // A stud stands over every stud connector: the stud top ring is at the
    // stud radius around it.
    let r = std_ldu("stud_diameter") / 2.0;
    let top = f64::from(lo[1]);
    for c in b
        .shape
        .connectors
        .iter()
        .filter(|c| c.kind == ConnKind::StudM)
    {
        let ring = b.mesh.positions.iter().filter(|p| {
            let d = (f64::from(p[0]) - f64::from(c.pos[0]))
                .hypot(f64::from(p[2]) - f64::from(c.pos[2]));
            (f64::from(p[1]) - top).abs() < 1e-3 && d < r + 1e-3
        });
        assert!(ring.count() >= 8, "a stud top over {c:?}");
    }
}

/// Golden meshes of the built-in catalog (manifold-rust, x86-64 Linux): the
/// triangle counts everywhere, the fingerprints where the kernel output is
/// verified bit for bit (docs/spikes/q01-kernel-conformance.md). A change here
/// changes what the assembler draws: review it, then update the numbers.
#[test]
fn golden_meshes() {
    let golden: &[(&str, Lod, usize, u64)] = &[
        ("ob.brick-2x4", Lod::Detail, 2320, 0x017c_ed97_b337_3937),
        ("ob.plate-1x2", Lod::Detail, 642, 0x1a2d_849f_a3cd_9aff),
        ("ob.tile-2x2", Lod::Detail, 528, 0xef31_b1d5_bb43_5b30),
        ("ob.brick-1x1", Lod::Low, 186, 0x7e29_fc72_94a2_a850),
        ("ob.brick-2x4", Lod::Simple, 472, 0xbc60_b276_bd8d_d48d),
        ("ob.brick-2x4", Lod::Hull, 12, 0xccd4_8067_dbb2_7725),
        ("ob.plate-1x2", Lod::Hull, 12, 0x945c_62b4_ddeb_2f79),
    ];
    for &(id, lod, triangles, canonical) in golden {
        let spec = BUILTIN.iter().find(|s| s.id == id).unwrap();
        let b = builtin::build(spec, lod).expect("builds");
        assert_eq!(b.mesh.triangle_count(), triangles, "{id} {lod:?}");
        let mesh = Mesh {
            positions: b.mesh.positions.clone(),
            indices: b.mesh.indices.clone(),
        };
        if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
            assert_eq!(
                mesh.canonical_fingerprint(),
                canonical,
                "{id} {lod:?}: mesh changed, got {:#018x}",
                mesh.canonical_fingerprint()
            );
        }
    }
}
