//! Assembler engine (docs/plan/07). M0 holds the struct-of-arrays instance table
//! and the lattice spatial hash key; snapping, collision, the connection graph and
//! the command log arrive in M3a.

use std::collections::HashMap;

use openbrick_units::Pose;

/// Index into the part catalog.
pub type PartId = u32;
/// Stable per-instance id (ULID as a string in M0; a 128-bit type later).
pub type Ulid = String;

/// Struct-of-arrays instance table.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Instances {
    pub ulid: Vec<Ulid>,
    pub part: Vec<PartId>,
    pub pose: Vec<Pose>,
    pub colour: Vec<u16>,
    pub flags: Vec<u8>,
    pub step: Vec<u16>,
    pub alive: Vec<bool>,
}

impl Instances {
    pub fn len(&self) -> usize {
        self.alive.iter().filter(|a| **a).count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn push(&mut self, ulid: Ulid, part: PartId, pose: Pose, colour: u16) -> usize {
        self.ulid.push(ulid);
        self.part.push(part);
        self.pose.push(pose);
        self.colour.push(colour);
        self.flags.push(0);
        self.step.push(0);
        self.alive.push(true);
        self.alive.len() - 1
    }
}

/// Lattice cell size for the spatial hash: one brick footprint (20 x 8 x 20 LDU).
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

/// A minimal spatial hash from cells to instance indices.
#[derive(Clone, Debug, Default)]
pub struct SpatialHash {
    cells: HashMap<[i32; 3], Vec<usize>>,
}

impl SpatialHash {
    pub fn insert(&mut self, cell: [i32; 3], instance: usize) {
        self.cells.entry(cell).or_default().push(instance);
    }
    pub fn get(&self, cell: [i32; 3]) -> &[usize] {
        self.cells.get(&cell).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_units::{LatticePose, Rot24};

    #[test]
    fn cells_partition_the_lattice() {
        assert_eq!(cell_of([0, 0, 0]), [0, 0, 0]);
        assert_eq!(cell_of([19, 7, 19]), [0, 0, 0]);
        assert_eq!(cell_of([20, 8, -1]), [1, 1, -1]);
        assert_ne!(cell_hash([0, 0, 0]), cell_hash([1, 0, 0]));
    }

    #[test]
    fn instances_push_and_count() {
        let mut t = Instances::default();
        let p = Pose::Lattice(LatticePose {
            pos: [0, 0, 0],
            rot: Rot24::IDENTITY,
        });
        t.push("01J".into(), 3001, p, 4);
        assert_eq!(t.len(), 1);
    }
}
