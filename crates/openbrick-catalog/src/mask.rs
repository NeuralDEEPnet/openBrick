//! Occupancy masks: the narrowphase of collision (docs/plan/07 section 6).
//!
//! A part's **mask** says which 1 LDU cells of its body box are solid. Studs are never part of it
//! (a stud always sits in the cavity of the part above, the stud-versus-cavity class of the rule;
//! stud versus solid is not tested, as with the boxes), so the mask is the *body* alone. The
//! broadphase stays the integer box of the body ([`crate::shape::PartShape::body`]); the mask only
//! decides whether two parts whose boxes overlap really share volume.
//!
//! **Cell size: 1 LDU (0.4 mm), not the 4 LDU the plan first drew.** A Technic pin (radius 6 LDU)
//! sits in a hole of radius 8 LDU: 2 LDU of clearance. A cell is classified by its centre, so a
//! sampled solid is off the true one by at most half a cell diagonal (0.71 LDU at 1 LDU, 2.83 at
//! 4 LDU), and two sampled solids whose true surfaces are 2 LDU apart can never overlap at 1 LDU
//! cells, whatever the integer offset between the two parts' grids; at 4 LDU they could, and a pin
//! would not fit its hole. Boxes of whole cells in integer LDU compose exactly under the 24
//! lattice rotations and any integer position, so no two grids ever need to line up.
//!
//! **Collision.** Two parts whose boxes overlap collide when the intersection of their solids contains a
//! 2 x 2 x 2 cell cube (0.8 mm) in the union, not box pair by box pair ([`Mask::solids_overlap_counted`]):
//! a one-cell sliver where shapes that mate touch is not a collision, a 3 LDU deep overlap of a round wall
//! merged into a staircase of boxes is.
//!
//! **What the mask is.** Centre-sampled, not conservative either way: it is exact at faces on
//! integer LDU planes (two bricks that touch do not overlap) and within 0.71 LDU (0.28 mm) of a
//! slanted or round surface. It therefore never lets two real solids interpenetrate by more than
//! that, and never forbids a pin in its hole, an axle in its cross-shaped hole or a brick in the
//! hole row of a Technic brick (`openbrick catalog` checks the pairs on the real library; the
//! tests check the synthetic ones).
//!
//! **Where masks exist.** Only for parts whose box is visibly wrong as a collision shape: a part
//! whose empty cells (other than the cavity open to the bottom, which a stud fills) are 1.5 percent
//! or more of its body box. Plain bricks, plates, tiles and baseplates have none and keep the box,
//! byte for byte as before.
//!
//! **Storage.** The importer writes the bit grid (x fastest, then z, then y, least significant bit
//! first) into the pack ([`MaskRef`], docs/plan/08 section 1); [`Mask::from_grid`] merges it into a
//! few hundred integer boxes, which is what the engine tests. Nothing here depends on the render
//! level of detail.

use std::sync::Arc;

use openbrick_units::Rot24;

use crate::shape::Aabb;

/// The edge of a mask cell, in LDU.
pub const MASK_CELL_LDU: i32 = 1;
/// The most cells one mask may span (its body box's volume in LDU cubed): 1,048,576, a 128 KiB bit
/// grid. A part whose body is larger has no mask and collides as its box.
pub const MAX_MASK_CELLS: u64 = 1 << 20;
/// How deep (LDU, on every axis) two solids must overlap for the narrowphase to call it a collision:
/// 2 LDU, 0.8 mm. A cell is classified by its centre, so two shapes drawn to touch can share a one
/// cell sliver; and a snap-fit part is meant to be pressed in a little. Applies where a mask is
/// involved; two boxes (parts without masks) collide at any overlap, as before.
pub const MIN_PENETRATION_LDU: i32 = 2;
/// The largest overlap region (cells) the narrowphase rasterises: 262,144, a 64 LDU cube. A larger
/// region is answered "collide" without looking, which is safe and bounds one call's work.
pub const MAX_OVERLAP_CELLS: u64 = 1 << 18;
/// The most boxes one mask may be merged into. The real parts need a few hundred; a hostile
/// checkerboard would need two million.
pub const MAX_MASK_BOXES: usize = 4096;
/// The most boxes all the masks of one pack may hold together (memory: 24 bytes each).
pub const MAX_PACK_MASK_BOXES: usize = 2_000_000;
/// The most bytes the mask blob of one pack may inflate to: 64 MiB, 1.7 times what the 2026-10
/// library with `--technic` needs (37.9 MiB for 1,899 masks, 318 million cells of body boxes).
/// Decoding costs about 0.6 s natively and 0.9 s in WebAssembly for that, so a hostile pack at the
/// cap costs the reader about 1.5 seconds, once; the reader does not keep the inflated blob.
pub const MAX_MASK_BLOB_BYTES: usize = 64 * 1024 * 1024;
/// A part gets a mask when this share of its body box (parts per ten thousand) is empty and not
/// open to the bottom.
pub const MASK_NEED_PER_10K: u64 = 150;

/// Why a bit grid is not a mask.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MaskError {
    #[error("a mask of {0} cells; the limit is {MAX_MASK_CELLS}")]
    TooManyCells(u64),
    #[error("the mask grid has {have} bytes; its dimensions need {need}")]
    WrongSize { have: usize, need: usize },
    #[error("the mask has bits set beyond its last cell")]
    StrayBits,
    #[error("the mask needs more than {MAX_MASK_BOXES} boxes; it is too fragmented to be a part")]
    TooManyBoxes,
}

/// A bit grid of solid cells over a box: the form a pack stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskGrid {
    /// The minimum corner of the grid (the body box's), in LDU.
    pub origin: [i32; 3],
    /// Cells along X, Y, Z.
    pub dims: [u32; 3],
    /// `ceil(cells / 8)` bytes, bit `i` of byte `i / 8` is the cell `i`; the cell index is
    /// `x + dims[0] * (z + dims[2] * y)`.
    pub bits: Vec<u8>,
}

/// The cells a grid of these dimensions has, or `None` past [`MAX_MASK_CELLS`].
pub fn cell_count(dims: [u32; 3]) -> Option<u64> {
    let n = dims
        .iter()
        .try_fold(1u64, |n, &d| n.checked_mul(u64::from(d)))?;
    (n <= MAX_MASK_CELLS).then_some(n)
}

impl MaskGrid {
    /// An empty grid over `body`; `None` when it spans more than [`MAX_MASK_CELLS`] cells.
    pub fn empty(body: &Aabb) -> Option<MaskGrid> {
        let s = body.size();
        if s.iter().any(|&v| v <= 0) {
            return None;
        }
        let dims = s.map(|v| v as u32);
        let n = cell_count(dims)?;
        Some(MaskGrid {
            origin: body.min,
            dims,
            bits: vec![0; n.div_ceil(8) as usize],
        })
    }

    fn index(&self, x: u32, y: u32, z: u32) -> usize {
        (x + self.dims[0] * (z + self.dims[2] * y)) as usize
    }

    pub fn get(&self, x: u32, y: u32, z: u32) -> bool {
        let i = self.index(x, y, z);
        self.bits[i / 8] >> (i % 8) & 1 == 1
    }

    pub fn set(&mut self, x: u32, y: u32, z: u32, on: bool) {
        let i = self.index(x, y, z);
        if on {
            self.bits[i / 8] |= 1 << (i % 8);
        } else {
            self.bits[i / 8] &= !(1 << (i % 8));
        }
    }

    /// Solid cells.
    pub fn solid(&self) -> u64 {
        self.bits.iter().map(|b| u64::from(b.count_ones())).sum()
    }

    /// Check the size and the padding bits.
    pub fn validate(&self) -> Result<u64, MaskError> {
        MaskGrid::check_bits(self.dims, &self.bits)
    }

    fn check_bits(dims: [u32; 3], bits: &[u8]) -> Result<u64, MaskError> {
        let n = cell_count(dims).ok_or_else(|| {
            MaskError::TooManyCells(
                dims.iter()
                    .fold(1u64, |n, &d| n.saturating_mul(u64::from(d))),
            )
        })?;
        let need = n.div_ceil(8) as usize;
        if bits.len() != need {
            return Err(MaskError::WrongSize {
                have: bits.len(),
                need,
            });
        }
        let used = (n % 8) as u32;
        if used != 0 && bits[need - 1] >> used != 0 {
            return Err(MaskError::StrayBits);
        }
        Ok(n)
    }
}

/// Working buffers for [`Mask::from_bits_in`].
#[derive(Default)]
pub struct MaskScratch {
    done: Vec<bool>,
    boxes: Vec<Aabb>,
    work: u64,
}

impl MaskScratch {
    /// The work done so far: cells of the declared body boxes visited, and boxes made. A count, so
    /// a test can bound a hostile decode without a clock.
    pub fn work(&self) -> u64 {
        self.work
    }
}

/// A read-only view of a bit grid (the same layout as [`MaskGrid`]).
struct MaskView<'a> {
    origin: [i32; 3],
    dims: [u32; 3],
    bits: &'a [u8],
}

impl MaskView<'_> {
    fn get(&self, x: u32, y: u32, z: u32) -> bool {
        let i = (x + self.dims[0] * (z + self.dims[2] * y)) as usize;
        self.bits[i / 8] >> (i % 8) & 1 == 1
    }

    fn validate(&self) -> Result<u64, MaskError> {
        MaskGrid::check_bits(self.dims, self.bits)
    }
}

/// A mask as boxes of whole cells in the part's own frame: the engine's narrowphase shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    boxes: Vec<Aabb>,
    /// Solid cells (LDU cubed).
    volume: u64,
}

impl Mask {
    /// Merge a bit grid into boxes (greedy: a run along X, then rows of equal runs along Z, then
    /// layers of equal rows along Y). Checked: size, padding and the box bound come first.
    pub fn from_grid(g: &MaskGrid) -> Result<Mask, MaskError> {
        g.validate()?;
        Mask::from_bits(g.origin, g.dims, &g.bits)
    }

    /// [`Mask::from_grid`] over bits that are already somewhere (the pack's mask blob), without a copy.
    /// The bits must be the size `dims` needs (the caller checked; a short slice is an error, not a panic).
    pub fn from_bits(origin: [i32; 3], dims: [u32; 3], bits: &[u8]) -> Result<Mask, MaskError> {
        Mask::from_bits_in(origin, dims, bits, &mut MaskScratch::default())
    }

    /// [`Mask::from_bits`] with the working buffers of the caller, reused from one part to the next
    /// (a pack of the real library decodes 1,899 masks; growing and freeing a vector for each
    /// fragments a WebAssembly heap that never shrinks).
    pub fn from_bits_in(
        origin: [i32; 3],
        dims: [u32; 3],
        bits: &[u8],
        scratch: &mut MaskScratch,
    ) -> Result<Mask, MaskError> {
        let g = MaskView { origin, dims, bits };
        g.validate()?;
        let [nx, ny, nz] = g.dims;
        scratch.work += u64::from(dims[0]) * u64::from(dims[1]) * u64::from(dims[2]);
        let done = &mut scratch.done;
        done.clear();
        done.resize(
            (u64::from(nx) * u64::from(ny) * u64::from(nz)) as usize,
            false,
        );
        let boxes = &mut scratch.boxes;
        boxes.clear();
        let mut volume = 0u64;
        let at = |x: u32, y: u32, z: u32| (x + nx * (z + nz * y)) as usize;
        for y in 0..ny {
            for z in 0..nz {
                for x in 0..nx {
                    if done[at(x, y, z)] || !g.get(x, y, z) {
                        continue;
                    }
                    let free = |x: u32, y: u32, z: u32| g.get(x, y, z) && !done[at(x, y, z)];
                    let mut wx = 1;
                    while x + wx < nx && free(x + wx, y, z) {
                        wx += 1;
                    }
                    let mut wz = 1;
                    'z: while z + wz < nz {
                        for i in 0..wx {
                            if !free(x + i, y, z + wz) {
                                break 'z;
                            }
                        }
                        wz += 1;
                    }
                    let mut wy = 1;
                    'y: while y + wy < ny {
                        for k in 0..wz {
                            for i in 0..wx {
                                if !free(x + i, y + wy, z + k) {
                                    break 'y;
                                }
                            }
                        }
                        wy += 1;
                    }
                    for yy in y..y + wy {
                        for zz in z..z + wz {
                            for xx in x..x + wx {
                                done[at(xx, yy, zz)] = true;
                            }
                        }
                    }
                    volume += u64::from(wx) * u64::from(wy) * u64::from(wz);
                    let lo = [
                        g.origin[0] + x as i32,
                        g.origin[1] + y as i32,
                        g.origin[2] + z as i32,
                    ];
                    scratch.work += 1;
                    boxes.push(Aabb {
                        min: lo,
                        max: [lo[0] + wx as i32, lo[1] + wy as i32, lo[2] + wz as i32],
                    });
                    if boxes.len() > MAX_MASK_BOXES {
                        return Err(MaskError::TooManyBoxes);
                    }
                }
            }
        }
        // Exactly as many as there are, not the doubled capacity a growing Vec leaves: a pack holds
        // hundreds of thousands of these.
        Ok(Mask {
            boxes: boxes.as_slice().to_vec(),
            volume,
        })
    }

    /// The boxes, in the part's frame.
    pub fn boxes(&self) -> &[Aabb] {
        &self.boxes
    }

    /// Solid cells (LDU cubed).
    pub fn volume(&self) -> u64 {
        self.volume
    }

    /// True when the solid of part A at `a_pose` and the solid of part B at `b_pose` collide inside
    /// `region` (a world box, the overlap of the two body boxes). A part without a mask is its `body`
    /// box. Touching is not colliding. See [`Mask::solids_overlap_counted`] for the rule and its cost.
    #[allow(clippy::too_many_arguments)]
    pub fn solids_overlap(
        a: Option<&Mask>,
        a_body: &Aabb,
        a_pose: (Rot24, [i32; 3]),
        b: Option<&Mask>,
        b_body: &Aabb,
        b_pose: (Rot24, [i32; 3]),
        region: &Aabb,
    ) -> bool {
        Mask::solids_overlap_counted(a, a_body, a_pose, b, b_body, b_pose, region).0
    }

    /// [`Mask::solids_overlap`] and the work it did (boxes visited, cells written and scanned: a
    /// count, so tests can bound the cost without a clock).
    ///
    /// **The rule.** The two solids collide when their intersection contains a cube of
    /// [`MIN_PENETRATION_LDU`] cells (2 x 2 x 2: 0.8 mm) anywhere, that is when they overlap at least
    /// that deep on every axis *as a union*, not box pair by box pair: a round wall merged into a
    /// staircase of boxes is as deep as the whole staircase. Sampled solids that mate (a pin in its
    /// hole, an axle in its cross) can share a one-cell sliver where the library's shapes touch,
    /// which is not a collision.
    ///
    /// **The cost.** Linear: the region's cells are rasterised once for each part (the merged boxes
    /// of a mask never overlap, so the writes are at most twice the region) and eroded in three
    /// passes, plus one transform of each box of each mask. No pair of boxes is ever compared. A
    /// region of more than [`MAX_OVERLAP_CELLS`] cells is not examined: the answer is "collide",
    /// the safe one (the part is refused, never wrongly accepted), at the cost of one counter.
    #[allow(clippy::too_many_arguments)]
    pub fn solids_overlap_counted(
        a: Option<&Mask>,
        a_body: &Aabb,
        a_pose: (Rot24, [i32; 3]),
        b: Option<&Mask>,
        b_body: &Aabb,
        b_pose: (Rot24, [i32; 3]),
        region: &Aabb,
    ) -> (bool, u64) {
        let d: [i64; 3] = [0, 1, 2].map(|k| i64::from(region.max[k]) - i64::from(region.min[k]));
        if d.iter().any(|&v| v < i64::from(MIN_PENETRATION_LDU)) {
            return (false, 1);
        }
        let cells = d[0] * d[1] * d[2];
        if cells as u64 > MAX_OVERLAP_CELLS {
            return (true, 1);
        }
        let (nx, ny) = (d[0] as usize, d[1] as usize);
        let n = cells as usize;
        let mut work = 0u64;
        OVERLAP_SCRATCH.with(|cell| {
            let mut v = cell.borrow_mut();
            v.clear();
            v.resize(n, 0u8);
            for (bit, m, body, pose) in [(1u8, a, a_body, a_pose), (2u8, b, b_body, b_pose)] {
                let mut fill = |w: Aabb| {
                    work += 1;
                    let lo: [i64; 3] = [0, 1, 2]
                        .map(|k| i64::from(w.min[k].max(region.min[k])) - i64::from(region.min[k]));
                    let hi: [i64; 3] = [0, 1, 2]
                        .map(|k| i64::from(w.max[k].min(region.max[k])) - i64::from(region.min[k]));
                    if (0..3).any(|k| lo[k] >= hi[k]) {
                        return;
                    }
                    for z in lo[2]..hi[2] {
                        for y in lo[1]..hi[1] {
                            let row = (lo[0] as usize) + nx * (y as usize + ny * z as usize);
                            let len = (hi[0] - lo[0]) as usize;
                            work += len as u64;
                            for c in &mut v[row..row + len] {
                                *c |= bit;
                            }
                        }
                    }
                };
                match m {
                    None => fill(body.transformed(pose.0, pose.1)),
                    Some(m) => {
                        for bx in &m.boxes {
                            fill(bx.transformed(pose.0, pose.1));
                        }
                    }
                }
            }
            work += n as u64;
        });
        // Erode the intersection by one cell on each axis in turn: a cell survives the three
        // passes only if a 2 x 2 x 2 cube of the intersection starts there.
        let found = OVERLAP_SCRATCH.with(|cell| {
            let mut v = cell.borrow_mut();
            let step = [1usize, nx, nx * ny];
            let extent = [nx, ny, d[2] as usize];
            for c in &mut *v {
                *c = u8::from(*c == 3);
            }
            for axis in 0..3 {
                // v[c] = v[c] && v[c + step], reading ascending so the neighbour is still the old value.
                for c in 0..n {
                    let coord = (c / step[axis]) % extent[axis];
                    let keep = v[c] == 1 && coord + 1 < extent[axis] && v[c + step[axis]] == 1;
                    v[c] = u8::from(keep);
                }
            }
            v.contains(&1)
        });
        work += 4 * n as u64;
        (found, work)
    }
}

thread_local! {
    /// The cells of one narrowphase call, reused so a snap does not allocate for each candidate.
    static OVERLAP_SCRATCH: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A shared mask (parts are cloned into catalogs; the boxes are not).
pub type SharedMask = Arc<Mask>;

/// Where a part's mask is in the pack's mask blob.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskRef {
    /// Byte offset of the grid in the inflated mask blob.
    pub offset: u64,
    /// The grid's size in bytes: `ceil(cells / 8)`, cells being the volume of the part's body box.
    pub bytes: u32,
    /// Solid cells, checked when the pack is read.
    pub solid: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_of(body: Aabb, f: impl Fn(i32, i32, i32) -> bool) -> MaskGrid {
        let mut g = MaskGrid::empty(&body).unwrap();
        for y in 0..g.dims[1] {
            for z in 0..g.dims[2] {
                for x in 0..g.dims[0] {
                    let on = f(
                        g.origin[0] + x as i32,
                        g.origin[1] + y as i32,
                        g.origin[2] + z as i32,
                    );
                    g.set(x, y, z, on);
                }
            }
        }
        g
    }

    #[test]
    fn a_grid_merges_into_boxes_with_the_same_volume() {
        // A ring: a 10 x 4 x 10 block with a 4 x 4 x 4 hole in the middle.
        let body = Aabb {
            min: [-5, 0, -5],
            max: [5, 4, 5],
        };
        let g = grid_of(body, |x, _, z| {
            !((-2..2).contains(&x) && (-2..2).contains(&z))
        });
        let m = Mask::from_grid(&g).unwrap();
        assert_eq!(m.volume(), g.solid());
        assert_eq!(m.volume(), 10 * 4 * 10 - 4 * 4 * 4);
        assert!(m.boxes().len() <= 4);
        let total: u64 = m
            .boxes()
            .iter()
            .map(|b| b.size().iter().map(|&v| v as u64).product::<u64>())
            .sum();
        assert_eq!(total, m.volume());
    }

    #[test]
    fn a_pin_fits_a_hole_and_a_box_does_not() {
        // A block with a 12 x 12 hole; a 10 x 10 post.
        let block_body = Aabb {
            min: [-20, 0, -20],
            max: [20, 8, 20],
        };
        let block = Mask::from_grid(&grid_of(block_body, |x, _, z| {
            !((-6..6).contains(&x) && (-6..6).contains(&z))
        }))
        .unwrap();
        let post_body = Aabb {
            min: [-5, -20, -5],
            max: [5, 20, 5],
        };
        let post = Mask::from_grid(&grid_of(post_body, |_, _, _| true)).unwrap();
        let region = block_body.transformed(Rot24::IDENTITY, [0, 0, 0]);
        let id = (Rot24::IDENTITY, [0, 0, 0]);
        // The post through the hole: the boxes overlap, the masks do not.
        assert!(block_body.overlaps(&post_body));
        assert!(!Mask::solids_overlap(
            Some(&block),
            &block_body,
            id,
            Some(&post),
            &post_body,
            id,
            &region
        ));
        // Moved sideways by 4 LDU it cuts the wall.
        assert!(Mask::solids_overlap(
            Some(&block),
            &block_body,
            id,
            Some(&post),
            &post_body,
            (Rot24::IDENTITY, [4, 0, 0]),
            &region
        ));
        // Against a part without a mask the box is solid.
        assert!(Mask::solids_overlap(
            None,
            &block_body,
            id,
            Some(&post),
            &post_body,
            id,
            &region
        ));
    }

    #[test]
    fn a_staircase_wall_is_as_deep_as_the_whole_staircase() {
        // A: solid where x < 10 - z over 10 x 10 x 3 cells: three boxes, each one cell deep in z.
        let body = Aabb {
            min: [0, 0, 0],
            max: [10, 10, 3],
        };
        let a = Mask::from_grid(&grid_of(body, |x, _, z| x < 10 - z)).unwrap();
        assert_eq!(a.boxes().len(), 3);
        // B: a plain box 8 x 10 x 3 sharing 240 cells, three deep in z.
        let b_body = Aabb {
            min: [0, 0, 0],
            max: [8, 10, 3],
        };
        let id = (Rot24::IDENTITY, [0, 0, 0]);
        let region = Aabb {
            min: [0, 0, 0],
            max: [8, 10, 3],
        };
        let (hit, _) =
            Mask::solids_overlap_counted(Some(&a), &body, id, None, &b_body, id, &region);
        assert!(hit, "three deep in z as a union, one deep in each box");
        // The same staircase one cell deep only (B 1 thick) is a sliver, not a collision.
        let thin = Aabb {
            min: [0, 0, 0],
            max: [8, 10, 1],
        };
        let thin_region = thin;
        assert!(!Mask::solids_overlap(
            Some(&a),
            &body,
            id,
            None,
            &thin,
            id,
            &thin_region
        ));
    }

    #[test]
    fn the_narrowphase_does_bounded_work_whatever_the_masks() {
        // A hostile mask: as many isolated cells as a mask may have boxes, in a big body.
        let body = Aabb {
            min: [0, 0, 0],
            max: [64, 2, 64],
        };
        let m = Mask::from_grid(&grid_of(body, |x, y, z| {
            y == 0 && x % 2 == 0 && z % 2 == 0 && (x / 2 + z / 2) % 2 == 0
        }))
        .unwrap();
        assert!(m.boxes().len() > 400);
        let id = (Rot24::IDENTITY, [0, 0, 0]);
        let (_, work) =
            Mask::solids_overlap_counted(Some(&m), &body, id, Some(&m), &body, id, &body);
        // Linear in the region (8,192 cells) and the boxes, nowhere near boxes x boxes.
        assert!(
            work < 10 * 64 * 2 * 64 + 8 * m.boxes().len() as u64,
            "{work}"
        );
        // A region over the cap is answered "collide" for one unit of work.
        let huge = Aabb {
            min: [0, 0, 0],
            max: [200, 200, 200],
        };
        assert_eq!(
            Mask::solids_overlap_counted(Some(&m), &body, id, None, &huge, id, &huge),
            (true, 1)
        );
    }

    #[test]
    fn hostile_grids_are_refused_before_anything_is_built() {
        let body = Aabb {
            min: [0; 3],
            max: [4; 3],
        };
        let mut g = MaskGrid::empty(&body).unwrap();
        assert!(Mask::from_grid(&g).unwrap().boxes().is_empty());
        g.bits.pop();
        assert!(matches!(
            Mask::from_grid(&g),
            Err(MaskError::WrongSize { .. })
        ));
        let huge = MaskGrid {
            origin: [0; 3],
            dims: [100_000, 100_000, 100_000],
            bits: vec![],
        };
        assert!(matches!(
            Mask::from_grid(&huge),
            Err(MaskError::TooManyCells(_))
        ));
        let overflow = MaskGrid {
            origin: [0; 3],
            dims: [u32::MAX, u32::MAX, u32::MAX],
            bits: vec![],
        };
        assert!(matches!(
            Mask::from_grid(&overflow),
            Err(MaskError::TooManyCells(_))
        ));
        // 4 x 4 x 4 = 64 cells: no stray bits possible; 3 x 3 x 3 = 27 has 5 padding bits.
        let mut odd = MaskGrid::empty(&Aabb {
            min: [0; 3],
            max: [3; 3],
        })
        .unwrap();
        odd.bits[3] = 0xff;
        assert_eq!(Mask::from_grid(&odd), Err(MaskError::StrayBits));
        // A checkerboard is fragmented beyond the bound.
        let mut checker = MaskGrid::empty(&Aabb {
            min: [0; 3],
            max: [80, 4, 80],
        })
        .unwrap();
        for y in 0..4 {
            for z in 0..80 {
                for x in 0..80 {
                    checker.set(x, y, z, (x + z + y) % 2 == 0);
                }
            }
        }
        assert_eq!(Mask::from_grid(&checker), Err(MaskError::TooManyBoxes));
    }
}
