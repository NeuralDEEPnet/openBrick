//! Plain 2D regions: fill-rule resolution and booleans on sets of polygons, for
//! artwork (docs/plan/06 section 5) and other planar work that no 3D solid needs.
//!
//! The work is done by Clipper2 (`clipper2-rust`, BSL-1.0, a pure-Rust port), the
//! library both Manifold backends use for their own cross-sections, so the result
//! does not depend on the kernel backend and builds for wasm32. Every function
//! returns a [`CrossSection`] in the form the kernel expects: non-overlapping
//! outer contours counter-clockwise and holes clockwise, so the Positive and the
//! NonZero fill rules both read it as the same region and [`CrossSection::area`]
//! is its area. Coordinates snap to a grid of 1e-8 (Clipper2's decimal
//! precision 8, the one manifold-rust's `CrossSection` uses).

use clipper2_rust::{
    difference_d, intersect_d, union_subjects_d, FillRule as ClipperFill, PathD, PathsD, Point,
};

use crate::{CrossSection, Polygon};

/// Decimal digits Clipper2 keeps (the precision of manifold-rust's cross-sections).
const PRECISION: i32 = 8;

/// How overlapping or self-intersecting contours fill (SVG `fill-rule`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillRule {
    /// Inside where the winding number is not zero (the SVG default).
    NonZero,
    /// Inside where a ray crosses an odd number of edges.
    EvenOdd,
}

fn to_paths(polygons: &[Polygon]) -> PathsD {
    polygons
        .iter()
        .map(|p| -> PathD { p.points.iter().map(|&[x, y]| Point::new(x, y)).collect() })
        .collect()
}

fn from_paths(paths: PathsD) -> CrossSection {
    CrossSection {
        polygons: paths
            .into_iter()
            .map(|p| Polygon {
                points: p.into_iter().map(|q| [q.x, q.y]).collect(),
            })
            .collect(),
    }
}

/// The region `contours` enclose under `rule`, with overlaps and
/// self-intersections resolved. Contours may run either way round; under
/// [`FillRule::EvenOdd`] two nested contours drawn the same way make a ring.
pub fn fill(contours: &[Polygon], rule: FillRule) -> CrossSection {
    let rule = match rule {
        FillRule::NonZero => ClipperFill::NonZero,
        FillRule::EvenOdd => ClipperFill::EvenOdd,
    };
    from_paths(union_subjects_d(&to_paths(contours), rule, PRECISION))
}

/// The union of regions already in kernel form (outer counter-clockwise, holes
/// clockwise).
pub fn union_all(sections: &[CrossSection]) -> CrossSection {
    let polygons: Vec<Polygon> = sections
        .iter()
        .flat_map(|s| s.polygons.iter().cloned())
        .collect();
    from_paths(union_subjects_d(
        &to_paths(&polygons),
        ClipperFill::Positive,
        PRECISION,
    ))
}

/// `a` minus `b`, both in kernel form.
pub fn difference(a: &CrossSection, b: &CrossSection) -> CrossSection {
    from_paths(difference_d(
        &to_paths(&a.polygons),
        &to_paths(&b.polygons),
        ClipperFill::Positive,
        PRECISION,
    ))
}

/// The part of `a` inside `b`, both in kernel form.
pub fn intersection(a: &CrossSection, b: &CrossSection) -> CrossSection {
    from_paths(intersect_d(
        &to_paths(&a.polygons),
        &to_paths(&b.polygons),
        ClipperFill::Positive,
        PRECISION,
    ))
}

/// The work a scanline sweep (Clipper2's Vatti algorithm, which every function
/// here and the kernel's offsets and extrusions run) does on a set of contours,
/// as counted by [`sweep_cost`]. A sweep's time grows with both numbers, not
/// with the vertex count alone: a star of 1,000 long, thin spikes (one path of
/// 2,000 points) keeps hundreds of edges active on every scanline, and 1,000
/// lines that all cross each other make half a million crossings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepCost {
    /// Edge-band pairs: a band is the strip between two consecutive distinct
    /// vertex heights, and each edge counts once for every band it spans; each
    /// horizontal edge counts itself and the edges that end on its ends.
    pub edge_bands: u64,
    /// Pairs of edges that cross each other, a horizontal edge and the edges
    /// that meet it inside its span or overlap it included.
    pub crossings: u64,
}

impl SweepCost {
    /// One crossing costs Clipper2 about as much as this many edge-band pairs:
    /// it splits both edges and adds output points. Measured with
    /// clipper2-rust 1.2.0 in a release build on x86-64: about 50 to 70 ns per
    /// edge-band pair (a star of 1,000 spikes, 291,048 pairs, about 15 ms per
    /// union) and about 1.3 microseconds per crossing (1,000 crossing lines,
    /// 1,995,003 crossings, about 2.5 s per union).
    pub const CROSSING_WEIGHT: u64 = 20;

    /// The weighted work: edge-band pairs plus crossings times
    /// [`SweepCost::CROSSING_WEIGHT`].
    pub fn work(&self) -> u64 {
        self.edge_bands
            .saturating_add(self.crossings.saturating_mul(Self::CROSSING_WEIGHT))
    }
}

/// Counts the [`SweepCost`] of `contours` in O(n log n + edge_bands log n),
/// without running a sweep; counting stops once the [`SweepCost::work`] passes
/// `limit`, so the count itself stays cheap on hostile input. Use it to refuse
/// input before handing it to Clipper2.
pub fn sweep_cost(contours: &[Polygon], limit: u64) -> SweepCost {
    // Non-horizontal edges as (low point, high point); horizontal ones as
    // (height, left x, right x).
    let mut edges: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let mut flat: Vec<(f64, f64, f64)> = Vec::new();
    for p in contours {
        let n = p.points.len();
        for i in 0..n {
            let (a, b) = (p.points[i], p.points[(i + 1) % n]);
            if !(a[1].is_finite() && b[1].is_finite() && a[0].is_finite() && b[0].is_finite()) {
                continue;
            }
            match a[1].total_cmp(&b[1]) {
                std::cmp::Ordering::Less => edges.push((a, b)),
                std::cmp::Ordering::Greater => edges.push((b, a)),
                std::cmp::Ordering::Equal => {
                    if a[0] != b[0] {
                        flat.push((a[1], a[0].min(b[0]), a[0].max(b[0])));
                    }
                }
            }
        }
    }
    let mut ys: Vec<f64> = edges.iter().flat_map(|e| [e.0[1], e.1[1]]).collect();
    ys.extend(flat.iter().map(|h| h.0));
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let band = |y: f64| ys.partition_point(|v| *v < y);
    let mut cost = SweepCost::default();
    for e in &edges {
        let spans = u64::try_from(band(e.1[1]) - band(e.0[1])).unwrap_or(u64::MAX);
        cost.edge_bands = cost.edge_bands.saturating_add(spans);
    }
    if cost.work() > limit {
        return cost;
    }
    // One pass up through the heights. At each height, every horizontal edge
    // meets the edges whose x there lies within its span (the other horizontal
    // edges at that height included); in each band above it, edges whose
    // left-to-right order at the top differs from the order at the bottom
    // cross, counted as inversions.
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&a, &b| edges[a].0[1].total_cmp(&edges[b].0[1]));
    flat.sort_by(|a, b| a.0.total_cmp(&b.0));
    let x_at = |e: &([f64; 2], [f64; 2]), y: f64| {
        let ([x0, y0], [x1, y1]) = *e;
        if y <= y0 {
            x0
        } else if y >= y1 {
            x1
        } else {
            x0 + (x1 - x0) * (y - y0) / (y1 - y0)
        }
    };
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let mut active: Vec<usize> = Vec::new();
    let (mut next, mut next_flat) = (0, 0);
    let mut xs: Vec<f64> = Vec::new();
    let mut starts: Vec<f64> = Vec::new();
    let mut ends: Vec<f64> = Vec::new();
    let mut row: Vec<(f64, f64)> = Vec::new();
    let mut tops: Vec<f64> = Vec::new();
    let mut scratch: Vec<f64> = Vec::new();
    for (b, &y0) in ys.iter().enumerate() {
        while next < order.len() && edges[order[next]].0[1] <= y0 {
            active.push(order[next]);
            next += 1;
        }
        // Every active edge reaches at least this height (heights are the
        // edges' own ends, and passed edges were dropped below).
        let here = flat[next_flat..].partition_point(|h| h.0 <= y0);
        if here > 0 {
            let hs = &flat[next_flat..next_flat + here];
            xs.clear();
            xs.extend(active.iter().map(|&e| x_at(&edges[e], y0)));
            xs.sort_by(f64::total_cmp);
            starts.clear();
            starts.extend(hs.iter().map(|h| h.1));
            starts.sort_by(f64::total_cmp);
            ends.clear();
            ends.extend(hs.iter().map(|h| h.2));
            ends.sort_by(f64::total_cmp);
            for h in hs {
                // Edges met strictly inside the span cross it; those at its ends
                // are usually its own neighbours.
                let inside = xs.partition_point(|x| *x < h.2) - xs.partition_point(|x| *x <= h.1);
                let at_ends =
                    xs.partition_point(|x| *x <= h.2) - xs.partition_point(|x| *x < h.1) - inside;
                // Other horizontal edges overlapping it (it overlaps itself).
                let overlapping = (starts.partition_point(|x| *x < h.2)
                    - ends.partition_point(|x| *x <= h.1))
                .saturating_sub(1);
                cost.edge_bands = cost.edge_bands.saturating_add(count(at_ends + 1));
                cost.crossings = cost.crossings.saturating_add(count(inside + overlapping));
            }
            next_flat += here;
            if cost.work() > limit {
                break;
            }
        }
        active.retain(|&e| edges[e].1[1] > y0);
        let Some(&y1) = ys.get(b + 1) else {
            break;
        };
        if active.len() < 2 {
            continue;
        }
        row.clear();
        row.extend(
            active
                .iter()
                .map(|&e| (x_at(&edges[e], y0), x_at(&edges[e], y1))),
        );
        row.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        tops.clear();
        tops.extend(row.iter().map(|r| r.1));
        cost.crossings = cost
            .crossings
            .saturating_add(inversions(&mut tops, &mut scratch));
        if cost.work() > limit {
            break;
        }
    }
    cost
}

/// The [`SweepCost`] of offsetting `section` by `delta` (negative insets) with
/// round joins of `circle_segments` chords per turn, as Clipper2 does it
/// (`ClipperOffset::OffsetPoint` and `DoRound` in clipper2-rust 1.2.0): it
/// first builds a raw path that follows every edge moved by `delta`, joined at
/// each vertex by an arc, or, where the join is concave, by going back through
/// the vertex itself, so an inset past a thin part folds over its neighbours;
/// then it unions that path. Insetting a dense outline by more than half its
/// width makes most of the cost: 40,000 points of thin wavy bars inset by 0.4
/// take 16 s. The raw path here has the same points, so its sweep cost is the
/// union's. Counting stops past `limit` as in [`sweep_cost`].
pub fn offset_sweep_cost(
    section: &CrossSection,
    delta: f64,
    circle_segments: u32,
    limit: u64,
) -> SweepCost {
    let steps_per_rad = f64::from(circle_segments.max(4)) / std::f64::consts::TAU;
    let normal = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let h = dx.hypot(dy);
        if h > 0.0 {
            [dy / h, -dx / h]
        } else {
            [0.0, 0.0]
        }
    };
    let raw: Vec<Polygon> = section
        .polygons
        .iter()
        .map(|p| {
            let mut pts = p.points.clone();
            pts.dedup();
            while pts.len() > 1 && pts.first() == pts.last() {
                pts.pop();
            }
            let n = pts.len();
            let norms: Vec<[f64; 2]> = (0..n).map(|i| normal(pts[i], pts[(i + 1) % n])).collect();
            let mut out = Vec::with_capacity(3 * n);
            for j in 0..n {
                let k = (j + n - 1) % n;
                let (nj, nk, pj) = (norms[j], norms[k], pts[j]);
                // The turn from the incoming normal to the outgoing one (Clipper2's
                // cross product is the negated usual one, with its arguments
                // the other way round).
                let sin_a = (nk[0] * nj[1] - nk[1] * nj[0]).clamp(-1.0, 1.0);
                let cos_a = nj[0] * nk[0] + nj[1] * nk[1];
                out.push([pj[0] + delta * nk[0], pj[1] + delta * nk[1]]);
                if cos_a > -0.999 && sin_a * delta < 0.0 {
                    out.push(pj);
                } else {
                    // The arc's inner points, rotating the offset vector.
                    let angle = sin_a.atan2(cos_a);
                    let steps = (steps_per_rad * angle.abs()).ceil().min(1e6);
                    let step = angle / steps.max(1.0);
                    let (mut v, (sn, cs)) = ([delta * nk[0], delta * nk[1]], step.sin_cos());
                    // In range: at most a million, checked above.
                    for _ in 1..(steps as u32) {
                        v = [v[0] * cs - v[1] * sn, v[0] * sn + v[1] * cs];
                        out.push([pj[0] + v[0], pj[1] + v[1]]);
                    }
                }
                out.push([pj[0] + delta * nj[0], pj[1] + delta * nj[1]]);
            }
            Polygon { points: out }
        })
        .collect();
    sweep_cost(&raw, limit)
}

/// Pairs i < j with `v[i] > v[j]`, by merge sort (sorts `v`).
fn inversions(v: &mut [f64], scratch: &mut Vec<f64>) -> u64 {
    let n = v.len();
    let mut count = 0u64;
    let mut width = 1;
    while width < n {
        let mut start = 0;
        while start < n {
            let mid = (start + width).min(n);
            let end = (start + 2 * width).min(n);
            if mid < end {
                scratch.clear();
                let (mut i, mut j) = (start, mid);
                while i < mid && j < end {
                    if v[j] < v[i] {
                        // v[j] is smaller than every remaining left element.
                        count += u64::try_from(mid - i).unwrap_or(u64::MAX);
                        scratch.push(v[j]);
                        j += 1;
                    } else {
                        scratch.push(v[i]);
                        i += 1;
                    }
                }
                scratch.extend_from_slice(&v[i..mid]);
                scratch.extend_from_slice(&v[j..end]);
                v[start..end].copy_from_slice(scratch);
            }
            start = end;
        }
        width *= 2;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit-free test shapes (no brick dimension is involved).
    fn square(min: [f64; 2], side: f64) -> Polygon {
        Polygon::rect(min, [side, side])
    }

    #[test]
    fn nested_same_way_contours_are_a_ring_under_even_odd_only() {
        let outer = Polygon::circle([0.0, 0.0], 10.0, 64);
        let inner = Polygon::circle([0.0, 0.0], 6.0, 64);
        let both = [outer.clone(), inner.clone()];
        let ring = fill(&both, FillRule::EvenOdd);
        let disc = fill(&both, FillRule::NonZero);
        let want_ring = outer.signed_area() - inner.signed_area();
        assert!((ring.area() - want_ring).abs() < 1e-6, "{}", ring.area());
        assert!((disc.area() - outer.signed_area()).abs() < 1e-6);
        // Kernel form: one counter-clockwise outer and one clockwise hole.
        let mut signs: Vec<bool> = ring
            .polygons
            .iter()
            .map(|p| p.signed_area() > 0.0)
            .collect();
        signs.sort();
        assert_eq!(signs, vec![false, true]);
    }

    #[test]
    fn clockwise_contours_fill_under_both_rules() {
        // SVG files draw shapes either way round; the result is always positive.
        let cw = square([0.0, 0.0], 2.0).reversed();
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let r = fill(std::slice::from_ref(&cw), rule);
            assert!((r.area() - 4.0).abs() < 1e-9, "{rule:?}: {}", r.area());
        }
    }

    #[test]
    fn a_self_intersecting_star_differs_between_the_rules() {
        // A pentagram drawn as one path: its centre pentagon has winding number 2.
        let points = (0..5)
            .map(|i| {
                let a =
                    std::f64::consts::FRAC_PI_2 + f64::from(i * 2) * std::f64::consts::TAU / 5.0;
                [a.cos(), a.sin()]
            })
            .collect();
        let star = [Polygon { points }];
        let nonzero = fill(&star, FillRule::NonZero).area();
        let evenodd = fill(&star, FillRule::EvenOdd).area();
        assert!(nonzero > evenodd + 0.1, "{nonzero} vs {evenodd}");
    }

    #[test]
    fn sweep_cost_counts_bands_and_crossings() {
        // A square: two vertical edges spanning one band, no crossing.
        let sq = square([0.0, 0.0], 2.0);
        let c = sweep_cost(std::slice::from_ref(&sq), u64::MAX);
        // The two horizontal edges each meet the two vertical ones and themselves.
        assert_eq!(
            c,
            SweepCost {
                edge_bands: 2 + 2 * 3,
                crossings: 0
            }
        );
        // A row of n separate squares: each horizontal edge meets only its own
        // square's edges, so the count grows with n, not n^2.
        let row: Vec<Polygon> = (0..100)
            .map(|i| square([3.0 * f64::from(i), 0.0], 2.0))
            .collect();
        assert_eq!(sweep_cost(&row, u64::MAX).edge_bands, 100 * (2 + 2 * 3));
        // The pentagram crosses itself five times.
        let points = (0..5)
            .map(|i| {
                let a =
                    std::f64::consts::FRAC_PI_2 + f64::from(i * 2) * std::f64::consts::TAU / 5.0;
                [a.cos(), a.sin()]
            })
            .collect();
        let star = sweep_cost(&[Polygon { points }], u64::MAX);
        assert_eq!(star.crossings, 5);
        // n lines that all cross between two heights: n (n - 1) / 2 crossings in
        // one band, although each edge spans that band only.
        let n = 40;
        let mut zig = Vec::new();
        for i in 0..n {
            zig.push([f64::from(i), 0.0]);
            zig.push([f64::from(2 * n - i), 10.0]);
        }
        let c = sweep_cost(&[Polygon { points: zig }], u64::MAX);
        let k = u64::try_from(n).unwrap();
        assert_eq!(c.edge_bands, 2 * k);
        assert!(c.crossings >= k * (k - 1) / 2, "{c:?}");
        // Counting stops past the limit; crossings weigh more than bands.
        assert!(sweep_cost(&[sq], 1).work() > 1);
        assert_eq!(
            c.work(),
            c.edge_bands + SweepCost::CROSSING_WEIGHT * c.crossings
        );
        let mut v = vec![3.0, 1.0, 2.0, 0.0];
        assert_eq!(inversions(&mut v, &mut Vec::new()), 5);
        assert_eq!(v, [0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn insetting_past_a_thin_part_costs_crossings() {
        // A comb: a bar with n thin teeth. Outsetting it, or insetting it by
        // less than half a tooth, crosses little; insetting it past the teeth
        // folds every tooth over its neighbours.
        let n = 50;
        let mut points = vec![[0.0, 0.0], [f64::from(n), 0.0], [f64::from(n), 1.0]];
        for i in (0..n).rev() {
            let x = f64::from(i);
            points.extend([
                [x + 0.6, 1.0],
                [x + 0.6, 5.0],
                [x + 0.4, 5.0],
                [x + 0.4, 1.0],
            ]);
        }
        points.push([0.0, 1.0]);
        let comb = CrossSection::from(vec![Polygon { points }]);
        assert!(comb.area() > 0.0, "counter-clockwise");
        let grow = offset_sweep_cost(&comb, 0.05, 8, u64::MAX);
        let shallow = offset_sweep_cost(&comb, -0.05, 8, u64::MAX);
        // Past the teeth and two of their neighbours on each side.
        let deep = offset_sweep_cost(&comb, -2.5, 8, u64::MAX);
        // Each concave join makes a small loop that crosses its neighbours (at
        // most two crossings per vertex); folding the teeth over each other
        // crosses far more (measured: 100, 104 and 2,298).
        let vertices = u64::try_from(comb.polygons[0].points.len()).unwrap();
        assert!(grow.crossings <= 2 * vertices && shallow.crossings <= 2 * vertices);
        assert!(
            deep.crossings > 10 * shallow.crossings,
            "{deep:?} vs {shallow:?}"
        );
    }

    #[test]
    fn booleans_of_overlapping_squares() {
        let a = fill(&[square([0.0, 0.0], 2.0)], FillRule::NonZero);
        let b = fill(&[square([1.0, 1.0], 2.0)], FillRule::NonZero);
        assert!((union_all(&[a.clone(), b.clone()]).area() - 7.0).abs() < 1e-9);
        assert!((intersection(&a, &b).area() - 1.0).abs() < 1e-9);
        assert!((difference(&a, &b).area() - 3.0).abs() < 1e-9);
        assert!(difference(&a, &a).polygons.is_empty());
    }
}
