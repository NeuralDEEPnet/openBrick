//! The spatial hash (docs/plan/07 section 4) and the connector hash.
//!
//! Both store small integers (table slots) in pooled singly linked chains, one chain per cell or
//! per position, so an entry costs 8 or 12 bytes and no allocation of its own: a 100,000-part model
//! needs a few tens of megabytes, not hundreds (docs/plan/07 section 14, bench/scale).
//!
//! The lattice cell of the Dimensional Standard (`CELL`, one stud by one plate by one stud) is the
//! unit of the fine lattice and of the [`MAX_BOX_CELLS`] limit; the hash itself uses a coarser cell,
//! [`HASH_CELL`] (8 studs by 12 plates by 8 studs), so a 2 x 4 brick touches one to four cells and
//! a cell holds a handful of parts. Collision is always decided by the exact integer boxes; the
//! hash only narrows the candidates, so the cell size changes speed, never an answer.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use openbrick_catalog::{Aabb, Conn, ConnKind, MAX_BOX_CELLS};
use openbrick_units::{PLATE_LDU, STUD_PITCH_LDU};

use crate::model::EngineError;

/// Fine lattice cell: one stud by one plate by one stud (docs/plan/07 section 4), from the
/// Dimensional Standard's lattice constants.
pub const CELL: [i32; 3] = [STUD_PITCH_LDU, PLATE_LDU, STUD_PITCH_LDU];

/// The cell of the spatial hash: 4 studs by 6 plates (two bricks) by 4 studs, so a 2 x 4 brick
/// touches one to eight cells and a cell of a dense model holds a few parts.
pub const HASH_CELL: [i32; 3] = [4 * STUD_PITCH_LDU, 6 * PLATE_LDU, 4 * STUD_PITCH_LDU];

/// End of a chain.
const NIL: u32 = u32::MAX;

/// A fast hasher for the engine's small integer keys (cells, positions, ids): a multiply-rotate
/// fold and a final mix. The keys are lattice coordinates the engine derives from parts, not secrets;
/// SipHash (the standard hasher) cost more than the rest of a connector lookup. A crafted file
/// could still make many keys collide, but the number of parts (150,000) and of connectors per
/// part is bounded, so the worst case is bounded too, and the worst a file can do is slow its own
/// reader.
#[derive(Default, Clone, Copy)]
pub(crate) struct Mix(u64);

impl Hasher for Mix {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    fn write_i32(&mut self, v: i32) {
        self.write_u64(u64::from(v as u32));
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    fn finish(&self) -> u64 {
        let mut z = self.0;
        z ^= z >> 32;
        z = z.wrapping_mul(0xff51_afd7_ed55_8ccd);
        z ^ (z >> 29)
    }
}

pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<Mix>>;

/// Teschner et al. 2003 hash of a lattice cell.
pub fn cell_hash(cell: [i32; 3]) -> u64 {
    let (x, y, z) = (cell[0] as i64, cell[1] as i64, cell[2] as i64);
    ((x.wrapping_mul(73_856_093)) ^ (y.wrapping_mul(19_349_663)) ^ (z.wrapping_mul(83_492_791)))
        as u64
}

/// Fine lattice cell containing an LDU position.
pub fn cell_of(pos: [i32; 3]) -> [i32; 3] {
    [
        pos[0].div_euclid(CELL[0]),
        pos[1].div_euclid(CELL[1]),
        pos[2].div_euclid(CELL[2]),
    ]
}

fn hash_cell_of(pos: [i32; 3]) -> [i32; 3] {
    [
        pos[0].div_euclid(HASH_CELL[0]),
        pos[1].div_euclid(HASH_CELL[1]),
        pos[2].div_euclid(HASH_CELL[2]),
    ]
}

/// Refuse a box that covers more than [`MAX_BOX_CELLS`] lattice cells: the hash holds one entry
/// per cell it covers, so an unbounded box (a hostile pack or document) would be unbounded work.
/// Counted, not iterated.
pub fn check_box(b: &Aabb) -> Result<(), EngineError> {
    let cells = b.lattice_cells();
    if cells > MAX_BOX_CELLS {
        return Err(EngineError::BoxTooLarge {
            cells,
            limit: MAX_BOX_CELLS,
        });
    }
    Ok(())
}

/// The hash cells of a box, `lo..=hi` on each axis, or `None` for an empty (inverted) box. A
/// stored box (`closed`) also covers the cell its maximum face lies on when that face is on a cell
/// border, so a ray that only grazes it there still finds it; a query box does not (boxes that only
/// touch share no cell). At most [`MAX_BOX_CELLS`] fine cells are covered, else an error.
type CellRange = ([i32; 3], [i32; 3]);

fn hash_range(b: &Aabb, closed: bool) -> Result<Option<CellRange>, EngineError> {
    check_box(b)?;
    if (0..3).any(|k| b.min[k] >= b.max[k]) {
        return Ok(None);
    }
    let lo = hash_cell_of(b.min);
    let hi = hash_cell_of([0, 1, 2].map(|k| {
        if closed {
            b.max[k]
        } else {
            b.max[k].saturating_sub(1)
        }
    }));
    Ok(Some((lo, hi)))
}

fn each_cell(lo: [i32; 3], hi: [i32; 3], mut f: impl FnMut([i32; 3])) {
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                f([x, y, z]);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    slot: u32,
    next: u32,
}

/// Table slots by the hash cells their boxes cover.
#[derive(Clone, Debug)]
pub struct SpatialHash {
    heads: FastMap<[i32; 3], u32>,
    nodes: Vec<Node>,
    free: u32,
    live: usize,
    /// The hash cells any box has covered since the hash was last empty (only grows).
    extent: Option<([i32; 3], [i32; 3])>,
}

impl Default for SpatialHash {
    fn default() -> Self {
        SpatialHash {
            heads: FastMap::default(),
            nodes: Vec::new(),
            free: NIL,
            live: 0,
            extent: None,
        }
    }
}

impl SpatialHash {
    /// Room for about `parts` more parts, so opening a big model does not rehash again and again.
    pub fn reserve(&mut self, parts: usize) {
        self.heads.reserve(parts.saturating_mul(3));
        self.nodes.reserve(parts.saturating_mul(4));
    }

    fn new_node(&mut self, slot: u32, next: u32) -> u32 {
        self.live += 1;
        if self.free != NIL {
            let at = self.free;
            self.free = self.nodes[at as usize].next;
            self.nodes[at as usize] = Node { slot, next };
            at
        } else {
            self.nodes.push(Node { slot, next });
            (self.nodes.len() - 1) as u32
        }
    }

    pub fn insert(&mut self, slot: u32, b: &Aabb) -> Result<(), EngineError> {
        let Some((lo, hi)) = hash_range(b, true)? else {
            return Ok(());
        };
        self.extent = Some(match self.extent {
            None => (lo, hi),
            Some((a, z)) => (
                [0, 1, 2].map(|k| a[k].min(lo[k])),
                [0, 1, 2].map(|k| z[k].max(hi[k])),
            ),
        });
        each_cell(lo, hi, |c| {
            let at = self.new_node(slot, NIL);
            match self.heads.entry(c) {
                Entry::Occupied(mut e) => {
                    self.nodes[at as usize].next = std::mem::replace(e.get_mut(), at);
                }
                Entry::Vacant(v) => {
                    v.insert(at);
                }
            }
        });
        Ok(())
    }

    pub fn remove(&mut self, slot: u32, b: &Aabb) -> Result<(), EngineError> {
        let Some((lo, hi)) = hash_range(b, true)? else {
            return Ok(());
        };
        each_cell(lo, hi, |c| {
            let Some(&head) = self.heads.get(&c) else {
                return;
            };
            let (mut prev, mut at) = (NIL, head);
            while at != NIL {
                let Node { slot: s, next } = self.nodes[at as usize];
                if s == slot {
                    if prev == NIL {
                        if next == NIL {
                            self.heads.remove(&c);
                        } else {
                            self.heads.insert(c, next);
                        }
                    } else {
                        self.nodes[prev as usize].next = next;
                    }
                    self.nodes[at as usize].next = self.free;
                    self.free = at;
                    self.live -= 1;
                    break;
                }
                prev = at;
                at = next;
            }
        });
        if self.heads.is_empty() {
            self.extent = None;
        }
        Ok(())
    }

    /// Calls `f` with every slot whose cells meet the cells of `b` (a slot in several of them is
    /// called several times; the caller tests the boxes themselves).
    pub fn for_each_near(&self, b: &Aabb, mut f: impl FnMut(u32)) -> Result<(), EngineError> {
        if let Some((lo, hi)) = hash_range(b, false)? {
            each_cell(lo, hi, |c| {
                let mut at = self.heads.get(&c).copied().unwrap_or(NIL);
                while at != NIL {
                    let n = &self.nodes[at as usize];
                    f(n.slot);
                    at = n.next;
                }
            });
        }
        Ok(())
    }

    /// Slots whose cells meet the cells of `b`, each once, ascending (candidates: the caller tests
    /// the boxes themselves).
    pub fn near(&self, b: &Aabb) -> Result<Vec<u32>, EngineError> {
        let mut out = Vec::new();
        if let Some((lo, hi)) = hash_range(b, false)? {
            each_cell(lo, hi, |c| {
                let mut at = self.heads.get(&c).copied().unwrap_or(NIL);
                while at != NIL {
                    out.push(self.nodes[at as usize].slot);
                    at = self.nodes[at as usize].next;
                }
            });
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// Visit the slots along a ray, cell by cell in the order the ray enters them, until no later
    /// cell can hold something nearer than the best hit so far. `visit` returns the distance `t`
    /// at which the ray hits that slot's box, or `f64::INFINITY` for a miss. Returns false when the
    /// ray would cross more cells than the hash holds entries (a ray across a model spread over an
    /// absurd extent): the caller then tests every part instead, which costs no more.
    pub fn walk_ray(
        &self,
        origin: [f64; 3],
        dir: [f64; 3],
        mut visit: impl FnMut(u32) -> f64,
    ) -> bool {
        let Some((lo, hi)) = self.extent else {
            return true;
        };
        let size = HASH_CELL.map(f64::from);
        // The ray's span inside the extent's box (slab test), from t = 0.
        let (mut t0, mut t1) = (0.0_f64, f64::INFINITY);
        for k in 0..3 {
            let (a, b) = (
                f64::from(lo[k]) * size[k],
                (f64::from(hi[k]) + 1.0) * size[k],
            );
            if dir[k].abs() < 1e-12 {
                if origin[k] < a || origin[k] >= b {
                    return true;
                }
                continue;
            }
            let (mut s, mut e) = ((a - origin[k]) / dir[k], (b - origin[k]) / dir[k]);
            if s > e {
                std::mem::swap(&mut s, &mut e);
            }
            t0 = t0.max(s);
            t1 = t1.min(e);
        }
        if t0 > t1 {
            return true;
        }
        let at = |t: f64, k: usize| origin[k] + t * dir[k];
        let clamp_cell = |v: f64, k: usize| {
            (v / size[k])
                .floor()
                .clamp(f64::from(lo[k]), f64::from(hi[k])) as i32
        };
        let start: [i32; 3] = [0, 1, 2].map(|k| clamp_cell(at(t0, k), k));
        let end: [i32; 3] = [0, 1, 2].map(|k| clamp_cell(at(t1, k), k));
        let crossings: i64 = (0..3)
            .map(|k| i64::from((end[k] - start[k]).abs()))
            .sum::<i64>()
            + 1;
        if crossings > self.heads.len() as i64 + 64 {
            return false;
        }
        let mut cell = start;
        let mut step = [0i32; 3];
        let mut t_max = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for k in 0..3 {
            if dir[k] > 0.0 {
                step[k] = 1;
                t_max[k] = ((f64::from(cell[k]) + 1.0) * size[k] - origin[k]) / dir[k];
                t_delta[k] = size[k] / dir[k];
            } else if dir[k] < 0.0 {
                step[k] = -1;
                t_max[k] = (f64::from(cell[k]) * size[k] - origin[k]) / dir[k];
                t_delta[k] = -size[k] / dir[k];
            }
        }
        let mut best = f64::INFINITY;
        loop {
            let mut at_node = self.heads.get(&cell).copied().unwrap_or(NIL);
            while at_node != NIL {
                let n = self.nodes[at_node as usize];
                best = best.min(visit(n.slot));
                at_node = n.next;
            }
            let k = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };
            if step[k] == 0 || t_max[k] > t1 || t_max[k] > best {
                break;
            }
            cell[k] += step[k];
            if cell[k] < lo[k] || cell[k] > hi[k] {
                break;
            }
            t_max[k] += t_delta[k];
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        self.heads.is_empty() && self.live == 0
    }

    /// Entries held (a part counts once per cell): for tests and the benchmark.
    pub fn entries(&self) -> usize {
        self.live
    }

    /// Every (cell, slot) pair, sorted: for the property tests, which compare it with a hash
    /// built from scratch.
    pub fn snapshot(&self) -> Vec<([i32; 3], u32)> {
        let mut out = Vec::with_capacity(self.live);
        for (&c, &head) in &self.heads {
            let mut at = head;
            while at != NIL {
                out.push((c, self.nodes[at as usize].slot));
                at = self.nodes[at as usize].next;
            }
        }
        out.sort_unstable();
        out
    }
}

#[derive(Clone, Copy, Debug)]
struct ConnNode {
    slot: u32,
    next: u32,
    kind: ConnKind,
    dir: [i8; 3],
}

impl ConnNode {
    fn conn(&self, pos: [i32; 3]) -> Conn {
        Conn {
            kind: self.kind,
            pos,
            dir: self.dir.map(i32::from),
        }
    }
}

fn small(dir: [i32; 3]) -> [i8; 3] {
    dir.map(|v| v.clamp(-127, 127) as i8)
}

/// Placed connectors by position, so a mate is one lookup (docs/plan/07 section 4: the connector
/// hash). A node keeps the part's table slot, the kind and the axis; the position is the key.
#[derive(Clone, Debug)]
pub struct ConnHash {
    heads: FastMap<[i32; 3], u32>,
    nodes: Vec<ConnNode>,
    free: u32,
    live: usize,
}

impl Default for ConnHash {
    fn default() -> Self {
        ConnHash {
            heads: FastMap::default(),
            nodes: Vec::new(),
            free: NIL,
            live: 0,
        }
    }
}

impl ConnHash {
    /// Room for about `parts` more parts of a dozen or so connectors each.
    pub fn reserve(&mut self, parts: usize) {
        self.heads.reserve(parts.saturating_mul(10));
        self.nodes.reserve(parts.saturating_mul(17));
    }

    pub fn insert(&mut self, slot: u32, conns: impl IntoIterator<Item = Conn>) {
        for c in conns {
            let node = ConnNode {
                slot,
                next: NIL,
                kind: c.kind,
                dir: small(c.dir),
            };
            self.live += 1;
            let at = if self.free != NIL {
                let at = self.free;
                self.free = self.nodes[at as usize].next;
                self.nodes[at as usize] = node;
                at
            } else {
                self.nodes.push(node);
                (self.nodes.len() - 1) as u32
            };
            match self.heads.entry(c.pos) {
                Entry::Occupied(mut e) => {
                    self.nodes[at as usize].next = std::mem::replace(e.get_mut(), at);
                }
                Entry::Vacant(v) => {
                    v.insert(at);
                }
            }
        }
    }

    pub fn remove(&mut self, slot: u32, conns: impl IntoIterator<Item = Conn>) {
        for c in conns {
            let Some(&head) = self.heads.get(&c.pos) else {
                continue;
            };
            let (mut prev, mut at) = (NIL, head);
            while at != NIL {
                let n = self.nodes[at as usize];
                if n.slot == slot && n.kind == c.kind && n.dir == small(c.dir) {
                    if prev == NIL {
                        if n.next == NIL {
                            self.heads.remove(&c.pos);
                        } else {
                            self.heads.insert(c.pos, n.next);
                        }
                    } else {
                        self.nodes[prev as usize].next = n.next;
                    }
                    self.nodes[at as usize].next = self.free;
                    self.free = at;
                    self.live -= 1;
                    break;
                }
                prev = at;
                at = n.next;
            }
        }
    }

    /// The slot of the part whose placed connector mates with `c`, other than `skip`; the lowest
    /// slot when several do (so the answer does not depend on the order parts were placed in).
    pub fn mate_of(&self, c: &Conn, skip: Option<u32>) -> Option<u32> {
        let mut at = self.heads.get(&c.pos).copied().unwrap_or(NIL);
        let mut found: Option<u32> = None;
        while at != NIL {
            let n = &self.nodes[at as usize];
            if Some(n.slot) != skip && c.mates(&n.conn(c.pos)) && found.is_none_or(|f| n.slot < f) {
                found = Some(n.slot);
            }
            at = n.next;
        }
        found
    }

    pub fn is_empty(&self) -> bool {
        self.heads.is_empty() && self.live == 0
    }

    pub fn entries(&self) -> usize {
        self.live
    }

    /// Every (position, slot, kind, axis), sorted: for the property tests.
    pub fn snapshot(&self) -> Vec<([i32; 3], u32, ConnKind, [i8; 3])> {
        let mut out = Vec::with_capacity(self.live);
        for (&pos, &head) in &self.heads {
            let mut at = head;
            while at != NIL {
                let n = &self.nodes[at as usize];
                out.push((pos, n.slot, n.kind, n.dir));
                at = n.next;
            }
        }
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_partition_the_lattice() {
        assert_eq!(cell_of([0, 0, 0]), [0, 0, 0]);
        assert_eq!(cell_of([19, 7, 19]), [0, 0, 0]);
        assert_eq!(cell_of([20, 8, -1]), [1, 1, -1]);
        assert_ne!(cell_hash([0, 0, 0]), cell_hash([1, 0, 0]));
        assert_eq!(hash_cell_of([79, 47, 79]), [0, 0, 0]);
        assert_eq!(hash_cell_of([80, 48, -1]), [1, 1, -1]);
    }

    #[test]
    fn a_stored_box_is_found_from_every_cell_it_reaches_and_touching_boxes_share_none() {
        let b = Aabb {
            min: [-40, -24, -20],
            max: [40, 0, 20],
        };
        let mut h = SpatialHash::default();
        h.insert(7, &b).unwrap();
        // A query in a cell the stored box does not reach shares no cell with it, while one that
        // overlaps does.
        let touching = Aabb {
            min: [160, -24, -20],
            max: [200, 0, 20],
        };
        assert!(
            h.near(&touching).unwrap().is_empty(),
            "a box in the next hash cell is not a candidate"
        );
        let overlapping = Aabb {
            min: [30, -24, -20],
            max: [60, 0, 20],
        };
        assert_eq!(h.near(&overlapping).unwrap(), vec![7]);
        h.remove(7, &b).unwrap();
        assert!(h.is_empty());
        assert_eq!(h.entries(), 0);
    }

    #[test]
    fn a_box_over_the_cell_limit_is_an_error_and_never_iterated() {
        let huge = Aabb {
            min: [-10_000_000, -10_000_000, -10_000_000],
            max: [10_000_000, 10_000_000, 10_000_000],
        };
        assert!(huge.lattice_cells() > MAX_BOX_CELLS);
        let t = std::time::Instant::now();
        let mut h = SpatialHash::default();
        for e in [
            h.insert(1, &huge).unwrap_err(),
            h.remove(1, &huge).unwrap_err(),
            h.near(&huge).unwrap_err(),
            check_box(&huge).unwrap_err(),
        ] {
            assert!(matches!(e, EngineError::BoxTooLarge { .. }), "{e}");
        }
        assert!(t.elapsed().as_millis() < 500, "refused without iterating");
        assert!(h.is_empty());
        // The limit itself is allowed; one cell more is not.
        let at_limit = Aabb {
            min: [0, 0, 0],
            max: [20 * 256, 8, 20 * 256],
        };
        assert_eq!(at_limit.lattice_cells(), MAX_BOX_CELLS);
        assert!(check_box(&at_limit).is_ok());
        let over = Aabb {
            max: [20 * 257, 8, 20 * 256],
            ..at_limit
        };
        assert!(check_box(&over).is_err());
        // An inverted box covers nothing and is not an error.
        let inverted = Aabb {
            min: [10, 0, 0],
            max: [0, 8, 20],
        };
        assert_eq!(inverted.lattice_cells(), 0);
        assert!(h.insert(2, &inverted).is_ok() && h.is_empty());
    }

    #[test]
    fn a_ray_walk_finds_the_boxes_it_passes_and_stops_early() {
        let mut h = SpatialHash::default();
        // Three boxes in a row along X, one far off to the side.
        for (slot, x) in [(0u32, 0), (1, 400), (2, 800)] {
            h.insert(
                slot,
                &Aabb {
                    min: [x - 10, -8, -10],
                    max: [x + 10, 0, 10],
                },
            )
            .unwrap();
        }
        let mut seen = Vec::new();
        let ok = h.walk_ray([-100.0, -4.0, 0.0], [1.0, 0.0, 0.0], |s| {
            seen.push(s);
            // Pretend only slot 0 is hit, at t = 90.
            if s == 0 {
                90.0
            } else {
                f64::INFINITY
            }
        });
        assert!(ok);
        assert!(seen.contains(&0));
        assert!(
            !seen.contains(&2),
            "the walk stops once the hit is nearer than the next cell: {seen:?}"
        );
        // A ray that misses the extent visits nothing.
        let mut none = 0;
        assert!(h.walk_ray([0.0, -500.0, 5000.0], [1.0, 0.0, 0.0], |_| {
            none += 1;
            f64::INFINITY
        }));
        assert_eq!(none, 0);
    }
}
