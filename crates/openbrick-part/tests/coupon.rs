//! The calibration coupon (docs/plan/05 section 4; backlog item 11 in
//! docs/plan/14) on the nominal fdm-0.4 process with the default options.
//!
//! Checked: one watertight, manifold, genus-0 body; the exact polyhedral volume
//! from an analytic model written here from the plan's rules (base slab with its
//! first-layer chamfer, n-gon studs and tubes, pocket walls and ribs, and the
//! labels' pixels counted from an independent table of the font); the bounding
//! box; cell values against the table in docs/plan/05 section 4 (read from the
//! document itself, so the generator's data and the plan cannot drift apart);
//! two identical copies; whole-LDU cell centres; one label per cell, counted
//! as separate blocks on a slice of the mesh, and none on a stud top; stud,
//! bore, pocket tube and rib sizes measured on slices of the mesh; that a part
//! built with the offsets the wizard records (chosen cell minus the standard
//! nominal) reproduces the chosen pocket on fdm-0.2, whose wall is quantised;
//! the print estimate and the bed; and the canonical mesh fingerprint, recorded
//! for `manifold-rust 0.19.1` and asserted on x86-64 Linux only, as in
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
use openbrick_print::{
    example_fit_profile, record, FitProfile, Offsets, ProcessProfile, RecordError, RecordInput,
};
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

/// The default coupon on fdm-0.2, whose 1.2 mm wall quantises to five 0.22 mm
/// lines (1.1 mm): the process where a rib measured against the quantised wall
/// instead of the standard one would show.
fn coupon_fdm_02() -> &'static Coupon {
    static C: OnceLock<Coupon> = OnceLock::new();
    C.get_or_init(|| {
        build_coupon(
            &ManifoldRust,
            &process("fdm-0.2"),
            &CouponOptions::default(),
        )
        .expect("the default fdm-0.2 coupon builds")
    })
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

/// The value of `v` nearest `x`.
fn nearest(v: &[f64], x: f64) -> f64 {
    v.iter()
        .copied()
        .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()))
        .unwrap()
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

/// The depth a pocket cell's ribs are built to on `p`. A rib value is measured
/// against the standard side wall, as the standard's rib_depth is, so the tip
/// sits side_wall + value from the outer face on every process; on a wall
/// quantised thinner the rib is deeper by the difference. This is the rib a
/// part gets with the recorded offset (value minus rib_depth), which is
/// (side_wall + rib_depth minus wall) + (value minus rib_depth) by docs/plan/04
/// section 6. On fdm-0.2: 1.2 - 1.1 + value = value + 0.1 mm.
fn built_rib(value: f64, p: &ProcessProfile) -> f64 {
    value + mm("side_wall") - quantised_wall(p)
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
                v += 8.0 * p.min_feature_mm * built_rib(rib.value_mm, p) * depth;
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
    // No tube wall for rule TUBE.WALL, whose fix is a fit-profile offset: the
    // coupon takes none. Every cell's wall is measured on the mesh instead, at
    // least minTubeWallMm (check_bore_and_pocket_cells).
    assert_eq!(
        (m.min_tube_wall_mm, m.min_tube_wall_at_bed_mm),
        (None, None)
    );
    // The labels are read to calibrate.
    assert!(m.fit_critical_artwork);

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
    // The nominals: the sweep values nearest the standard tube OD and rib depth.
    let od0 = nearest(&ods, mm("tube_outer_diameter"));
    let rib0 = nearest(&ribs, mm("rib_depth"));
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

/// Where the plane z cuts the mesh's triangles: one segment per cut triangle.
fn slice_segments(mesh: &Mesh, z: f64) -> Vec<[[f64; 2]; 2]> {
    let mut segs = Vec::new();
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
        if cut.len() == 2 {
            segs.push([cut[0], cut[1]]);
        }
    }
    segs
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// A bounding box: (min, max).
type Bounds = ([f64; 2], [f64; 2]);

const EMPTY: Bounds = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);

/// Grows `b` to take in the box from `lo` to `hi`.
fn grow(b: &mut Bounds, lo: [f64; 2], hi: [f64; 2]) {
    b.0 = [b.0[0].min(lo[0]), b.0[1].min(lo[1])];
    b.1 = [b.1[0].max(hi[0]), b.1[1].max(hi[1])];
}

/// End points on a grid, with the segment each belongs to.
type PointGrid = std::collections::HashMap<(i64, i64), Vec<(usize, [f64; 2])>>;

/// Groups the slice segments `keep` accepts (by midpoint) into blocks. First
/// the segments join into closed loops (one segment ends where the next
/// starts); then loops whose bounding boxes come within `link` mm of each other
/// (the gap between the boxes, horizontally and vertically) share a block.
/// Returns each block's bounding box.
fn slice_blocks(mesh: &Mesh, z: f64, keep: impl Fn([f64; 2]) -> bool, link: f64) -> Vec<Bounds> {
    let segs: Vec<_> = slice_segments(mesh, z)
        .into_iter()
        .filter(|[a, b]| keep([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]))
        .collect();
    // Loops: end points that coincide (both cut the same mesh edge; 1e-9 mm,
    // as in `section_area`) join their segments. A micrometre grid finds them.
    const CLOSE: f64 = 1e-9;
    let key = |q: [f64; 2]| ((q[0] * 1e6).round() as i64, (q[1] * 1e6).round() as i64);
    let mut parent: Vec<usize> = (0..segs.len()).collect();
    let mut at = PointGrid::new();
    for (i, seg) in segs.iter().enumerate() {
        for q in seg {
            let (kx, ky) = key(*q);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &(j, r) in at.get(&(kx + dx, ky + dy)).into_iter().flatten() {
                        if (q[0] - r[0]).abs() <= CLOSE && (q[1] - r[1]).abs() <= CLOSE {
                            let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                            parent[a] = b;
                        }
                    }
                }
            }
            at.entry((kx, ky)).or_default().push((i, *q));
        }
    }
    let mut loops: std::collections::BTreeMap<usize, Bounds> = std::collections::BTreeMap::new();
    for (i, seg) in segs.iter().enumerate() {
        let b = loops.entry(root(&mut parent, i)).or_insert(EMPTY);
        for q in seg {
            grow(b, *q, *q);
        }
    }
    // Blocks: loops whose boxes are within `link` of each other.
    let loops: Vec<Bounds> = loops.into_values().collect();
    let mut parent: Vec<usize> = (0..loops.len()).collect();
    for i in 0..loops.len() {
        for j in 0..i {
            let gap = |k: usize| {
                (loops[j].0[k] - loops[i].1[k])
                    .max(loops[i].0[k] - loops[j].1[k])
                    .max(0.0)
            };
            if gap(0).hypot(gap(1)) <= link {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut blocks: std::collections::BTreeMap<usize, Bounds> = std::collections::BTreeMap::new();
    for (i, (lo, hi)) in loops.iter().enumerate() {
        grow(
            blocks.entry(root(&mut parent, i)).or_insert(EMPTY),
            *lo,
            *hi,
        );
    }
    blocks.into_values().collect()
}

#[test]
fn every_cell_carries_its_own_label() {
    let c = coupon();
    let s = &c.summary;
    let stroke = s.label_stroke_mm;
    let z = s.base_mm + s.label_relief_mm / 2.0;
    // The label block of each cell: centred under the cell, one stroke below
    // its footprint.
    let boxes: Vec<Bounds> = c
        .cells
        .iter()
        .map(|cell| {
            let (_, [w, h]) = label_pixels(&cell.label);
            let [cx, cy] = cell.center_mm;
            let top = cy - reserve(cell) - stroke;
            let half = w as f64 * stroke / 2.0;
            ([cx - half, top - h as f64 * stroke], [cx + half, top])
        })
        .collect();
    for (cell, (lo, hi)) in c.cells.iter().zip(&boxes) {
        // Values in mm with two decimals; a pocket's OD over its rib depth.
        let want = match &cell.cross {
            None => format!("{:.2}", cell.value_mm),
            Some(r) => format!("{:.2}\n{:.2}", cell.value_mm, r.value_mm),
        };
        assert_eq!(cell.label, want);
        // The block holds exactly the label's pixels.
        let (pixels, _) = label_pixels(&cell.label);
        let margin = stroke / 2.0;
        let area = section_area(mesh(), z, |q| {
            q[0] > lo[0] - margin
                && q[0] < hi[0] + margin
                && q[1] > lo[1] - margin
                && q[1] < hi[1] + margin
        });
        let expected = pixels as f64 * stroke * stroke;
        assert!(
            (area - expected).abs() < 1e-3,
            "label {:?} of row {}: area {area} vs {expected}",
            cell.label,
            cell.row
        );
    }

    // Count the labels on the mesh itself: at half the relief, everything
    // outside the cells' clear footprints is label. The glyphs of one label are
    // one pixel apart, side by side and line over line (the boxes of '.' and
    // '7' too, though their pixels are farther apart), while neighbouring
    // labels keep a word space of three pixels and labels of neighbouring
    // lines a whole footprint. Joining glyph boxes up to two pixels apart
    // therefore makes each label one block and nothing more.
    let outside = |q: [f64; 2]| {
        c.cells.iter().all(|cell| {
            let [cx, cy] = cell.center_mm;
            (q[0] - cx).abs().max((q[1] - cy).abs()) > reserve(cell) + stroke / 2.0
        })
    };
    let blocks = slice_blocks(mesh(), z, outside, 2.0 * stroke);
    assert_eq!(blocks.len(), c.cells.len(), "one label block per cell");
    // Each block is exactly one cell's label box (every glyph fills its first
    // and last column and its top and bottom row, so the boxes are tight).
    for (cell, (lo, hi)) in c.cells.iter().zip(&boxes) {
        let matching = blocks
            .iter()
            .filter(|(blo, bhi)| {
                (0..2).all(|k| (blo[k] - lo[k]).abs() < 1e-4 && (bhi[k] - hi[k]).abs() < 1e-4)
            })
            .count();
        assert_eq!(matching, 1, "label {:?} of row {}", cell.label, cell.row);
    }

    // Never text on a stud top (CLAUDE.md): nothing in a stud cell's footprint
    // rises above the stud.
    let top = s.base_mm + mm("stud_height");
    for cell in c.cells.iter().filter(|x| x.cell_id == "stud") {
        let [cx, cy] = cell.center_mm;
        let highest = mesh()
            .positions
            .iter()
            .filter(|q| {
                (f64::from(q[0]) - cx)
                    .abs()
                    .max((f64::from(q[1]) - cy).abs())
                    < reserve(cell)
            })
            .map(|q| f64::from(q[2]))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (highest - top).abs() < 1e-4,
            "stud {}: {highest}",
            cell.value_mm
        );
    }
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
fn slice_extent(mesh: &Mesh, z: f64, centre: [f64; 2], reach: f64) -> Bounds {
    let mut b = EMPTY;
    for q in slice_segments(mesh, z).iter().flatten() {
        if (q[0] - centre[0]).abs() < reach && (q[1] - centre[1]).abs() < reach {
            grow(&mut b, *q, *q);
        }
    }
    b
}

/// How far the rib on the wall whose outer face is the plane x = `outer_x`
/// reaches from that face, at height z: the largest x - `outer_x` of the slice
/// points within `half_width` (plus f32 slack) of the rib's centre line y =
/// `line_y`, short of x = `x_max`. The rib's vertical tip edges cross the plane
/// exactly at its tip; the wall's inner face and the outer face are nearer.
fn rib_tip(mesh: &Mesh, z: f64, outer_x: f64, x_max: f64, line_y: f64, half_width: f64) -> f64 {
    slice_segments(mesh, z)
        .iter()
        .flatten()
        .filter(|q| {
            (q[1] - line_y).abs() <= half_width + 1e-4 && q[0] > outer_x - 1e-4 && q[0] < x_max
        })
        .map(|q| q[0] - outer_x)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Bore and pocket cells of `c`, built on `p`, measure their values on slices.
fn check_bore_and_pocket_cells(c: &Coupon, p: &ProcessProfile) {
    let mesh = &c.built.objects[0].mesh;
    let n = segments(p);
    let s = &c.summary;
    // Above the labels, inside the tubes and walls.
    let z = s.base_mm + (s.label_relief_mm + s.cell_depth_mm) / 2.0;
    let wall = quantised_wall(p);
    let (mut bores, mut pockets) = (0, 0);
    for cell in &c.cells {
        let x = cell.value_mm;
        match cell.cell_id.as_str() {
            "tube" => {
                bores += 1;
                // The bore is the cell's value, never value plus a fit offset.
                let od = mm("tube_outer_diameter").max(x + 2.0 * p.min_tube_wall_mm);
                let split = (x + od) / 4.0;
                let (ro, ri) = ring_radii(mesh, z, cell.center_mm, split, reserve(cell), n);
                assert!((2.0 * ri - x).abs() < 1e-4, "bore {x}: {}", 2.0 * ri);
                assert!(
                    (2.0 * ro - od).abs() < 1e-4,
                    "bore {x}: tube OD {}",
                    2.0 * ro
                );
                // The wall a part gets with this bore offset, thin on purpose
                // but never under the process minimum.
                assert!(
                    ro - ri >= p.min_tube_wall_mm - 1e-4,
                    "bore {x}: wall {}",
                    ro - ri
                );
            }
            "pocketTubeOD" => {
                pockets += 1;
                let rib = cell.cross.as_ref().unwrap().value_mm;
                let id = mm("tube_inner_diameter").min(x - 2.0 * p.min_tube_wall_mm);
                // Half a module from the pocket centre is outside the tube
                // (radius at most 3.33 mm here) and inside the wall faces and
                // the rib tips (at least 7.9 - 1.2 - 0.5 = 6.2 mm away).
                let reach = mm("module") / 2.0;
                let (ro, ri) = ring_radii(mesh, z, cell.center_mm, (x + id) / 4.0, reach, n);
                assert!(
                    (2.0 * ro - x).abs() < 1e-4,
                    "pocket tube OD {x}: {}",
                    2.0 * ro
                );
                assert!((2.0 * ri - id).abs() < 1e-4);
                assert!(
                    ro - ri >= p.min_tube_wall_mm - 1e-4,
                    "pocket tube OD {x}: wall {}",
                    ro - ri
                );
                // The cavity loop (wall inner faces with the eight ribs): its area
                // gives the depth the ribs were built to.
                let side = 2.0 * reserve(cell);
                let inner = side - 2.0 * wall;
                let [cx, cy] = cell.center_mm;
                let hole = -section_area(mesh, z, |q| {
                    let (dx, dy) = ((q[0] - cx).abs(), (q[1] - cy).abs());
                    dx.max(dy) < side / 2.0 - wall / 2.0 && dx.hypot(dy) > reach
                });
                let measured = (inner * inner - hole) / (8.0 * p.min_feature_mm);
                assert!(
                    (measured - built_rib(rib, p)).abs() < 1e-4,
                    "pocket rib {rib}: measured {measured}"
                );
                // And where the tip of the rib on the left wall, at the lower
                // stud centre line, sits: side_wall + the cell's value from the
                // outer face, as on a part (docs/plan/04 section 6).
                let tip = rib_tip(
                    mesh,
                    z,
                    cx - side / 2.0,
                    cx,
                    cy - mm("module") / 2.0,
                    p.min_feature_mm / 2.0,
                );
                assert!(
                    (tip - (mm("side_wall") + rib)).abs() < 1e-4,
                    "pocket rib {rib}: tip {tip} mm from the outer face"
                );
            }
            _ => {}
        }
    }
    assert!(bores > 0 && pockets > 0);
}

#[test]
fn bore_and_pocket_cells_measure_their_values() {
    check_bore_and_pocket_cells(coupon(), &process("fdm-0.4"));
}

#[test]
fn bore_and_pocket_cells_measure_their_values_on_a_quantised_wall() {
    // fdm-0.2: 1.1 mm walls, so every rib is built 0.1 mm deeper than its value
    // and its tip still sits side_wall + value from the outer face.
    let p = process("fdm-0.2");
    assert!((quantised_wall(&p) - mm("side_wall")).abs() > 0.05);
    check_bore_and_pocket_cells(coupon_fdm_02(), &p);
}

#[test]
fn a_part_built_with_the_recorded_offsets_matches_its_pocket_cell() {
    // The wizard and `openbrick calibrate record` write offset = chosen cell
    // minus the standard nominal of the cell's standardId (docs/plan/05 section
    // 4). A part built with those offsets must reproduce the chosen pocket: the
    // same tube OD and the same rib tip, measured from the outer face. fdm-0.2
    // is the case that matters: its wall quantises to 1.1 mm, so a cell whose
    // rib were the literal value on that wall would sit 0.1 mm (two coupon
    // steps) shallower than the part's rib (1.1 + 0.4 against 1.1 + (1.2 + 0.3
    // - 1.1) + (0.4 - 0.3) for the 0.40 cell).
    let p = process("fdm-0.2");
    let c = coupon_fdm_02();
    let mesh = &c.built.objects[0].mesh;
    let n = segments(&p);
    let s = &c.summary;
    let (module, undersize) = (mm("module"), mm("undersize_per_side"));
    let half_rib = p.min_feature_mm / 2.0;
    let z_cell = s.base_mm + (s.label_relief_mm + s.cell_depth_mm) / 2.0;
    // Half-way up a plate's cavity (the part opens downward).
    let z_part = (mm("plate_height") - mm("roof").max(p.min_roof_mm)) / 2.0;
    let doc = part("body.plate", [2, 2], None, true, true);
    let mut checked = 0;
    for cell in c.cells.iter().filter(|x| x.copy == 0 && x.cross.is_some()) {
        let (od, rib) = (cell.value_mm, cell.cross.as_ref().unwrap().value_mm);
        let fit = FitProfile {
            process: p.id.clone(),
            offsets_mm: Offsets {
                tube_outer_diameter: od - mm("tube_outer_diameter"),
                rib_depth: rib - mm("rib_depth"),
                ..Offsets::default()
            },
            ..example_fit_profile()
        };
        let b = build(&doc, &p, Some(&fit));
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        let part_mesh = &b.objects[0].mesh;
        let (lo, hi) = part_mesh.bounds();
        let (lo, hi) = (lo.map(f64::from), hi.map(f64::from));
        let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        // The part's rib on its left wall at its first stud centre line.
        let line = lo[1] + module / 2.0 - undersize;
        let part_tip = rib_tip(part_mesh, z_part, lo[0], centre[0], line, half_rib);
        let [cx, cy] = cell.center_mm;
        let side = 2.0 * reserve(cell);
        let cell_tip = rib_tip(
            mesh,
            z_cell,
            cx - side / 2.0,
            cx,
            cy - module / 2.0,
            half_rib,
        );
        assert!(
            (part_tip - cell_tip).abs() < 1e-4,
            "pocket {od} / {rib}: part rib tip {part_tip} mm, cell rib tip {cell_tip} mm"
        );
        assert!((cell_tip - (mm("side_wall") + rib)).abs() < 1e-4);
        // The tube OD: the part's tube (reach: half a module, clear of walls
        // and ribs) against the cell's value.
        let (ro, _) = ring_radii(
            part_mesh,
            z_part,
            centre,
            (od + mm("tube_inner_diameter")) / 4.0,
            module / 2.0,
            n,
        );
        assert!(
            (2.0 * ro - od).abs() < 1e-4,
            "pocket {od}: part tube OD {}",
            2.0 * ro
        );
        checked += 1;
    }
    assert_eq!(checked, 13, "the cross: six ODs and seven rib depths");
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
fn a_rib_tip_inside_a_thicker_wall_is_refused() {
    // A process whose lines quantise the wall thicker than side_wall + the
    // shallowest rib value cannot show that cell: 0.75 mm lines make the 1.2 mm
    // wall two lines, 1.5 mm, while the 0.20 cell's tip belongs 1.2 + 0.2 =
    // 1.4 mm from the outer face, inside the wall. The coupon says so instead
    // of building a cell that does not show its value.
    let mut p = process("fdm-0.4");
    p.wall_quantise_mm = 0.75;
    assert!((quantised_wall(&p) - 2.0 * p.wall_quantise_mm).abs() < 1e-12);
    match build_coupon(&ManifoldRust, &p, &CouponOptions::default()) {
        Err(EvalError::Feature { feature, message }) => {
            assert_eq!(feature, "coupon");
            assert!(message.contains("WALL.QUANTISED"), "{message}");
        }
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("the 0.20 rib cell cannot be built on a 1.5 mm wall"),
    }
}

#[test]
fn the_nominal_pocket_is_the_standard_one_on_every_process() {
    // Rib values are measured against the standard side wall, so the nominal
    // rib cell is the standard rib_depth whatever the process does to the wall
    // (fdm-0.2 quantises it to five 0.22 mm lines, 1.1 mm): the cell a part
    // gets with a zero ribDepth offset. Likewise the nominal tube OD.
    for id in ["fdm-0.4", "fdm-0.2", "resin-msla"] {
        let p = process(id);
        let l = coupon_layout(&p, &CouponOptions::default()).expect(id);
        let (pockets, step) = plan_row("2x2 pockets");
        let rib0 = nearest(&sweep(pockets[2], pockets[3], step), mm("rib_depth"));
        let od0 = nearest(
            &sweep(pockets[0], pockets[1], step),
            mm("tube_outer_diameter"),
        );
        let od_line: Vec<_> = l.cells.iter().filter(|x| x.row == 2).collect();
        assert!(od_line
            .iter()
            .all(|x| close(x.cross.as_ref().unwrap().value_mm, rib0)));
        let rib_line: Vec<_> = l.cells.iter().filter(|x| x.row == 3).collect();
        assert!(rib_line.iter().all(|x| close(x.value_mm, od0)));
        // The label stroke stays the plan's 0.8 mm, above every bundled
        // process's minimum feature.
        assert!(l.summary.label_stroke_mm >= p.min_feature_mm);
        assert_eq!(
            l.summary.label_stroke_mm,
            CouponOptions::default().label_stroke_mm
        );
    }
    let fdm_02 = process("fdm-0.2");
    assert!((quantised_wall(&fdm_02) - 5.0 * fdm_02.wall_quantise_mm).abs() < 1e-12);
}

#[test]
fn a_fit_profile_cannot_reach_the_coupon() {
    // build_coupon takes no fit profile and CouponOptions has no field for one
    // (a compile_fail doc test on CouponOptions shows it); JSON options naming
    // one are refused rather than silently ignored. That no offset reaches the
    // geometry is measured on the mesh by the tests above: every stud, bore and
    // tube OD is its cell's value, and every rib tip sits side_wall + its value
    // from the outer face.
    let fit = serde_json::to_value(example_fit_profile()).unwrap();
    let mut opts = serde_json::to_value(CouponOptions::default()).unwrap();
    opts["fit"] = fit.clone();
    let err = serde_json::from_value::<CouponOptions>(opts).unwrap_err();
    assert!(err.to_string().contains("unknown field `fit`"), "{err}");
    // Also when the other fields are left to their defaults.
    let err =
        serde_json::from_value::<CouponOptions>(serde_json::json!({ "fit": fit })).unwrap_err();
    assert!(err.to_string().contains("unknown field `fit`"), "{err}");
}

#[test]
fn json_options_take_the_defaults_for_missing_fields() {
    // A wizard, CLI or wasm caller names only what it changes.
    let o: CouponOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(o, CouponOptions::default());
    let o: CouponOptions =
        serde_json::from_str(r#"{ "pockets": "full", "bedMm": [256, 256] }"#).unwrap();
    assert_eq!(
        o,
        CouponOptions {
            pockets: PocketSet::Full,
            bed_mm: [256.0, 256.0],
            ..CouponOptions::default()
        }
    );
    // The plan's defaults (docs/plan/05 section 4): two copies, labels 0.6 mm
    // high and 0.8 mm wide.
    let d = CouponOptions::default();
    assert_eq!(d.copies, 2);
    assert_eq!((d.label_relief_mm, d.label_stroke_mm), (0.6, 0.8));
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
    let c = coupon_fdm_02();
    let m = &c.built.measurements;
    assert!(m.is_manifold && m.components == 1 && m.genus == 0);
    let expected = analytic_volume(c, &p);
    let rel = ((m.volume_mm3 - expected) / expected).abs();
    assert!(rel < VOLUME_TOL, "volume {} vs {expected}", m.volume_mm3);
    assert_eq!(m.min_body_wall_mm, Some(quantised_wall(&p)));
    assert_ne!(c.built.content_hash, coupon().built.content_hash);
}

#[test]
fn record_accepts_exactly_the_pockets_each_pocket_set_prints() {
    // openbrick_print::record decides which pockets a coupon printed from the
    // coupon data alone (openbrick-print cannot depend on this crate), so it
    // repeats the rule of plan_lines: check, pair by pair over the whole cross,
    // that the two never drift apart. A pocket the set prints records; any other
    // is refused, naming the printed choices.
    let p = process("fdm-0.4");
    let spec = openbrick_print::coupon_v1();
    let [od_id, rib_id] = openbrick_print::POCKET_CELLS;
    let ods = spec.cell(od_id).expect("tube OD sweep").values();
    let ribs = spec.cell(rib_id).expect("rib sweep").values();
    // The beds of larger_pocket_sets_lay_out_on_larger_beds.
    for (set, bed_mm) in [
        (PocketSet::Cross, CouponOptions::default().bed_mm),
        (PocketSet::CrossAndDiagonal, [250.0, 210.0]),
        (PocketSet::Full, [256.0, 256.0]),
    ] {
        let layout = coupon_layout(
            &p,
            &CouponOptions {
                pockets: set,
                bed_mm,
                ..CouponOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("{}: {e}", set.name()));
        let printed: Vec<(f64, f64)> = layout
            .cells
            .iter()
            .filter_map(|c| {
                let rib = c.cross.as_ref()?;
                assert_eq!((c.cell_id.as_str(), rib.cell_id.as_str()), (od_id, rib_id));
                Some((c.value_mm, rib.value_mm))
            })
            .collect();
        // The print crate lists the same pockets.
        let listed = spec.printed_pockets(set.name()).expect("a known set");
        for &(o, r) in &printed {
            assert!(
                listed.iter().any(|&(lo, lr)| close(lo, o) && close(lr, r)),
                "{}: the coupon prints {o}/{r}, which openbrick-print does not list",
                set.name()
            );
        }
        for &(lo, lr) in &listed {
            assert!(
                printed.iter().any(|&(o, r)| close(lo, o) && close(lr, r)),
                "{}: openbrick-print lists {lo}/{lr}, which the coupon does not print",
                set.name()
            );
        }
        let mut accepted = 0;
        for &od in &ods {
            for &rib in &ribs {
                let input = RecordInput {
                    process: p.id.clone(),
                    printer: "Coupon check".into(),
                    nozzle_mm: p.line_width_mm,
                    material: "PLA".into(),
                    intent: "fits-genuine-bricks".into(),
                    cells: [(od_id.to_string(), od), (rib_id.to_string(), rib)].into(),
                    pocket_set: Some(set.name().into()),
                    ..RecordInput::default()
                };
                let on_coupon = printed.iter().any(|&(o, r)| close(o, od) && close(r, rib));
                match record(&input) {
                    Ok(f) => {
                        assert!(on_coupon, "{}: {od}/{rib} is not printed", set.name());
                        let cal = f.calibration.expect("calibration");
                        assert_eq!(cal.pocket_set.as_deref(), Some(set.name()));
                        accepted += 1;
                    }
                    Err(RecordError::PocketNotPrinted { set: s, .. }) => {
                        assert!(!on_coupon, "{}: {od}/{rib} is printed", set.name());
                        assert_eq!(s, set.name());
                    }
                    Err(e) => panic!("{}: {od}/{rib}: {e}", set.name()),
                }
            }
        }
        assert_eq!(accepted, listed.len(), "{}", set.name());
        if set == PocketSet::Cross {
            // The nominal row and column: one pocket fewer than the two sweeps,
            // since they share the nominal pocket.
            assert_eq!(accepted, ods.len() + ribs.len() - 1);
        }
        if set == PocketSet::Full {
            assert_eq!(accepted, ods.len() * ribs.len());
        }
    }
}

#[test]
fn the_coupon_validates_without_findings() {
    // validate() on the coupon's own measurements: no TUBE.WALL advice to
    // change a fit profile the coupon never takes (its bore cells are as thin
    // as a part's tube gets with that bore offset, on purpose), and nothing
    // else either, on fdm-0.4 (whose 0.8 mm recommended tube wall the 4.95 to
    // 5.05 bore cells are under) and on fdm-0.2.
    for (c, id) in [(coupon(), "fdm-0.4"), (coupon_fdm_02(), "fdm-0.2")] {
        let p = process(id);
        let report = openbrick_part::validate(&c.built, &p, None);
        assert!(report.findings.is_empty(), "{id}: {:?}", report.findings);
        assert!(report.export_allowed());
    }
    // The labels are fit-critical artwork: a stroke under the process
    // minimum would block export, not just warn.
    let p = process("fdm-0.4");
    let mut thin = coupon().built.clone();
    thin.measurements.min_stroke_mm = Some(p.min_feature_mm / 2.0);
    let report = openbrick_part::validate(&thin, &p, None);
    let feature_min: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.rule == "FEATURE.MIN")
        .map(|f| f.severity)
        .collect();
    assert_eq!(feature_min, vec![openbrick_print::Severity::Blocker]);
}

#[test]
fn the_print_estimate_is_always_a_finite_number() {
    // JSON has no infinity: a time that overflowed would be written as null
    // and not read back. A speed so small that the extrusion rate underflows to
    // zero (5e-324 mm/s), or that leaves the bounding box over the rate past
    // f64 (1e-305 mm/s: 0.4 x 0.2 x 1e-305 = 8e-307 mm3/s, and the default
    // coupon's box is about 6.1e4 mm3, so about 7.6e310 s), is refused.
    let p = process("fdm-0.4");
    for speed in [5e-324, 1e-305] {
        let o = CouponOptions {
            estimate_speed_mm_s: speed,
            ..CouponOptions::default()
        };
        match coupon_layout(&p, &o) {
            Err(EvalError::Feature { feature, message }) => {
                assert_eq!(feature, "coupon");
                assert!(message.contains("estimateSpeedMmS"), "{message}");
            }
            other => panic!("speed {speed:e}: {other:?}"),
        }
    }
    let e = &coupon().estimate;
    assert!(e.time_s.is_finite() && e.time_s > 0.0);
    let json = serde_json::to_string(e).expect("serialises");
    let back: openbrick_part::coupon::PrintEstimate =
        serde_json::from_str(&json).expect("reads back");
    assert_eq!(&back, e);
}
