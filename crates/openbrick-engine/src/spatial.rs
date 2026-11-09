//! The lattice spatial hash (docs/plan/07 section 4) and the connector hash.

use std::collections::HashMap;

use openbrick_catalog::{Aabb, Conn, MAX_BOX_CELLS};
use openbrick_units::{PLATE_LDU, STUD_PITCH_LDU};

use crate::model::{EngineError, InstanceId};

/// Lattice cell size for the spatial hash: one stud by one plate by one stud
/// (docs/plan/07 section 4), from the Dimensional Standard's lattice constants.
pub const CELL: [i32; 3] = [STUD_PITCH_LDU, PLATE_LDU, STUD_PITCH_LDU];

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

/// Refuse a box that covers more than [`MAX_BOX_CELLS`] cells: the hash holds one entry per cell,
/// so an unbounded box (a hostile pack or document) would be unbounded work. Counted, not iterated.
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

/// The cells a box covers: from the cell of its minimum corner to the cell of
/// the point just inside its maximum corner (a box that ends on a cell border
/// does not reach into the next cell). At most [`MAX_BOX_CELLS`] of them, else an error.
fn cells(b: &Aabb) -> Result<impl Iterator<Item = [i32; 3]>, EngineError> {
    check_box(b)?;
    let lo = cell_of(b.min);
    let hi = cell_of([
        b.max[0].saturating_sub(1),
        b.max[1].saturating_sub(1),
        b.max[2].saturating_sub(1),
    ]);
    Ok((lo[0]..=hi[0]).flat_map(move |x| {
        (lo[1]..=hi[1]).flat_map(move |y| (lo[2]..=hi[2]).map(move |z| [x, y, z]))
    }))
}

/// Instances by the cells their body boxes cover.
#[derive(Clone, Debug, Default)]
pub struct SpatialHash {
    cells: HashMap<[i32; 3], Vec<InstanceId>>,
}

impl SpatialHash {
    pub fn insert(&mut self, id: InstanceId, b: &Aabb) -> Result<(), EngineError> {
        for c in cells(b)? {
            self.cells.entry(c).or_default().push(id);
        }
        Ok(())
    }

    pub fn remove(&mut self, id: InstanceId, b: &Aabb) -> Result<(), EngineError> {
        for c in cells(b)? {
            if let Some(v) = self.cells.get_mut(&c) {
                v.retain(|&i| i != id);
                if v.is_empty() {
                    self.cells.remove(&c);
                }
            }
        }
        Ok(())
    }

    /// Instances whose cells meet the cells of `b`, each once, in id order
    /// (candidates: the caller tests the boxes themselves).
    pub fn near(&self, b: &Aabb) -> Result<Vec<InstanceId>, EngineError> {
        let mut out: Vec<InstanceId> = cells(b)?
            .filter_map(|c| self.cells.get(&c))
            .flatten()
            .copied()
            .collect();
        out.sort_unstable();
        out.dedup();
        Ok(out)
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
        assert_eq!(cells(&b).unwrap().count(), 4 * 3 * 2);
        let mut h = SpatialHash::default();
        h.insert(7, &b).unwrap();
        let touching = Aabb {
            min: [40, -24, -20],
            max: [60, 0, 20],
        };
        assert!(
            h.near(&touching).unwrap().is_empty(),
            "touching boxes share no cell"
        );
        h.remove(7, &b).unwrap();
        assert!(h.is_empty());
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
    }
}
