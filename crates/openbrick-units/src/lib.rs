//! Integer LDU lattice types, the 24 lattice-preserving rotations, and the
//! OpenBrick3D Dimensional Standard loaded from `standards/dimensions-v1.json`.
//!
//! Conventions (see docs/plan/04): positions are integer LDU (1 LDU = 0.4 mm),
//! the frame is LDraw's right-handed frame with minus-Y up, a brick is 24 LDU
//! tall, a plate 8 LDU, and the stud pitch is 20 LDU.

pub mod about;
pub mod credit;
pub mod rot24;
pub mod standard;

pub use about::DISCLAIMER;
pub use credit::Credit;
pub use rot24::Rot24;
pub use standard::{Feature, Standard, STANDARD};

/// Nominal millimetres per LDraw unit. The measured value is 0.3993 mm; the
/// nominal 0.4 mm is used for all conversions at the file boundary.
pub const LDU_MM: f64 = 0.4;
/// Stud pitch in LDU.
pub const STUD_PITCH_LDU: i32 = 20;
/// Plate height in LDU.
pub const PLATE_LDU: i32 = 8;
/// Brick height in LDU (three plates).
pub const BRICK_LDU: i32 = 24;
/// Half-stud (jumper) offset in LDU.
pub const HALF_STUD_LDU: i32 = 10;
/// Half-plate (bracket) offset in LDU.
pub const HALF_PLATE_LDU: i32 = 4;
/// Finest common lattice for studs and plates (gcd of 20 and 8).
pub const LATTICE_LDU: i32 = 4;
/// Finest lattice once half-stud jumpers are included (gcd of 10 and 8).
pub const FINE_LATTICE_LDU: i32 = 2;

/// Convert LDU to millimetres.
#[inline]
pub fn ldu_to_mm(ldu: i32) -> f64 {
    ldu as f64 * LDU_MM
}

/// Convert millimetres to the nearest integer LDU.
#[inline]
pub fn mm_to_ldu(mm: f64) -> i32 {
    (mm / LDU_MM).round() as i32
}

/// A lattice pose: integer LDU position plus one of the 24 lattice rotations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LatticePose {
    pub pos: [i32; 3],
    pub rot: Rot24,
}

/// A free pose for hinged, clipped or otherwise off-lattice parts.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FreePose {
    pub translation: [f64; 3],
    /// Row-major 3x3 rotation matrix.
    pub rotation: [[f64; 3]; 3],
}

/// Either a lattice pose or a free pose.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Pose {
    Lattice(LatticePose),
    Free(FreePose),
}

impl LatticePose {
    pub const IDENTITY: LatticePose = LatticePose {
        pos: [0, 0, 0],
        rot: Rot24::IDENTITY,
    };

    /// Compose two lattice poses: `self` applied after `other`.
    pub fn compose(&self, other: &LatticePose) -> LatticePose {
        let rotated = self.rot.apply(other.pos);
        LatticePose {
            pos: [
                self.pos[0] + rotated[0],
                self.pos[1] + rotated[1],
                self.pos[2] + rotated[2],
            ],
            rot: self.rot.compose(other.rot),
        }
    }

    /// True when the position lies on the finest common stud/plate lattice.
    pub fn is_on_lattice(&self) -> bool {
        self.pos.iter().all(|c| c % LATTICE_LDU == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_relations_hold() {
        assert_eq!(BRICK_LDU, 3 * PLATE_LDU);
        assert_eq!(
            5 * PLATE_LDU,
            2 * STUD_PITCH_LDU,
            "five plates equal two studs"
        );
        assert_eq!(LATTICE_LDU, gcd(STUD_PITCH_LDU, PLATE_LDU));
        assert_eq!(FINE_LATTICE_LDU, gcd(HALF_STUD_LDU, PLATE_LDU));
        assert!((ldu_to_mm(STUD_PITCH_LDU) - 8.0).abs() < 1e-12);
        assert_eq!(mm_to_ldu(9.6), BRICK_LDU);
    }

    fn gcd(a: i32, b: i32) -> i32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
}
