//! Downward-facing regions of a mesh in its print orientation (rules
//! `ROOF.BRIDGE`, `OVERHANG.MAX` and `OVERHANG.BRIDGED`, docs/plan/05 section
//! 5): which faces hang steeper than a process can print, where material holds
//! them, and the span a bridge across each has.
//!
//! **Steep faces.** The mesh is in print orientation, +Z up, the bed at its
//! lowest point. A triangle hangs at the angle between its plane and the
//! vertical: 0 degrees for a wall, 90 for a ceiling, asin(-nz) for a unit
//! normal pointing down. Triangles steeper than `max_overhang_deg` are
//! *steep*, except those the bed holds up: every corner at most `on_bed_mm`
//! (plus [`BED_TOLERANCE_MM`]) above the lowest point. The evaluator passes
//! the first-layer chamfer height there: features start one chamfer up so the
//! chamfer stays clear (the wall ribs of openbrick-part do), and the squashed
//! first layer that the chamfer makes room for carries their undersides
//! (docs/plan/05 section 6). Slivers lower than the welding distance and
//! regions smaller than [`MIN_REGION_AREA_MM2`] are kernel residue at seams
//! and are left out.
//!
//! **Regions and anchors.** Steep triangles that share an edge form one
//! region. An edge of a region's rim is *anchored* when the triangle beyond it
//! reaches below the edge: there is material under that side (a wall, a tube, a
//! chamfer running down to the bed). Other rim edges are *free*: the open end
//! of a tunnel or of a horizontal hole, the outer edge of a ledge, whose
//! neighbour rises from it. Vertices are welded first
//! ([`crate::intersect::weld`]), so adjacency follows positions, not indices,
//! and a kernel's near-twin vertices leave no sliver behind.
//!
//! **Span.** A slicer lays bridge strands from anchor to anchor across the
//! shortest way, so the length a region asks a strand to span is twice the
//! farthest distance from any point of the region (seen from above) to its
//! nearest anchored rim edge. A ceiling held all round, W x L with W <= L,
//! spans W; a disc its diameter; a tunnel W wide, open at both ends, W; a ledge
//! d deep held along one side 2 d, the strand that would hold its outer edge
//! from both sides. A region with no anchored rim edge floats: it has no span
//! and always needs support. The farthest point is found with the
//! pole-of-inaccessibility search of Garcia-Castellanos and Lombardo (2007,
//! the "polylabel" quadtree method): square cells are refined best-first until
//! no cell can hold a point more than [`SPAN_PRECISION_MM`] / 2 farther from
//! the anchors than the best one found, so the span is exact to
//! [`SPAN_PRECISION_MM`] and never overstated (a part of a region thinner than
//! that precision may be missed). Distances go to rim edges
//! through a uniform grid; inside tests count rim crossings of a ray
//! (even-odd), so the region seen from above must not overlap itself (true of
//! a region of faces pointing down unless it winds over itself).

use std::collections::{BinaryHeap, HashMap};

use crate::Mesh;

/// Triangles whose corners all lie this close to the lowest point of the mesh
/// rest on the bed, in millimetres: 0.1 micrometre, far below a layer and far
/// above f32 rounding at bed height.
pub const BED_TOLERANCE_MM: f64 = 1e-4;

/// The span of a region is found to this precision, in millimetres (the
/// reported span is at most this much short of the true one).
pub const SPAN_PRECISION_MM: f64 = 0.005;

/// A triangle's neighbour reaches below a rim edge when its third corner is at
/// least this much lower than the edge's lower end, in millimetres.
const BELOW_MM: f64 = 1e-6;

/// Upper bound on cell evaluations of one span search: a resource guard. A
/// search that hits it reports the best span found so far, which is never more
/// than the true span.
const MAX_SPAN_EVALUATIONS: usize = 200_000;

/// Regions with less face area than this, in mm², are left out: a thousandth
/// of a square millimetre (a 0.03 mm square) is far below what a 0.4 mm line,
/// 0.16 mm² a step, can print or sag, and such specks are kernel residue at
/// seams, not shape.
pub const MIN_REGION_AREA_MM2: f64 = 1e-3;

/// One connected region of steep downward faces.
#[derive(Clone, Debug, PartialEq)]
pub struct DownRegion {
    /// Number of triangles.
    pub triangles: usize,
    /// Area of the faces themselves, mm².
    pub area_mm2: f64,
    /// Lowest and highest corner, mm, in the frame of the mesh.
    pub z_min_mm: f64,
    pub z_max_mm: f64,
    /// The steepest face, degrees from vertical (90 for a ceiling).
    pub max_angle_deg: f64,
    /// Length of the rim with no material below it, mm (0 for a ceiling held
    /// all round).
    pub free_rim_mm: f64,
    /// The span (see the module documentation), mm; `None` when no rim edge is
    /// anchored (a floating region).
    pub span_mm: Option<f64>,
    /// The point farthest from the anchors (x, y in mm), or the centre of the
    /// region's bounding box when it floats.
    pub centre_mm: [f64; 2],
}

/// The steep downward regions of `mesh` in print orientation (see the module
/// documentation), sorted by height, then position. Faces at most `on_bed_mm`
/// above the bed count as held by it.
pub fn downward_regions(mesh: &Mesh, max_overhang_deg: f64, on_bed_mm: f64) -> Vec<DownRegion> {
    let n_tri = mesh.triangle_count();
    if n_tri == 0 {
        return Vec::new();
    }
    let (canon, pos) = crate::intersect::weld(&mesh.positions);
    let bed_zone = on_bed_mm.max(0.0) + BED_TOLERANCE_MM;
    let z_bed = pos.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let tri = |t: usize| -> Option<[u32; 3]> {
        let i = mesh.indices.get(3 * t..3 * t + 3)?;
        Some([
            *canon.get(i[0] as usize)?,
            *canon.get(i[1] as usize)?,
            *canon.get(i[2] as usize)?,
        ])
    };
    let sin_max = max_overhang_deg.to_radians().sin();
    // Steep triangles, with their angle and area.
    let mut steep: Vec<(usize, f64, f64)> = Vec::new();
    for t in 0..n_tri {
        let Some(v) = tri(t) else { continue };
        let [a, b, c] = v.map(|i| pos[i as usize]);
        if [a, b, c].iter().all(|p| p[2] <= z_bed + bed_zone) {
            continue;
        }
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * w[2] - u[2] * w[1],
            u[2] * w[0] - u[0] * w[2],
            u[0] * w[1] - u[1] * w[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len == 0.0 || !len.is_finite() {
            continue;
        }
        // A sliver lower than the welding distance (twice its area over its
        // longest edge) has no direction worth the name: the kernel's
        // near-collinear triangles, as in `crate::intersect`.
        let edge = |p: [f64; 3], q: [f64; 3]| {
            ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
        };
        let longest = edge(a, b).max(edge(b, c)).max(edge(c, a));
        if len / longest < crate::intersect::WELD_MM {
            continue;
        }
        let down = -n[2] / len;
        if down > sin_max + 1e-12 {
            let angle = down.clamp(-1.0, 1.0).asin().to_degrees();
            steep.push((t, angle, len / 2.0));
        }
    }
    if steep.is_empty() {
        return Vec::new();
    }
    // Union-find over steep triangles sharing an edge.
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let mut edge_tris: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (k, &(t, _, _)) in steep.iter().enumerate() {
        let Some(v) = tri(t) else { continue };
        for e in 0..3 {
            edge_tris
                .entry(key(v[e], v[(e + 1) % 3]))
                .or_default()
                .push(k);
        }
    }
    let mut parent: Vec<usize> = (0..steep.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for list in edge_tris.values() {
        for w in list.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    // Rim edges lie on exactly one steep triangle. Whether a non-steep
    // triangle beyond each reaches below it.
    let rim: HashMap<(u32, u32), usize> = edge_tris
        .iter()
        .filter(|(_, l)| l.len() == 1)
        .map(|(e, l)| (*e, l[0]))
        .collect();
    let mut on_rim = vec![false; pos.len()];
    for &(a, b) in rim.keys() {
        on_rim[a as usize] = true;
        on_rim[b as usize] = true;
    }
    let mut is_steep = vec![false; n_tri];
    for &(t, _, _) in &steep {
        is_steep[t] = true;
    }
    let mut anchored_edge: HashMap<(u32, u32), bool> = HashMap::new();
    for (t, &skip) in is_steep.iter().enumerate() {
        if skip {
            continue;
        }
        let Some(v) = tri(t) else { continue };
        if !v.iter().any(|&i| on_rim[i as usize]) {
            continue;
        }
        for e in 0..3 {
            let (a, b, c) = (v[e], v[(e + 1) % 3], v[(e + 2) % 3]);
            let k = key(a, b);
            if rim.contains_key(&k) {
                let low = pos[a as usize][2].min(pos[b as usize][2]);
                let below = pos[c as usize][2] < low - BELOW_MM;
                *anchored_edge.entry(k).or_insert(false) |= below;
            }
        }
    }
    // Triangles and rim edges by region.
    type Group = (Vec<usize>, Vec<((u32, u32), bool)>);
    let mut groups: HashMap<usize, Group> = HashMap::new();
    for k in 0..steep.len() {
        let r = find(&mut parent, k);
        groups.entry(r).or_default().0.push(k);
    }
    for (e, &k) in &rim {
        let r = find(&mut parent, k);
        let g = groups.get_mut(&r).expect("the region of a rim edge exists");
        g.1.push((*e, anchored_edge.get(e).copied().unwrap_or(false)));
    }
    let mut out: Vec<DownRegion> = groups
        .into_values()
        .map(|(members, mut edges)| {
            edges.sort_unstable();
            let mut z_min = f64::INFINITY;
            let mut z_max = f64::NEG_INFINITY;
            let mut area = 0.0;
            let mut angle: f64 = 0.0;
            let mut tris2 = Vec::with_capacity(members.len());
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for &k in &members {
                let (t, a, ar) = steep[k];
                angle = angle.max(a);
                area += ar;
                if let Some(v) = tri(t) {
                    let p = v.map(|i| pos[i as usize]);
                    for q in &p {
                        z_min = z_min.min(q[2]);
                        z_max = z_max.max(q[2]);
                        for d in 0..2 {
                            lo[d] = lo[d].min(q[d]);
                            hi[d] = hi[d].max(q[d]);
                        }
                    }
                    tris2.push(p.map(|q| [q[0], q[1]]));
                }
            }
            let flat = |&((a, b), _): &((u32, u32), bool)| {
                let (p, q) = (pos[a as usize], pos[b as usize]);
                [[p[0], p[1]], [q[0], q[1]]]
            };
            let rim2: Vec<[[f64; 2]; 2]> = edges.iter().map(flat).collect();
            let anchors: Vec<[[f64; 2]; 2]> = edges.iter().filter(|e| e.1).map(flat).collect();
            let free_rim: f64 = edges
                .iter()
                .filter(|e| !e.1)
                .map(|&((a, b), _)| {
                    let (p, q) = (pos[a as usize], pos[b as usize]);
                    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
                })
                .sum();
            let (span, centre) = if anchors.is_empty() {
                (None, [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
            } else {
                let (r, c) = farthest_from_anchors(&rim2, &anchors, lo, hi, &tris2);
                (Some(2.0 * r), c)
            };
            DownRegion {
                triangles: members.len(),
                area_mm2: area,
                z_min_mm: z_min,
                z_max_mm: z_max,
                max_angle_deg: angle,
                free_rim_mm: free_rim,
                span_mm: span,
                centre_mm: centre,
            }
        })
        .collect();
    out.retain(|r| r.area_mm2 >= MIN_REGION_AREA_MM2);
    out.sort_by(|a, b| {
        a.z_min_mm
            .total_cmp(&b.z_min_mm)
            .then(a.centre_mm[0].total_cmp(&b.centre_mm[0]))
            .then(a.centre_mm[1].total_cmp(&b.centre_mm[1]))
            .then(a.triangles.cmp(&b.triangles))
    });
    out
}

/// Rim edges in a uniform grid, for distance and inside queries.
struct EdgeGrid<'a> {
    edges: &'a [[[f64; 2]; 2]],
    lo: [f64; 2],
    cell: f64,
    dims: [usize; 2],
    /// Edges per cell (by bounding box).
    cells: Vec<Vec<u32>>,
    /// Edges per row (by y range), for the inside test.
    rows: Vec<Vec<u32>>,
}

impl<'a> EdgeGrid<'a> {
    fn new(edges: &'a [[[f64; 2]; 2]], lo: [f64; 2], hi: [f64; 2]) -> EdgeGrid<'a> {
        let w = (hi[0] - lo[0]).max(1e-9);
        let h = (hi[1] - lo[1]).max(1e-9);
        // About two edges per cell, at most 512 cells a side.
        let cell = (w * h / (edges.len().max(1) as f64 / 2.0))
            .sqrt()
            .max(w.max(h) / 512.0)
            .max(1e-6);
        let dims = [
            ((w / cell).ceil() as usize).max(1),
            ((h / cell).ceil() as usize).max(1),
        ];
        let mut g = EdgeGrid {
            edges,
            lo,
            cell,
            dims,
            cells: vec![Vec::new(); dims[0] * dims[1]],
            rows: vec![Vec::new(); dims[1]],
        };
        for (i, e) in edges.iter().enumerate() {
            let x0 = g.ix(e[0][0].min(e[1][0]), 0);
            let x1 = g.ix(e[0][0].max(e[1][0]), 0);
            let y0 = g.ix(e[0][1].min(e[1][1]), 1);
            let y1 = g.ix(e[0][1].max(e[1][1]), 1);
            for y in y0..=y1 {
                g.rows[y].push(i as u32);
                for x in x0..=x1 {
                    g.cells[y * dims[0] + x].push(i as u32);
                }
            }
        }
        g
    }

    /// The cell index of a coordinate along axis `k`, clamped to the grid.
    fn ix(&self, v: f64, k: usize) -> usize {
        let i = ((v - self.lo[k]) / self.cell).floor();
        if i <= 0.0 {
            0
        } else {
            (i as usize).min(self.dims[k] - 1)
        }
    }

    /// Distance from p to the nearest edge.
    fn distance(&self, p: [f64; 2]) -> f64 {
        let (cx, cy) = (self.ix(p[0], 0) as isize, self.ix(p[1], 1) as isize);
        let mut best = f64::INFINITY;
        let max_ring = self.dims[0].max(self.dims[1]) as isize;
        for ring in 0..=max_ring {
            // Every cell of this ring is at least (ring - 1) cells away.
            if (ring as f64 - 1.0) * self.cell > best {
                break;
            }
            for y in cy - ring..=cy + ring {
                if y < 0 || y >= self.dims[1] as isize {
                    continue;
                }
                for x in cx - ring..=cx + ring {
                    if (y - cy).abs() != ring && (x - cx).abs() != ring {
                        continue;
                    }
                    if x < 0 || x >= self.dims[0] as isize {
                        continue;
                    }
                    for &e in &self.cells[y as usize * self.dims[0] + x as usize] {
                        best = best.min(seg_distance(p, self.edges[e as usize]));
                    }
                }
            }
        }
        best
    }

    /// p lies inside the edges (even-odd count of crossings to the right). A
    /// point outside the grid's rows is outside.
    fn inside(&self, p: [f64; 2]) -> bool {
        let mut inside = false;
        for &e in &self.rows[self.ix(p[1], 1)] {
            let [a, b] = self.edges[e as usize];
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if x > p[0] {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

fn seg_distance(p: [f64; 2], e: [[f64; 2]; 2]) -> f64 {
    let [a, b] = e;
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    (x * x + y * y).sqrt()
}

/// A square cell of the span search, ordered by the largest distance to the
/// anchors that a point of it inside the region could still have.
struct Cell {
    c: [f64; 2],
    half: f64,
    /// Distance from the centre to the anchors, or minus infinity when the
    /// centre is outside the region.
    d: f64,
    max: f64,
}

impl PartialEq for Cell {
    fn eq(&self, o: &Cell) -> bool {
        self.cmp(o) == std::cmp::Ordering::Equal
    }
}
impl Eq for Cell {}
impl PartialOrd for Cell {
    fn partial_cmp(&self, o: &Cell) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cell {
    fn cmp(&self, o: &Cell) -> std::cmp::Ordering {
        self.max
            .total_cmp(&o.max)
            .then(o.c[0].total_cmp(&self.c[0]))
            .then(o.c[1].total_cmp(&self.c[1]))
    }
}

struct Search<'a> {
    rim: EdgeGrid<'a>,
    anchors: EdgeGrid<'a>,
}

impl Search<'_> {
    fn cell(&self, c: [f64; 2], half: f64) -> Option<Cell> {
        let reach = half * std::f64::consts::SQRT_2;
        let inside = self.rim.inside(c);
        // A cell whose centre is outside and farther from the rim than its
        // corners holds no point of the region.
        if !inside && self.rim.distance(c) > reach {
            return None;
        }
        let to_anchor = self.anchors.distance(c);
        Some(Cell {
            c,
            half,
            d: if inside { to_anchor } else { f64::NEG_INFINITY },
            max: to_anchor + reach,
        })
    }
}

/// The largest distance from a point of the region (inside `rim`) to the
/// nearest of `anchors`, and that point (see the module documentation). `tris`
/// seeds the search with a point known to be inside.
fn farthest_from_anchors(
    rim: &[[[f64; 2]; 2]],
    anchors: &[[[f64; 2]; 2]],
    lo: [f64; 2],
    hi: [f64; 2],
    tris: &[[[f64; 2]; 3]],
) -> (f64, [f64; 2]) {
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let size = w.min(h);
    if rim.is_empty() || anchors.is_empty() || size <= 0.0 || !w.max(h).is_finite() {
        return (0.0, middle);
    }
    let s = Search {
        rim: EdgeGrid::new(rim, lo, hi),
        anchors: EdgeGrid::new(anchors, lo, hi),
    };
    // Seed: the centroid of the largest triangle, inside the region.
    let seed = tris
        .iter()
        .max_by(|a, b| tri_area(a).total_cmp(&tri_area(b)))
        .map(|t| {
            [
                (t[0][0] + t[1][0] + t[2][0]) / 3.0,
                (t[0][1] + t[1][1] + t[2][1]) / 3.0,
            ]
        })
        .unwrap_or(middle);
    let mut best = (s.anchors.distance(seed), seed);
    // One square cell over the whole region to start (a thin region would
    // otherwise need very many cells of its width).
    let mut queue = BinaryHeap::new();
    queue.extend(s.cell(middle, w.max(h) / 2.0));
    let mut evaluations = 1;
    let precision = SPAN_PRECISION_MM / 2.0;
    while let Some(cell) = queue.pop() {
        if cell.d > best.0 {
            best = (cell.d, cell.c);
        }
        // Ordered by max: no cell left can beat the best by more.
        if cell.max - best.0 <= precision || evaluations >= MAX_SPAN_EVALUATIONS {
            break;
        }
        let q = cell.half / 2.0;
        if q * std::f64::consts::SQRT_2 < precision / 4.0 {
            // Finer than the precision: a point of the region in this cell is
            // within it of the centre, which is outside; leave it.
            continue;
        }
        for (dx, dy) in [(-q, -q), (q, -q), (-q, q), (q, q)] {
            queue.extend(s.cell([cell.c[0] + dx, cell.c[1] + dy], q));
        }
        evaluations += 4;
    }
    (best.0.max(0.0), best.1)
}

fn tri_area(t: &[[f64; 2]; 3]) -> f64 {
    ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0])).abs()
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed box mesh (outward winding).
    fn cuboid(min: [f32; 3], size: [f32; 3]) -> Mesh {
        let [x, y, z] = min;
        let [a, b, c] = size;
        let positions = vec![
            [x, y, z],
            [x + a, y, z],
            [x + a, y + b, z],
            [x, y + b, z],
            [x, y, z + c],
            [x + a, y, z + c],
            [x + a, y + b, z + c],
            [x, y + b, z + c],
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7,
            6, 3, 0, 4, 3, 4, 7,
        ];
        Mesh { positions, indices }
    }

    /// An arch: a block (8 + `width`) x 4 x 5 mm on two 4 mm pillars, with a
    /// tunnel `width` wide and 3 mm high along y, open at both ends.
    fn arch(width: f32) -> Mesh {
        let w = width;
        // The front outline in x-z, counter-clockwise seen from -y.
        let outline: Vec<[f32; 2]> = vec![
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 3.0],
            [4.0 + w, 3.0],
            [4.0 + w, 0.0],
            [8.0 + w, 0.0],
            [8.0 + w, 5.0],
            [0.0, 5.0],
        ];
        let n = outline.len() as u32;
        let mut positions = Vec::new();
        for &[x, z] in &outline {
            positions.push([x, 0.0, z]);
        }
        for &[x, z] in &outline {
            positions.push([x, 4.0, z]);
        }
        let mut indices = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            indices.extend([i, j + n, j, i, i + n, j + n]);
        }
        let front = [
            [0, 1, 2],
            [0, 2, 7],
            [7, 2, 3],
            [7, 3, 6],
            [3, 4, 5],
            [3, 5, 6],
        ];
        for t in front {
            indices.extend([t[0], t[1], t[2]]);
            indices.extend([t[0] + n, t[2] + n, t[1] + n]);
        }
        Mesh { positions, indices }
    }

    #[test]
    fn the_arch_is_closed_and_outward() {
        // Volume by the divergence theorem: positive for outward winding.
        let m = arch(12.0);
        let v: f64 = m
            .indices
            .chunks_exact(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.positions[i as usize].map(f64::from));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum();
        assert!(
            (v - (20.0 * 4.0 * 5.0 - 12.0 * 4.0 * 3.0)).abs() < 1e-6,
            "{v}"
        );
    }

    #[test]
    fn a_box_on_the_bed_has_no_overhang() {
        assert!(downward_regions(&cuboid([0.0; 3], [10.0, 10.0, 5.0]), 50.0, 0.0).is_empty());
    }

    #[test]
    fn a_box_floating_above_the_bed_floats() {
        let mut m = cuboid([0.0; 3], [10.0, 10.0, 5.0]);
        let other = cuboid([20.0, 0.0, 2.0], [6.0, 4.0, 1.0]);
        let base = m.positions.len() as u32;
        m.positions.extend(&other.positions);
        m.indices.extend(other.indices.iter().map(|i| i + base));
        let r = downward_regions(&m, 50.0, 0.0);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].span_mm, None);
        // Held up by the first layer when it is no higher than that.
        assert!(downward_regions(&m, 50.0, 2.0).is_empty());
        assert!((r[0].free_rim_mm - 20.0).abs() < 1e-9);
        assert!((r[0].max_angle_deg - 90.0).abs() < 1e-9);
        assert!((r[0].area_mm2 - 24.0).abs() < 1e-9);
        assert!((r[0].z_min_mm - 2.0).abs() < 1e-9);
    }

    #[test]
    fn an_arch_is_a_bridge_as_wide_as_its_opening() {
        for width in [4.8_f32, 12.0, 14.0] {
            let r = downward_regions(&arch(width), 50.0, 0.2);
            assert_eq!(r.len(), 1, "{r:?}");
            // Held by the pillars, open at the tunnel's ends.
            assert!(
                (r[0].free_rim_mm - 2.0 * f64::from(width)).abs() < 1e-5,
                "{r:?}"
            );
            let span = r[0].span_mm.expect("a bridge");
            let want = f64::from(width);
            assert!(
                span <= want + 1e-9 && span >= want - SPAN_PRECISION_MM,
                "{width}: {span}"
            );
        }
    }

    #[test]
    fn a_ceiling_held_all_round_spans_its_largest_gap() {
        // Seen from above: a 20 x 20 square with a 4 x 4 pillar in the middle,
        // every edge anchored. The farthest point sits in a corner, as far from
        // the two sides as from the pillar's corner.
        let rim: Vec<[[f64; 2]; 2]> = vec![
            [[0.0, 0.0], [20.0, 0.0]],
            [[20.0, 0.0], [20.0, 20.0]],
            [[20.0, 20.0], [0.0, 20.0]],
            [[0.0, 20.0], [0.0, 0.0]],
            [[8.0, 8.0], [8.0, 12.0]],
            [[8.0, 12.0], [12.0, 12.0]],
            [[12.0, 12.0], [12.0, 8.0]],
            [[12.0, 8.0], [8.0, 8.0]],
        ];
        let tris = vec![[[0.0, 0.0], [20.0, 0.0], [0.0, 8.0]]];
        let (r, c) = farthest_from_anchors(&rim, &rim, [0.0, 0.0], [20.0, 20.0], &tris);
        // Centre (r, r) at distance r from (8, 8): (8 - r) sqrt 2 = r.
        let s2 = std::f64::consts::SQRT_2;
        let want = 8.0 * s2 / (1.0 + s2);
        assert!(
            r <= want + 1e-9 && r >= want - SPAN_PRECISION_MM / 2.0,
            "{r} {c:?}"
        );
    }

    #[test]
    fn steepness_follows_the_limit() {
        // A 45 degree face prints at a 50 degree limit, not at 40.
        let m = Mesh {
            positions: vec![
                [0.0, 0.0, 5.0],
                [0.0, 4.0, 5.0],
                [2.0, 0.0, 3.0],
                [9.0, 9.0, 0.0],
            ],
            indices: vec![0, 1, 2],
        };
        assert!(downward_regions(&m, 50.0, 0.0).is_empty());
        let r = downward_regions(&m, 40.0, 0.0);
        assert_eq!(r.len(), 1);
        assert!((r[0].max_angle_deg - 45.0).abs() < 1e-9);
    }
}
