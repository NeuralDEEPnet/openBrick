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
use openbrick_geom::{round_segments, Mesh};
use openbrick_io::{write_3mf, write_stl_binary, ExportMeta, ExportObject, GENERATOR};
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
    assert!(
        b.warnings.iter().any(|w| w.contains("face down")),
        "{:?}",
        b.warnings
    );

    // The slab is the top t of the tile (its 0.3 mm top chamfer lies inside it)
    // minus the square; the body is the thickened tile below it.
    let rc = top_chamfer();
    assert!(rc <= t);
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
    check_fingerprint("mark accent", &b.objects[1].mesh, 0x9505_1b0a_e141_ba4d);

    // Each relief mode builds the example part.
    for mode in ["deboss", "inlay"] {
        let params = ParamValues::new().with("relief", json!(mode));
        let b = run_with_assets(&logo_tile(), &params, &p, None, &assets, false)
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        assert!(b.measurements.is_manifold, "{mode}");
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
