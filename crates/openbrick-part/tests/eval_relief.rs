//! relief.svg (backlog item 9 in docs/plan/14; docs/plan/06 sections 5 and 7,
//! docs/plan/05 sections 5 to 7): golden logo tiles in each mode, the fill
//! rules, the morphological opening, the example part with its own artwork, the
//! content hash, exports, and hostile SVG input.
//!
//! The parts are `parts/ndn.logo-tile.obpart.json` (a 2x4 plate-height tile with
//! a 0.3 mm top chamfer, three tubes and the artwork on top) on the nominal
//! fdm-0.4 process, with the artwork swapped through the `logo` parameter.
//! Volumes are checked against independent models: the tile from
//! `common::analytic_volume`, the artwork from its shape. The opening rounds the
//! square's corners with the kernel's polygonal round joins, whose vertices lie
//! on the true arc, so the square's area lies between the bevelled square
//! (side^2 - 2 r^2) and the exactly rounded one (side^2 - (4 - pi) r^2), r being
//! half of minFeature; the tests use that interval, and the emboss test checks
//! the exact polygon: `round_segments` chords per turn, so each corner keeps a
//! fan of chords of a quarter circle.
//!
//! Fingerprints are recorded for `manifold-rust 0.19.1` and asserted on x86-64
//! Linux only, like the other golden tests (see tests/eval_golden.rs).

mod common;

use std::f64::consts::PI;
use std::path::Path;

use common::*;
use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::{round_segments, Kernel, Mesh, Solid};
use openbrick_io::{
    threemf_model_xml_with_layout, write_3mf, write_3mf_with_layout, write_stl_binary,
    ExportLayout, ExportMeta, ExportObject, GENERATOR,
};
use openbrick_part::eval::{
    EVALUATOR_REVISION, FLATTEN_TOLERANCE_MM, MAX_ARTWORK_POINTS, MAX_ARTWORK_SWEEP,
    MAX_SVG_ATTRIBUTES, MAX_SVG_BYTES, MAX_SVG_DEPTH,
};
use openbrick_part::{Built, EvalError, MemoryAssets, ParamValues, PartDoc};
use openbrick_print::{Offsets, ProcessProfile};
use serde_json::{json, Value};

const LOGO_TILE: &str = include_str!("../../../parts/ndn.logo-tile.obpart.json");

fn logo_tile() -> PartDoc {
    PartDoc::parse(LOGO_TILE).expect("the example part parses")
}

/// The logo tile with one field of its `art` feature replaced.
fn logo_tile_with_art_field(field: &str, v: Value) -> PartDoc {
    let mut doc: Value = serde_json::from_str(LOGO_TILE).expect("JSON");
    let art = doc["features"]
        .as_array_mut()
        .and_then(|f| f.iter_mut().find(|f| f["id"] == "art"))
        .expect("the art feature");
    art[field] = v;
    PartDoc::parse(&doc.to_string()).expect("parses")
}

/// The logo tile whose body has no `round` field: sharp top edges.
fn logo_tile_without_round() -> PartDoc {
    let mut doc: Value = serde_json::from_str(LOGO_TILE).expect("JSON");
    let body = &mut doc["features"][0];
    assert_eq!(body["id"], "body");
    body.as_object_mut().expect("object").remove("round");
    PartDoc::parse(&doc.to_string()).expect("parses")
}

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/svg")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn fdm() -> ProcessProfile {
    process("fdm-0.4")
}

/// Evaluate `doc` with `svg` as its artwork (through the `logo` parameter).
fn eval_doc(doc: &PartDoc, svg: Vec<u8>, params: ParamValues) -> Result<Built, EvalError> {
    let assets = MemoryAssets::new().with("art.svg", svg);
    let params = params.with("logo", json!("art.svg"));
    run_with_assets(doc, &params, &fdm(), None, &assets, false)
}

fn tile(svg: Vec<u8>, params: ParamValues) -> Result<Built, EvalError> {
    eval_doc(&logo_tile(), svg, params)
}

fn param_default(name: &str) -> f64 {
    logo_tile().params[name]
        .default
        .as_f64()
        .unwrap_or_else(|| panic!("{name} has a numeric default"))
}

/// The tile's top chamfer from its `round` field ("0.3mm" in the document).
fn top_chamfer() -> f64 {
    let doc: Value = serde_json::from_str(LOGO_TILE).expect("JSON");
    doc["features"][0]["round"]["radius"]
        .as_str()
        .and_then(|s| s.strip_suffix("mm"))
        .and_then(|s| s.parse().ok())
        .expect("a radius in mm")
}

/// Geometry of the 2x4 logo tile's top face (docs/plan/06 section 7).
struct Face {
    /// Body footprint after the undersize.
    w: f64,
    d: f64,
    /// Body height (plate).
    h: f64,
    /// The face box: inside the top chamfer, minus the margin.
    box_min: [f64; 2],
    box_max: [f64; 2],
}

fn face() -> Face {
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let (w, d) = (2.0 * pitch - 2.0 * gap, 4.0 * pitch - 2.0 * gap);
    let inset = top_chamfer() + param_default("margin");
    Face {
        w,
        d,
        h: mm("plate_height"),
        box_min: [inset, inset],
        box_max: [w - inset, d - inset],
    }
}

impl Face {
    fn box_size(&self) -> [f64; 2] {
        [
            self.box_max[0] - self.box_min[0],
            self.box_max[1] - self.box_min[1],
        ]
    }

    fn centre(&self) -> [f64; 2] {
        [self.w / 2.0, self.d / 2.0]
    }
}

/// The tile without artwork (the analytic model of tests/common).
fn tile_shape() -> Shape {
    Shape {
        size: [2, 4],
        plates: 1,
        studs: false,
        tubes: true,
        top_chamfer: top_chamfer(),
    }
}

/// The plain tile volume with the roof thickened by `extra` and no wall-to-roof
/// chamfer, through the analytic model: it reads the roof as the larger of the
/// standard roof and the profile's minRoofMm, and the chamfer from
/// wallRoofFilletMm, so a profile with those changed describes the thickened body.
fn thickened_tile_volume(p: &ProcessProfile, extra: f64) -> f64 {
    let mut q = p.clone();
    q.min_roof_mm = mm("roof").max(p.min_roof_mm) + extra;
    q.wall_roof_fillet_mm.insert("plate".into(), 0.0);
    analytic_volume(tile_shape(), &q, &Offsets::default())
}

/// The interval holding the area of a square of side `s` opened at radius `r`
/// with polygonal round joins (see the module comment).
fn opened_square_area(s: f64, r: f64) -> (f64, f64) {
    (s * s - 2.0 * r * r, s * s - (4.0 - PI) * r * r)
}

fn assert_between(what: &str, v: f64, (lo, hi): (f64, f64)) {
    let slack = 1e-9 * hi.abs().max(1.0);
    assert!(
        v >= lo - slack && v <= hi + slack,
        "{what}: {v} not in [{lo}, {hi}]"
    );
}

fn bounds(mesh: &Mesh) -> ([f64; 3], [f64; 3]) {
    let (lo, hi) = mesh.bounds();
    (lo.map(f64::from), hi.map(f64::from))
}

fn assert_bounds(what: &str, mesh: &Mesh, lo: [f64; 3], hi: [f64; 3]) {
    let (a, b) = bounds(mesh);
    for k in 0..3 {
        assert!(
            (a[k] - lo[k]).abs() < 1e-4 && (b[k] - hi[k]).abs() < 1e-4,
            "{what}: axis {k} spans {} to {}, expected {} to {}",
            a[k],
            b[k],
            lo[k],
            hi[k]
        );
    }
}

fn check_fingerprint(what: &str, mesh: &Mesh, want: u64) {
    eprintln!(
        "{what}: canonical 0x{:016x}, {} triangles",
        mesh.canonical_fingerprint(),
        mesh.triangle_count()
    );
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        assert_eq!(
            mesh.canonical_fingerprint(),
            want,
            "{what}: mesh changed (evaluator revision {EVALUATOR_REVISION}; see the module comment)"
        );
    }
}

fn r0() -> f64 {
    fdm().min_feature_mm / 2.0
}

/// Pieces of a mesh connected through shared vertices. Two closed shells that
/// only touch (meshes merged without a union) count as two; the kernel's union
/// of two touching solids, done right, is one.
fn shells(mesh: &Mesh) -> usize {
    let mut parent: Vec<usize> = (0..mesh.positions.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let a = find(&mut parent, t[0] as usize);
        for &v in &t[1..] {
            let b = find(&mut parent, v as usize);
            parent[b] = a;
        }
    }
    let mut roots: Vec<usize> = mesh
        .indices
        .iter()
        .map(|&i| find(&mut parent, i as usize))
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots.len()
}

#[test]
fn emboss_square_golden() {
    let p = fdm();
    let f = face();
    let h = param_default("reliefHeight");
    let b = tile(fixture("square.svg"), ParamValues::new()).expect("builds");
    eprintln!("warnings: {:?}", b.warnings);
    let roles: Vec<&str> = b.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(roles, ["body", "accent"]);
    let (body, accent) = (&b.objects[0], &b.objects[1]);

    // The body is the plain tile: the relief is its own object.
    let tile_v = analytic_volume(tile_shape(), &p, &Offsets::default());
    assert!(
        ((body.volume_mm3 - tile_v) / tile_v).abs() < 1e-6,
        "{} vs {tile_v}",
        body.volume_mm3
    );
    // The square fills the narrower side of the face box, centred.
    let s = f.box_size()[0].min(f.box_size()[1]);
    let (a_lo, a_hi) = opened_square_area(s, r0());
    assert_between("accent volume", accent.volume_mm3, (a_lo * h, a_hi * h));
    let [cx, cy] = f.centre();
    assert_bounds(
        "accent",
        &accent.mesh,
        [cx - s / 2.0, cy - s / 2.0, f.h],
        [cx + s / 2.0, cy + s / 2.0, f.h + h],
    );
    let m = &b.measurements;
    assert!((m.bounds_max_mm[2] - (f.h + h)).abs() < 1e-4);
    assert!((m.volume_mm3 - body.volume_mm3 - accent.volume_mm3).abs() < 1e-9);
    assert!(m.is_manifold);
    assert_eq!(m.components, 2, "the body and one square");
    assert_eq!(m.genus, 0);
    assert_eq!(m.relief_height_mm, Some(h));
    // Only the corners are rounded: at most 2 r^2 of s^2.
    let pct = m.removed_area_pct.expect("measured");
    let bevelled = 100.0 * 2.0 * r0() * r0() / (s * s);
    assert!(
        pct > 0.0 && pct <= bevelled * (1.0 + 1e-6),
        "{pct} vs {bevelled}"
    );
    let stroke = m.min_stroke_mm.expect("measured");
    assert!(
        (stroke - s).abs() < 0.02,
        "a square of side {s} measures {stroke}"
    );
    assert_eq!(
        m.roof_mm,
        Some(mm("roof").max(p.min_roof_mm)),
        "emboss leaves the roof"
    );

    // The fused single-colour object: one watertight solid with both volumes.
    let fused = b.fused.as_ref().expect("two objects");
    assert_eq!(fused.role, "body");
    assert!((fused.volume_mm3 - m.volume_mm3).abs() < 1e-6 * m.volume_mm3);
    assert_eq!(shells(&body.mesh) + shells(&accent.mesh), 2);
    assert_eq!(shells(&fused.mesh), 1, "one solid, not two touching shells");
    let art = b.artwork.as_ref().expect("artwork report");
    assert_eq!(
        (art.mode.as_str(), art.face_on_bed.as_str()),
        ("emboss", "bottom")
    );
    assert_eq!(art.face_z_mm, f.h);
    assert_eq!(art.height_mm, h);
    assert!((art.area_after_mm2 * h - accent.volume_mm3).abs() < 1e-6);
    assert!(
        (art.area_before_mm2 - s * s).abs() < 1e-6,
        "{}",
        art.area_before_mm2
    );
    for k in 0..2 {
        assert!((art.box_min_mm[k] - f.box_min[k]).abs() < 1e-12);
        assert!((art.box_max_mm[k] - f.box_max[k]).abs() < 1e-12);
    }

    // The opening rounds each corner with chords of a quarter circle of radius
    // r: round_segments(r, 0.05 mm) = 8 per turn at r = 0.4 mm, so 2 per
    // corner, whose sagitta r (1 - cos 22.5 deg) = 0.030 mm stays within the
    // flattening tolerance (one chord, a 45 degree bevel, was 0.117 mm off).
    // The accent is the exact 12-sided polygon: each corner keeps the fan of
    // its 2 chords, area r^2 sin(45 deg), instead of the full r^2 square.
    let r = r0();
    let n = round_segments(r, FLATTEN_TOLERANCE_MM);
    assert_eq!(n, 8);
    let chords = f64::from(n / 4);
    let fan = chords * r * r * (std::f64::consts::FRAC_PI_2 / chords).sin() / 2.0;
    let exact = s * s - 4.0 * (r * r - fan);
    assert!(
        (accent.volume_mm3 / h - exact).abs() < 1e-6,
        "{} vs {exact}",
        accent.volume_mm3 / h
    );
    // Within the tolerance of true arcs: the area between each quarter arc and
    // its chords is under the arc length times the sagitta bound.
    let true_round = s * s - (4.0 - PI) * r * r;
    assert!(true_round - accent.volume_mm3 / h < 2.0 * PI * r * FLATTEN_TOLERANCE_MM);
    // A 12-sided prism: 10 + 10 triangles for top and bottom, 24 for the sides.
    assert_eq!(accent.mesh.triangle_count(), 44);
    check_fingerprint("emboss body", &body.mesh, 0x0cd4_561e_4989_fbd0);
    check_fingerprint("emboss accent", &accent.mesh, 0xa5bb_6b50_033e_0c81);
    check_fingerprint("emboss fused", &fused.mesh, 0x195b_5430_f6d4_a511);
}

#[test]
fn a_thin_diagonal_bar_measures_its_width() {
    // A 120 x 8 unit bar turned 45 degrees about the viewBox centre: its
    // bounding box is (120 + 8) / sqrt 2 units square, fitted across the face
    // box, so the bar is 8 units wide on the part. Its area is far below the
    // square of half the face width, so only a search from the thin end finds
    // it (and the rotation exercises the transforms).
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
        <rect x="-60" y="-4" width="120" height="8" transform="translate(50 50) rotate(45)"/>
    </svg>"#;
    let b = tile(svg.to_vec(), ParamValues::new()).expect("builds");
    let unit = face().box_size()[0] / (128.0 / 2f64.sqrt());
    let width = 8.0 * unit;
    let stroke = b.measurements.min_stroke_mm.expect("measured");
    assert!(
        (stroke - width).abs() < 0.05,
        "a {width} mm bar measures {stroke}"
    );
    assert!(stroke >= fdm().min_feature_mm);
}

#[test]
fn a_relief_in_the_body_colour_joins_the_body() {
    let doc = logo_tile_with_art_field("colorRole", json!("body"));
    let b = eval_doc(&doc, fixture("square.svg"), ParamValues::new()).expect("builds");
    assert_eq!(b.objects.len(), 1, "one colour, one object");
    assert!(b.fused.is_none(), "already single-colour");
    assert_eq!(shells(&b.objects[0].mesh), 1);
    assert_eq!(b.measurements.components, 1);
    // The same solid as the two-colour body and relief together.
    let two = tile(fixture("square.svg"), ParamValues::new()).expect("builds");
    assert!((b.measurements.volume_mm3 - two.measurements.volume_mm3).abs() < 1e-6);
    // An inlay in the body colour is just a deboss printed face down.
    let b = eval_doc(
        &doc,
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("inlay")),
    )
    .expect("builds");
    assert_eq!(b.objects.len(), 1);
    assert_eq!(shells(&b.objects[0].mesh), 1);
    assert!(
        b.warnings.iter().any(|w| w.contains("plain deboss")),
        "{:?}",
        b.warnings
    );
}

#[test]
fn without_the_opening_the_square_is_exact() {
    // minFeature 0 turns the opening off: the relief is exactly s x s x h.
    let doc = logo_tile_with_art_field("minFeature", json!("0mm"));
    let f = face();
    let h = param_default("reliefHeight");
    let b = eval_doc(&doc, fixture("square.svg"), ParamValues::new()).expect("builds");
    let s = f.box_size()[0].min(f.box_size()[1]);
    let accent = &b.objects[1];
    assert!(
        (accent.volume_mm3 - s * s * h).abs() < 1e-9 * s * s * h,
        "{} vs {}",
        accent.volume_mm3,
        s * s * h
    );
    assert_eq!(b.measurements.removed_area_pct, Some(0.0));
    assert!(b.artwork.as_ref().expect("report").removed.is_empty());
}

#[test]
fn deboss_on_a_plate_keeps_the_cavity_and_clamps() {
    // Backlog item 9 acceptance: on a plate-height tile the cavity is 2.2 mm, so
    // a deboss may take only 0.4 mm before the cavity would drop under the
    // 1.8 mm a stud needs (docs/plan/05 section 6). The document asks for 0.6.
    let p = fdm();
    let f = face();
    let requested = param_default("reliefHeight");
    let b = tile(
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("deboss")),
    )
    .expect("builds");
    let roof = mm("roof").max(p.min_roof_mm);
    let room = (f.h - roof) - p.min_cavity_depth_mm;
    let m = &b.measurements;
    let depth = m.relief_height_mm.expect("measured");
    assert!(depth < requested, "clamped");
    assert!(
        depth <= room && room - depth < 1e-3,
        "{depth} vs room {room}"
    );
    let cavity = m.cavity_depth_mm.expect("measured");
    assert!(cavity >= p.min_cavity_depth_mm, "{cavity}");
    assert!(cavity - p.min_cavity_depth_mm < 1e-3, "{cavity}");
    assert_eq!(
        m.roof_mm,
        Some(roof),
        "the roof under the artwork keeps its thickness"
    );
    let clamped = format!("clamped to {room:.3} mm");
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("`art`") && w.contains(&clamped) && w.contains("CAVITY.DEPTH")),
        "{:?}",
        b.warnings
    );
    let art = b.artwork.as_ref().expect("report");
    assert_eq!((art.height_mm, art.requested_height_mm), (depth, requested));

    // One object: the body with the recess, its roof thickened by the depth and
    // its wall-to-roof chamfer gone (no depth is left for it above 1.8 mm).
    assert_eq!(b.objects.len(), 1);
    assert!(b.fused.is_none());
    let body = &b.objects[0];
    let s = f.box_size()[0].min(f.box_size()[1]);
    let (a_lo, a_hi) = opened_square_area(s, r0());
    let full = thickened_tile_volume(&p, depth);
    assert_between(
        "deboss volume",
        body.volume_mm3,
        (full - a_hi * depth, full - a_lo * depth),
    );
    assert!(m.is_manifold && m.components == 1 && m.genus == 0);
    assert!(
        (m.bounds_max_mm[2] - f.h).abs() < 1e-4,
        "nothing stands proud"
    );

    // Slices: just above the lowered ceiling the body is solid across; just
    // below it the cavity is open; in the recess the square is missing.
    let outer = f.w * f.d;
    let ceiling = f.h - roof - depth;
    let above = section_area(&body.mesh, ceiling + 0.05, |_| true);
    assert!((above - outer).abs() < 1e-6 * outer, "{above} vs {outer}");
    let below = section_area(&body.mesh, ceiling - 0.05, |_| true);
    assert!(
        below < 0.6 * outer,
        "the cavity is open below the ceiling: {below}"
    );
    let recess = section_area(&body.mesh, f.h - depth + 0.05, |_| true);
    assert_between("recess slice", recess, (outer - a_hi, outer - a_lo));
    check_fingerprint("deboss body", &body.mesh, 0x5cb4_8925_a718_ab13);
}

#[test]
fn deboss_on_a_brick_is_not_clamped() {
    let p = fdm();
    let h = param_default("reliefHeight");
    let b = tile(
        fixture("square.svg"),
        ParamValues::new()
            .with("relief", json!("deboss"))
            .with("height", json!("brick")),
    )
    .expect("builds");
    let m = &b.measurements;
    assert_eq!(m.relief_height_mm, Some(h));
    assert!(
        b.warnings.iter().all(|w| !w.contains("clamped")),
        "{:?}",
        b.warnings
    );
    let roof = mm("roof").max(p.min_roof_mm);
    let cavity = mm("brick_height") - roof - h;
    assert!((m.cavity_depth_mm.expect("measured") - cavity).abs() < 1e-12);
    assert_eq!(m.roof_mm, Some(roof));
}

#[test]
fn inlay_splits_off_the_face_slab_minus_the_logo() {
    let p = fdm();
    let f = face();
    let b = tile(
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("inlay")),
    )
    .expect("builds");
    let roles: Vec<&str> = b.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(roles, ["body", "accent"]);
    let (body, slab) = (&b.objects[0], &b.objects[1]);
    let t = b.measurements.relief_height_mm.expect("measured");
    assert!(t > 0.0);
    let art = b.artwork.as_ref().expect("report");
    assert_eq!(
        (art.mode.as_str(), art.face_on_bed.as_str()),
        ("inlay", "top")
    );
    // The warning says which variant this is: the single-extruder swap (the
    // default inlayStyle), with the artwork a recess in the body colour; how
    // to get the multi-material variant (the artwork fill as its own part);
    // and where the swap goes when the layers do not add up to the slab depth.
    let inlay = b
        .warnings
        .iter()
        .find(|w| w.contains("face down"))
        .unwrap_or_else(|| panic!("{:?}", b.warnings));
    for words in [
        "single-extruder filament-swap variant (inlayStyle swap)",
        "recess in the body colour",
        "set inlayStyle to `parts`",
        "artwork fill becomes its own `accent` part",
        "first layer the slicer's preview shows as the body",
        &format!("{t:.3} mm face slab"),
    ] {
        assert!(inlay.contains(words), "`{words}` in: {inlay}");
    }
    assert_eq!(art.inlay_style.as_deref(), Some("swap"));

    // The slab is the top t of the tile (its 0.3 mm top chamfer lies inside it)
    // minus the square; the body is the thickened tile below it.
    let rc = top_chamfer();
    assert!(rc <= t);
    // The tile's own 0.3 mm chamfer is above the first-layer chamfer, so it
    // stays, and it is the chamfer of the face on the bed.
    assert!(rc >= p.first_layer_chamfer_mm);
    assert_eq!(art.face_chamfer_mm, rc);
    assert!(
        b.warnings
            .iter()
            .all(|w| !w.contains("top chamfer is raised")),
        "{:?}",
        b.warnings
    );
    // The square leaves no face material thinner than minFeature: a 1.3 mm
    // rim (chamfer plus margin) to the long edges, more to the short ones.
    assert!(art.thin_face_area_mm2 < 1e-9, "{}", art.thin_face_area_mm2);
    let slab_full = f.w * f.d * t - edge_chamfer(f.w, f.d, rc);
    let s = f.box_size()[0].min(f.box_size()[1]);
    let (a_lo, a_hi) = opened_square_area(s, r0());
    assert_between(
        "slab volume",
        slab.volume_mm3,
        (slab_full - a_hi * t, slab_full - a_lo * t),
    );
    let full = thickened_tile_volume(&p, t);
    assert!(
        (body.volume_mm3 - (full - slab_full)).abs() < 1e-6 * full,
        "{} vs {}",
        body.volume_mm3,
        full - slab_full
    );
    assert_bounds("slab", &slab.mesh, [0.0, 0.0, f.h - t], [f.w, f.d, f.h]);
    let (_, body_hi) = bounds(&body.mesh);
    assert!((body_hi[2] - (f.h - t)).abs() < 1e-4);
    // Touching, not overlapping: the fused part is the debossed tile.
    let fused = b.fused.as_ref().expect("two objects");
    assert!((fused.volume_mm3 - body.volume_mm3 - slab.volume_mm3).abs() < 1e-6 * full);
    assert_eq!(shells(&fused.mesh), 1, "one solid, not two touching shells");
    assert_between(
        "fused volume",
        fused.volume_mm3,
        (full - a_hi * t, full - a_lo * t),
    );
    let m = &b.measurements;
    assert!(m.is_manifold);
    assert_eq!(m.components, 2);
    assert_eq!(m.genus, 1, "the slab is a frame around the square");
    check_fingerprint("inlay body", &body.mesh, 0x2300_1b60_c274_3581);
    check_fingerprint("inlay slab", &slab.mesh, 0xf569_6c2c_0869_0014);
}

#[test]
fn inlay_without_round_puts_the_first_layer_chamfer_on_the_bed() {
    // An inlay prints face down, so its top edges are the first layer. Without
    // a `round` chamfer they get the first-layer chamfer that replaces
    // elephant-foot compensation (docs/plan/05 section 6), with a warning; the
    // face box moves inside it.
    let p = fdm();
    let c = p.first_layer_chamfer_mm;
    assert!(c > 0.0);
    let b = eval_doc(
        &logo_tile_without_round(),
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("inlay")),
    )
    .expect("builds");
    eprintln!("warnings: {:?}", b.warnings);
    let raised = format!("top chamfer is raised from 0 mm to {c} mm");
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains(&raised) && w.contains("elephant-foot")),
        "{:?}",
        b.warnings
    );
    let art = b.artwork.as_ref().expect("report");
    assert_eq!(art.face_on_bed, "top");
    assert_eq!(art.face_chamfer_mm, c);
    let m = &b.measurements;
    let t = m.relief_height_mm.expect("measured");
    assert!(c <= t, "the chamfer lies inside the slab");

    // The face box is inside the raised chamfer.
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let (w, d, h) = (
        2.0 * pitch - 2.0 * gap,
        4.0 * pitch - 2.0 * gap,
        mm("plate_height"),
    );
    let inset = c + param_default("margin");
    for k in 0..2 {
        assert!((art.box_min_mm[k] - inset).abs() < 1e-12);
    }
    let s = (w - 2.0 * inset).min(d - 2.0 * inset);
    let (a_lo, a_hi) = opened_square_area(s, r0());
    let (body, slab) = (&b.objects[0], &b.objects[1]);
    assert_eq!((body.role.as_str(), slab.role.as_str()), ("body", "accent"));

    // Independent volumes: the slab is the top t of a tile with a c chamfer on
    // its top edges, minus the square; the body is the tile with that chamfer
    // and the roof thickened by t, minus the slab.
    let slab_full = w * d * t - edge_chamfer(w, d, c);
    assert_between(
        "slab volume",
        slab.volume_mm3,
        (slab_full - a_hi * t, slab_full - a_lo * t),
    );
    let mut q = p.clone();
    q.min_roof_mm = mm("roof").max(p.min_roof_mm) + t;
    q.wall_roof_fillet_mm.insert("plate".into(), 0.0);
    let full = analytic_volume(
        Shape {
            top_chamfer: c,
            ..tile_shape()
        },
        &q,
        &Offsets::default(),
    );
    assert!(
        (body.volume_mm3 - (full - slab_full)).abs() < 1e-6 * full,
        "{} vs {}",
        body.volume_mm3,
        full - slab_full
    );

    // On the bed (the face plane) the slab's outline is inset by the chamfer:
    // a slice just inside the face is the rectangle inset by c - 0.05 minus
    // the square; lower down, below the chamfer, the slab reaches the outline.
    let z = h - 0.05;
    let inner = (w - 2.0 * (c - 0.05)) * (d - 2.0 * (c - 0.05));
    let face = section_area(&slab.mesh, z, |_| true);
    assert_between("slab at the bed", face, (inner - a_hi, inner - a_lo));
    let below = section_area(&slab.mesh, h - c - 0.05, |_| true);
    assert_between(
        "slab below the chamfer",
        below,
        (w * d - a_hi, w * d - a_lo),
    );
    assert_bounds("slab", &slab.mesh, [0.0, 0.0, h - t], [w, d, h]);

    // The Measurements first-layer fields keep describing the bottom edges at
    // z = 0 (where the walls and tubes get the first-layer chamfer); the face
    // on the bed is described by the artwork report above.
    assert_eq!(m.first_layer_chamfer_mm, Some(c));
    let wall = m.min_body_wall_mm.expect("measured");
    assert!((m.min_body_wall_at_bed_mm.expect("measured") - (wall - 2.0 * c)).abs() < 1e-12);
    let plain = eval_doc(
        &logo_tile_without_round(),
        fixture("square.svg"),
        ParamValues::new(),
    )
    .expect("emboss builds");
    let pm = &plain.measurements;
    assert_eq!(
        (m.min_body_wall_at_bed_mm, m.min_tube_wall_at_bed_mm),
        (pm.min_body_wall_at_bed_mm, pm.min_tube_wall_at_bed_mm)
    );
    // Emboss prints face up and leaves the sharp top edges alone.
    assert_eq!(plain.artwork.as_ref().expect("report").face_chamfer_mm, 0.0);
    assert!(plain
        .warnings
        .iter()
        .all(|w| !w.contains("top chamfer is raised")));

    let fused = b.fused.as_ref().expect("two objects");
    assert_eq!(shells(&fused.mesh), 1);
    assert!(m.is_manifold && m.components == 2 && m.genus == 1);
    // Both chamfers, the tile's 0.3 mm and this 0.2 mm, lie inside the 0.4 mm
    // slab, so the body below the split is the rounded inlay's body exactly.
    check_fingerprint("inlay sharp body", &body.mesh, 0x2300_1b60_c274_3581);
    check_fingerprint("inlay sharp slab", &slab.mesh, 0x12c4_6d6b_269c_a77f);
}

/// The logo tile without its `art` feature: the plain tile the reliefs are cut
/// from or stand on.
fn plain_logo_tile() -> PartDoc {
    let mut doc: Value = serde_json::from_str(LOGO_TILE).expect("JSON");
    doc["features"]
        .as_array_mut()
        .expect("features")
        .retain(|f| f["id"] != "art");
    PartDoc::parse(&doc.to_string()).expect("parses")
}

/// The multi-material inlay of the square.
fn parts_inlay(svg: Vec<u8>, params: ParamValues) -> Result<Built, EvalError> {
    tile(
        svg,
        params
            .with("relief", json!("inlay"))
            .with("inlayStyle", json!("parts")),
    )
}

/// The volume two meshes share, through the kernel: each mesh back into a
/// solid, then their intersection.
fn shared_volume(a: &Mesh, b: &Mesh) -> f64 {
    let k = ManifoldRust;
    let a = k.solid_from_mesh(a).expect("a closed mesh");
    let b = k.solid_from_mesh(b).expect("a closed mesh");
    k.intersection(&a, &b).expect("intersection").volume_mm3()
}

/// The opened square's exact area: the 12-sided polygon of the emboss test
/// (each corner keeps the fan of its round-join chords).
fn opened_square_exact(s: f64) -> f64 {
    let r = r0();
    let n = round_segments(r, FLATTEN_TOLERANCE_MM);
    let chords = f64::from(n / 4);
    let fan = chords * r * r * (std::f64::consts::FRAC_PI_2 / chords).sin() / 2.0;
    s * s - 4.0 * (r * r - fan)
}

#[test]
fn inlay_parts_fill_and_body_make_the_plain_tile() {
    // The multi-material inlay (inlayStyle parts): the artwork fill is the
    // accent object, the opened square reliefHeight deep into the face, and the
    // body is the plain tile recessed by exactly that region. Nothing is
    // thickened or clamped: on fdm-0.4 the 1.0 mm roof keeps 0.4 mm under the
    // 0.6 mm fill, more than one 0.16 mm layer.
    let p = fdm();
    let f = face();
    let h = param_default("reliefHeight");
    let roof = mm("roof").max(p.min_roof_mm);
    let b = parts_inlay(fixture("square.svg"), ParamValues::new()).expect("builds");
    eprintln!("warnings: {:?}", b.warnings);
    let roles: Vec<&str> = b.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(roles, ["body", "accent"]);
    let (body, fill) = (&b.objects[0], &b.objects[1]);
    let m = &b.measurements;
    assert_eq!(m.relief_height_mm, Some(h), "not clamped");
    assert!(roof - h >= 0.16 - 1e-12);
    assert_eq!(m.roof_mm, Some(roof), "the roof is not thickened");
    assert_eq!(
        m.cavity_depth_mm,
        Some(f.h - roof),
        "nor the cavity lowered"
    );
    let art = b.artwork.as_ref().expect("report");
    assert_eq!(
        (
            art.mode.as_str(),
            art.inlay_style.as_deref(),
            art.face_on_bed.as_str()
        ),
        ("inlay", Some("parts"), "top")
    );
    assert_eq!(art.face_chamfer_mm, top_chamfer(), "0.3 mm is above 0.2");
    assert!(art.thin_face_area_mm2 < 1e-9);

    // Exact volumes. The fill is the opened square (the exact 12-gon of the
    // emboss test) times the depth; the body is the plain tile minus it; the
    // two add up to the plain tile, analytically and as the kernel builds it
    // without the relief.
    let s = f.box_size()[0].min(f.box_size()[1]);
    let fill_v = opened_square_exact(s) * h;
    assert!(
        (fill.volume_mm3 - fill_v).abs() < 1e-6,
        "{} vs {fill_v}",
        fill.volume_mm3
    );
    let plain = analytic_volume(tile_shape(), &p, &Offsets::default());
    assert!(
        ((body.volume_mm3 + fill.volume_mm3 - plain) / plain).abs() < 1e-6,
        "{} + {} vs {plain}",
        body.volume_mm3,
        fill.volume_mm3
    );
    let kernel_plain = run_with_assets(
        &plain_logo_tile(),
        &ParamValues::new(),
        &p,
        None,
        &MemoryAssets::new(),
        false,
    )
    .expect("the plain tile builds");
    let plain_k = kernel_plain.objects[0].volume_mm3;
    assert!(
        ((body.volume_mm3 + fill.volume_mm3 - plain_k) / plain_k).abs() < 1e-9,
        "{} + {} vs {plain_k}",
        body.volume_mm3,
        fill.volume_mm3
    );
    assert!(((body.volume_mm3 - (plain_k - fill_v)) / plain_k).abs() < 1e-9);

    // In place: the fill spans the square in plan and the top h of the face;
    // the body keeps the whole tile's outline and height.
    let [cx, cy] = f.centre();
    assert_bounds(
        "fill",
        &fill.mesh,
        [cx - s / 2.0, cy - s / 2.0, f.h - h],
        [cx + s / 2.0, cy + s / 2.0, f.h],
    );
    assert_bounds("body", &body.mesh, [0.0, 0.0, 0.0], [f.w, f.d, f.h]);
    // Halfway down the fill, a slice of the body misses exactly the fill (the
    // slices read the f32 mesh, about 1e-7 relative).
    let z = f.h - h / 2.0;
    let outer = f.w * f.d;
    let body_slice = section_area(&body.mesh, z, |_| true);
    let fill_slice = section_area(&fill.mesh, z, |_| true);
    let square = opened_square_exact(s);
    assert!(
        ((fill_slice - square) / square).abs() < 1e-6,
        "{fill_slice} vs {square}"
    );
    assert!(
        ((body_slice + fill_slice - outer) / outer).abs() < 1e-6,
        "{body_slice} + {fill_slice} vs {outer}"
    );

    // Touching, not overlapping: the kernel finds no shared volume, while the
    // same fill pushed 0.1 mm down shares 0.1 mm of the recess's floor.
    assert!(shared_volume(&body.mesh, &fill.mesh) < 1e-9);
    let mut sunk = fill.mesh.clone();
    for v in &mut sunk.positions {
        v[2] -= 0.1;
    }
    let pushed = shared_volume(&body.mesh, &sunk);
    assert!(
        (pushed - opened_square_exact(s) * 0.1).abs() < 1e-3,
        "the check sees an overlap: {pushed}"
    );

    // The fused single-colour object is the plain tile, one solid.
    let fused = b.fused.as_ref().expect("two objects");
    assert_eq!(shells(&fused.mesh), 1, "one solid, not two touching shells");
    assert!(((fused.volume_mm3 - plain_k) / plain_k).abs() < 1e-9);
    assert!(m.is_manifold);
    assert_eq!(m.components, 2);
    assert_eq!(m.genus, 0, "a recess and a plug, no handle");
    // The feature volume sum differs from the union exactly as the plain
    // tile's does (where the ribs meet the wall-to-roof chamfer): the fill and
    // the recess add nothing to it.
    let pm = &kernel_plain.measurements;
    eprintln!(
        "feature sum - volume: parts inlay {}, plain tile {}",
        m.feature_volume_sum_mm3 - m.volume_mm3,
        pm.feature_volume_sum_mm3 - pm.volume_mm3
    );
    assert!(
        ((m.feature_volume_sum_mm3 - m.volume_mm3) - (pm.feature_volume_sum_mm3 - pm.volume_mm3))
            .abs()
            < 1e-7 * m.volume_mm3
    );

    // The warnings say which variant, which face goes on the bed and which
    // filament goes where; nothing is clamped or raised.
    let variant = b
        .warnings
        .iter()
        .find(|w| w.contains("inlayStyle parts"))
        .unwrap_or_else(|| panic!("{:?}", b.warnings));
    for words in [
        "multi-material variant",
        "top face on the bed",
        "the `accent` object is the artwork fill, 0.600 mm deep",
        "own filament",
    ] {
        assert!(variant.contains(words), "`{words}` in: {variant}");
    }
    let roles = b
        .warnings
        .iter()
        .find(|w| w.contains("colour roles"))
        .unwrap_or_else(|| panic!("{:?}", b.warnings));
    for words in [
        "2 colour roles, each its own part",
        "`body` (the body) and `accent` (the artwork fill of `art`)",
        "top face (the artwork) on the bed",
        "turn it over in the slicer",
    ] {
        assert!(roles.contains(words), "`{words}` in: {roles}");
    }
    assert!(
        b.warnings
            .iter()
            .all(|w| !w.contains("clamped") && !w.contains("top chamfer is raised")),
        "{:?}",
        b.warnings
    );
    // The validator finds nothing but the missing fit profile of the
    // fit-critical tubes.
    let report = openbrick_part::validate(&b, &p, None);
    assert!(!report.findings.is_empty());
    assert!(
        report.findings.iter().all(|f| f.rule == "FIT.MISSING"),
        "{:?}",
        report.findings
    );

    check_fingerprint("inlay parts body", &body.mesh, 0x0426_099d_077a_5bf5);
    check_fingerprint("inlay parts fill", &fill.mesh, 0x182e_f9fa_d3d2_7a4d);
    // The fused object has the plain tile's volume (above) but not its mesh:
    // the union through the bonding copy triangulates the face around the
    // square differently (2,680 against 2,656 triangles). The plain tile is
    // the emboss body of `emboss_square_golden`.
    check_fingerprint("inlay parts fused", &fused.mesh, 0x52c1_190c_110b_7449);
    check_fingerprint(
        "plain tile",
        &kernel_plain.objects[0].mesh,
        0x0cd4_561e_4989_fbd0,
    );
    assert_eq!(kernel_plain.objects[0].mesh.triangle_count(), 2656);
    assert_eq!(fused.mesh.triangle_count(), 2680);
}

#[test]
fn inlay_parts_keep_a_layer_of_roof_under_the_fill() {
    // 1.5 mm asked on a 1.0 mm roof: clamped so the body keeps one 0.16 mm
    // layer (the thinnest of fdm-0.4) under the fill, with a warning; the
    // fill never reaches the cavity. A brick has the same roof.
    let p = fdm();
    let roof = mm("roof").max(p.min_roof_mm);
    let layer = p
        .layer_height_mm
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let b = parts_inlay(
        fixture("square.svg"),
        ParamValues::new().with("reliefHeight", json!(1.5)),
    )
    .expect("builds");
    let depth = b.measurements.relief_height_mm.expect("measured");
    assert!(
        depth <= roof - layer + 1e-12 && roof - layer - depth < 1e-3,
        "{depth}"
    );
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("clamped to 0.840 mm") && w.contains("one 0.16 mm layer")),
        "{:?}",
        b.warnings
    );
    let (body, fill) = (&b.objects[0], &b.objects[1]);
    assert!(shared_volume(&body.mesh, &fill.mesh) < 1e-9);
    let (lo, _) = bounds(&fill.mesh);
    let ceiling = mm("plate_height") - roof;
    assert!(lo[2] > ceiling, "{} above the ceiling {ceiling}", lo[2]);
    assert!(b.measurements.is_manifold && b.measurements.components == 2);

    // The style is checked, and only inlay reads it.
    let e = tile(
        fixture("square.svg"),
        ParamValues::new()
            .with("relief", json!("inlay"))
            .with("inlayStyle", json!("paint")),
    )
    .expect_err("not a style");
    assert!(
        matches!(&e, EvalError::Param { name, .. } if name == "inlayStyle"),
        "{e}"
    );
    let doc = logo_tile_with_art_field("inlayStyle", json!("paint"));
    let e = eval_doc(
        &doc,
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("inlay")),
    )
    .expect_err("not a style");
    assert!(
        matches!(&e, EvalError::Field { field, .. } if field == "inlayStyle"),
        "{e}"
    );
    assert!(e.to_string().contains("swap"), "{e}");
    let emboss = tile(
        fixture("square.svg"),
        ParamValues::new().with("inlayStyle", json!("parts")),
    )
    .expect("builds");
    assert!(
        emboss
            .warnings
            .iter()
            .any(|w| w.contains("applies to inlay only") && w.contains("its own part already")),
        "{:?}",
        emboss.warnings
    );
    assert_eq!(emboss.artwork.as_ref().expect("report").inlay_style, None);
    // In the body's own colour the parts inlay is the plain tile.
    let doc = logo_tile_with_art_field("colorRole", json!("body"));
    let one = eval_doc(
        &doc,
        fixture("square.svg"),
        ParamValues::new()
            .with("relief", json!("inlay"))
            .with("inlayStyle", json!("parts")),
    )
    .expect("builds");
    assert_eq!(one.objects.len(), 1);
    assert_eq!(shells(&one.objects[0].mesh), 1);
    assert!(
        one.warnings
            .iter()
            .any(|w| w.contains("without visible artwork")),
        "{:?}",
        one.warnings
    );
}

#[test]
fn emboss_stands_on_the_face_without_overlapping_the_body() {
    // The embossed artwork is its own object standing on the top face: its
    // bottom is the face plane, its footprint inside the face box, and the
    // kernel finds no volume it shares with the body. The same holds for the
    // swap inlay's slab, which touches the body in the split plane.
    let f = face();
    let h = param_default("reliefHeight");
    let b = tile(fixture("square.svg"), ParamValues::new()).expect("builds");
    let (body, accent) = (&b.objects[0], &b.objects[1]);
    // The same f32 height: the artwork starts where the body ends.
    let (lo, hi) = bounds(&accent.mesh);
    let (_, body_hi) = bounds(&body.mesh);
    assert_eq!(lo[2], body_hi[2], "the artwork starts on the face");
    assert!((lo[2] - f.h).abs() < 1e-6, "the face is the body top");
    assert!((hi[2] - (f.h + h)).abs() < 1e-6);
    for k in 0..2 {
        assert!(lo[k] >= f.box_min[k] - 1e-6 && hi[k] <= f.box_max[k] + 1e-6);
    }
    assert!(shared_volume(&body.mesh, &accent.mesh) < 1e-9);
    // Sunk 0.1 mm into the face it would share that layer: the check is live.
    let mut sunk = accent.mesh.clone();
    for v in &mut sunk.positions {
        v[2] -= 0.1;
    }
    let s = f.box_size()[0].min(f.box_size()[1]);
    let pushed = shared_volume(&body.mesh, &sunk);
    assert!(
        (pushed - opened_square_exact(s) * 0.1).abs() < 1e-3,
        "{pushed}"
    );
    // Halfway up the accent is the whole opened square.
    let slice = section_area(&accent.mesh, f.h + h / 2.0, |_| true);
    let square = opened_square_exact(s);
    assert!(
        ((slice - square) / square).abs() < 1e-6,
        "{slice} vs {square}"
    );
    // The colour-role warning says which is which and that it prints face up.
    let roles = b
        .warnings
        .iter()
        .find(|w| w.contains("colour roles"))
        .unwrap_or_else(|| panic!("{:?}", b.warnings));
    assert!(
        roles.contains("`body` (the body) and `accent` (the raised artwork of `art`)")
            && roles.contains("bottom face on the bed"),
        "{roles}"
    );

    let swap = tile(
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("inlay")),
    )
    .expect("builds");
    assert!(shared_volume(&swap.objects[0].mesh, &swap.objects[1].mesh) < 1e-9);
    let roles = swap
        .warnings
        .iter()
        .find(|w| w.contains("colour roles"))
        .unwrap_or_else(|| panic!("{:?}", swap.warnings));
    assert!(
        roles.contains("`accent` (the face slab around the artwork of `art`)")
            && roles.contains("top face (the artwork) on the bed"),
        "{roles}"
    );
    // A single-colour part has no colour-role warning.
    let doc = logo_tile_with_art_field("colorRole", json!("body"));
    let one = eval_doc(&doc, fixture("square.svg"), ParamValues::new()).expect("builds");
    assert!(one.warnings.iter().all(|w| !w.contains("colour roles")));
}

#[test]
fn the_feature_volume_sum_counts_the_roof_overlap_once() {
    // Deboss and inlay extend the tubes, pins and ribs into the thickened roof,
    // so the union merges them through an overlap instead of a shared plane.
    // The feature volume sum takes that designed overlap off once, so rule
    // MESH.VOLUME compares like with like. Counted twice, it put large tiles
    // past the 5 % tolerance (checked below from the analytic overlap).
    let mut fit = openbrick_print::example_fit_profile();
    // 4.80 mm bores (a cell a user can record) and the example 6.65 mm tubes.
    fit.offsets_mm.tube_inner_diameter = 0.0;
    let p = fdm();
    let assets = MemoryAssets::new().with("art.svg", fixture("square.svg"));
    for (size, mode) in [([16, 16], "deboss"), ([2, 4], "inlay"), ([1, 4], "deboss")] {
        let params = ParamValues::new()
            .with("logo", json!("art.svg"))
            .with("size", json!(size))
            .with("relief", json!(mode));
        let start = std::time::Instant::now();
        let b = run_with_assets(&logo_tile(), &params, &p, Some(&fit), &assets, false)
            .unwrap_or_else(|e| panic!("{size:?} {mode}: {e}"));
        let m = &b.measurements;
        let dev = (m.volume_mm3 - m.feature_volume_sum_mm3).abs() / m.feature_volume_sum_mm3;
        eprintln!(
            "{size:?} {mode}: volume {:.3}, feature sum {:.3}, {dev:e}, {:?}",
            m.volume_mm3,
            m.feature_volume_sum_mm3,
            start.elapsed()
        );
        // Every solid touches or overlaps by design, and the plate-height
        // cut leaves no wall-to-roof chamfer for the ribs to overlap: the sum
        // is the union exactly, up to rounding.
        assert!(dev < 1e-7, "{size:?} {mode}: {dev:e}");
        let report = openbrick_part::validate(&b, &p, Some(&fit));
        assert!(
            report.findings.iter().all(|f| f.rule != "MESH.VOLUME"),
            "{size:?} {mode}: {:?}",
            report.findings
        );
        if size == [16, 16] {
            // The overlap from the plan's shapes: 15 x 15 tubes, regular
            // n-gon rings with their vertices on the circles, and 2 (16 + 16)
            // ribs minFeature wide, each reaching min(depth, roof / 2) into
            // the roof (225 x 64/2 sin(2 pi/64) (3.325^2 - 2.4^2) x 0.4 =
            // 1,495 mm3, plus 8 mm3 of ribs). Counted twice, the sum would
            // exceed the union by more than the rule's tolerance.
            let reach = m
                .relief_height_mm
                .expect("depth")
                .min(m.roof_mm.expect("roof") / 2.0);
            let ro = m.tube_outer_diameter_mm.expect("tubes") / 2.0;
            let ri = m.tube_inner_diameter_mm.expect("tubes") / 2.0;
            let tubes = 225.0 * ngon(segments(&p)) * (ro * ro - ri * ri) * reach;
            let ribs = 64.0 * p.min_feature_mm * m.rib_depth_mm.expect("ribs") * reach;
            let twice = 100.0 * (tubes + ribs) / (m.volume_mm3 + tubes + ribs);
            eprintln!("overlap {tubes:.1} + {ribs:.1} mm3: {twice:.2} % if counted twice");
            assert!(
                twice > openbrick_print::MESH_VOLUME_TOLERANCE_PCT,
                "{twice}"
            );
        }
    }
}

/// Two 4.9 x 10 unit blocks 0.2 units apart in a 10 x 10 viewBox: fitted 13.2
/// mm wide (1.32 mm per unit), so 6.468 mm wide blocks and a 0.264 mm gap.
fn two_blocks() -> Vec<u8> {
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
        <rect width="4.9" height="10"/><rect x="5.1" width="4.9" height="10"/>
    </svg>"#
        .to_vec()
}

fn feature_min(b: &Built) -> Vec<openbrick_print::Severity> {
    openbrick_part::validate(b, &fdm(), None)
        .findings
        .iter()
        .filter(|f| f.rule == "FEATURE.MIN")
        .map(|f| f.severity)
        .collect()
}

#[test]
fn thin_face_material_left_by_a_cut_is_reported() {
    // Deboss and inlay print the face around the cuts. The 0.264 mm wall
    // between the two blocks, under the 0.8 mm minFeature, is still built, but
    // it is shown with the removed regions and counted in removed_area_pct, so
    // rule FEATURE.MIN reports it. Emboss prints the blocks themselves (6.468
    // mm wide) and is unchanged.
    let f = face();
    let unit = f.box_size()[0] / 10.0;
    let (gap, height) = (0.2 * unit, 10.0 * unit);
    let r = r0();
    let [cx, cy] = f.centre();
    let emboss = tile(two_blocks(), ParamValues::new()).expect("builds");
    let e = emboss.artwork.as_ref().expect("report");
    let emboss_pct = emboss.measurements.removed_area_pct.expect("measured");
    eprintln!("emboss: {emboss_pct} %, warnings {:?}", emboss.warnings);
    assert_eq!(e.thin_face_area_mm2, 0.0);
    assert!(emboss_pct < 1.0, "only the corners: {emboss_pct}");
    assert!(feature_min(&emboss).is_empty());
    let inside = |art: &openbrick_part::eval::Artwork, p: [f64; 2]| {
        art.removed
            .iter()
            .map(|poly| winding(&poly.points, p))
            .sum::<i32>()
            != 0
    };
    assert!(!inside(e, [cx, cy]));

    for mode in ["deboss", "inlay"] {
        let b = tile(two_blocks(), ParamValues::new().with("relief", json!(mode))).expect(mode);
        let art = b.artwork.as_ref().expect("report");
        let m = &b.measurements;
        let pct = m.removed_area_pct.expect("measured");
        eprintln!(
            "{mode}: thin {} mm2, {pct} %, warnings {:?}",
            art.thin_face_area_mm2, b.warnings
        );
        // At least the wall between the straight sides of the blocks (their
        // corners are rounded by r), at most the wall over the blocks' whole
        // height plus, at each of its two ends, the mouth around the rounded
        // corners: within r of the wall on either side and within r of the
        // end, (gap + 2 r) x 2 r.
        let (lo, hi) = (
            gap * (height - 2.0 * r),
            gap * height + 2.0 * (gap + 2.0 * r) * (2.0 * r),
        );
        assert!(
            art.thin_face_area_mm2 >= lo && art.thin_face_area_mm2 <= hi,
            "{mode}: {} not in [{lo}, {hi}]",
            art.thin_face_area_mm2
        );
        // The stroke share is the emboss one; the thin wall comes on top.
        let thin_pct = 100.0 * art.thin_face_area_mm2 / art.area_before_mm2;
        assert!(
            (pct - (emboss_pct + thin_pct)).abs() < 1e-9,
            "{mode}: {pct}"
        );
        assert!(
            inside(art, [cx, cy]),
            "{mode}: the wall is red in the preview"
        );
        assert!(!inside(art, [cx + 2.0, cy]), "{mode}: the blocks are not");
        assert!(
            b.warnings.iter().any(|w| w.contains("between the recesses")
                && w.contains("FEATURE.MIN")
                && w.contains("maxRemovedAreaPct")),
            "{mode}: {:?}",
            b.warnings
        );
        assert_eq!(
            feature_min(&b),
            vec![openbrick_print::Severity::Warning],
            "{mode}"
        );
        // The geometry still has the wall: two recesses, not one.
        assert!(m.is_manifold);
    }

    // A rim at the body edge: the square fills the face box across, and with
    // no margin the rim to the long edges is only the 0.3 mm top chamfer (a
    // knife edge at the face), under minFeature on both sides.
    let fit = json!({ "margin": "0mm", "align": "center", "keepAspect": true });
    let doc = logo_tile_with_art_field("fit", fit);
    let rc = top_chamfer();
    let s = f.w - 2.0 * rc;
    let b = eval_doc(
        &doc,
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("deboss")),
    )
    .expect("builds");
    let art = b.artwork.as_ref().expect("report");
    eprintln!("rim: thin {} mm2", art.thin_face_area_mm2);
    // The two rims along the square's straight sides at least; at most the
    // rims over its whole side plus, at each of their four ends, the region
    // within r of the end and within rc + r of the edge.
    let (lo, hi) = (
        2.0 * rc * (s - 2.0 * r),
        2.0 * rc * s + 4.0 * (rc + r) * (2.0 * r),
    );
    assert!(
        art.thin_face_area_mm2 >= lo && art.thin_face_area_mm2 <= hi,
        "rim {} not in [{lo}, {hi}]",
        art.thin_face_area_mm2
    );
    assert!(inside(art, [rc / 2.0, cy]), "the rim is red");
    assert_eq!(feature_min(&b), vec![openbrick_print::Severity::Warning]);
    // With the tile's own 1 mm margin the rim is 1.3 mm: nothing to report.
    let b = tile(
        fixture("square.svg"),
        ParamValues::new().with("relief", json!("deboss")),
    )
    .expect("builds");
    let art = b.artwork.as_ref().expect("report");
    assert!(art.thin_face_area_mm2 < 1e-9, "{}", art.thin_face_area_mm2);
    let emboss = tile(fixture("square.svg"), ParamValues::new()).expect("builds");
    assert_eq!(
        b.measurements.removed_area_pct,
        emboss.measurements.removed_area_pct
    );
}

/// The logo tile with its top chamfer set to `round` mm (`None`: no `round`)
/// and the artwork fitted with a `margin` mm margin, filling the face box in
/// both directions (keepAspect false), so a square leaves a rim of margin plus
/// chamfer all around.
fn logo_tile_filling(round: Option<f64>, margin: f64) -> PartDoc {
    let mut doc: Value = serde_json::from_str(LOGO_TILE).expect("JSON");
    let body = &mut doc["features"][0];
    assert_eq!(body["id"], "body");
    match round {
        Some(c) => body["round"]["radius"] = json!(format!("{c}mm")),
        None => {
            body.as_object_mut().expect("object").remove("round");
        }
    }
    let art = &mut doc["features"][2];
    assert_eq!(art["id"], "art");
    art["fit"] = json!({ "margin": format!("{margin}mm"), "align": "center", "keepAspect": false });
    PartDoc::parse(&doc.to_string()).expect("parses")
}

/// The thin face area, removed_area_pct and FEATURE.MIN findings of `doc` with
/// `svg` in `mode`, each mode with reliefHeight `depth` mm when given.
fn thin_report(
    doc: &PartDoc,
    svg: Vec<u8>,
    mode: &str,
    depth: Option<f64>,
) -> (f64, f64, Vec<openbrick_print::Severity>, Built) {
    let mut params = ParamValues::new().with("relief", json!(mode));
    if let Some(d) = depth {
        params = params.with("reliefHeight", json!(d));
    }
    let b = eval_doc(doc, svg, params).unwrap_or_else(|e| panic!("{mode}: {e}"));
    let art = b.artwork.as_ref().expect("report");
    let pct = b.measurements.removed_area_pct.expect("measured");
    eprintln!(
        "{mode}: thin {} mm2, removed {pct} %, warnings {:?}",
        art.thin_face_area_mm2, b.warnings
    );
    (art.thin_face_area_mm2, pct, feature_min(&b), b)
}

/// Bounds on the thin area of a rim `rim` wide between a `w` x `d` outline and
/// a square that fills the face box, opened at radius `r`: at most the ring
/// between the two (the square's corners, which the stroke opening rounds, are
/// counted as removed artwork, not as face material); at least the ring less,
/// at each corner, its part within `rim + 2 r` of the outline's corner. The
/// rounded corners of the square leave the ring fatter there, and the opening
/// keeps that, but a point of a side strip farther than `rim + r` from the
/// corner lies within `rim - r` of both the outline and the square, so no
/// eroded point is there and nothing kept reaches farther than `rim + 2 r`.
fn rim_bounds(w: f64, d: f64, rim: f64, r: f64) -> (f64, f64) {
    let ring = w * d - (w - 2.0 * rim) * (d - 2.0 * rim);
    let l = rim + 2.0 * r;
    (ring - 4.0 * (l * l - (l - rim) * (l - rim)), ring)
}

#[test]
fn face_material_exactly_min_feature_wide_is_not_thin() {
    // "Thinner than minFeature" is strict: a rim of exactly minFeature (margin
    // plus top chamfer) is what the warning's hint asks for, so following it
    // clears the report. An inset by exactly half of it would leave nothing of
    // a strip that wide; the check opens at half of it less 1 um instead.
    // Before that, the 0.8 mm rim around this square counted 66.6 mm2, 15.6 %.
    let p = fdm();
    let mf = p.min_feature_mm;
    let rc = top_chamfer();
    let margin = mf - rc;
    assert_eq!(rc + margin, mf, "the rim is exactly minFeature");
    let emboss = tile(fixture("square.svg"), ParamValues::new()).expect("builds");
    let corners_only = emboss.measurements.removed_area_pct.expect("measured");
    for mode in ["deboss", "inlay"] {
        let (thin, pct, findings, b) = thin_report(
            &logo_tile_filling(Some(rc), margin),
            fixture("square.svg"),
            mode,
            None,
        );
        assert_eq!(thin, 0.0, "{mode}");
        assert!(findings.is_empty(), "{mode}: {findings:?}");
        assert!(
            b.warnings.iter().all(|w| !w.contains("maxRemovedAreaPct")),
            "{mode}: {:?}",
            b.warnings
        );
        // Only the opening's rounding of the square's corners is removed.
        assert!(pct < corners_only, "{mode}: {pct} vs {corners_only}");

        // A hundredth of a millimetre less and the rim is thin all along the
        // sides of the square.
        let less = margin - 0.01;
        let (thin, _, findings, b) = thin_report(
            &logo_tile_filling(Some(rc), less),
            fixture("square.svg"),
            mode,
            None,
        );
        let f = face();
        let bounds = rim_bounds(f.w, f.d, rc + less, r0());
        assert_between(&format!("{mode} rim"), thin, bounds);
        assert_eq!(findings, vec![openbrick_print::Severity::Warning], "{mode}");
        assert!(
            b.warnings.iter().any(|w| w.contains("at the body edge")),
            "{mode}: {:?}",
            b.warnings
        );
    }
    // Sharp top edges: the rim is the margin alone.
    let (thin, _, findings, _) = thin_report(
        &logo_tile_filling(None, mf),
        fixture("square.svg"),
        "deboss",
        None,
    );
    assert_eq!(thin, 0.0);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_rim_over_a_chamfer_deeper_than_the_cut_is_measured_at_the_cut() {
    // The material between a recess and the body edge is the margin wide at
    // the face and widens down the 45 degree chamfer, to margin plus depth at
    // the bottom of the cut when the chamfer is deeper than the cut. A 0.6 mm
    // chamfer with a 0.4 mm cut and a 0.3 mm margin leaves 0.7 mm, under
    // minFeature, through the whole cut (and at the bed for an inlay, whose
    // slab face is inset by the whole chamfer). Measured on the body outline it
    // read 0.9 mm and was not reported.
    let p = fdm();
    let mf = p.min_feature_mm;
    let (c, depth, margin) = (0.6, 0.4, 0.3);
    assert!(margin + depth < mf && mf < margin + c);
    let f = face();
    let r = r0();
    for mode in ["deboss", "inlay"] {
        let doc = logo_tile_filling(Some(c), margin);
        let (thin, _, findings, b) = thin_report(&doc, fixture("square.svg"), mode, Some(depth));
        assert_eq!(b.measurements.relief_height_mm, Some(depth), "{mode}");
        assert_eq!(b.artwork.as_ref().expect("report").face_chamfer_mm, c);
        // The rim between the outline at the bottom of the cut (inset by
        // c - depth) and the face box (inset by c + margin).
        let out = c - depth;
        let bounds = rim_bounds(f.w - 2.0 * out, f.d - 2.0 * out, margin + depth, r);
        assert_between(&format!("{mode} rim"), thin, bounds);
        assert_eq!(findings, vec![openbrick_print::Severity::Warning], "{mode}");

        // A margin of minFeature less the depth makes the rim exactly
        // minFeature at the bottom of the cut: not thin.
        let doc = logo_tile_filling(Some(c), mf - depth);
        let (thin, _, findings, _) = thin_report(&doc, fixture("square.svg"), mode, Some(depth));
        assert_eq!(thin, 0.0, "{mode}");
        assert!(findings.is_empty(), "{mode}: {findings:?}");
    }
}

/// A plus sign, its arms one third of its size: fitted 13.2 mm wide on the
/// tile, so 4.4 mm arms, and at least the 1.3 mm rim (margin plus chamfer) to
/// the body edge.
fn plus() -> Vec<u8> {
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 3 3">
        <path d="M1 0 H2 V1 H3 V2 H2 V3 H1 V2 H0 V1 H1 Z"/>
    </svg>"#
        .to_vec()
}

#[test]
fn rounded_corners_of_the_face_material_are_not_thin() {
    // The plus sign's four inside corners are sharp corners of the face
    // material around it, which the opening rounds (r^2 (1 - pi/4) each, plus
    // the chords: 0.187 mm2 in all), but no wall or rim is thinner than
    // minFeature: nothing is reported, and deboss and inlay remove what emboss
    // removes. The sample mark (its narrowest face material about 2.7 mm, the
    // rim 1.3 mm) reported 0.354 mm2 of corners before.
    let p = fdm();
    for (what, svg) in [("plus", plus()), ("mark", mark_svg())] {
        let emboss = tile(svg.clone(), ParamValues::new()).expect("builds");
        let e_pct = emboss.measurements.removed_area_pct.expect("measured");
        for mode in ["deboss", "inlay"] {
            let (thin, pct, _, b) = thin_report(&logo_tile(), svg.clone(), mode, None);
            assert_eq!(thin, 0.0, "{what} {mode}");
            assert_eq!(pct, e_pct, "{what} {mode}");
            assert!(
                b.warnings
                    .iter()
                    .all(|w| !w.contains("face material thinner")),
                "{what} {mode}: {:?}",
                b.warnings
            );
        }
    }
    // A sharp wedge of face material is thin near its tip and still counts:
    // two triangles whose facing sides meet at 30 degrees leave a wedge that
    // stays under minFeature for r cot(15 degrees) = 1.49 mm from its tip.
    let wedge = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
        <path d="M0 0 H5 L2.32 10 H0 Z"/><path d="M10 0 H5 L7.68 10 H10 Z"/>
    </svg>"#
        .to_vec();
    let (thin, _, _, _) = thin_report(&logo_tile(), wedge, "deboss", None);
    let r = p.min_feature_mm / 2.0;
    assert!(thin >= r * r, "the wedge's tip: {thin}");
}

#[test]
fn the_removed_area_warning_uses_the_rules_threshold() {
    // The evaluator's warning and rule FEATURE.MIN read the same profile value
    // with the same comparison: removed more than maxRemovedAreaPct. At exactly
    // the threshold neither reports; just under it both do; a profile without
    // the value never makes the removed area a trigger.
    let assets = MemoryAssets::new().with("art.svg", fixture("square.svg"));
    let params = ParamValues::new().with("logo", json!("art.svg"));
    let run_on = |q: &ProcessProfile| {
        let b = run_with_assets(&logo_tile(), &params, q, None, &assets, false).expect("builds");
        let warned = b.warnings.iter().any(|w| w.contains("maxRemovedAreaPct"));
        let rule = openbrick_part::validate(&b, q, None)
            .findings
            .iter()
            .any(|f| f.rule == "FEATURE.MIN");
        (
            b.measurements.removed_area_pct.expect("measured"),
            warned,
            rule,
        )
    };
    let mut q = fdm();
    let (pct, _, _) = run_on(&q);
    assert!(pct > 0.0);
    q.max_removed_area_pct = Some(pct);
    assert_eq!(run_on(&q), (pct, false, false), "at the threshold");
    q.max_removed_area_pct = Some(pct * (1.0 - 1e-6));
    let (_, warned, rule) = run_on(&q);
    assert!(warned && rule, "just over the threshold");
    q.max_removed_area_pct = None;
    assert_eq!(run_on(&q), (pct, false, false), "no threshold");
}

#[test]
fn hairline_is_reported_and_removed() {
    let f = face();
    let h = param_default("reliefHeight");
    let b = tile(fixture("hairline.svg"), ParamValues::new()).expect("builds");
    let accent = &b.objects[1];
    let m = &b.measurements;
    let pct = m.removed_area_pct.expect("measured");
    assert!(pct > 0.0, "{pct}");
    // Two separate blocks are left; no accent triangle crosses the middle of the
    // gap, where the hairline was.
    let [cx, cy] = f.centre();
    let crosses = |mesh: &Mesh| {
        mesh.indices.as_chunks::<3>().0.iter().any(|t| {
            let xs = t.map(|i| f64::from(mesh.positions[i as usize][0]));
            xs.iter().any(|&x| x < cx) && xs.iter().any(|&x| x > cx)
        })
    };
    assert!(!crosses(&accent.mesh), "the hairline is gone");
    assert_eq!(m.components, 3, "the body and two blocks");
    // The removed region covers the hairline, for the preview.
    let art = b.artwork.as_ref().expect("report");
    let inside = |p: [f64; 2]| {
        art.removed
            .iter()
            .map(|poly| winding(&poly.points, p))
            .sum::<i32>()
            != 0
    };
    assert!(inside([cx, cy]), "the hairline is in the removed region");
    assert!(!inside([cx, cy + 3.0]), "the empty gap beside it is not");
    // Blocks of 3 units at 1.32 mm per unit: about 3.96 mm wide.
    let unit = f.box_size()[0] / 10.0;
    let stroke = m.min_stroke_mm.expect("measured");
    assert!(
        (stroke - 3.0 * unit).abs() < 0.05,
        "blocks {} mm wide measure {stroke}",
        3.0 * unit
    );
    assert!(
        b.warnings.iter().any(|w| w.contains("removed")),
        "{:?}",
        b.warnings
    );

    // The same file without the opening keeps the hairline.
    let doc = logo_tile_with_art_field("minFeature", json!("0mm"));
    let b = eval_doc(&doc, fixture("hairline.svg"), ParamValues::new()).expect("builds");
    assert!(
        crosses(&b.objects[1].mesh),
        "without the opening the hairline stays"
    );
    assert_eq!(b.measurements.components, 2);
    assert!(b.objects[1].volume_mm3 > accent.volume_mm3 + 0.1 * h);
}

/// Winding number of `poly` around `p` (crossing count with orientation).
fn winding(poly: &[[f64; 2]], p: [f64; 2]) -> i32 {
    let mut w = 0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= p[1] && b[1] > p[1] && cross > 0.0 {
            w += 1;
        } else if a[1] > p[1] && b[1] <= p[1] && cross < 0.0 {
            w -= 1;
        }
    }
    w
}

#[test]
fn evenodd_and_nonzero_rings() {
    // Two circles of radius 40 and 20 units drawn the same way round, fitted
    // into the face box: 80 units across the narrower side.
    let f = face();
    let h = param_default("reliefHeight");
    let k = f.box_size()[0].min(f.box_size()[1]) / 80.0;
    let (big, small) = (40.0 * k, 20.0 * k);
    // Flattened points lie on the curves, so the area is short by at most the
    // tolerance times the perimeter (and usvg's arc-to-cubic error, far less).
    let slack = 2.0 * FLATTEN_TOLERANCE_MM * 2.0 * PI * (big + small);
    let ring = tile(fixture("ring-evenodd.svg"), ParamValues::new()).expect("builds");
    let disc = tile(fixture("ring-nonzero.svg"), ParamValues::new()).expect("builds");
    let area = |b: &Built| b.objects[1].volume_mm3 / h;
    let want_ring = PI * (big * big - small * small);
    let want_disc = PI * big * big;
    assert!(
        (area(&ring) - want_ring).abs() < slack,
        "{} vs {want_ring}",
        area(&ring)
    );
    assert!(
        (area(&disc) - want_disc).abs() < slack,
        "{} vs {want_disc}",
        area(&disc)
    );
    assert!(area(&disc) - area(&ring) > PI * small * small - 2.0 * slack);
    assert_eq!(ring.measurements.genus, 1, "evenodd: a ring");
    assert_eq!(disc.measurements.genus, 0, "nonzero: a disc");
    // A 3.3 mm wide ring is far above minFeature: the opening removes nothing
    // measurable.
    assert!(ring.measurements.removed_area_pct.expect("measured") < 0.01);
    let stroke = ring.measurements.min_stroke_mm.expect("measured");
    assert!(
        (stroke - (big - small)).abs() < 0.1,
        "ring width {} measures {stroke}",
        big - small
    );
}

#[test]
fn the_example_logo_tile_builds_with_its_own_artwork() {
    // parts/ndn.logo-tile.obpart.json with parts/assets/mark.svg, as shipped:
    // every op is built, nothing is skipped.
    let p = fdm();
    let assets = example_assets();
    let b = run_with_assets(&logo_tile(), &ParamValues::new(), &p, None, &assets, false)
        .expect("the example part builds with its default artwork");
    eprintln!("warnings: {:?}", b.warnings);
    assert!(
        b.warnings.iter().all(|w| !w.contains("skipped")),
        "{:?}",
        b.warnings
    );
    let roles: Vec<&str> = b.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(roles, ["body", "accent"]);
    let m = &b.measurements;
    assert!(m.is_manifold);
    let pct = m.removed_area_pct.expect("measured");
    assert!(pct > 0.0, "the mark's hairline is removed: {pct}");
    let stroke = m.min_stroke_mm.expect("measured");
    assert!(stroke >= p.min_feature_mm, "{stroke}");
    assert_eq!(m.relief_height_mm, Some(param_default("reliefHeight")));
    let art = b.artwork.as_ref().expect("report");
    assert!(!art.removed.is_empty());
    assert_eq!(art.strokes_ignored, 0);
    let fused = b.fused.as_ref().expect("fused");
    assert!(
        (fused.volume_mm3 - b.objects[0].volume_mm3 - b.objects[1].volume_mm3).abs()
            < 1e-6 * fused.volume_mm3
    );
    assert_eq!(shells(&fused.mesh), 1, "one solid, not two touching shells");

    // Independent checks of the fit and the opening, from the numbers in
    // parts/assets/mark.svg: a hexagon ring (outer vertices at y = 2 and 98,
    // vertical sides at x = 8.431 and 91.569; inner hexagon 12 units in), a
    // hub of radius 14 at (50, 50), two 20 x 8 spokes and a 1.5 unit hairline
    // from the ring to the hub. keepAspect fits the 83.138 x 96 unit box into
    // the face box: k mm per unit from the width.
    let f = face();
    let accent = &b.objects[1];
    let h = param_default("reliefHeight");
    let k = f.box_size()[0] / (91.569 - 8.431);
    assert!(96.0 * k <= f.box_size()[1], "the width limits the fit");
    let [cx, cy] = f.centre();
    let (lo, hi) = bounds(&accent.mesh);
    // The vertical sides are straight: the opening keeps them.
    assert!((lo[0] - (cx - 41.569 * k)).abs() < 1e-4, "x min {}", lo[0]);
    assert!((hi[0] - (cx + 41.569 * k)).abs() < 1e-4, "x max {}", hi[0]);
    // The 120 degree top and bottom vertices are rounded: the opening's arc
    // sits r (1 / sin 60 deg - 1) inside the vertex, its chords within the
    // flattening tolerance of the arc.
    let r = r0();
    let tip = r * (1.0 / 60f64.to_radians().sin() - 1.0);
    for (got, vertex) in [(lo[1], cy - 48.0 * k), (hi[1], cy + 48.0 * k)] {
        let inside = (got - vertex).abs();
        assert!(
            inside >= tip - 1e-4 && inside <= tip + FLATTEN_TOLERANCE_MM + 1e-4,
            "a vertex at y {vertex} is rounded to {got}"
        );
    }
    assert!((art.area_after_mm2 * h - accent.volume_mm3).abs() < 1e-6 * accent.volume_mm3);
    // The area before the opening: the two hexagons by the shoelace formula,
    // the hub, the spokes outside the ring and the hub, and the hairline
    // between them (closed-form integrals of the hub's circle).
    let shoelace = |p: &[[f64; 2]]| {
        (0..p.len())
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            .abs()
            / 2.0
    };
    let outer = [
        [50.0, 2.0],
        [91.569, 26.0],
        [91.569, 74.0],
        [50.0, 98.0],
        [8.431, 74.0],
        [8.431, 26.0],
    ];
    let inner = [
        [50.0, 14.0],
        [81.177, 32.0],
        [81.177, 68.0],
        [50.0, 86.0],
        [18.823, 68.0],
        [18.823, 32.0],
    ];
    let rh = 14.0;
    // The integral of sqrt(rh^2 - u^2) over [-a, a].
    let chord = |a: f64| a * (rh * rh - a * a).sqrt() + rh * rh * (a / rh).asin();
    // A spoke runs from the ring (x = 18.823) to x = 38, 4 units either side
    // of the hub's centre line; the hub covers sqrt(rh^2 - u^2) - 12 of it.
    let spoke = (38.0 - 18.823) * 8.0 - (chord(4.0) - 12.0 * 8.0);
    // The hairline, 0.75 either side of x = 50: from the ring's inner edge
    // (14 + |u| slope) down to the hub (50 - sqrt(rh^2 - u^2)).
    let slope = (32.0 - 14.0) / (81.177 - 50.0);
    let hairline = 36.0 * 1.5 - chord(0.75) - slope * 0.75 * 0.75;
    let units = shoelace(&outer) - shoelace(&inner) + PI * rh * rh + 2.0 * spoke + hairline;
    let analytic = units * k * k;
    // The hub is the only curve: its flattened points lie on usvg's cubics
    // (within 3e-4 r of the circle), chords at most the tolerance inside.
    let rk = rh * k;
    let (a_lo, a_hi) = (
        analytic - 2.0 * PI * rk * FLATTEN_TOLERANCE_MM,
        analytic + 2.0 * PI * rk * rk * 3e-4,
    );
    eprintln!("mark area {} vs analytic {analytic}", art.area_before_mm2);
    assert_between("mark area", art.area_before_mm2, (a_lo, a_hi));
    // The opening removed at least the whole hairline (0.198 mm wide).
    assert!(1.5 * k < p.min_feature_mm);
    let removed = art.area_before_mm2 - art.area_after_mm2;
    assert!(
        removed >= hairline * k * k - 1.5 * k * FLATTEN_TOLERANCE_MM,
        "removed {removed} mm2, hairline {} mm2",
        hairline * k * k
    );
    check_fingerprint("mark accent", &b.objects[1].mesh, 0x9505_1b0a_e141_ba4d);

    // Each relief mode, and both inlay styles, build the example part.
    for (mode, style) in [("deboss", "swap"), ("inlay", "swap"), ("inlay", "parts")] {
        let params = ParamValues::new()
            .with("relief", json!(mode))
            .with("inlayStyle", json!(style));
        let b = run_with_assets(&logo_tile(), &params, &p, None, &assets, false)
            .unwrap_or_else(|e| panic!("{mode} {style}: {e}"));
        assert!(b.measurements.is_manifold, "{mode} {style}");
        assert_eq!(b.objects.len(), if mode == "deboss" { 1 } else { 2 });
        if style == "parts" {
            // The mark's fill and the recessed body touch, never overlap.
            let (body, fill) = (&b.objects[0].mesh, &b.objects[1].mesh);
            assert!(shared_volume(body, fill) < 1e-9, "{mode} {style}");
        }
    }
    // relief.text is still not built.
    let keychain = PartDoc::parse(include_str!(
        "../../../parts/ndn.nameplate-keychain.obpart.json"
    ))
    .expect("parses");
    match run(&keychain, &ParamValues::new(), &p, None, false) {
        Err(EvalError::Unsupported { op, .. }) => assert_eq!(op, "relief.text"),
        other => panic!(
            "expected relief.text to be unsupported, got {:?}",
            other.err()
        ),
    }
}

#[test]
fn the_content_hash_covers_the_artwork_bytes() {
    let p = fdm();
    let hash = |svg: Vec<u8>| {
        let assets = MemoryAssets::new().with("assets/mark.svg", svg);
        run_with_assets(&logo_tile(), &ParamValues::new(), &p, None, &assets, false)
            .expect("builds")
            .content_hash
    };
    let mark = hash(mark_svg());
    assert_eq!(mark, hash(mark_svg()), "deterministic");
    // Another logo under the same asset name is another input.
    assert_ne!(mark, hash(fixture("square.svg")));
    // So is one changed byte that changes no geometry (a comment).
    let mut touched = mark_svg();
    touched.extend_from_slice(b"<!-- -->\n");
    assert_ne!(mark, hash(touched));
}

fn plate_with(features: &str) -> PartDoc {
    PartDoc::parse(&format!(
        r#"{{ "openbrick": "1.0", "id": "t.t", "version": "0.1.0", "name": "t",
              "license": "CC0-1.0", "credit": {{ "author": "t" }},
              "features": [ {{ "id": "body", "op": "body.plate", "size": [2, 4] }}, {features} ] }}"#
    ))
    .expect("parses")
}

fn fails(doc: &PartDoc) -> EvalError {
    let assets = MemoryAssets::new().with("art.svg", fixture("square.svg"));
    run_with_assets(doc, &ParamValues::new(), &fdm(), None, &assets, false).expect_err("must fail")
}

#[test]
fn artwork_and_studs_cannot_share_a_face() {
    let art = r#"{ "id": "art", "op": "relief.svg", "on": "body.top", "src": "art.svg", "height": "0.6mm" }"#;
    let studs = r#"{ "id": "studs", "op": "stud.field", "on": "body.top" }"#;
    for order in [format!("{studs}, {art}"), format!("{art}, {studs}")] {
        let e = fails(&plate_with(&order));
        assert!(
            matches!(&e, EvalError::Feature { feature, .. } if feature == "art"),
            "{e}"
        );
        let text = e.to_string();
        assert!(
            text.contains("cannot share a face with studs") && text.contains("body.tile"),
            "{text}"
        );
    }
    // Without the studs the plate takes the artwork.
    let assets = MemoryAssets::new().with("art.svg", fixture("square.svg"));
    run_with_assets(
        &plate_with(art),
        &ParamValues::new(),
        &fdm(),
        None,
        &assets,
        false,
    )
    .expect("a plate without studs takes artwork");
    // Other faces, a second relief, a bad mode and a missing height.
    let e = fails(&plate_with(
        r#"{ "id": "art", "op": "relief.svg", "on": "body.bottom", "src": "art.svg", "height": "0.6mm" }"#,
    ));
    assert!(
        matches!(&e, EvalError::Unsupported { detail, .. } if detail.contains("only `top`")),
        "{e}"
    );
    let e = fails(&plate_with(&format!(
        r#"{art}, {{ "id": "art2", "op": "relief.svg", "src": "art.svg", "height": "0.6mm" }}"#
    )));
    assert!(
        e.to_string().contains("already has the relief `art`"),
        "{e}"
    );
    let e = fails(&plate_with(
        r#"{ "id": "art", "op": "relief.svg", "src": "art.svg", "height": "0.6mm", "mode": "engrave" }"#,
    ));
    assert!(
        matches!(&e, EvalError::Field { field, .. } if field == "mode"),
        "{e}"
    );
    let e = fails(&plate_with(
        r#"{ "id": "art", "op": "relief.svg", "src": "art.svg" }"#,
    ));
    assert!(
        matches!(&e, EvalError::Field { field, .. } if field == "height"),
        "{e}"
    );
    let e = fails(&plate_with(
        r#"{ "id": "art", "op": "relief.svg", "src": "art.svg", "height": "0.6mm", "fit": { "margin": "9mm" } }"#,
    ));
    assert!(e.to_string().contains("lower the margin"), "{e}");
}

#[test]
fn artwork_thinner_than_min_feature_everywhere_is_an_error() {
    // A 100 x 1 bar fitted 13.2 mm wide is 0.132 mm tall: the opening removes
    // everything, and an empty relief is reported as an error, not built.
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 1"><rect width="100" height="1"/></svg>"#;
    let e = tile(svg.to_vec(), ParamValues::new()).expect_err("nothing survives");
    assert!(
        matches!(&e, EvalError::Feature { feature, .. } if feature == "art"),
        "{e}"
    );
    assert!(e.to_string().contains("removed all of it"), "{e}");
}

#[test]
fn strokes_text_and_images_are_ignored_with_warnings() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
        <rect width="10" height="10" stroke="red" stroke-width="2"/>
        <path d="M0 0 L10 10" stroke="black" fill="none"/>
    </svg>"#;
    let b = tile(svg.to_vec(), ParamValues::new()).expect("builds");
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("2 strokes") && w.contains("v1.1")),
        "{:?}",
        b.warnings
    );
    assert_eq!(b.artwork.as_ref().expect("report").strokes_ignored, 2);
    // Only the fill counts: the square, fitted, not its stroke.
    let s = face().box_size()[0];
    assert!((b.artwork.as_ref().unwrap().area_before_mm2 - s * s).abs() < 1e-6);

    let b = tile(fixture("text.svg"), ParamValues::new()).expect("builds");
    assert!(
        b.warnings.iter().any(|w| w.contains("1 <text> element ")),
        "{:?}",
        b.warnings
    );
    let b = tile(fixture("image.svg"), ParamValues::new()).expect("builds");
    assert!(
        b.warnings.iter().any(|w| w.contains("3 <image> elements")),
        "{:?}",
        b.warnings
    );
    // Nothing from the images was drawn: the relief is just the square.
    assert!((b.artwork.as_ref().unwrap().area_before_mm2 - s * s).abs() < 1e-6);
}

#[test]
fn hostile_and_malformed_svgs_fail_cleanly() {
    let asset_error = |svg: Vec<u8>, want: &str| match tile(svg, ParamValues::new()) {
        Err(EvalError::Asset { name, message }) => {
            assert_eq!(name, "art.svg");
            assert!(message.contains(want), "expected `{want}` in: {message}");
        }
        Err(other) => panic!("expected an asset error with `{want}`, got {other}"),
        Ok(_) => panic!("expected an asset error with `{want}`, got a part"),
    };
    asset_error(fixture("unclosed.svg"), "not well-formed XML");
    asset_error(fixture("zero-viewbox.svg"), "no valid size");
    asset_error(
        fixture("deep-groups.svg"),
        &format!("more than {MAX_SVG_DEPTH} levels"),
    );
    asset_error(fixture("use-bomb.svg"), "expands to more than");
    asset_error(fixture("entity-bomb.svg"), "declares XML entities");
    asset_error(fixture("marker.svg"), "<marker>");
    asset_error(fixture("not-svg.svg"), "root element is <html>");
    asset_error(Vec::new(), "not well-formed XML");
    asset_error(vec![0xff, 0xfe, 0x00, 0x3c], "not UTF-8");
    asset_error(vec![0x1f, 0x8b, 0x08, 0x00], "compressed SVG");
    let mut big = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1"/><!--"#.to_vec();
    big.resize(MAX_SVG_BYTES + 1, b' ');
    big.extend_from_slice(b"--></svg>");
    asset_error(big, "the limit is");
    // NaN and overflowing numbers: usvg drops every shape and attribute it
    // cannot read. What is left is the 1 x 1 square whose `scale(NaN)`
    // transform was dropped, so the part builds with that square fitted.
    let b = tile(fixture("nan-numbers.svg"), ParamValues::new()).expect("builds");
    let s = face().box_size()[0];
    assert!((b.artwork.as_ref().unwrap().area_before_mm2 - s * s).abs() < 1e-6);
    assert!(b.measurements.is_manifold && b.measurements.volume_mm3.is_finite());

    // A viewBox 1e30 wide still fits f32: the square builds, fitted as usual.
    let b = tile(fixture("huge-viewbox.svg"), ParamValues::new()).expect("builds");
    assert!((b.artwork.as_ref().unwrap().area_before_mm2 - s * s).abs() < 1e-3 * s * s);
    // One that overflows f32 (1e39 becomes infinity) leaves a square usvg
    // drops as invalid: a clear error, never a panic or a NaN mesh.
    asset_error(fixture("overflow-viewbox.svg"), "no filled shapes");
}

#[test]
fn relief_objects_export_as_closed_meshes() {
    // The 3MF writer refuses any object that is not a closed, consistently
    // oriented mesh, and expects objects that touch but do not overlap
    // (docs/plan/05 section 7): the body and the embossed relief pass, and so
    // does the fused single-colour object, one shell for a single-material STL.
    let b = tile(fixture("square.svg"), ParamValues::new()).expect("builds");
    let meta = ExportMeta {
        part_id: "ndn.logo-tile".into(),
        version: "1.2.0".into(),
        content_hash: b.content_hash.clone(),
        fit_profile_id: "nominal".into(),
        process_id: "fdm-0.4".into(),
        generator: GENERATOR.into(),
        kernel_id: b.kernel_id.clone(),
        title: "Logo tile".into(),
        serial: None,
    };
    let objects: Vec<ExportObject> = b
        .objects
        .iter()
        .map(|o| ExportObject::new(o.role.clone(), o.role.clone(), None, &o.mesh))
        .collect();
    let two = write_3mf(&objects, &meta).expect("body and accent are closed meshes");
    let fused = b.fused.as_ref().expect("fused");
    let one = [ExportObject::new("logo tile", "body", None, &fused.mesh)];
    let single = write_3mf(&one, &meta).expect("the fused object is one closed mesh");
    let stl = write_stl_binary(&one).expect("STL");
    assert_eq!(stl.len(), 84 + 50 * fused.mesh.triangle_count());
    // Optional: write the files for an external check (scripts/validate-3mf.py).
    if let Ok(dir) = std::env::var("OPENBRICK_RELIEF_EXPORT_DIR") {
        let dir = Path::new(&dir);
        std::fs::write(dir.join("logo-tile-two-colour.3mf"), &two).expect("write");
        std::fs::write(dir.join("logo-tile-fused.3mf"), &single).expect("write");
        std::fs::write(dir.join("logo-tile-fused.stl"), &stl).expect("write");
    }
}

/// Display colours for the exports, one per role (any distinct pair works).
const BODY_COLOUR: [u8; 4] = [0xD0, 0x30, 0x20, 0xFF];
const ACCENT_COLOUR: [u8; 4] = [0xF2, 0xCD, 0x37, 0xFF];

#[test]
fn multi_material_exports_are_one_object_with_parts() {
    // The two-colour logo tiles in the part-designer 3MF layout: one mesh
    // object per colour role, one components object referencing both with no
    // transform, one build item. So a slicer that reads components as parts
    // (Bambu Studio) loads one object with a body part and a logo part in
    // place, and PrusaSlicer, which splits components, offers to merge them
    // into one multi-part object (docs/spikes/q18-slicer-multimaterial-import.md).
    // scripts/validate-3mf.py reads the written files with lib3mf: the parts
    // keep their place and do not overlap.
    let layout = ExportLayout::PART_DESIGNER;
    assert_eq!(layout, ExportLayout::OneObjectWithParts);
    let p = fdm();
    let builds = [
        (
            "logo-tile-emboss-parts",
            tile(fixture("square.svg"), ParamValues::new()),
        ),
        (
            "logo-tile-inlay-parts",
            parts_inlay(fixture("square.svg"), ParamValues::new()),
        ),
        (
            "logo-tile-mark-inlay-parts",
            run_with_assets(
                &logo_tile(),
                &ParamValues::new()
                    .with("relief", json!("inlay"))
                    .with("inlayStyle", json!("parts")),
                &p,
                None,
                &example_assets(),
                false,
            ),
        ),
    ];
    for (name, b) in builds {
        let b = b.unwrap_or_else(|e| panic!("{name}: {e}"));
        let meta = ExportMeta {
            part_id: "ndn.logo-tile".into(),
            version: logo_tile().version.clone(),
            content_hash: b.content_hash.clone(),
            fit_profile_id: "nominal".into(),
            process_id: p.id.clone(),
            generator: GENERATOR.into(),
            kernel_id: b.kernel_id.clone(),
            title: format!("Logo tile ({name})"),
            serial: None,
        };
        let colour = |role: &str| {
            Some(if role == "body" {
                BODY_COLOUR
            } else {
                ACCENT_COLOUR
            })
        };
        let objects: Vec<ExportObject> = b
            .objects
            .iter()
            .map(|o| {
                ExportObject::new(
                    format!("logo tile {}", o.role),
                    o.role.clone(),
                    colour(&o.role),
                    &o.mesh,
                )
            })
            .collect();
        assert_eq!(objects.len(), 2, "{name}");
        let bytes = write_3mf_with_layout(&objects, &meta, layout)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // Deterministic: the same build writes the same bytes, and the other
        // layout writes another file.
        assert_eq!(
            bytes,
            write_3mf_with_layout(&objects, &meta, layout).unwrap()
        );
        assert_ne!(bytes, write_3mf(&objects, &meta).unwrap());
        let xml = threemf_model_xml_with_layout(&objects, &meta, layout).expect("model");
        assert_eq!(xml.matches("<item ").count(), 1, "{name}");
        assert_eq!(xml.matches("<component ").count(), 2, "{name}");
        assert_eq!(xml.matches("<components>").count(), 1, "{name}");
        assert!(
            !xml.contains("transform="),
            "{name}: the parts stay in place"
        );
        // The parts touch and do not overlap.
        assert!(
            shared_volume(&b.objects[0].mesh, &b.objects[1].mesh) < 1e-9,
            "{name}"
        );
        if let Ok(dir) = std::env::var("OPENBRICK_RELIEF_EXPORT_DIR") {
            let dir = Path::new(&dir);
            std::fs::write(dir.join(format!("{name}.3mf")), &bytes).expect("write");
        }
    }
}

/// The asset error `tile` returns for `svg`, and how long it took.
fn refused(svg: Vec<u8>) -> (String, std::time::Duration) {
    let start = std::time::Instant::now();
    match tile(svg, ParamValues::new()) {
        Err(EvalError::Asset { message, .. }) => (message, start.elapsed()),
        Err(other) => panic!("expected an asset error, got {other}"),
        Ok(_) => panic!("expected an asset error, got a part"),
    }
}

#[test]
fn reference_chains_cycles_and_css_bombs_are_refused() {
    // A chain of 100 clip paths: usvg recurses once per link; chains of about
    // 1000 links overflowed a 1 MiB stack, which aborts the process.
    let (e, _) = refused(fixture("clip-chain.svg"));
    assert!(
        e.contains(&format!("more than {MAX_SVG_DEPTH} levels deep")),
        "{e}"
    );
    // A cycle of three clip paths, which usvg's own repair misses.
    let (e, _) = refused(fixture("clip-cycle.svg"));
    assert!(e.contains("refers back"), "{e}");
    // A 1.4 KB stylesheet whose one selector backtracks through 30 ancestors:
    // usvg spent 44 s on it. Refused before usvg runs.
    let (e, took) = refused(fixture("css-backtrack.svg"));
    assert!(e.contains("selector steps"), "{e}");
    assert!(took.as_secs() < 10, "{took:?}");
    // One element with too many attributes: roxmltree checks each against the
    // earlier ones (40,000 took 2.4 s, growing with the square).
    let mut many = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" "#,
    );
    for i in 0..=MAX_SVG_ATTRIBUTES {
        many.push_str(&format!("a{i}=\"\" "));
    }
    many.push_str("/></svg>");
    let (e, _) = refused(many.into_bytes());
    assert!(e.contains("attributes"), "{e}");
}

/// The tile with an SVG that chains `<use>` (or patterns) `n` deep.
fn use_chain(n: usize) -> String {
    let mut s = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><defs>"#,
    );
    for i in 0..n {
        s.push_str(&format!(r##"<use id="g{i}" xlink:href="#g{}"/>"##, i + 1));
    }
    s.push_str(&format!(
        r##"<rect id="g{n}" width="10" height="10"/></defs><use xlink:href="#g0"/></svg>"##
    ));
    s
}

fn pattern_chain(n: usize) -> String {
    let mut s =
        String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs>"#);
    for i in 0..n {
        s.push_str(&format!(
            r##"<pattern id="p{i}" width="1" height="1" patternUnits="userSpaceOnUse"><rect width="1" height="1" fill="url(#p{})"/></pattern>"##,
            i + 1
        ));
    }
    s.push_str(&format!(
        r##"<pattern id="p{n}" width="1" height="1" patternUnits="userSpaceOnUse"><rect width="1" height="1"/></pattern></defs><rect width="10" height="10" fill="url(#p0)"/></svg>"##
    ));
    s
}

#[test]
fn the_longest_accepted_chains_evaluate_on_a_wasm_sized_stack() {
    // The whole evaluation, on a 1 MiB thread (the wasm32 default stack), with
    // the longest `<use>` and pattern chains the limit accepts: the two kinds
    // that take the most stack per level in usvg. This debug build's frames are
    // larger than a release build's.
    let worker = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            for (kind, make) in [
                ("use", use_chain as fn(usize) -> String),
                ("pattern", pattern_chain),
            ] {
                let n = (1..=2 * MAX_SVG_DEPTH)
                    .rev()
                    .find(|&n| tile(make(n).into_bytes(), ParamValues::new()).is_ok())
                    .unwrap_or_else(|| panic!("{kind}: no chain accepted"));
                eprintln!("{kind}: longest accepted chain {n}");
                assert!(n >= 20, "{kind}: {n}");
                let e = tile(make(n + 1).into_bytes(), ParamValues::new()).expect_err("refused");
                assert!(e.to_string().contains("levels deep"), "{e}");
            }
        })
        .expect("thread");
    worker.join().expect("no stack overflow on a 1 MiB stack");
}

#[test]
fn invisible_shapes_are_left_out_and_paints_counted() {
    // Only the visible 10 x 10 square counts, fitted to the face box; a shape
    // made invisible by its own or a group's opacity neither builds nor changes
    // the fit.
    let s = face().box_size()[0];
    for hidden in [
        r#"<rect y="-20" width="10" height="10" opacity="0"/>"#,
        r#"<g opacity="0"><rect y="-20" width="10" height="10"/></g>"#,
        r#"<g opacity="0.5"><g opacity="0"><rect y="-20" width="10" height="10"/></g></g>"#,
        r#"<rect y="-20" width="10" height="10" fill-opacity="0"/>"#,
        r#"<rect y="-20" width="10" height="10" visibility="hidden"/>"#,
        r#"<rect y="-20" width="10" height="10" display="none"/>"#,
    ] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/>{hidden}</svg>"#
        );
        let b = tile(svg.into_bytes(), ParamValues::new()).expect("builds");
        let area = b.artwork.as_ref().expect("report").area_before_mm2;
        assert!((area - s * s).abs() < 1e-6, "{hidden}: {area}");
        assert!(
            b.warnings.iter().all(|w| !w.contains("paints")),
            "{:?}",
            b.warnings
        );
    }
    // A half-transparent group still builds (it is visible).
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><g opacity="0.5"><rect width="10" height="10"/></g></svg>"#;
    let b = tile(svg.to_vec(), ParamValues::new()).expect("builds");
    assert!((b.artwork.as_ref().expect("report").area_before_mm2 - s * s).abs() < 1e-6);
    // Knockout artwork: a white disc painted over a black square is built as
    // the whole square (paint order is ignored), with a warning.
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="black"/><circle cx="5" cy="5" r="3" fill="white"/></svg>"#;
    let b = tile(svg.to_vec(), ParamValues::new()).expect("builds");
    assert!((b.artwork.as_ref().expect("report").area_before_mm2 - s * s).abs() < 1e-6);
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("2 different paints") && w.contains("paint order is ignored")),
        "{:?}",
        b.warnings
    );
}

/// One closed path through `points` (SVG units).
fn path_svg(points: &[[f64; 2]], view: [f64; 2]) -> Vec<u8> {
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}"><path d="M"#,
        view[0], view[1]
    );
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            s.push('L');
        }
        s.push_str(&format!("{:.4} {:.4}", p[0], p[1]));
    }
    s.push_str(r#"Z"/></svg>"#);
    s.into_bytes()
}

#[test]
fn outlines_too_complex_to_sweep_are_refused_quickly() {
    use std::f64::consts::TAU;
    // A star of long thin spikes, the radius alternating 40 and 20 units (the
    // reviewer's probe): 20,000 points took 178 s and 150,000 points did not
    // finish in 12 minutes, because Clipper2's sweep meets thousands of edges
    // on every scanline. Each is now refused in well under a second.
    let star = |n: usize| -> Vec<u8> {
        let points: Vec<[f64; 2]> = (0..n)
            .map(|i| {
                let a = i as f64 / n as f64 * TAU;
                let r = if i % 2 == 0 { 40.0 } else { 20.0 };
                [50.0 + r * a.cos(), 50.0 + r * a.sin()]
            })
            .collect();
        path_svg(&points, [100.0, 100.0])
    };
    let (e, took) = refused(star(20_000));
    assert!(
        e.contains("too complex") && e.contains(&format!("limit {MAX_ARTWORK_SWEEP}")),
        "{e}"
    );
    eprintln!("20,000-point star refused in {took:?}");
    assert!(took.as_secs() < 10, "{took:?}");
    let (e, took) = refused(star(150_000));
    assert!(
        e.contains(&format!("more than {MAX_ARTWORK_POINTS} points")),
        "{e}"
    );
    eprintln!("150,000-point star refused in {took:?}");
    assert!(took.as_secs() < 10, "{took:?}");

    // 1,000 lines that all cross each other between two heights: 1,995,003
    // crossings (the reviewer's sweep would add half a million output points).
    let mut zig = Vec::new();
    for i in 0..1000 {
        let t = f64::from(i) / 10.0;
        zig.push([t, 0.0]);
        zig.push([100.0 - t, 100.0]);
    }
    let (e, took) = refused(path_svg(&zig, [100.0, 100.0]));
    assert!(e.contains("too complex") && e.contains("crossings"), "{e}");
    assert!(took.as_secs() < 10, "{took:?}");

    // A comb of 4,000 teeth on a solid bar: its outline sweeps cheaply, but the
    // opening's inset folds every tooth (0.0017 mm wide, under minFeature) over
    // hundreds of neighbours: about 8.5 million crossings in Clipper2's raw
    // offset path. That one inset takes 1.7 s in a release build and grows
    // with the square of the teeth (by that law 12,000 teeth, still under the
    // point limit, would take about 15 s per offset), so it is refused first.
    let mut comb = vec![[0.0, 50.0], [0.0, 30.0]];
    for i in 0..4000 {
        let x = f64::from(i) * 0.025;
        comb.extend([[x, 30.0], [x, 0.0], [x + 0.0125, 0.0], [x + 0.0125, 30.0]]);
    }
    comb.extend([[100.0, 30.0], [100.0, 50.0]]);
    let (e, took) = refused(path_svg(&comb, [100.0, 50.0]));
    assert!(
        e.contains("too detailed for the") && e.contains("minFeature"),
        "{e}"
    );
    eprintln!("4,000-tooth comb refused in {took:?}");
    assert!(took.as_secs() < 10, "{took:?}");
}

/// Rule FEATURE.MIN blocks damaged artwork only when the artwork is
/// fit-critical (a label someone reads to calibrate); the evaluator tells the
/// validator which through `Measurements::fit_critical_artwork`.
#[test]
fn fit_critical_artwork_turns_feature_min_into_a_blocker() {
    let mark = include_bytes!("../../../parts/assets/mark.svg").to_vec();
    let fit = openbrick_print::example_fit_profile();
    let p = fdm();
    let check = |doc: &PartDoc| {
        let assets = MemoryAssets::new().with("art.svg", mark.clone());
        let params = ParamValues::default().with("logo", json!("art.svg"));
        let built = run_with_assets(doc, &params, &p, Some(&fit), &assets, false).expect("builds");
        let report = openbrick_part::validate(&built, &p, Some(&fit));
        let feature_min: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule == "FEATURE.MIN")
            .map(|f| f.severity)
            .collect();
        (built.measurements.fit_critical_artwork, feature_min)
    };

    // The sample mark loses 1.03 % of its area to the opening, above the 1 %
    // maxRemovedAreaPct of fdm-0.4, so FEATURE.MIN fires either way.
    let (critical, severities) = check(&logo_tile());
    assert!(!critical);
    assert_eq!(severities, vec![openbrick_print::Severity::Warning]);

    let (critical, severities) = check(&logo_tile_with_art_field("fitCritical", json!(true)));
    assert!(critical);
    assert_eq!(severities, vec![openbrick_print::Severity::Blocker]);
}
