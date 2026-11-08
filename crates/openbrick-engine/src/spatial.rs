//! The lattice spatial hash (docs/plan/07 section 4) and the connector hash.

use std::collections::HashMap;

use openbrick_catalog::{Aabb, Conn};

use crate::model::InstanceId;

/// Lattice cell size for the spatial hash: one brick footprint (20 x 8 x 20 LDU,
/// one stud by one plate).
pub const CELL: [i32; 3] = [20, 8, 20];

/// Teschner et al. 2003 hash of a lattice cell.
pub fn cell_hash(cell: [i32; 3]) -> u64 {
    let (x, y, z) = (cell[0] as i64, cell[1] as i64, cell[2] as i64);
    ((x.wrapping_mul(73_856_093)) ^ (y.wrapping_mul(19_349_663)) ^ (z.wrapping_mul(83_492_791)))
        as u64
}

/// Cell containing an LDU position.
pub fn cell_of(pos: [i32; 3]) -> [i32; 3] {
    [
        pos[0].div_euclid(CELL[0]),
        pos[1].div_euclid(CELL[1]),
        pos[2].div_euclid(CELL[2]),
    ]
}

/// The cells a box covers: from the cell of its minimum corner to the cell of
/// the point just inside its maximum corner (a box that ends on a cell border
/// does not reach into the next cell).
fn cells(b: &Aabb) -> impl Iterator<Item = [i32; 3]> {
    let lo = cell_of(b.min);
    let hi = cell_of([b.max[0] - 1, b.max[1] - 1, b.max[2] - 1]);
    (lo[0]..=hi[0]).flat_map(move |x| {
        (lo[1]..=hi[1]).flat_map(move |y| (lo[2]..=hi[2]).map(move |z| [x, y, z]))
    })
}

/// Instances by the cells their body boxes cover.
#[derive(Clone, Debug, Default)]
pub struct SpatialHash {
    cells: HashMap<[i32; 3], Vec<InstanceId>>,
}

impl SpatialHash {
    pub fn insert(&mut self, id: InstanceId, b: &Aabb) {
        for c in cells(b) {
            self.cells.entry(c).or_default().push(id);
        }
    }

    pub fn remove(&mut self, id: InstanceId, b: &Aabb) {
        for c in cells(b) {
            if let Some(v) = self.cells.get_mut(&c) {
                v.retain(|&i| i != id);
                if v.is_empty() {
                    self.cells.remove(&c);
                }
            }
        }
    }

    /// Instances whose cells meet the cells of `b`, each once, in id order
    /// (candidates: the caller tests the boxes themselves).
    pub fn near(&self, b: &Aabb) -> Vec<InstanceId> {
        let mut out: Vec<InstanceId> = cells(b)
            .filter_map(|c| self.cells.get(&c))
            .flatten()
            .copied()
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// Placed connectors by position, so a mate is one lookup (docs/plan/07
/// section 4: the connector hash).
#[derive(Clone, Debug, Default)]
pub struct ConnHash {
    at: HashMap<[i32; 3], Vec<(InstanceId, Conn)>>,
}

impl ConnHash {
    pub fn insert(&mut self, id: InstanceId, conns: &[Conn]) {
        for c in conns {
            self.at.entry(c.pos).or_default().push((id, *c));
        }
    }

    pub fn remove(&mut self, id: InstanceId, conns: &[Conn]) {
        for c in conns {
            if let Some(v) = self.at.get_mut(&c.pos) {
                v.retain(|(i, _)| *i != id);
                if v.is_empty() {
                    self.at.remove(&c.pos);
                }
            }
        }
    }

    /// The instance whose placed connector mates with `c`, other than `skip`.
    pub fn mate_of(&self, c: &Conn, skip: Option<InstanceId>) -> Option<InstanceId> {
        self.at
            .get(&c.pos)?
            .iter()
            .filter(|(i, _)| Some(*i) != skip)
            .find(|(_, other)| c.mates(other))
            .map(|(i, _)| *i)
    }

    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
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
    }

    #[test]
    fn a_box_covers_only_the_cells_it_reaches_into() {
        let b = Aabb {
            min: [-40, -24, -20],
            max: [40, 0, 20],
        };
        assert_eq!(cells(&b).count(), 4 * 3 * 2);
        let mut h = SpatialHash::default();
        h.insert(7, &b);
        let touching = Aabb {
            min: [40, -24, -20],
            max: [60, 0, 20],
        };
        assert!(h.near(&touching).is_empty(), "touching boxes share no cell");
        h.remove(7, &b);
        assert!(h.is_empty());
    }
}
