//! The calibration coupon (docs/plan/05 section 4; backlog item 11 in
//! docs/plan/14) on the nominal fdm-0.4 process with the default options.
//!
//! Checked: one watertight, manifold, genus-0 body; the exact polyhedral volume
//! from an analytic model written here from the plan's rules (base slab with its
//! first-layer chamfer, n-gon studs and tubes, pocket walls and ribs, and the
//! labels' pixels counted from an independent table of the font); the bounding
//! box; cell values against the table in docs/plan/05 section 4 (read from the
//! document itself, so the generator's data and the plan cannot drift apart);
//! two identical copies; whole-LDU cell centres; one label per cell; stud,
//! bore, pocket tube and rib sizes measured on slices of the mesh; the print
//! estimate and the bed; and the canonical mesh fingerprint, recorded for
//! `manifold-rust 0.19.1` and asserted on x86-64 Linux only, as in
//! `eval_golden.rs`. If the fingerprint changes on purpose, bump
//! `COUPON_REVISION` and record the new value.

mod common;

use std::sync::OnceLock;

use common::*;
use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::Mesh;
use openbrick_part::coupon::{
    build_coupon, coupon_layout, Coupon, CouponCellLayout, CouponOptions, PocketSet,
    COUPON_REVISION,
};
use openbrick_part::EvalError;
use openbrick_print::ProcessProfile;
use openbrick_units::ldu_to_mm;

/// Canonical fingerprint of the default fdm-0.4 coupon on manifold-rust 0.19.1.
const CANONICAL_FDM_04: u64 = 0x9f39_5ccc_8e5e_fea2;

const VOLUME_TOL: f64 = 1e-6;
const BOUNDS_TOL: f64 = 1e-4;

fn coupon() -> &'static Coupon {
    static C: OnceLock<Coupon> = OnceLock::new();
    C.get_or_init(|| {
        build_coupon(
            &ManifoldRust,
            &process("fdm-0.4"),
            &CouponOptions::default(),
        )
        .expect("the default coupon builds")
    })
}

fn mesh() -> &'static Mesh {
    &coupon().built.objects[0].mesh
}

// --- The plan table, read from the document --------------------------------

const PLAN_05: &str = include_str!("../../../docs/plan/05-3d-printing-pipeline.md");

/// Numbers in a table cell such as "OD 4.60 to 4.95 mm".
fn numbers(cell: &str) -> Vec<f64> {
    cell.split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

/// The sweep cell and step of the row of docs/plan/05 section 4 whose first
/// column starts with `row`.
fn plan_row(row: &str) -> (Vec<f64>, f64) {
    let line = PLAN_05
        .lines()
        .find(|l| l.starts_with(&format!("| {row}")))
        .unwrap_or_else(|| panic!("docs/plan/05 has no table row `{row}`"));
    let cols: Vec<&str> = line.split('|').map(str::trim).collect();
    let step = numbers(cols[3]);
    assert_eq!(step.len(), 1, "{line}");
    (numbers(cols[2]), step[0])
}

fn sweep(start: f64, end: f64, step: f64) -> Vec<f64> {
    let n = ((end - start) / step).round() as usize;
    (0..=n).map(|i| start + i as f64 * step).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// --- An independent model of the coupon ------------------------------------

/// Width and filled pixels of every glyph of the label font, counted from the
/// drawing in the coupon's font module (`src/coupon/font.rs`).
const GLYPH_PIXELS: &[(char, usize, usize)] = &[
    ('0', 3, 12),
    ('1', 3, 8),
    ('2', 3, 11),
    ('3', 3, 10),
    ('4', 3, 9),
    ('5', 3, 11),
    ('6', 3, 12),
    ('7', 3, 7),
    ('8', 3, 13),
    ('9', 3, 12),
    ('.', 1, 1),
];

fn glyph(c: char) -> (usize, usize) {
    GLYPH_PIXELS
        .iter()
        .find(|g| g.0 == c)
        .map(|g| (g.1, g.2))
        .unwrap_or_else(|| panic!("no glyph {c:?}"))
}

/// Filled pixels and (width, height) in pixels of a label: one pixel between
/// glyphs and between lines, five rows per line.
fn label_pixels(label: &str) -> (usize, [usize; 2]) {
    let lines: Vec<&str> = label.split('\n').collect();
    let mut filled = 0;
    let mut width = 0;
    for l in &lines {
        let w: usize = l.chars().map(|c| glyph(c).0).sum::<usize>() + l.chars().count() - 1;
        width = width.max(w);
        filled += l.chars().map(|c| glyph(c).1).sum::<usize>();
    }
    (filled, [width, lines.len() * 6 - 1])
}

/// Half the side of the genuine piece a cell takes: a 1x1 for stud and bore
/// cells, a 2x2 for pockets (n studs at the module pitch minus the undersize).
fn reserve(cell: &CouponCellLayout) -> f64 {
    let studs = if cell.cross.is_some() { 2.0 } else { 1.0 };
    (studs * mm("module") - 2.0 * mm("undersize_per_side")) / 2.0
}

/// Whole lines of `wall_quantise_mm` nearest the standard side wall, at least
/// `minWallMm` (docs/plan/04 section 6), as the evaluator test model does.
fn quantised_wall(p: &ProcessProfile) -> f64 {
    let q = p.wall_quantise_mm;
    (mm("side_wall") / q)
        .round()
        .max((p.min_wall_mm / q - 1e-9).ceil())
        * q
}

/// The exact volume of the coupon: every part only touches the others, so the
/// union is the sum.
fn analytic_volume(c: &Coupon, p: &ProcessProfile) -> f64 {
    let s = &c.summary;
    let [w, d] = s.footprint_mm;
    let t = mm("roof").max(p.min_roof_mm);
    let depth = mm("plate_height") - t;
    let k = ngon(segments(p));
    let mut v = w * d * t - edge_chamfer(w, d, p.first_layer_chamfer_mm);
    let stroke = s.label_stroke_mm;
    for cell in &c.cells {
        v += label_pixels(&cell.label).0 as f64 * stroke * stroke * s.label_relief_mm;
        let x = cell.value_mm;
        match (cell.cell_id.as_str(), &cell.cross) {
            ("stud", None) => {
                let (r, h) = (x / 2.0, mm("stud_height"));
                let ch = p.stud_top_chamfer_mm.min(x / 4.0).min(h / 2.0);
                v += k * (r * r * (h - ch) + frustum(r, r - ch, ch));
            }
            ("tube", None) => {
                // docs/plan/05 section 3: the outer diameter grows with the bore.
                let od = mm("tube_outer_diameter").max(x + 2.0 * p.min_tube_wall_mm);
                v += k * ((od / 2.0).powi(2) - (x / 2.0).powi(2)) * depth;
            }
            ("pocketTubeOD", Some(rib)) => {
                let side = 2.0 * reserve(cell);
                let wall = quantised_wall(p);
                let inner = side - 2.0 * wall;
                let id = mm("tube_inner_diameter").min(x - 2.0 * p.min_tube_wall_mm);
                v += (side * side - inner * inner) * depth;
                // One rib per wall per stud centre line: 2 x 2 walls x 2 lines.
                v += 8.0 * p.min_feature_mm * rib.value_mm * depth;
                v += k * ((x / 2.0).powi(2) - (id / 2.0).powi(2)) * depth;
            }
            other => panic!("unexpected cell {other:?}"),
        }
    }
    v
}

// --- Tests -----------------------------------------------------------------

#[test]
fn default_coupon_is_one_manifold_body_with_the_analytic_volume() {
    let c = coupon();
    let p = process("fdm-0.4");
    let b = &c.built;
    let m = &b.measurements;
    let mesh = mesh();
    let expected = analytic_volume(c, &p);
    eprintln!(
        "coupon fdm-0.4: canonical 0x{:016x}, {} triangles, volume {:.3} mm3 (analytic {:.3}), {:.1} g PLA, {:.0} min, {:?} mm",
        mesh.canonical_fingerprint(),
        mesh.triangle_count(),
        m.volume_mm3,
        expected,
        c.estimate.mass_g_pla,
        c.estimate.time_s / 60.0,
        c.summary.footprint_mm
    );
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert_eq!(b.objects.len(), 1);
    assert_eq!(b.objects[0].role, "body");
    assert!(b.connectors.is_empty(), "the coupon is not a part");
    assert!(m.is_manifold);
    // Every cell, label and tube stands on the one base.
    assert_eq!(m.components, 1);
    // Bores, pockets and the holes in "0", "4", "6", "8", "9" are all blind.
    assert_eq!(m.genus, 0);
    let rel = ((m.volume_mm3 - expected) / expected).abs();
    assert!(
        rel < VOLUME_TOL,
        "volume {} vs analytic {expected} (relative {rel:e})",
        m.volume_mm3
    );
    assert!((m.feature_volume_sum_mm3 - m.volume_mm3).abs() / m.volume_mm3 < VOLUME_TOL);

    // Bounds: the base outline, and plate height (the bore tubes and pocket walls
    // stand a plate cavity tall on a roof-thick base).
    let [w, d] = c.summary.footprint_mm;
    let want_max = [w, d, mm("plate_height")];
    let axes = m.bounds_min_mm.iter().zip(&m.bounds_max_mm).zip(&want_max);
    for (k, ((lo, hi), want)) in axes.enumerate() {
        assert!(lo.abs() < BOUNDS_TOL, "min[{k}] {lo}");
        assert!((hi - want).abs() < BOUNDS_TOL, "max[{k}] {hi} vs {want}");
    }
    assert!((c.summary.height_mm - mm("plate_height")).abs() < 1e-12);
    assert_eq!(m.roof_mm, Some(mm("roof")));
    assert_eq!(m.min_stroke_mm, Some(p.min_feature_mm));
    assert_eq!(m.relief_height_mm, Some(c.summary.label_relief_mm));
    assert_eq!(m.min_body_wall_mm, Some(mm("side_wall")));
    assert!(m.min_tube_wall_mm.unwrap() >= p.min_tube_wall_mm);

    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        assert_eq!(
            mesh.canonical_fingerprint(),
            CANONICAL_FDM_04,
            "the coupon mesh changed (coupon revision {COUPON_REVISION})"
        );
    }
}

#[test]
fn cells_follow_the_plan_table_in_order() {
    let c = coupon();
    let p = process("fdm-0.4");
    let per_copy = c.summary.rows_per_copy;
    assert_eq!(per_copy, 4, "studs, bores and the two lines of the cross");
    let line = |row: usize| -> Vec<&CouponCellLayout> {
        let mut v: Vec<_> = c.cells.iter().filter(|x| x.row == row).collect();
        v.sort_by(|a, b| a.center_ldu[0].cmp(&b.center_ldu[0]));
        v
    };
    let values = |cells: &[&CouponCellLayout]| cells.iter().map(|x| x.value_mm).collect::<Vec<_>>();
    let same =
        |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| close(*x, *y));

    let (studs, stud_step) = plan_row("Studs (male)");
    let studs_line = line(0);
    assert!(studs_line
        .iter()
        .all(|x| x.cell_id == "stud" && x.cross.is_none()));
    assert!(same(
        &values(&studs_line),
        &sweep(studs[0], studs[1], stud_step)
    ));

    let (bores, bore_step) = plan_row("Tube bores");
    let bores_line = line(1);
    assert!(bores_line
        .iter()
        .all(|x| x.cell_id == "tube" && x.cross.is_none()));
    assert!(same(
        &values(&bores_line),
        &sweep(bores[0], bores[1], bore_step)
    ));

    let (pockets, pocket_step) = plan_row("2x2 pockets");
    let ods = sweep(pockets[0], pockets[1], pocket_step);
    let ribs = sweep(pockets[2], pockets[3], pocket_step);
    // The nominals: the sweep values nearest the standard tube OD and the rib a
    // part gets on fdm-0.4 (standard wall plus rib minus the 1.2 mm wall).
    let nearest = |v: &[f64], x: f64| {
        v.iter()
            .copied()
            .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()))
            .unwrap()
    };
    let od0 = nearest(&ods, mm("tube_outer_diameter"));
    let rib0 = nearest(
        &ribs,
        mm("side_wall") + mm("rib_depth") - quantised_wall(&p),
    );
    let od_line = line(2);
    assert!(od_line
        .iter()
        .all(|x| x.cell_id == "pocketTubeOD" && close(x.cross.as_ref().unwrap().value_mm, rib0)));
    assert!(same(&values(&od_line), &ods));
    let rib_line = line(3);
    assert!(rib_line.iter().all(|x| close(x.value_mm, od0)));
    let rib_values: Vec<f64> = rib_line
        .iter()
        .map(|x| {
            let cross = x.cross.as_ref().unwrap();
            assert_eq!(cross.cell_id, "pocketRib");
            cross.value_mm
        })
        .collect();
    assert!(same(&rib_values, &ribs));

    // Neighbouring cells differ by exactly the row step (docs/plan/04 section 9).
    for (vals, step) in [
        (values(&studs_line), stud_step),
        (values(&bores_line), bore_step),
        (values(&od_line), pocket_step),
        (rib_values, pocket_step),
    ] {
        for pair in vals.windows(2) {
            assert!(close(pair[1] - pair[0], step), "{pair:?}");
        }
    }
    // Lines run from the back of the coupon to the front, in plan order.
    for r in 1..per_copy {
        assert!(line(r)[0].center_ldu[1] < line(r - 1)[0].center_ldu[1]);
    }
    // Rows the coupon data adds later (Technic holes, pins, ...) may be listed
    // as not built yet; the four M1 sweeps never are.
    for id in ["stud", "tube", "pocketTubeOD", "pocketRib"] {
        assert!(!c.summary.unbuilt_cells.iter().any(|u| u == id), "{id}");
    }
}

#[test]
fn every_row_appears_twice() {
    let c = coupon();
    assert_eq!(c.summary.copies, 2);
    let per_copy = c.summary.rows_per_copy;
    let first: Vec<_> = c.cells.iter().filter(|x| x.copy == 0).collect();
    let second: Vec<_> = c.cells.iter().filter(|x| x.copy == 1).collect();
    assert_eq!(first.len(), second.len());
    assert_eq!(c.cells.len(), 2 * first.len());
    // Stacked: the second copy is the first moved by one whole-LDU offset.
    let dy = first[0].center_ldu[1] - second[0].center_ldu[1];
    assert!(dy > 0);
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(b.row, a.row + per_copy);
        assert_eq!(
            (&a.cell_id, a.value_mm, &a.cross, &a.label),
            (&b.cell_id, b.value_mm, &b.cross, &b.label)
        );
        assert_eq!(b.center_ldu, [a.center_ldu[0], a.center_ldu[1] - dy]);
    }
}

#[test]
fn cells_sit_on_whole_ldu_at_a_fixed_pitch() {
    let c = coupon();
    let s = &c.summary;
    assert_eq!(s.footprint_mm, s.footprint_ldu.map(ldu_to_mm));
    for cell in &c.cells {
        assert_eq!(cell.center_mm, cell.center_ldu.map(ldu_to_mm));
    }
    for row in 0..s.copies as usize * s.rows_per_copy {
        let mut xs: Vec<i32> = c
            .cells
            .iter()
            .filter(|x| x.row == row)
            .map(|x| x.center_ldu[0])
            .collect();
        xs.sort();
        let pitches: Vec<i32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            pitches.windows(2).all(|w| w[0] == w[1]),
            "row {row}: {pitches:?}"
        );
        // Neighbouring footprints keep at least one label stroke between them.
        let cell = c.cells.iter().find(|x| x.row == row).unwrap();
        assert!(ldu_to_mm(pitches[0]) >= 2.0 * reserve(cell) + s.label_stroke_mm);
    }
}

#[test]
fn every_cell_carries_its_own_label() {
    let c = coupon();
    let s = &c.summary;
    let stroke = s.label_stroke_mm;
    let z = s.base_mm + s.label_relief_mm / 2.0;
    let mut labelled = 0;
    for cell in &c.cells {
        // Values in mm with two decimals; a pocket's OD over its rib depth.
        let want = match &cell.cross {
            None => format!("{:.2}", cell.value_mm),
            Some(r) => format!("{:.2}\n{:.2}", cell.value_mm, r.value_mm),
        };
        assert_eq!(cell.label, want);
        // The label block: centred under the cell, one stroke below its footprint.
        let (pixels, [w, h]) = label_pixels(&cell.label);
        let [cx, cy] = cell.center_mm;
        let top = cy - reserve(cell) - stroke;
        let (x0, x1) = (cx - w as f64 * stroke / 2.0, cx + w as f64 * stroke / 2.0);
        let (y0, y1) = (top - h as f64 * stroke, top);
        let margin = stroke / 2.0;
        let area = section_area(mesh(), z, |q| {
            q[0] > x0 - margin && q[0] < x1 + margin && q[1] > y0 - margin && q[1] < y1 + margin
        });
        let expected = pixels as f64 * stroke * stroke;
        assert!(
            (area - expected).abs() < 1e-3,
            "label {:?} of row {}: area {area} vs {expected}",
            cell.label,
            cell.row
        );
        labelled += 1;
    }
    assert_eq!(labelled, c.cells.len(), "one label per cell");
}

#[test]
fn stud_cells_measure_their_value() {
    let c = coupon();
    let p = process("fdm-0.4");
    let n = segments(&p);
    // Between the label tops (base + relief) and the start of the stud-top chamfer.
    let z = c.summary.base_mm
        + (c.summary.label_relief_mm + mm("stud_height") - p.stud_top_chamfer_mm) / 2.0;
    // Tessellation: a stud is a regular 64-gon with its vertices on the nominal
    // circle (circumradius r). Its inscribed radius is r cos(pi / 64), so the
    // printed polygon is up to d (1 - cos(pi / 64)) = 4.95 x 0.0012045 =
    // 0.0060 mm narrower across its flats than across its corners; the circle of
    // equal area has diameter d sqrt(64 sin(2 pi / 64) / (2 pi)) = 0.999197 d,
    // 0.0040 mm under d at 4.95 mm. Both are well under half the 0.05 mm step,
    // so neighbouring cells keep their order whichever way a slicer reads them.
    let flats = (std::f64::consts::PI / f64::from(n)).cos();
    let equal_area = (ngon(n) / std::f64::consts::PI).sqrt();
    let studs: Vec<_> = c.cells.iter().filter(|x| x.cell_id == "stud").collect();
    assert_eq!(studs.len(), 16);
    for cell in studs {
        let d = cell.value_mm;
        let (r, inner) = ring_radii(mesh(), z, cell.center_mm, 0.0, reserve(cell), n);
        assert_eq!(inner, 0.0, "a stud is solid");
        // The circumradius from the slice's area equals the value to f32 precision.
        assert!((2.0 * r - d).abs() < 1e-4, "stud {d}: measured {}", 2.0 * r);
        // And the measured polygon lies between its inscribed and circumscribed circles.
        let d_eq = 2.0 * r * equal_area;
        assert!(d_eq <= d + 1e-4 && d_eq >= d * flats - 1e-4);
        assert!((d - d_eq) < 0.025, "stays within half a step");
        // Vertices at 0, 90, 180 and 270 degrees: the slice's extent is d.
        let (lo, hi) = slice_extent(mesh(), z, cell.center_mm, reserve(cell));
        assert!((hi[0] - lo[0] - d).abs() < 1e-4 && (hi[1] - lo[1] - d).abs() < 1e-4);
    }
}

/// Bounding box of the points where mesh edges cross the plane z, within
/// `reach` (max norm) of `centre`.
fn slice_extent(mesh: &Mesh, z: f64, centre: [f64; 2], reach: f64) -> ([f64; 2], [f64; 2]) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for tri in mesh.indices.as_chunks::<3>().0 {
        let p = tri.map(|i| mesh.positions[i as usize].map(f64::from));
        for e in 0..3 {
            let (a, b) = (p[e], p[(e + 1) % 3]);
            if (a[2] - z) * (b[2] - z) < 0.0 {
                let t = (z - a[2]) / (b[2] - a[2]);
                let q = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                if (q[0] - centre[0]).abs() < reach && (q[1] - centre[1]).abs() < reach {
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
            }
        }
    }
    (lo, hi)
}

#[test]
fn bore_and_pocket_cells_measure_their_values() {
    let c = coupon();
    let p = process("fdm-0.4");
    let n = segments(&p);
    let s = &c.summary;
    // Above the labels, inside the tubes and walls.
    let z = s.base_mm + (s.label_relief_mm + s.cell_depth_mm) / 2.0;
    let wall = quantised_wall(&p);
    for cell in &c.cells {
        let x = cell.value_mm;
        match cell.cell_id.as_str() {
            "tube" => {
                // The bore is the cell's value, never value plus a fit offset.
                let od = mm("tube_outer_diameter").max(x + 2.0 * p.min_tube_wall_mm);
                let split = (x + od) / 4.0;
                let (ro, ri) = ring_radii(mesh(), z, cell.center_mm, split, reserve(cell), n);
                assert!((2.0 * ri - x).abs() < 1e-4, "bore {x}: {}", 2.0 * ri);
                assert!(
                    (2.0 * ro - od).abs() < 1e-4,
                    "bore {x}: tube OD {}",
                    2.0 * ro
                );
            }
            "pocketTubeOD" => {
                let rib = cell.cross.as_ref().unwrap().value_mm;
                let id = mm("tube_inner_diameter").min(x - 2.0 * p.min_tube_wall_mm);
                // Half a module from the pocket centre is outside the tube
                // (radius at most 3.33 mm here) and inside the wall faces and
                // the rib tips (6.7 and at least 6.2 mm away).
                let reach = mm("module") / 2.0;
                let (ro, ri) = ring_radii(mesh(), z, cell.center_mm, (x + id) / 4.0, reach, n);
                assert!(
                    (2.0 * ro - x).abs() < 1e-4,
                    "pocket tube OD {x}: {}",
                    2.0 * ro
                );
                assert!((2.0 * ri - id).abs() < 1e-4);
                // The cavity loop (wall inner faces with the eight ribs): its area
                // gives the rib depth.
                let side = 2.0 * reserve(cell);
                let inner = side - 2.0 * wall;
                let [cx, cy] = cell.center_mm;
                let hole = -section_area(mesh(), z, |q| {
                    let (dx, dy) = ((q[0] - cx).abs(), (q[1] - cy).abs());
                    dx.max(dy) < side / 2.0 - wall / 2.0 && dx.hypot(dy) > reach
                });
                let measured = (inner * inner - hole) / (8.0 * p.min_feature_mm);
                assert!(
                    (measured - rib).abs() < 1e-4,
                    "pocket rib {rib}: measured {measured}"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn the_default_fits_a_180_bed_and_prints_in_under_two_hours() {
    let c = coupon();
    let p = process("fdm-0.4");
    let opts = CouponOptions::default();
    let s = &c.summary;
    let [bx, by] = opts.bed_mm.map(|b| b - 2.0 * opts.bed_margin_mm);
    assert!(
        s.footprint_mm[0] <= bx && s.footprint_mm[1] <= by,
        "{:?}",
        s.footprint_mm
    );
    // The estimate: volume over line width x thickest layer x the assumed speed.
    let e = &c.estimate;
    let layer = p.layer_height_mm.iter().copied().fold(0.0, f64::max);
    let flow = p.line_width_mm * layer * opts.estimate_speed_mm_s;
    assert!((e.flow_mm3_s - flow).abs() < 1e-12);
    assert!((e.time_s - e.volume_mm3 / flow).abs() < 1e-6);
    assert!((e.mass_g_pla - e.volume_mm3 / 1000.0 * 1.24).abs() < 1e-9);
    assert!(e.time_s < 2.0 * 3600.0, "{} min", e.time_s / 60.0);
    // The richer pocket sets do not fit the default bed twice: the default is
    // the largest set that does.
    for pockets in [PocketSet::CrossAndDiagonal, PocketSet::Full] {
        let o = CouponOptions {
            pockets,
            ..CouponOptions::default()
        };
        match coupon_layout(&p, &o) {
            Err(EvalError::Feature { feature, message }) => {
                assert_eq!(feature, "coupon");
                assert!(message.contains("SIZE.BED"), "{message}");
            }
            other => panic!("{pockets:?} should not fit 180 x 180 mm: {other:?}"),
        }
    }
}

#[test]
fn larger_pocket_sets_lay_out_on_larger_beds() {
    let p = process("fdm-0.4");
    let diagonal = coupon_layout(
        &p,
        &CouponOptions {
            pockets: PocketSet::CrossAndDiagonal,
            bed_mm: [250.0, 210.0],
            ..CouponOptions::default()
        },
    )
    .expect("fits a 250 x 210 mm bed");
    // The cross (6 + 7) plus the diagonal without the nominal pocket (5).
    assert_eq!(diagonal.cells.len(), 2 * (8 + 6 + 6 + 7 + 5));
    let diag: Vec<_> = diagonal.cells.iter().filter(|x| x.row == 4).collect();
    for x in &diag {
        let rib = x.cross.as_ref().unwrap().value_mm;
        // The i-th OD with the i-th rib depth: OD - 6.40 = rib - 0.20 here.
        assert!(close(
            x.value_mm - diag[0].value_mm,
            rib - diag[0].cross.as_ref().unwrap().value_mm
        ));
    }
    let full = coupon_layout(
        &p,
        &CouponOptions {
            pockets: PocketSet::Full,
            bed_mm: [256.0, 256.0],
            ..CouponOptions::default()
        },
    )
    .expect("fits a 256 x 256 mm bed side by side");
    assert_eq!(full.cells.len(), 2 * (8 + 6 + 6 * 7));
    assert_eq!(
        full.summary.arrangement,
        openbrick_part::coupon::Arrangement::SideBySide
    );
    // Every (OD, rib) pair exactly once per copy.
    let mut pairs: Vec<(i64, i64)> = full
        .cells
        .iter()
        .filter(|x| x.copy == 0 && x.cross.is_some())
        .map(|x| {
            let r = x.cross.as_ref().unwrap().value_mm;
            (
                (x.value_mm * 100.0).round() as i64,
                (r * 100.0).round() as i64,
            )
        })
        .collect();
    pairs.sort();
    pairs.dedup();
    assert_eq!(pairs.len(), 42);
}

#[test]
fn the_nominal_rib_follows_the_process_wall() {
    // fdm-0.2 quantises the 1.2 mm wall to five 0.22 mm lines (1.1 mm), so a
    // part's rib is 0.1 mm deeper (docs/plan/04 section 6) and the OD line of
    // the cross sits at that rib depth.
    let p = process("fdm-0.2");
    let l = coupon_layout(&p, &CouponOptions::default()).expect("fdm-0.2 coupon");
    let wall = quantised_wall(&p);
    assert!((wall - 5.0 * p.wall_quantise_mm).abs() < 1e-12);
    let rib = mm("side_wall") + mm("rib_depth") - wall;
    let od_line: Vec<_> = l.cells.iter().filter(|x| x.row == 2).collect();
    assert!(od_line
        .iter()
        .all(|x| (x.cross.as_ref().unwrap().value_mm - rib).abs() < 0.025 + 1e-9));
    // The label stroke stays the plan's 0.8 mm, above fdm-0.2's minimum feature.
    assert!(l.summary.label_stroke_mm >= p.min_feature_mm);
    assert_eq!(
        l.summary.label_stroke_mm,
        CouponOptions::default().label_stroke_mm
    );
}

#[test]
fn a_fit_profile_cannot_reach_the_coupon() {
    // build_coupon takes no fit profile, CouponOptions has no field for one (a
    // compile_fail doc test on CouponOptions shows it), and JSON options naming
    // one are refused rather than silently ignored.
    let fit = serde_json::to_value(openbrick_print::example_fit_profile()).unwrap();
    let mut opts = serde_json::to_value(CouponOptions::default()).unwrap();
    opts["fit"] = fit;
    let err = serde_json::from_value::<CouponOptions>(opts).unwrap_err();
    assert!(err.to_string().contains("unknown field `fit`"), "{err}");
    let m = &coupon().built.measurements;
    assert_eq!(m.fit_profile, None);
}

#[test]
fn layout_and_hash_are_deterministic_and_bad_options_are_refused() {
    let p = process("fdm-0.4");
    let a = coupon_layout(&p, &CouponOptions::default()).unwrap();
    assert_eq!(a.cells, coupon().cells);
    assert_eq!(a.summary, coupon().summary);
    assert_eq!(coupon().built.content_hash.len(), 64);
    let bad = [
        CouponOptions {
            copies: 0,
            ..CouponOptions::default()
        },
        CouponOptions {
            bed_mm: [0.0, 180.0],
            ..CouponOptions::default()
        },
        CouponOptions {
            label_stroke_mm: -1.0,
            ..CouponOptions::default()
        },
        CouponOptions {
            estimate_speed_mm_s: f64::NAN,
            ..CouponOptions::default()
        },
        // Resource and sanity guards: no overflow, a clear error instead.
        CouponOptions {
            copies: u32::MAX,
            ..CouponOptions::default()
        },
        CouponOptions {
            label_stroke_mm: 1e300,
            ..CouponOptions::default()
        },
        CouponOptions {
            bed_margin_mm: 1e300,
            ..CouponOptions::default()
        },
    ];
    for o in bad {
        assert!(
            matches!(coupon_layout(&p, &o), Err(EvalError::Feature { .. })),
            "{o:?}"
        );
    }
    // One copy fits easily and has half the cells.
    let one = coupon_layout(
        &p,
        &CouponOptions {
            copies: 1,
            ..CouponOptions::default()
        },
    )
    .unwrap();
    assert_eq!(2 * one.cells.len(), a.cells.len());
}

#[test]
fn the_fdm_0_2_coupon_matches_its_model_too() {
    // A different wall (five 0.22 mm lines), rib width, tube wall minimum,
    // first-layer chamfer and circle segments: the same model must hold.
    let p = process("fdm-0.2");
    let c = build_coupon(&ManifoldRust, &p, &CouponOptions::default()).expect("builds");
    let m = &c.built.measurements;
    assert!(m.is_manifold && m.components == 1 && m.genus == 0);
    let expected = analytic_volume(&c, &p);
    let rel = ((m.volume_mm3 - expected) / expected).abs();
    assert!(rel < VOLUME_TOL, "volume {} vs {expected}", m.volume_mm3);
    assert_eq!(m.min_body_wall_mm, Some(quantised_wall(&p)));
    assert_ne!(c.built.content_hash, coupon().built.content_hash);
}
