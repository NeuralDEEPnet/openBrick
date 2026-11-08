//! Assembler engine (docs/plan/07): the struct-of-arrays instance table, the
//! lattice spatial hash and the connector hash, snapping, collision, the
//! connection graph, commands with undo and redo, and the `.obrick` document.
//!
//! Everything is integer LDU in the LDraw frame (minus Y up; the ground is the
//! plane Y = 0 and parts stand at Y <= 0) with poses from the 24 lattice
//! rotations ([`openbrick_units::Rot24`]). The only floating-point input is the
//! pointer ray of a snap query; every pose that comes out is on the lattice.
//!
//! The part shapes (collision box, pick box, connectors) come from the catalog
//! ([`openbrick_catalog`]), which derives them from the part's features with the
//! evaluator, so no connector is typed in by hand (CLAUDE.md).
//!
//! What the alpha leaves for later (docs/plan/11, M3a and M3b): free poses,
//! submodels, steps, occupancy masks (box collision is exact for the built-in
//! catalog, whose studs always sit in the cavity of the part above),
//! articulation points and the structural report beyond "floating", ULIDs for
//! collaboration (ids are small integers, unique within a document).

pub mod document;
pub mod graph;
pub mod history;
pub mod model;
pub mod snap;
mod spatial;

pub use document::{Bom, BomLine, DocError, ModelDoc, PackRequirement, MODEL_PRAGMA};
pub use graph::Stats;
pub use history::Command;
pub use model::{EngineError, Instance, InstanceId, Instances, Model};
pub use snap::{Hit, Ray, Snap};
pub use spatial::{cell_hash, cell_of, CELL};

/// The largest coordinate, in LDU, a placed part may have on any axis: 10^8, the bound
/// `schemas/obrick.schema.json` states (40,000 km at 0.4 mm per LDU), far inside `i32`
/// so no box or connector arithmetic can overflow. Poses beyond it are refused.
pub const MAX_COORD: i32 = 100_000_000;

/// The largest magnitude of a pointer ray's origin, in LDU: ten times [`MAX_COORD`].
pub const MAX_RAY: f64 = 1.0e9;

/// The stud grid every vertical connector of a placed part lies on: X and Z
/// are odd multiples of half a stud (`HALF_STUD_LDU`, so a 2 x 2 placed at the
/// origin has its studs at plus and minus 10), and Y is a whole number of
/// plates. [`snap`] only produces poses on it for the built-in parts and for
/// parts whose studs all share one phase.
pub fn on_stud_grid(p: [i32; 3]) -> bool {
    use openbrick_units::{HALF_STUD_LDU, PLATE_LDU, STUD_PITCH_LDU};
    let horizontal = |v: i32| (v - HALF_STUD_LDU).rem_euclid(STUD_PITCH_LDU) == 0;
    horizontal(p[0]) && horizontal(p[2]) && p[1].rem_euclid(PLATE_LDU) == 0
}

/// The finest grid a stud or receptacle may lie on: X and Z on whole multiples
/// of half a stud, Y on whole multiples of half a plate. [`Model`] refuses a
/// pose that puts one of these connectors anywhere else. Parts from an LDraw
/// pack can have studs and receptacles half a stud apart (jumper plates) or
/// half a plate off a plate boundary (brackets); the built-in parts always lie
/// on the coarser [`on_stud_grid`].
pub fn on_fine_grid(p: [i32; 3]) -> bool {
    use openbrick_units::{HALF_PLATE_LDU, HALF_STUD_LDU};
    p[0].rem_euclid(HALF_STUD_LDU) == 0
        && p[2].rem_euclid(HALF_STUD_LDU) == 0
        && p[1].rem_euclid(HALF_PLATE_LDU) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_grid_is_half_stud_and_half_plate() {
        assert!(on_fine_grid([10, 4, -30]));
        assert!(on_fine_grid([0, -4, 0]));
        assert!(!on_fine_grid([5, 0, 0]));
        assert!(!on_fine_grid([0, 2, 0]));
    }

    #[test]
    fn stud_grid_is_half_stud_phased() {
        assert!(on_stud_grid([10, 0, -10]));
        assert!(on_stud_grid([-30, -24, 50]));
        assert!(!on_stud_grid([0, 0, 10]));
        assert!(!on_stud_grid([10, 4, 10]));
    }
}
