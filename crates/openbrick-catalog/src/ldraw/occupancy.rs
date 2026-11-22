//! Occupancy masks from a part's own triangles (docs/plan/07 section 6; the format and the cell
//! size are in [`crate::mask`]).
//!
//! The mask of a part is the set of 1 LDU cells of its body box whose **centre is inside the
//! solid**. Inside is decided by ray parity along each of the three axes through the cell centre
//! (the rays are nudged by a few micro-LDU so none runs exactly along a mesh edge), and a cell is
//! solid when at least two axes say so: the library's meshes are not watertight (a stud's bottom
//! is open, some parts omit faces that are never seen), and one axis alone would flood a column.
//! The triangles of the studs are left out: a stud sits in the cavity of the part above and is
//! not part of the collision body. Nothing about the render mesh's level of detail enters: the
//! importer calls this with the walk at the standard primitive resolution whatever the user chose
//! for the render mesh.
//!
//! A part gets a mask only when its box is visibly the wrong shape: when at least
//! [`MASK_NEED_PER_10K`] of ten thousand of the cells of its box are empty and are *not* the
//! cavity open to the bottom (a brick's underside, which a stud fills). A plain brick, plate or
//! tile therefore has none and collides as its box, exactly as before.

use super::flatten::Soup;
use crate::mask::{cell_count, MaskGrid, MASK_NEED_PER_10K};
use crate::shape::Aabb;

/// What the voxeliser found, for the import report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OccupancyStats {
    pub cells: u64,
    pub solid: u64,
    /// Empty cells that are not open to the bottom.
    pub empty_not_cavity: u64,
}

const EPS: [f64; 3] = [0.000_731, 0.000_473, 0.000_917];

/// The crossings of the triangles with the lines along `axis` through the cell centres, as
/// `(column, t, side)` sorted by column then `t`; `side` is which way the triangle faces along the
/// axis (by its winding).
fn crossings(soup: &Soup, body: &Aabb, dims: [usize; 3], axis: usize) -> Vec<(u32, f32, i8)> {
    let (b, c) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let ncols_b = dims[b];
    let origin = body.min.map(f64::from);
    let mut out: Vec<(u32, f32, i8)> = Vec::new();
    for (tri, &stud) in soup.tris.iter().zip(&soup.stud) {
        if stud {
            continue;
        }
        // Triangle in the (b, c) plane relative to the box, and its extent in cells.
        let p: [[f64; 3]; 3] = [0, 1, 2].map(|i| {
            [
                tri[i][axis] - origin[axis],
                tri[i][b] - origin[b],
                tri[i][c] - origin[c],
            ]
        });
        let (mut b0, mut b1, mut c0, mut c1) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for q in &p {
            b0 = b0.min(q[1]);
            b1 = b1.max(q[1]);
            c0 = c0.min(q[2]);
            c1 = c1.max(q[2]);
        }
        let ib0 = ((b0 - 0.5 - EPS[b]).ceil().max(0.0)) as i64;
        let ib1 = (((b1 - 0.5 - EPS[b]).floor()) as i64).min(dims[b] as i64 - 1);
        let ic0 = ((c0 - 0.5 - EPS[c]).ceil().max(0.0)) as i64;
        let ic1 = (((c1 - 0.5 - EPS[c]).floor()) as i64).min(dims[c] as i64 - 1);
        if ib1 < ib0 || ic1 < ic0 {
            continue;
        }
        // Barycentric set-up in 2D.
        let (ax, ay) = (p[0][1], p[0][2]);
        let (v0x, v0y) = (p[1][1] - ax, p[1][2] - ay);
        let (v1x, v1y) = (p[2][1] - ax, p[2][2] - ay);
        let det = v0x * v1y - v0y * v1x;
        if det.abs() < 1e-12 {
            continue; // edge-on to the ray
        }
        // +1 where the ray enters the solid (the face's outward normal points against the ray), -1
        // where it leaves. `det` is the axis component of the normal in the frame (axis, b, c),
        // which is left-handed for the Y axis (b, c = X, Z).
        let n_axis = if axis == 1 { -det } else { det };
        let sign: i8 = if n_axis < 0.0 { 1 } else { -1 };
        for ic in ic0..=ic1 {
            let py = ic as f64 + 0.5 + EPS[c] - ay;
            for ib in ib0..=ib1 {
                let px = ib as f64 + 0.5 + EPS[b] - ax;
                let u = (px * v1y - py * v1x) / det;
                let v = (v0x * py - v0y * px) / det;
                if u < 0.0 || v < 0.0 || u + v >= 1.0 {
                    continue;
                }
                let t = p[0][0] + u * (p[1][0] - p[0][0]) + v * (p[2][0] - p[0][0]);
                out.push(((ic as usize * ncols_b + ib as usize) as u32, t as f32, sign));
            }
        }
    }
    out.sort_unstable_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)).then(x.2.cmp(&y.2)));
    out
}

/// Add one vote to the cells inside the solid along `axis`.
fn vote(votes: &mut [u8], dims: [usize; 3], axis: usize, hits: &[(u32, f32, i8)]) {
    let (b, c) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let stride = |a: usize| match a {
        0 => 1,
        2 => dims[0],
        _ => dims[0] * dims[2],
    };
    let ncols_b = dims[b];
    let mut i = 0;
    while i < hits.len() {
        let col = hits[i].0;
        let mut j = i;
        while j < hits.len() && hits[j].0 == col {
            j += 1;
        }
        // The winding number along the line: inside where it is above zero. Unlike parity this
        // keeps two solids that overlap (a roof cube over wall cubes) and two faces that coincide
        // (the seam between two blocks) right.
        let (ib, ic) = (col as usize % ncols_b, col as usize / ncols_b);
        let base = ib * stride(b) + ic * stride(c);
        let mut winding = 0i32;
        for k in i..j {
            let t_from = f64::from(hits[k].1);
            winding += i32::from(hits[k].2);
            let t_to = hits
                .get(k + 1)
                .filter(|_| k + 1 < j)
                .map(|h| f64::from(h.1));
            if winding <= 0 {
                continue;
            }
            let Some(t_to) = t_to else { break };
            let lo = (t_from - 0.5).ceil().max(0.0) as i64;
            let hi = ((t_to - 0.5).ceil() as i64).min(dims[axis] as i64);
            for cell in lo..hi {
                let v = &mut votes[base + cell as usize * stride(axis)];
                *v = v.saturating_add(1);
            }
        }
        i = j;
    }
}

/// The occupancy grid of a part's body (its triangles without the studs) over `body`, and what it
/// found; the grid is `None` when the box is already the right shape (module documentation), when
/// the box spans more than [`crate::mask::MAX_MASK_CELLS`] cells or when the part has no solid at all.
pub fn occupancy(soup: &Soup, body: &Aabb) -> (Option<MaskGrid>, OccupancyStats) {
    let size = body.size();
    if size.iter().any(|&v| v <= 0) {
        return (None, OccupancyStats::default());
    }
    let dims_u = size.map(|v| v as u32);
    let Some(cells) = cell_count(dims_u) else {
        return (None, OccupancyStats::default());
    };
    let dims = size.map(|v| v as usize);
    let mut votes = vec![0u8; cells as usize];
    for axis in 0..3 {
        let hits = crossings(soup, body, dims, axis);
        vote(&mut votes, dims, axis, &hits);
    }
    let mut grid = MaskGrid::empty(body).expect("the size was checked");
    let (nx, ny, nz) = (dims[0], dims[1], dims[2]);
    let mut stats = OccupancyStats {
        cells,
        ..OccupancyStats::default()
    };
    for y in 0..ny {
        for z in 0..nz {
            for x in 0..nx {
                if votes[x + nx * (z + nz * y)] >= 2 {
                    grid.set(x as u32, y as u32, z as u32, true);
                    stats.solid += 1;
                }
            }
        }
    }
    // The underside cavity: empty cells open to the bottom (larger Y) with solid somewhere above
    // them in their column. A column with no solid at all (a through hole, the corner of a round
    // part) is not a cavity; no stud fills it.
    let mut cavity = 0u64;
    for z in 0..nz {
        for x in 0..nx {
            let solid_at = |y: usize| votes[x + nx * (z + nz * y)] >= 2;
            let Some(top) = (0..ny).find(|&y| solid_at(y)) else {
                continue;
            };
            let mut y = ny;
            while y > top + 1 && !solid_at(y - 1) {
                y -= 1;
                cavity += 1;
            }
        }
    }
    stats.empty_not_cavity = cells - stats.solid - cavity;
    if stats.solid == 0 || stats.empty_not_cavity * 10_000 < MASK_NEED_PER_10K * cells {
        return (None, stats);
    }
    (Some(grid), stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuboid(soup: &mut Soup, lo: [f64; 3], hi: [f64; 3]) {
        let p = |x: usize, y: usize, z: usize| {
            [
                if x == 0 { lo[0] } else { hi[0] },
                if y == 0 { lo[1] } else { hi[1] },
                if z == 0 { lo[2] } else { hi[2] },
            ]
        };
        let quads = [
            [p(0, 0, 0), p(1, 0, 0), p(1, 1, 0), p(0, 1, 0)],
            [p(0, 0, 1), p(0, 1, 1), p(1, 1, 1), p(1, 0, 1)],
            [p(0, 0, 0), p(0, 1, 0), p(0, 1, 1), p(0, 0, 1)],
            [p(1, 0, 0), p(1, 0, 1), p(1, 1, 1), p(1, 1, 0)],
            [p(0, 0, 0), p(0, 0, 1), p(1, 0, 1), p(1, 0, 0)],
            [p(0, 1, 0), p(1, 1, 0), p(1, 1, 1), p(0, 1, 1)],
        ];
        let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        for q in quads {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                // Wound so the normal (b - a) x (c - a) points out of the box.
                let u = [0, 1, 2].map(|k| t[1][k] - t[0][k]);
                let v = [0, 1, 2].map(|k| t[2][k] - t[0][k]);
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let out: f64 = (0..3).map(|k| n[k] * (t[0][k] - centre[k])).sum();
                soup.tris
                    .push(if out > 0.0 { t } else { [t[0], t[2], t[1]] });
                soup.stud.push(false);
            }
        }
    }

    #[test]
    fn a_cuboid_is_full_and_needs_no_mask() {
        let mut soup = Soup::default();
        cuboid(&mut soup, [-10.0, 0.0, -10.0], [10.0, 8.0, 10.0]);
        let body = Aabb {
            min: [-10, 0, -10],
            max: [10, 8, 10],
        };
        let (g, st) = occupancy(&soup, &body);
        assert!(g.is_none());
        assert_eq!(st.solid, 20 * 8 * 20);
        assert_eq!(st.empty_not_cavity, 0);
    }

    #[test]
    fn two_posts_make_a_mask_and_the_stud_class_is_left_out() {
        let mut soup = Soup::default();
        cuboid(&mut soup, [-10.0, 0.0, -10.0], [-2.0, 8.0, 10.0]);
        cuboid(&mut soup, [2.0, 0.0, -10.0], [10.0, 8.0, 10.0]);
        // A "stud" over the gap that must not count.
        let n = soup.tris.len();
        cuboid(&mut soup, [-4.0, -4.0, -4.0], [4.0, 0.0, 4.0]);
        for s in &mut soup.stud[n..] {
            *s = true;
        }
        let body = Aabb {
            min: [-10, 0, -10],
            max: [10, 8, 10],
        };
        let (g, st) = occupancy(&soup, &body);
        let g = g.expect("a gap of 4 x 8 x 20 is 20 percent of the box");
        assert_eq!(st.solid, 2 * 8 * 8 * 20);
        assert_eq!(g.solid(), st.solid);
        // The gap is empty at its centre, the posts are solid.
        assert!(!g.get(10, 4, 10) && g.get(2, 4, 10) && g.get(17, 4, 10));
    }

    #[test]
    fn the_cavity_under_a_roof_is_not_a_reason_for_a_mask() {
        // A hollow box: roof 0..4, walls, open at the bottom (Y larger). Built as five cuboids.
        let mut soup = Soup::default();
        cuboid(&mut soup, [-10.0, 0.0, -10.0], [10.0, 4.0, 10.0]);
        cuboid(&mut soup, [-10.0, 4.0, -10.0], [-6.0, 24.0, 10.0]);
        cuboid(&mut soup, [6.0, 4.0, -10.0], [10.0, 24.0, 10.0]);
        cuboid(&mut soup, [-6.0, 4.0, -10.0], [6.0, 24.0, -6.0]);
        cuboid(&mut soup, [-6.0, 4.0, 6.0], [6.0, 24.0, 10.0]);
        let body = Aabb {
            min: [-10, 0, -10],
            max: [10, 24, 10],
        };
        let (g, st) = occupancy(&soup, &body);
        assert!(g.is_none(), "{st:?}");
        assert_eq!(st.empty_not_cavity, 0);
    }
}
