//! Self-intersection test of a triangle mesh (rule `MESH.SELF_INTERSECT`,
//! docs/plan/05 section 5): which pairs of triangles meet anywhere but in the
//! vertices and edges they share.
//!
//! **Definition.** Two triangles intersect when one passes through the
//! other: an edge of one (a closed segment) meets the open interior of the
//! other, or the two lie in one plane, face the same way and their open
//! interiors overlap (a doubled face, which turns inside and outside around).
//! Two faces that touch back to back are contact, not a crossing: a
//! zero-thickness fin (a pair of triangles folded flat over their shared
//! edge), or two shells pressed together, keep inside and outside apart, and a
//! slicer reads them as one solid (measured on manifold-rust 0.19.1: the fused
//! single-colour mesh of a parts inlay keeps such a fin where the artwork fill
//! meets the recess it fills). Contact on the boundaries of both only (two
//! triangles meeting at a corner,
//! along an edge, an edge running along another's edge) is not counted: a
//! closed kernel mesh has such contacts wherever it keeps a zero-area sliver
//! triangle, which is collinear once written as f32 (measured on
//! manifold-rust 0.19.1: a cavity-rim chamfer edge running along a wall's
//! bottom edge on every plate). Two surfaces that really cross pierce each
//! other's faces, so this misses only a crossing that runs exactly from edge
//! to edge of both triangles of every pair along it. Triangles that share
//! vertices need no special case: what they share lies on their boundaries.
//!
//! **Vertices.** Vertices closer than [`WELD_MM`] (10 nm) in every coordinate
//! are one vertex ([`weld`]), whatever their indices. Kernels leave such
//! near-twins where an exact vertex and a rounded copy of it meet (measured on
//! manifold-rust 0.19.1: a plate corner at y = 0 and at y = 3e-9 mm), and
//! without welding the two triangles would touch at a point. The evaluator
//! builds no feature thinner than a micrometre (`MIN_LENGTH_MM` in
//! openbrick-part), so welding at 10 nm never merges two real corners.
//!
//! **Arithmetic: exact predicates on a fine grid.** The welded coordinates
//! are snapped to integer multiples of [`GRID_MM`], 2^-19 mm (about 2 nm). An
//! f32 of magnitude 16 mm or more is already a multiple of that grid (its last
//! bit is worth at least 2^-19 mm), and smaller ones move by at most 2^-20 mm,
//! five times less than the welding distance, so snapping never merges two
//! vertices welding kept apart. On the integers every predicate (the
//! orientation of four points in space, of three points in a plane) is
//! evaluated on integers: differences fit 30 bits, 2 x 2 minors 61 bits
//! (`i64`) and the 3 x 3 determinant 93 bits (`i128`), behind a floating-point
//! filter that answers only where its error bound proves the sign. Two tests
//! have a dead band of [`WELD_MM`]: a vertex that close to another triangle's
//! plane lies in it, and, within a plane, a vertex that close to an edge's
//! line lies on it. Kernels write T-junctions (a vertex on another triangle's
//! edge) rounded to f32, so the vertex lands 0.1 micrometre or less to one
//! side and the two faces overlap by a sliver of that width (measured on
//! manifold-rust 0.19.1: an inlay tile's side face); the band reads them as
//! touching. For the same reason a crossing counts only when it lies more than
//! the band inside the face it crosses, and two crossing faces only when they
//! share more than the band of the line where their planes meet: the exact
//! tests decide whether, the band how far. Measured on manifold-rust 0.19.1, an
//! emboss wall's corner lands 1e-6 mm inside the top face it stands on, which
//! is the last bit of an f32 at 24 mm. Every decision is a
//! deterministic function of the snapped coordinates, so the result never
//! depends on the order of the triangles. The
//! guard is [`MAX_COORD_MM`]: coordinates must stay inside +-1024 mm (2^29 grid
//! steps); a mesh outside it is reported, not tested.
//!
//! **Speed.** Candidate pairs come from a bounding-volume hierarchy over the
//! triangles' boxes (median split, self-join); each candidate is first
//! rejected when one triangle lies strictly on one side of the other's plane.
//! Triangles with zero area, and slivers less than [`WELD_MM`] high (the
//! kernel's near-collinear triangles, collinear or nearly so once written as
//! f32), cannot bound a solid and are skipped; they are counted in
//! [`SelfIntersections::degenerate`]. A sliver's neighbours then meet across it
//! on their boundaries, which is not an intersection.

use std::collections::HashMap;

use crate::Mesh;

/// Vertices closer than this in every coordinate are one vertex, in
/// millimetres (see the module documentation).
pub const WELD_MM: f64 = 1e-5;

/// The grid the coordinates are snapped to, in millimetres: 2^-19 mm.
pub const GRID_MM: f64 = 1.0 / (1u64 << 19) as f64;

/// The largest coordinate magnitude the test accepts, in millimetres (2^29
/// grid steps, see the module documentation).
pub const MAX_COORD_MM: f64 = 1024.0;

/// What [`self_intersections`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelfIntersections {
    /// Pairs of triangles that intersect.
    pub pairs: usize,
    /// The first intersecting pair found (triangle indices, smaller first), for
    /// messages and debugging.
    pub first: Option<[usize; 2]>,
    /// Pairs whose boxes overlap, which the exact test looked at (a measure
    /// of the work done).
    pub candidates: usize,
    /// Triangles skipped because they have zero area, are slivers under
    /// [`WELD_MM`] high, or name a vertex the mesh does not have.
    pub degenerate: usize,
    /// The mesh has coordinates outside +-[`MAX_COORD_MM`] or that are not
    /// finite, so it was not tested.
    pub out_of_range: bool,
}

/// Merge vertices closer than [`WELD_MM`] in every coordinate: for each vertex
/// of `positions`, the index of its cluster in the returned list of positions.
/// Clusters are transitive: a chain of near-twins, each within the distance
/// of the next, joins one cluster even where its ends lie farther apart, so no
/// two returned positions are closer than [`WELD_MM`] in every coordinate.
/// A cluster's position is its first vertex, and clusters are numbered in the
/// order of their first vertex, so the result is deterministic.
pub fn weld(positions: &[[f32; 3]]) -> (Vec<u32>, Vec<[f64; 3]>) {
    // Cells four welding distances wide: a near-twin can only lie in the
    // neighbouring cell across a face the point is within the distance of.
    // Every vertex goes into its cell (`head` holds the last, `next` chains
    // the earlier ones); `parent` is a union-find forest over the vertices
    // whose roots are the first vertex of each cluster.
    let size = 4.0 * WELD_MM;
    let n = positions.len();
    let mut head: HashMap<[i64; 3], u32, CellHash> = HashMap::with_capacity_and_hasher(n, CellHash);
    let mut next: Vec<u32> = Vec::with_capacity(n);
    let mut parent: Vec<u32> = Vec::with_capacity(n);
    let points: Vec<[f64; 3]> = positions.iter().map(|p| p.map(f64::from)).collect();
    fn root(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            let up = parent[parent[i as usize] as usize];
            parent[i as usize] = up;
            i = up;
        }
        i
    }
    for (id, q) in points.iter().enumerate() {
        let id = id as u32;
        parent.push(id);
        let c = q.map(|x| (x / size).floor() as i64);
        // Per axis: this cell, and the neighbour across a near face (0: none).
        let reach = |k: usize| -> i64 {
            let f = q[k] - c[k] as f64 * size;
            if f < WELD_MM {
                -1
            } else if size - f < WELD_MM {
                1
            } else {
                0
            }
        };
        let r = [reach(0), reach(1), reach(2)];
        'cells: for mask in 0..8u8 {
            let mut cell = c;
            for k in 0..3 {
                if mask & (1 << k) != 0 {
                    if r[k] == 0 {
                        continue 'cells;
                    }
                    cell[k] += r[k];
                }
            }
            let mut at = head.get(&cell).copied();
            while let Some(j) = at {
                let o = points[j as usize];
                if (0..3).all(|k| (o[k] - q[k]).abs() < WELD_MM) {
                    // Join the clusters under the earlier root.
                    let (a, b) = (root(&mut parent, j), root(&mut parent, id));
                    let (lo, hi) = (a.min(b), a.max(b));
                    parent[hi as usize] = lo;
                }
                at = (next[j as usize] != u32::MAX).then_some(next[j as usize]);
            }
        }
        let prev = head.insert(c, id);
        next.push(prev.unwrap_or(u32::MAX));
    }
    // Number the clusters in the order of their first vertex (their root).
    let mut cluster = vec![u32::MAX; n];
    let mut reps: Vec<[f64; 3]> = Vec::new();
    let mut map = Vec::with_capacity(n);
    for id in 0..n as u32 {
        let r = root(&mut parent, id) as usize;
        if cluster[r] == u32::MAX {
            cluster[r] = reps.len() as u32;
            reps.push(points[r]);
        }
        map.push(cluster[r]);
    }
    (map, reps)
}

/// A fast hasher for integer cell keys (a multiply-xorshift mix): the keys
/// come from the mesh, not from an adversary, so a keyed hash buys nothing.
#[derive(Clone, Copy, Default)]
struct CellHash;

impl std::hash::BuildHasher for CellHash {
    type Hasher = CellHasher;
    fn build_hasher(&self) -> CellHasher {
        CellHasher(0)
    }
}

#[derive(Clone, Copy)]
struct CellHasher(u64);

impl std::hash::Hasher for CellHasher {
    fn finish(&self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

type P = [i64; 3];

/// Snap a coordinate to the grid; `None` outside the accepted range.
fn snap(x: f64) -> Option<i64> {
    if !x.is_finite() || x.abs() > MAX_COORD_MM {
        return None;
    }
    Some((x / GRID_MM).round() as i64)
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Sign of the orientation of d against the plane through a, b, c (the
/// convention of Shewchuk's `orient3d`); zero when the four are coplanar.
/// Exact: differences fit 30 bits, minors 61 bits, the determinant 93.
pub(crate) fn orient3d(a: P, b: P, c: P, d: P) -> i8 {
    det3(sub(a, d), sub(b, d), sub(c, d))
}

/// The normal (b - a) x (c - a), exact (61 bits).
fn normal(a: P, b: P, c: P) -> P {
    let u = sub(b, a);
    let v = sub(c, a);
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

/// The axis to drop when projecting a plane with this normal: the one of the
/// largest normal component, so a triangle of nonzero area keeps a nonzero
/// area in the projection.
fn drop_axis(n: P) -> usize {
    let a = n.map(i64::abs);
    if a[0] >= a[1] && a[0] >= a[2] {
        0
    } else if a[1] >= a[2] {
        1
    } else {
        2
    }
}

type Q = [i64; 2];

fn project(p: P, axis: usize) -> Q {
    match axis {
        0 => [p[1], p[2]],
        1 => [p[2], p[0]],
        _ => [p[0], p[1]],
    }
}

/// Sign of the 2D orientation of c against the line a -> b (exact, 61 bits).
///
/// Within [`WELD_MM`] of the line (measured in the projection) it is 0: the
/// dead band of the module documentation. The cross product is exact (61
/// bits); its conversion to f64 and the length are within a relative 1e-15,
/// far from the band's five grid steps.
fn orient2d(a: Q, b: Q, c: Q) -> i8 {
    let l = (b[0] - a[0]) * (c[1] - a[1]);
    let r = (b[1] - a[1]) * (c[0] - a[0]);
    let cross = (l - r) as f64;
    let (dx, dy) = ((b[0] - a[0]) as f64, (b[1] - a[1]) as f64);
    if cross.abs() <= BAND * (dx * dx + dy * dy).sqrt() {
        0
    } else if cross > 0.0 {
        1
    } else {
        -1
    }
}

/// [`WELD_MM`] in grid steps: the width of the dead band of the plane and line
/// tests.
const BAND: f64 = WELD_MM / GRID_MM;

/// The open interiors of two coplanar triangles overlap (2D, both of
/// nonzero area): no edge line of either has the other triangle wholly on its
/// closed outer side (the separating axis test, exact).
fn open_tris_overlap(t: [Q; 3], u: [Q; 3]) -> bool {
    let separated_by = |a: [Q; 3], b: [Q; 3]| {
        let o = orient2d(a[0], a[1], a[2]);
        (0..3).any(|i| {
            let (p, q) = (a[i], a[(i + 1) % 3]);
            b.iter().all(|&x| o * orient2d(p, q, x) <= 0)
        })
    };
    !separated_by(t, u) && !separated_by(u, t)
}

/// The closed segment p q, lying in the plane of the triangle t, meets t's
/// open interior (2D, t of nonzero area).
fn coplanar_seg_open_tri(p: Q, q: Q, t: [Q; 3]) -> bool {
    let o = orient2d(t[0], t[1], t[2]);
    // An edge line of t with the segment on its closed outer side.
    let outside = (0..3).any(|i| {
        let (a, b) = (t[i], t[(i + 1) % 3]);
        o * orient2d(a, b, p) <= 0 && o * orient2d(a, b, q) <= 0
    });
    if outside {
        return false;
    }
    // The segment's line with t on one closed side.
    let s = t.map(|x| orient2d(p, q, x));
    let one_side = s.iter().all(|&v| v >= 0) || s.iter().all(|&v| v <= 0);
    !one_side
}

/// The closed segment p q meets the open interior of the triangle t (nonzero
/// area); `op`, `oq` are the orientations of p and q against t's plane.
fn seg_open_tri(p: P, q: P, op: i8, oq: i8, t: &Tri) -> bool {
    if op * oq > 0 {
        return false;
    }
    let [a, b, c] = t.p;
    if op == 0 && oq == 0 {
        let ax = t.axis;
        return coplanar_seg_open_tri(project(p, ax), project(q, ax), t.p.map(|v| project(v, ax)));
    }
    // The segment crosses the plane (or ends in it) at one point, which lies
    // in the open interior when the line through p q passes strictly inside
    // every edge, and more than the dead band inside (a T-junction rounded to
    // f32 puts its corner a micrometre or less inside the neighbouring face).
    let s1 = orient3d(p, q, a, b);
    let s2 = orient3d(p, q, b, c);
    let s3 = orient3d(p, q, c, a);
    let inside = (s1 > 0 && s2 > 0 && s3 > 0) || (s1 < 0 && s2 < 0 && s3 < 0);
    inside && crossing_clear_of_edges(p, q, t)
}

/// The point where p q crosses t's plane lies farther than the dead band
/// ([`WELD_MM`]) from each edge line of t, measured in f64 in t's plane.
fn crossing_clear_of_edges(p: P, q: P, t: &Tri) -> bool {
    let f = |v: P| v.map(|c| c as f64);
    let (pf, qf) = (f(p), f(q));
    let a = f(t.p[0]);
    let dist =
        |x: [f64; 3]| t.n[0] * (x[0] - a[0]) + t.n[1] * (x[1] - a[1]) + t.n[2] * (x[2] - a[2]);
    let (dp, dq) = (dist(pf), dist(qf));
    let k = if dp == dq { 0.0 } else { dp / (dp - dq) };
    let x = [
        pf[0] + k * (qf[0] - pf[0]),
        pf[1] + k * (qf[1] - pf[1]),
        pf[2] + k * (qf[2] - pf[2]),
    ];
    let n_len = (t.n[0] * t.n[0] + t.n[1] * t.n[1] + t.n[2] * t.n[2]).sqrt();
    (0..3).all(|i| {
        let (u, v) = (f(t.p[i]), f(t.p[(i + 1) % 3]));
        let e = [v[0] - u[0], v[1] - u[1], v[2] - u[2]];
        let w = [x[0] - u[0], x[1] - u[1], x[2] - u[2]];
        // |e x w| / |e| is the distance from x to the edge's line.
        let c = [
            e[1] * w[2] - e[2] * w[1],
            e[2] * w[0] - e[0] * w[2],
            e[0] * w[1] - e[1] * w[0],
        ];
        let cross = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        let len = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
        n_len > 0.0 && cross > BAND * len
    })
}

/// The length of the stretch two crossing triangles share on the line where
/// their planes meet, in grid steps (f64): the intervals each cuts from that
/// line, overlapped.
fn shared_stretch(t1: &Tri, t2: &Tri) -> f64 {
    let f = |v: P| v.map(|c| c as f64);
    let d = [
        t1.n[1] * t2.n[2] - t1.n[2] * t2.n[1],
        t1.n[2] * t2.n[0] - t1.n[0] * t2.n[2],
        t1.n[0] * t2.n[1] - t1.n[1] * t2.n[0],
    ];
    let d_len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if d_len == 0.0 {
        return 0.0;
    }
    let d = d.map(|c| c / d_len);
    let interval = |t: &Tri, other: &Tri| -> (f64, f64) {
        let o = f(other.p[0]);
        let side = |x: [f64; 3]| {
            other.n[0] * (x[0] - o[0]) + other.n[1] * (x[1] - o[1]) + other.n[2] * (x[2] - o[2])
        };
        let v = t.p.map(f);
        let fs = v.map(side);
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut take = |x: [f64; 3]| {
            let s = d[0] * x[0] + d[1] * x[1] + d[2] * x[2];
            lo = lo.min(s);
            hi = hi.max(s);
        };
        for i in 0..3 {
            let j = (i + 1) % 3;
            if fs[i] == 0.0 {
                take(v[i]);
            }
            if fs[i] * fs[j] < 0.0 {
                let k = fs[i] / (fs[i] - fs[j]);
                take([
                    v[i][0] + k * (v[j][0] - v[i][0]),
                    v[i][1] + k * (v[j][1] - v[i][1]),
                    v[i][2] + k * (v[j][2] - v[i][2]),
                ]);
            }
        }
        (lo, hi)
    };
    let (lo1, hi1) = interval(t1, t2);
    let (lo2, hi2) = interval(t2, t1);
    hi1.min(hi2) - lo1.max(lo2)
}

/// Sign of the determinant of the rows u, v, w (u . (v x w)), exact.
///
/// A floating-point filter answers first: the entries (at most 2^30) are exact
/// in f64, and when the result exceeds Shewchuk's error bound for this
/// expression, (7 + 56 eps) eps times the permanent ("Adaptive precision
/// floating-point arithmetic and fast robust geometric predicates", 1997,
/// `o3derrboundA`), its sign is the exact sign. Otherwise the integers decide.
fn det3(u: P, v: P, w: P) -> i8 {
    let f = |p: P| p.map(|x| x as f64);
    let (a, b, c) = (f(u), f(v), f(w));
    let (bc0, cb0) = (b[1] * c[2], b[2] * c[1]);
    let (bc1, cb1) = (b[2] * c[0], b[0] * c[2]);
    let (bc2, cb2) = (b[0] * c[1], b[1] * c[0]);
    let det = a[0] * (bc0 - cb0) + a[1] * (bc1 - cb1) + a[2] * (bc2 - cb2);
    let permanent = a[0].abs() * (bc0.abs() + cb0.abs())
        + a[1].abs() * (bc1.abs() + cb1.abs())
        + a[2].abs() * (bc2.abs() + cb2.abs());
    const EPS: f64 = f64::EPSILON / 2.0;
    let bound = (7.0 + 56.0 * EPS) * EPS * permanent;
    if det > bound {
        return 1;
    }
    if -det > bound {
        return -1;
    }
    exact_det3(u, v, w)
}

/// [`det3`] in integers only.
fn exact_det3(u: P, v: P, w: P) -> i8 {
    let m0 = v[1] * w[2] - v[2] * w[1];
    let m1 = v[2] * w[0] - v[0] * w[2];
    let m2 = v[0] * w[1] - v[1] * w[0];
    (i128::from(u[0]) * i128::from(m0)
        + i128::from(u[1]) * i128::from(m1)
        + i128::from(u[2]) * i128::from(m2))
    .signum() as i8
}

/// The intervals two triangles cut from the line where their planes meet
/// overlap, by the orientation tests of Guigue and Devillers ("Fast and
/// robust triangle-triangle overlap test using orientation predicates",
/// Journal of Graphics Tools 8(1), 2003), transcribed with exact signs.
/// `strict`: the overlap has positive length; otherwise touching counts.
/// Both triangles must cross the other's plane (not lie in it).
fn interval_overlap(t1: [P; 3], t2: [P; 3], strict: bool) -> bool {
    let side = |t: [P; 3], x: P| det3(sub(x, t[2]), sub(t[0], t[2]), sub(t[1], t[2]));
    let [p1, q1, r1] = t1;
    let [p2, q2, r2] = t2;
    let (dp1, dq1, dr1) = (side(t2, p1), side(t2, q1), side(t2, r1));
    let (dp2, dq2, dr2) = (side(t1, p2), side(t1, q2), side(t1, r2));
    let tt = |a: [P; 3], b: [P; 3], d: [i8; 3]| tri_tri_3d(a, b, d, strict);
    if dp1 > 0 {
        if dq1 > 0 {
            tt([r1, p1, q1], [p2, r2, q2], [dp2, dr2, dq2])
        } else if dr1 > 0 {
            tt([q1, r1, p1], [p2, r2, q2], [dp2, dr2, dq2])
        } else {
            tt([p1, q1, r1], [p2, q2, r2], [dp2, dq2, dr2])
        }
    } else if dp1 < 0 {
        if dq1 < 0 {
            tt([r1, p1, q1], [p2, q2, r2], [dp2, dq2, dr2])
        } else if dr1 < 0 {
            tt([q1, r1, p1], [p2, q2, r2], [dp2, dq2, dr2])
        } else {
            tt([p1, q1, r1], [p2, r2, q2], [dp2, dr2, dq2])
        }
    } else if dq1 < 0 {
        if dr1 >= 0 {
            tt([q1, r1, p1], [p2, r2, q2], [dp2, dr2, dq2])
        } else {
            tt([p1, q1, r1], [p2, q2, r2], [dp2, dq2, dr2])
        }
    } else if dq1 > 0 {
        if dr1 > 0 {
            tt([p1, q1, r1], [p2, r2, q2], [dp2, dr2, dq2])
        } else {
            tt([q1, r1, p1], [p2, q2, r2], [dp2, dq2, dr2])
        }
    } else if dr1 > 0 {
        tt([r1, p1, q1], [p2, q2, r2], [dp2, dq2, dr2])
    } else if dr1 < 0 {
        tt([r1, p1, q1], [p2, r2, q2], [dp2, dr2, dq2])
    } else {
        false
    }
}

/// `TRI_TRI_3D` of Guigue and Devillers: permute the second triangle so its
/// first vertex is alone on its side of the first triangle's plane.
fn tri_tri_3d(t1: [P; 3], t2: [P; 3], d: [i8; 3], strict: bool) -> bool {
    let [p1, q1, r1] = t1;
    let [p2, q2, r2] = t2;
    let [dp2, dq2, dr2] = d;
    let cmm = |a: [P; 3], b: [P; 3]| check_min_max(a, b, strict);
    if dp2 > 0 {
        if dq2 > 0 {
            cmm([p1, r1, q1], [r2, p2, q2])
        } else if dr2 > 0 {
            cmm([p1, r1, q1], [q2, r2, p2])
        } else {
            cmm([p1, q1, r1], [p2, q2, r2])
        }
    } else if dp2 < 0 {
        if dq2 < 0 {
            cmm([p1, q1, r1], [r2, p2, q2])
        } else if dr2 < 0 {
            cmm([p1, q1, r1], [q2, r2, p2])
        } else {
            cmm([p1, r1, q1], [p2, q2, r2])
        }
    } else if dq2 < 0 {
        if dr2 >= 0 {
            cmm([p1, r1, q1], [q2, r2, p2])
        } else {
            cmm([p1, q1, r1], [p2, q2, r2])
        }
    } else if dq2 > 0 {
        if dr2 > 0 {
            cmm([p1, r1, q1], [p2, q2, r2])
        } else {
            cmm([p1, q1, r1], [q2, r2, p2])
        }
    } else if dr2 > 0 {
        cmm([p1, q1, r1], [r2, p2, q2])
    } else if dr2 < 0 {
        cmm([p1, r1, q1], [r2, p2, q2])
    } else {
        false
    }
}

/// `CHECK_MIN_MAX` of Guigue and Devillers: the two interval end
/// comparisons.
fn check_min_max(t1: [P; 3], t2: [P; 3], strict: bool) -> bool {
    let [p1, q1, r1] = t1;
    let [p2, q2, r2] = t2;
    let a = det3(sub(q2, q1), sub(p2, q1), sub(p1, q1));
    let b = det3(sub(r2, p1), sub(p2, p1), sub(r1, p1));
    if strict {
        a < 0 && b < 0
    } else {
        a <= 0 && b <= 0
    }
}

/// A triangle of zero area, or a sliver whose lowest height (twice its area
/// over its longest edge) is under [`WELD_MM`]: it bounds nothing, and its
/// neighbours meet across it on their boundaries.
fn is_sliver(p: &[P; 3], n: P) -> bool {
    if n == [0, 0, 0] {
        return true;
    }
    let len = |a: P, b: P| {
        let d = sub(a, b).map(|c| c as f64);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let longest = len(p[0], p[1]).max(len(p[1], p[2])).max(len(p[2], p[0]));
    let twice_area = n.map(|c| c as f64);
    let twice_area = (twice_area[0] * twice_area[0]
        + twice_area[1] * twice_area[1]
        + twice_area[2] * twice_area[2])
        .sqrt();
    twice_area / longest * GRID_MM < WELD_MM
}

/// One triangle prepared for the test.
#[derive(Clone, Copy)]
struct Tri {
    /// Welded vertex ids.
    ids: [u32; 3],
    p: [P; 3],
    axis: usize,
    /// The normal (p1 - p0) x (p2 - p0), rounded to f64, for the fast side
    /// test ([`Tri::side`]).
    n: [f64; 3],
}

impl Tri {
    fn new(ids: [u32; 3], p: [P; 3], n: P) -> Tri {
        Tri {
            ids,
            p,
            axis: drop_axis(n),
            n: n.map(|c| c as f64),
        }
    }

    /// The side of the plane x lies on, as `orient3d(p0, p1, p2, x)` gives it
    /// (minus the sign of n . (x - p0)), and 0 within [`WELD_MM`] of the plane
    /// (the dead band of the module documentation). The f64 dot product is
    /// within 8 eps |n| |x - p0| of the exact one, at most 2e-6 grid steps for
    /// the largest mesh: nothing next to the band's five grid steps.
    fn side(&self, x: P) -> i8 {
        let a = self.p[0];
        let d = [
            (x[0] - a[0]) as f64,
            (x[1] - a[1]) as f64,
            (x[2] - a[2]) as f64,
        ];
        let dot = self.n[0] * d[0] + self.n[1] * d[1] + self.n[2] * d[2];
        let len = (self.n[0] * self.n[0] + self.n[1] * self.n[1] + self.n[2] * self.n[2]).sqrt();
        if dot.abs() <= BAND * len {
            0
        } else if dot > 0.0 {
            -1
        } else {
            1
        }
    }
}

/// The vertices of `t` lie on one side of a plane, given their orientations
/// `o` against it; shared vertices (`shared`) lie in it and are ignored. With
/// no vertex left to decide, they do not.
fn off_plane(o: [i8; 3], shared: [bool; 3]) -> bool {
    let mut sign = 0;
    for k in 0..3 {
        if shared[k] {
            continue;
        }
        if o[k] == 0 || (sign != 0 && o[k] != sign) {
            return false;
        }
        sign = o[k];
    }
    sign != 0
}

/// Whether two triangles intersect by the definition of the module.
fn tri_tri(t1: &Tri, t2: &Tri) -> bool {
    let shared1 = t1.ids.map(|i| t2.ids.contains(&i));
    let shared2 = t2.ids.map(|i| t1.ids.contains(&i));
    let n_shared = shared1.iter().filter(|&&b| b).count();
    let mut s = [0i8; 3];
    for k in 0..3 {
        if !shared2[k] {
            s[k] = t1.side(t2.p[k]);
        }
    }
    // T2 meets T1's plane in shared vertices at most: what they share lies on
    // their boundaries.
    if off_plane(s, shared2) {
        return false;
    }
    if s == [0, 0, 0] {
        // In one plane: only faces that point the same way can be a doubled
        // surface; back to back (a fin, two shells pressed together) they are
        // contact, which keeps inside and outside apart.
        let same_way = t1.n[0] * t2.n[0] + t1.n[1] * t2.n[1] + t1.n[2] * t2.n[2] > 0.0;
        let ax = t1.axis;
        return same_way
            && open_tris_overlap(t1.p.map(|v| project(v, ax)), t2.p.map(|v| project(v, ax)));
    }
    if n_shared >= 2 {
        // Not coplanar, sharing an edge: the planes meet in its line.
        return false;
    }
    let mut r = [0i8; 3];
    for k in 0..3 {
        if !shared1[k] {
            r[k] = t2.side(t1.p[k]);
        }
    }
    if off_plane(r, shared1) {
        return false;
    }
    let edge_hits = (0..3).any(|i| {
        let j = (i + 1) % 3;
        seg_open_tri(t1.p[i], t1.p[j], r[i], r[j], t2)
    }) || (0..3).any(|i| {
        let j = (i + 1) % 3;
        seg_open_tri(t2.p[i], t2.p[j], s[i], s[j], t1)
    });
    if edge_hits {
        return true;
    }
    // Both cross the other's plane: the open stretches they cut from the line
    // the planes share lie inside both, so a stretch of positive length in
    // common is a crossing, even when its ends fall on edges of both.
    let mixed = |o: [i8; 3]| o.contains(&1) && o.contains(&-1);
    mixed(s) && mixed(r) && interval_overlap(t1.p, t2.p, true) && shared_stretch(t1, t2) > BAND
}

/// Axis-aligned box of a triangle, in grid units.
#[derive(Clone, Copy)]
struct Aabb {
    lo: P,
    hi: P,
}

impl Aabb {
    fn of(p: &[P; 3]) -> Aabb {
        let mut lo = p[0];
        let mut hi = p[0];
        for v in &p[1..] {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        Aabb { lo, hi }
    }

    fn join(self, o: Aabb) -> Aabb {
        let mut r = self;
        for k in 0..3 {
            r.lo[k] = r.lo[k].min(o.lo[k]);
            r.hi[k] = r.hi[k].max(o.hi[k]);
        }
        r
    }

    fn overlaps(&self, o: &Aabb) -> bool {
        (0..3).all(|k| self.lo[k] <= o.hi[k] && o.lo[k] <= self.hi[k])
    }
}

/// Bounding-volume hierarchy node: a box and either two children or a run of
/// triangles in `order`.
struct Node {
    bx: Aabb,
    /// Children (left, right), or `None` for a leaf.
    kids: Option<(usize, usize)>,
    start: usize,
    end: usize,
}

const LEAF: usize = 8;

fn build(nodes: &mut Vec<Node>, order: &mut [u32], boxes: &[Aabb], start: usize) -> usize {
    let bx = order[1..]
        .iter()
        .fold(boxes[order[0] as usize], |b, &t| b.join(boxes[t as usize]));
    let id = nodes.len();
    nodes.push(Node {
        bx,
        kids: None,
        start,
        end: start + order.len(),
    });
    if order.len() <= LEAF {
        return id;
    }
    let axis = (0..3).max_by_key(|&k| bx.hi[k] - bx.lo[k]).unwrap_or(0);
    let centre = |t: u32| boxes[t as usize].lo[axis] + boxes[t as usize].hi[axis];
    let mid = order.len() / 2;
    order.select_nth_unstable_by_key(mid, |&t| (centre(t), t));
    let (left, right) = order.split_at_mut(mid);
    let l = build(nodes, left, boxes, start);
    let r = build(nodes, right, boxes, start + mid);
    nodes[id].kids = Some((l, r));
    id
}

struct Join<'a> {
    nodes: &'a [Node],
    order: &'a [u32],
    tris: &'a [Tri],
    boxes: &'a [Aabb],
    out: SelfIntersections,
}

impl Join<'_> {
    fn pair(&mut self, a: u32, b: u32) {
        let (ba, bb) = (&self.boxes[a as usize], &self.boxes[b as usize]);
        if !ba.overlaps(bb) {
            return;
        }
        self.out.candidates += 1;
        if tri_tri(&self.tris[a as usize], &self.tris[b as usize]) {
            self.out.pairs += 1;
            let p = [a.min(b) as usize, a.max(b) as usize];
            if self.out.first.is_none_or(|f| p < f) {
                self.out.first = Some(p);
            }
        }
    }

    fn within(&mut self, n: usize) {
        match self.nodes[n].kids {
            Some((l, r)) => {
                self.within(l);
                self.within(r);
                self.across(l, r);
            }
            None => {
                let (s, e) = (self.nodes[n].start, self.nodes[n].end);
                for i in s..e {
                    for j in i + 1..e {
                        self.pair(self.order[i], self.order[j]);
                    }
                }
            }
        }
    }

    fn across(&mut self, a: usize, b: usize) {
        if !self.nodes[a].bx.overlaps(&self.nodes[b].bx) {
            return;
        }
        match (self.nodes[a].kids, self.nodes[b].kids) {
            (None, None) => {
                for i in self.nodes[a].start..self.nodes[a].end {
                    for j in self.nodes[b].start..self.nodes[b].end {
                        self.pair(self.order[i], self.order[j]);
                    }
                }
            }
            (Some((l, r)), None) => {
                self.across(l, b);
                self.across(r, b);
            }
            (None, Some((l, r))) => {
                self.across(a, l);
                self.across(a, r);
            }
            (Some((al, ar)), Some((bl, br))) => {
                // Split the larger box first.
                let size = |n: usize| {
                    let bx = &self.nodes[n].bx;
                    (0..3).map(|k| bx.hi[k] - bx.lo[k]).max().unwrap_or(0)
                };
                if size(a) >= size(b) {
                    self.across(al, b);
                    self.across(ar, b);
                } else {
                    self.across(a, bl);
                    self.across(a, br);
                }
            }
        }
    }
}

/// Test a mesh for self-intersections (see the module documentation).
pub fn self_intersections(mesh: &Mesh) -> SelfIntersections {
    let mut out = SelfIntersections::default();
    // Weld near-twins, then snap the representatives to the grid.
    let (map, reps) = weld(&mesh.positions);
    let mut snapped: Vec<P> = Vec::with_capacity(reps.len());
    for r in &reps {
        let (Some(x), Some(y), Some(z)) = (snap(r[0]), snap(r[1]), snap(r[2])) else {
            out.out_of_range = true;
            return out;
        };
        snapped.push([x, y, z]);
    }
    let mut tris = Vec::with_capacity(mesh.triangle_count());
    for t in mesh.indices.chunks_exact(3) {
        let ids = [t[0], t[1], t[2]].map(|i| map.get(i as usize).copied());
        let [Some(i0), Some(i1), Some(i2)] = ids else {
            out.degenerate += 1;
            // Keep the triangle numbers aligned with the mesh: a placeholder
            // that is never tested.
            tris.push(None);
            continue;
        };
        let p = [i0, i1, i2].map(|i| snapped[i as usize]);
        let n = normal(p[0], p[1], p[2]);
        if is_sliver(&p, n) {
            out.degenerate += 1;
            tris.push(None);
            continue;
        }
        tris.push(Some(Tri::new([i0, i1, i2], p, n)));
    }
    let live: Vec<u32> = (0..tris.len() as u32)
        .filter(|&i| tris[i as usize].is_some())
        .collect();
    if live.len() < 2 {
        return out;
    }
    let placeholder = Tri::new([u32::MAX; 3], [[0; 3]; 3], [0, 0, 1]);
    let tris: Vec<Tri> = tris.into_iter().map(|t| t.unwrap_or(placeholder)).collect();
    let boxes: Vec<Aabb> = tris.iter().map(|t| Aabb::of(&t.p)).collect();
    let mut order = live;
    let mut nodes = Vec::with_capacity(2 * order.len() / LEAF + 1);
    build(&mut nodes, &mut order, &boxes, 0);
    let mut join = Join {
        nodes: &nodes,
        order: &order,
        tris: &tris,
        boxes: &boxes,
        out,
    };
    join.within(0);
    join.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_of_near_twins_welds_into_one_vertex() {
        // A, B and C 8 nm apart in x: A to C is 16 nm, beyond WELD_MM, but the
        // chain joins them, in whatever order they come. D is far away.
        let (a, b, c) = (1.0f32, 1.0 + 8e-6, 1.0 + 16e-6);
        let d = [2.0f32, 1.0, 1.0];
        for (order, first) in [
            ([a, b, c], a),
            ([a, c, b], a),
            ([c, a, b], c),
            ([b, c, a], b),
        ] {
            let mut pts: Vec<[f32; 3]> = order.iter().map(|&x| [x, 1.0, 1.0]).collect();
            pts.push(d);
            let (map, reps) = weld(&pts);
            assert_eq!(map, vec![0, 0, 0, 1], "{order:?}");
            assert_eq!(
                reps,
                vec![[f64::from(first), 1.0, 1.0], d.map(f64::from)],
                "{order:?}"
            );
        }
        // A long chain, 273 nm end to end, is still one vertex.
        let pts: Vec<[f32; 3]> = (0..40).map(|i| [1.0 + i as f32 * 7e-6, 1.0, 1.0]).collect();
        let (map, reps) = weld(&pts);
        assert!(map.iter().all(|&m| m == 0) && reps.len() == 1, "{map:?}");
    }

    /// An axis-aligned box as a closed mesh (12 triangles, outward winding).
    fn cube(min: [f32; 3], size: f32) -> Mesh {
        let [x, y, z] = min;
        let s = size;
        let positions = vec![
            [x, y, z],
            [x + s, y, z],
            [x + s, y + s, z],
            [x, y + s, z],
            [x, y, z + s],
            [x + s, y, z + s],
            [x + s, y + s, z + s],
            [x, y + s, z + s],
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, // bottom
            4, 5, 6, 4, 6, 7, // top
            0, 1, 5, 0, 5, 4, // front
            1, 2, 6, 1, 6, 5, // right
            2, 3, 7, 2, 7, 6, // back
            3, 0, 4, 3, 4, 7, // left
        ];
        Mesh { positions, indices }
    }

    fn merged(a: &Mesh, b: &Mesh) -> Mesh {
        let mut m = a.clone();
        let base = m.positions.len() as u32;
        m.positions.extend(&b.positions);
        m.indices.extend(b.indices.iter().map(|i| i + base));
        m
    }

    #[test]
    fn a_closed_box_does_not_intersect_itself() {
        let r = self_intersections(&cube([0.0; 3], 8.0));
        assert_eq!((r.pairs, r.first, r.degenerate), (0, None, 0));
        assert!(!r.out_of_range);
    }

    #[test]
    fn two_overlapping_boxes_in_one_mesh_intersect() {
        let m = merged(&cube([0.0; 3], 8.0), &cube([4.0, 4.0, 4.0], 8.0));
        let r = self_intersections(&m);
        assert!(r.pairs > 0, "{r:?}");
        let [a, b] = r.first.expect("a pair");
        assert!(a < 12 && b >= 12, "one triangle of each box: {a} {b}");
    }

    #[test]
    fn boxes_apart_do_not_intersect_and_boxes_touching_do() {
        let apart = merged(&cube([0.0; 3], 8.0), &cube([9.0, 0.0, 0.0], 8.0));
        assert_eq!(self_intersections(&apart).pairs, 0);
        // Face to face, back to back: contact, not a crossing.
        let touching = merged(&cube([0.0; 3], 8.0), &cube([8.0, 0.0, 0.0], 8.0));
        assert_eq!(self_intersections(&touching).pairs, 0);
        // A face doubled the same way round is counted.
        let mut doubled = cube([0.0; 3], 8.0);
        doubled.indices.extend_from_slice(&[0, 2, 1]);
        assert_eq!(self_intersections(&doubled).pairs, 1);
        // Separated by a little more than the welding distance, at a
        // coordinate small enough for f32 to hold the gap.
        let gap = (WELD_MM * 2.0) as f32;
        let near = merged(&cube([-0.5, 0.0, 0.0], 0.5), &cube([gap, 0.0, 0.0], 0.5));
        assert_eq!(self_intersections(&near).pairs, 0);
    }

    #[test]
    fn a_box_inside_another_does_not_touch_it() {
        // Nested shells without contact are not a self-intersection (a hollow
        // solid has two shells); MESH.MANIFOLD and the volume rule judge them.
        let m = merged(&cube([0.0; 3], 8.0), &cube([2.0; 3], 4.0));
        assert_eq!(self_intersections(&m).pairs, 0);
    }

    #[test]
    fn shared_vertices_count_by_position_and_a_pierce_through_a_shared_vertex_is_found() {
        // Two triangles sharing a corner by position only (separate indices),
        // meeting nowhere else: no intersection.
        let m = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
                [0.0, 0.0, 0.0],
                [-4.0, 0.0, 1.0],
                [0.0, -4.0, 1.0],
            ],
            indices: vec![0, 1, 2, 3, 4, 5],
        };
        assert_eq!(self_intersections(&m).pairs, 0);
        // A triangle from the shared corner that passes through the first.
        let m = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
                [2.0, 2.0, -1.0],
                [1.0, 1.0, 1.0],
            ],
            indices: vec![0, 1, 2, 0, 3, 4],
        };
        assert_eq!(self_intersections(&m).pairs, 1);
        // Coplanar, sharing a corner, the second's edge running inside the
        // first: the wedge test finds it.
        let m = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 3.0, 0.0],
            ],
            indices: vec![0, 1, 2, 0, 3, 4],
        };
        assert_eq!(self_intersections(&m).pairs, 1);
    }

    #[test]
    fn a_fin_folded_over_a_shared_edge_and_a_hinge_do_not_intersect() {
        let fold = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [2.0, 3.0, 0.0],
                [2.0, 1.0, 0.0],
            ],
            indices: vec![0, 1, 2, 1, 0, 3],
        };
        // Folded flat, the two face opposite ways: a zero-thickness fin.
        assert_eq!(self_intersections(&fold).pairs, 0);
        // Wound the same way round (an inconsistent mesh), they overlap.
        let flipped = Mesh {
            indices: vec![0, 1, 2, 0, 1, 3],
            ..fold.clone()
        };
        assert_eq!(self_intersections(&flipped).pairs, 1);
        let hinge = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [2.0, 3.0, 0.0],
                [2.0, -3.0, 0.0],
            ],
            indices: vec![0, 1, 2, 1, 0, 3],
        };
        assert_eq!(self_intersections(&hinge).pairs, 0);
    }

    #[test]
    fn degenerate_triangles_and_out_of_range_meshes_are_reported() {
        let m = Mesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]],
            indices: vec![0, 1, 2],
        };
        assert_eq!(self_intersections(&m).degenerate, 1);
        // A near-collinear sliver (the one measured on an inlay tile's side
        // face: 13.166667 lies 1e-7 mm off the line) beside a proper triangle
        // on its long edge: as a triangle it would fold over its neighbour.
        let m = Mesh {
            positions: vec![
                [13.166_667, 31.8, 2.8],
                [0.0, 31.8, 2.3],
                [0.0, 31.8, 2.393_75],
                [15.8, 31.8, 2.9],
            ],
            indices: vec![0, 1, 2, 3, 1, 0],
        };
        let r = self_intersections(&m);
        assert_eq!((r.pairs, r.degenerate), (0, 1), "{r:?}");
        let far = cube([2000.0, 0.0, 0.0], 1.0);
        assert!(self_intersections(&far).out_of_range);
    }

    #[test]
    fn contact_on_boundaries_only_is_not_an_intersection() {
        // A wall triangle whose bottom edge runs along a chamfer triangle's
        // top edge (the T-junction a zero-area sliver leaves between them).
        let m = Mesh {
            positions: vec![
                [1.2, 30.6, 0.2],
                [3.5, 30.6, 0.2],
                [3.5, 30.6, 1.9],
                [1.0, 30.8, 0.0],
                [14.6, 30.6, 0.2],
            ],
            indices: vec![0, 1, 2, 3, 4, 0],
        };
        assert_eq!(self_intersections(&m).pairs, 0);
        // An emboss wall standing on the top face, its corner rounded to f32 a
        // micrometre inside the face's edge (measured on the brand badge): it
        // touches, it does not cross.
        let m = Mesh {
            positions: vec![
                [23.973_263, 7.047_463, 3.2],
                [24.355_96, 7.255_667, 3.2],
                [23.949_85, 7.041_322, 3.2],
                [23.994_526, 7.059_031, 3.8],
                [23.994_526, 7.059_031, 3.2],
            ],
            indices: vec![0, 1, 2, 3, 4, 0],
        };
        assert_eq!(self_intersections(&m).pairs, 0);
        // Two triangles crossing exactly along a line from edge to edge of
        // both (the diagonal of one face and the edge of another): found by
        // the interval test.
        let m = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
                [2.0, -2.0, -1.0],
                [2.0, 6.0, 1.0],
                [-2.0, 2.0, 0.0],
            ],
            indices: vec![0, 1, 2, 3, 4, 5],
        };
        let r = self_intersections(&m);
        assert_eq!(r.pairs, 1, "{r:?}");
    }

    /// A tiny deterministic generator (64-bit LCG, Knuth's constants).
    struct Lcg(u64);
    impl Lcg {
        fn small(&mut self) -> i64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) % 7) as i64 - 3
        }
        fn point(&mut self) -> P {
            [self.small(), self.small(), self.small()]
        }
    }

    /// A rational number num / den with den > 0.
    #[derive(Clone, Copy)]
    struct Ratio(i128, i128);
    impl Ratio {
        fn lt(self, o: Ratio) -> bool {
            self.0 * o.1 < o.0 * self.1
        }
        fn le(self, o: Ratio) -> bool {
            self.0 * o.1 <= o.0 * self.1
        }
    }

    fn dot(a: [i128; 3], b: [i128; 3]) -> i128 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn wide(p: P) -> [i128; 3] {
        p.map(i128::from)
    }

    /// Where `t` meets the plane through `o` with normal `n`, as an interval
    /// of the parameter d . x along the line direction `d`.
    fn interval(t: [P; 3], o: P, n: [i128; 3], d: [i128; 3]) -> (Ratio, Ratio) {
        let f = |x: P| dot(n, wide(sub(x, o)));
        let mut pts: Vec<Ratio> = Vec::new();
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            let (fa, fb) = (f(a), f(b));
            if fa == 0 {
                pts.push(Ratio(dot(d, wide(a)), 1));
            }
            if fa * fb < 0 {
                // x = a + fa / (fa - fb) (b - a)
                let mut num = dot(d, wide(a)) * (fa - fb) + fa * dot(d, wide(sub(b, a)));
                let mut den = fa - fb;
                if den < 0 {
                    num = -num;
                    den = -den;
                }
                pts.push(Ratio(num, den));
            }
        }
        let lo = pts
            .iter()
            .copied()
            .fold(pts[0], |m, x| if x.lt(m) { x } else { m });
        let hi = pts
            .iter()
            .copied()
            .fold(pts[0], |m, x| if m.lt(x) { x } else { m });
        (lo, hi)
    }

    #[test]
    fn the_interval_test_agrees_with_exact_rationals() {
        // Property test: random triangles on a small integer grid, every pair
        // where both cross the other's plane, against the overlap of their
        // intervals computed in exact rational arithmetic.
        let mut g = Lcg(0x0b51_c0de);
        let mut checked = 0;
        for _ in 0..200_000 {
            let t1 = [g.point(), g.point(), g.point()];
            let t2 = [g.point(), g.point(), g.point()];
            let n1 = normal(t1[0], t1[1], t1[2]).map(i128::from);
            let n2 = normal(t2[0], t2[1], t2[2]).map(i128::from);
            if n1 == [0; 3] || n2 == [0; 3] {
                continue;
            }
            let s = t2.map(|x| orient3d(t1[0], t1[1], t1[2], x));
            let r = t1.map(|x| orient3d(t2[0], t2[1], t2[2], x));
            let mixed = |o: [i8; 3]| o.contains(&1) && o.contains(&-1);
            if !(mixed(s) && mixed(r)) {
                continue;
            }
            let d = [
                n1[1] * n2[2] - n1[2] * n2[1],
                n1[2] * n2[0] - n1[0] * n2[2],
                n1[0] * n2[1] - n1[1] * n2[0],
            ];
            let (a1, b1) = interval(t1, t2[0], n2, d);
            let (a2, b2) = interval(t2, t1[0], n1, d);
            let lo = if a1.lt(a2) { a2 } else { a1 };
            let hi = if b1.lt(b2) { b1 } else { b2 };
            assert_eq!(
                interval_overlap(t1, t2, true),
                lo.lt(hi),
                "strict {t1:?} {t2:?}"
            );
            assert_eq!(
                interval_overlap(t1, t2, false),
                lo.le(hi),
                "closed {t1:?} {t2:?}"
            );
            checked += 1;
        }
        assert!(checked > 10_000, "{checked} crossing pairs");
    }

    #[test]
    fn the_side_test_agrees_with_the_exact_one_outside_its_band() {
        let mut g = Lcg(0x5eed);
        for scale in [1_i64, 1 << 20, (1 << 29) / 3] {
            for _ in 0..20_000 {
                let p = |g: &mut Lcg| [g.small() * scale, g.small() * scale, g.small() * scale];
                let t = [p(&mut g), p(&mut g), p(&mut g)];
                let n = normal(t[0], t[1], t[2]);
                if n == [0; 3] {
                    continue;
                }
                let tri = Tri::new([0, 1, 2], t, n);
                let x = p(&mut g);
                let exact = orient3d(t[0], t[1], t[2], x);
                let side = tri.side(x);
                // Points on the plane read as on it; points off it read on
                // their side, unless within the band (only at scale 1, where
                // the grid points are a nanometre apart).
                assert!(side == exact || (side == 0 && scale == 1), "{t:?} {x:?}");
                if exact == 0 {
                    assert_eq!(side, 0);
                }
            }
        }
        // 10 nm off a plane at the origin is in the band; 20 nm is not.
        let t = [[0, 0, 0], [1 << 20, 0, 0], [0, 1 << 20, 0]];
        let tri = Tri::new([0, 1, 2], t, normal(t[0], t[1], t[2]));
        let nm = |n: f64| (n * 1e-6 / GRID_MM).round() as i64;
        assert_eq!(tri.side([5, 5, nm(9.0)]), 0);
        assert_ne!(tri.side([5, 5, nm(20.0)]), 0);
    }

    #[test]
    fn the_result_does_not_depend_on_triangle_order() {
        let m = merged(&cube([0.0; 3], 8.0), &cube([4.0, 4.0, 4.0], 8.0));
        let mut rev = m.clone();
        let tris: Vec<[u32; 3]> = m
            .indices
            .chunks_exact(3)
            .rev()
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        rev.indices = tris.concat();
        assert_eq!(self_intersections(&m).pairs, self_intersections(&rev).pairs);
    }
}
