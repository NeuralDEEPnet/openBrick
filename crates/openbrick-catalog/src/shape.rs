//! What the assembler engine needs to know about a part: its integer-LDU boxes
//! and its connectors, both in the part's own LDraw frame (minus Y up, origin
//! centred on the top stud group, stud bases at Y = 0; docs/plan/04 section 3).
//! Everything here is derived from the part evaluator's output
//! ([`crate::builtin`]), never typed in by hand (CLAUDE.md).

use openbrick_units::Rot24;

/// An axis-aligned box in integer LDU, `min` inclusive and `max` exclusive in
/// the sense that two boxes that only touch (`a.max == b.min`) do not overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Aabb {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl Aabb {
    /// True when the interiors intersect; boxes that only touch on a face,
    /// an edge or a corner do not overlap (docs/plan/07 section 6: touching
    /// is allowed).
    pub fn overlaps(&self, other: &Aabb) -> bool {
        (0..3).all(|k| self.min[k] < other.max[k] && other.min[k] < self.max[k])
    }

    /// This box rotated by `rot` and moved by `pos`. Lattice rotations map an
    /// axis-aligned box onto an axis-aligned box, so the result is exact.
    pub fn transformed(&self, rot: Rot24, pos: [i32; 3]) -> Aabb {
        let a = rot.apply(self.min);
        let b = rot.apply(self.max);
        let mut out = Aabb {
            min: [0; 3],
            max: [0; 3],
        };
        for k in 0..3 {
            // Saturating: the engine bounds positions (`MAX_COORD`), so this never
            // saturates for a placed part; a hostile pose must still not panic.
            out.min[k] = a[k].min(b[k]).saturating_add(pos[k]);
            out.max[k] = a[k].max(b[k]).saturating_add(pos[k]);
        }
        out
    }

    /// Size along each axis.
    pub fn size(&self) -> [i32; 3] {
        [
            self.max[0].saturating_sub(self.min[0]),
            self.max[1].saturating_sub(self.min[1]),
            self.max[2].saturating_sub(self.min[2]),
        ]
    }

    /// The smallest box holding both.
    pub fn union(&self, other: &Aabb) -> Aabb {
        let mut out = *self;
        for k in 0..3 {
            out.min[k] = out.min[k].min(other.min[k]);
            out.max[k] = out.max[k].max(other.max[k]);
        }
        out
    }
}

/// The connector kinds the engine handles (the docs/plan/06 section 6 names): a
/// stud on top and the receptacle under a body that takes one (tubes, pins and
/// walls together; `tubeF`), which the built-in catalog and the LDraw pack
/// derive, plus the Technic pin and axle connectors the LDraw pack derives from
/// its primitives. Studs and receptacles mate pointing at each other; a pin or
/// an axle mates with the hole it sits in when their centres coincide and their
/// axes are parallel (an axis has no sign, so either direction fits).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum ConnKind {
    #[serde(rename = "studM")]
    StudM,
    #[serde(rename = "tubeF")]
    TubeF,
    #[serde(rename = "pinM")]
    PinM,
    #[serde(rename = "pinF")]
    PinF,
    #[serde(rename = "axleM")]
    AxleM,
    #[serde(rename = "axleF")]
    AxleF,
}

impl ConnKind {
    /// The evaluator's name of this kind.
    pub fn name(self) -> &'static str {
        match self {
            ConnKind::StudM => "studM",
            ConnKind::TubeF => "tubeF",
            ConnKind::PinM => "pinM",
            ConnKind::PinF => "pinF",
            ConnKind::AxleM => "axleM",
            ConnKind::AxleF => "axleF",
        }
    }

    /// The kind from the evaluator's name, if the engine handles it.
    pub fn from_name(name: &str) -> Option<ConnKind> {
        match name {
            "studM" => Some(ConnKind::StudM),
            "tubeF" => Some(ConnKind::TubeF),
            "pinM" => Some(ConnKind::PinM),
            "pinF" => Some(ConnKind::PinF),
            "axleM" => Some(ConnKind::AxleM),
            "axleF" => Some(ConnKind::AxleF),
            _ => None,
        }
    }

    /// The kind this one mates with.
    pub fn mate(self) -> ConnKind {
        match self {
            ConnKind::StudM => ConnKind::TubeF,
            ConnKind::TubeF => ConnKind::StudM,
            ConnKind::PinM => ConnKind::PinF,
            ConnKind::PinF => ConnKind::PinM,
            ConnKind::AxleM => ConnKind::AxleF,
            ConnKind::AxleF => ConnKind::AxleM,
        }
    }

    /// Pins and axles: their direction is an axis, not a facing.
    pub fn is_axial(self) -> bool {
        !matches!(self, ConnKind::StudM | ConnKind::TubeF)
    }

    /// The part that goes into the other (stud, pin, axle).
    pub fn is_male(self) -> bool {
        matches!(self, ConnKind::StudM | ConnKind::PinM | ConnKind::AxleM)
    }

    /// Studs and receptacles sit on the stud grid; pins and axles do not have to.
    pub fn is_stud_kind(self) -> bool {
        !self.is_axial()
    }
}

/// One connector: kind, position and the unit lattice vector pointing out of
/// the part along its axis (studs up `(0, -1, 0)`, receptacles down).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Conn {
    pub kind: ConnKind,
    pub pos: [i32; 3],
    pub dir: [i32; 3],
}

impl Conn {
    /// This connector on a part placed at `pos` with rotation `rot`.
    pub fn transformed(&self, rot: Rot24, pos: [i32; 3]) -> Conn {
        let p = rot.apply(self.pos);
        Conn {
            kind: self.kind,
            pos: [
                p[0].saturating_add(pos[0]),
                p[1].saturating_add(pos[1]),
                p[2].saturating_add(pos[2]),
            ],
            dir: rot.apply(self.dir),
        }
    }

    /// True when two placed connectors mate: compatible kinds at the same
    /// point, pointing at each other (studs and receptacles) or along one axis
    /// (pins and axles).
    pub fn mates(&self, other: &Conn) -> bool {
        if self.kind.mate() != other.kind || self.pos != other.pos {
            return false;
        }
        let neg = [-other.dir[0], -other.dir[1], -other.dir[2]];
        if self.kind.is_axial() {
            self.dir == other.dir || self.dir == neg
        } else {
            self.dir == neg
        }
    }
}

/// The engine's view of one part.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartShape {
    /// The body without its studs: the collision box. Studs are left out
    /// because they sit in the cavity of the part above (the stud-vs-cavity
    /// class rule of docs/plan/07 section 6).
    pub body: Aabb,
    /// The body with its studs: the box a pointer ray hits.
    pub pick: Aabb,
    pub connectors: Vec<Conn>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_boxes_do_not_overlap() {
        let a = Aabb {
            min: [0, 0, 0],
            max: [20, 8, 20],
        };
        let b = Aabb {
            min: [20, 0, 0],
            max: [40, 8, 20],
        };
        assert!(!a.overlaps(&b) && !b.overlaps(&a));
        let c = Aabb {
            min: [19, 7, 19],
            max: [40, 8, 20],
        };
        assert!(a.overlaps(&c));
    }

    #[test]
    fn yaw_turn_swaps_the_footprint() {
        let a = Aabb {
            min: [-10, 0, -40],
            max: [10, 24, 40],
        };
        let t = a.transformed(Rot24::yaw_quarter_turns(1), [5, -24, 0]);
        assert_eq!(t.size(), [80, 24, 20]);
        assert_eq!(t.min[1], -24);
    }

    #[test]
    fn studs_mate_with_receptacles_facing_them() {
        let stud = Conn {
            kind: ConnKind::StudM,
            pos: [10, 0, 0],
            dir: [0, -1, 0],
        };
        let tube = Conn {
            kind: ConnKind::TubeF,
            pos: [10, 0, 0],
            dir: [0, 1, 0],
        };
        assert!(stud.mates(&tube) && tube.mates(&stud));
        assert!(!stud.mates(&stud));
        // Turned over about X, the receptacle points up: same point, no mate.
        let over = Rot24::from_matrix([[1, 0, 0], [0, -1, 0], [0, 0, -1]]).unwrap();
        let upside_down = tube.transformed(over, [0, 0, 0]);
        assert_eq!(upside_down.pos, stud.pos);
        assert!(!stud.mates(&upside_down));
    }
}
