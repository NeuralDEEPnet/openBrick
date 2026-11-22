//! What the assembler engine needs to know about a part: its integer-LDU boxes
//! and its connectors, both in the part's own LDraw frame (minus Y up, origin
//! centred on the top stud group, stud bases at Y = 0; docs/plan/04 section 3).
//! Everything here is derived from the part evaluator's output
//! ([`crate::builtin`]), never typed in by hand (CLAUDE.md).

use openbrick_units::{Rot24, PLATE_LDU, STUD_PITCH_LDU};

/// The widest a part's box may be along any axis, in LDU (8,192 LDU is 3.3 m; the widest part of
/// the 2026-10 LDraw library is 960). A pack that claims more is refused.
pub const MAX_BOX_EXTENT_LDU: i32 = 8192;
/// The most lattice cells (one stud by one plate by one stud) one body box may cover; the engine's
/// spatial hash holds one entry per cell, so this bounds its work. The 2026-10 library's largest
/// part covers about 21,000.
pub const MAX_BOX_CELLS: u64 = 65_536;

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

    /// How many lattice cells (one stud by one plate by one stud) the box covers: the cells from the
    /// one of its minimum corner to the one just inside its maximum corner. Counted, never iterated,
    /// so it is cheap for any box; an inverted box covers none.
    pub fn lattice_cells(&self) -> u64 {
        let cell = [STUD_PITCH_LDU, PLATE_LDU, STUD_PITCH_LDU];
        let mut n = 1u64;
        for ((lo, hi), c) in self.min.iter().zip(&self.max).zip(cell) {
            let c = i64::from(c);
            let lo = i64::from(*lo).div_euclid(c);
            let hi = (i64::from(*hi) - 1).div_euclid(c);
            n = n.saturating_mul((hi - lo + 1).max(0) as u64);
        }
        n
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

/// How two mated connectors hold each other: the class of a connection, for the statistics, the
/// hover line and the rule that decides whether two connectors mate. Every [`ConnKind`] belongs to
/// one, and its mate to the same one.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Link {
    /// A stud in a receptacle (tube, pin or wall around it).
    Stud,
    /// A Technic pin in a round hole.
    Pin,
    /// An axle in a cross-shaped hole.
    Axle,
    /// A clip gripping a bar, handle or stem.
    Clip,
    /// A ball in a socket.
    Ball,
    /// Two hinge halves whose fingers interleave on one axis.
    Hinge,
}

impl Link {
    /// Every class, in the order of their bits.
    pub const ALL: [Link; 6] = [
        Link::Stud,
        Link::Pin,
        Link::Axle,
        Link::Clip,
        Link::Ball,
        Link::Hinge,
    ];

    /// The bit of this class in a set of classes (`Snap::links`).
    pub fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// The short name the page's hover line and statistics use.
    pub fn name(self) -> &'static str {
        match self {
            Link::Stud => "stud",
            Link::Pin => "pin",
            Link::Axle => "axle",
            Link::Clip => "clip",
            Link::Ball => "ball",
            Link::Hinge => "hinge",
        }
    }

    /// What a connection of this class is, in words for the hover line: "pin in hole".
    pub fn phrase(self) -> &'static str {
        match self {
            Link::Stud => "stud in receptacle",
            Link::Pin => "pin in hole",
            Link::Axle => "axle in hole",
            Link::Clip => "clip on bar",
            Link::Ball => "ball in socket",
            Link::Hinge => "hinge fingers interleaved",
        }
    }

    /// The classes in a set of bits, in order.
    pub fn in_set(bits: u32) -> impl Iterator<Item = Link> {
        Link::ALL.into_iter().filter(move |l| bits & l.bit() != 0)
    }

    /// True when the connection leaves a rotation free about its axis (or, for a ball, about its
    /// point): the engine keeps the part at the one lattice pose it was placed in (its default
    /// pose) and the connection records the freedom. A stud grips on the lattice, an axle locks.
    pub fn free_rotation(self) -> bool {
        matches!(self, Link::Pin | Link::Clip | Link::Ball | Link::Hinge)
    }
}

/// The connector kinds the engine handles (the docs/plan/06 section 6 names): a
/// stud on top and the receptacle under a body that takes one (tubes, pins and
/// walls together; `tubeF`), which the built-in catalog and the LDraw pack
/// derive, plus the Technic pin and axle connectors, the clip, bar, ball, socket and hinge
/// connectors the LDraw pack derives from its primitives and the joint table
/// (`standards/joints-v1.json`). Studs and receptacles mate pointing at each other; a pin, an
/// axle, a clip with its bar and the two halves of a hinge mate when their centres coincide and
/// their axes are parallel (an axis has no sign, so either direction fits); a ball and its socket
/// mate when their centres coincide, whatever way the stem points.
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
    /// A clip's grip: where a bar or handle sits, along the bar's axis.
    #[serde(rename = "clipF")]
    ClipF,
    /// A bar, handle, stem or rod a clip grips, along its axis.
    #[serde(rename = "barM")]
    BarM,
    #[serde(rename = "ballM")]
    BallM,
    #[serde(rename = "socketF")]
    SocketF,
    /// A hinge half with an odd number of fingers (one or three).
    #[serde(rename = "hingeO")]
    HingeO,
    /// A hinge half with an even number of fingers (two).
    #[serde(rename = "hingeE")]
    HingeE,
}

impl ConnKind {
    /// Every kind, in declaration order.
    pub const ALL: [ConnKind; 12] = [
        ConnKind::StudM,
        ConnKind::TubeF,
        ConnKind::PinM,
        ConnKind::PinF,
        ConnKind::AxleM,
        ConnKind::AxleF,
        ConnKind::ClipF,
        ConnKind::BarM,
        ConnKind::BallM,
        ConnKind::SocketF,
        ConnKind::HingeO,
        ConnKind::HingeE,
    ];

    /// The evaluator's name of this kind.
    pub fn name(self) -> &'static str {
        match self {
            ConnKind::StudM => "studM",
            ConnKind::TubeF => "tubeF",
            ConnKind::PinM => "pinM",
            ConnKind::PinF => "pinF",
            ConnKind::AxleM => "axleM",
            ConnKind::AxleF => "axleF",
            ConnKind::ClipF => "clipF",
            ConnKind::BarM => "barM",
            ConnKind::BallM => "ballM",
            ConnKind::SocketF => "socketF",
            ConnKind::HingeO => "hingeO",
            ConnKind::HingeE => "hingeE",
        }
    }

    /// The kind from the evaluator's name, if the engine handles it.
    pub fn from_name(name: &str) -> Option<ConnKind> {
        ConnKind::ALL.into_iter().find(|k| k.name() == name)
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
            ConnKind::ClipF => ConnKind::BarM,
            ConnKind::BarM => ConnKind::ClipF,
            ConnKind::BallM => ConnKind::SocketF,
            ConnKind::SocketF => ConnKind::BallM,
            ConnKind::HingeO => ConnKind::HingeE,
            ConnKind::HingeE => ConnKind::HingeO,
        }
    }

    /// How a connection of this kind holds (see [`Link`]).
    pub fn link(self) -> Link {
        match self {
            ConnKind::StudM | ConnKind::TubeF => Link::Stud,
            ConnKind::PinM | ConnKind::PinF => Link::Pin,
            ConnKind::AxleM | ConnKind::AxleF => Link::Axle,
            ConnKind::ClipF | ConnKind::BarM => Link::Clip,
            ConnKind::BallM | ConnKind::SocketF => Link::Ball,
            ConnKind::HingeO | ConnKind::HingeE => Link::Hinge,
        }
    }

    /// Pins, axles, clips, bars and hinges: their direction is an axis, not a facing.
    pub fn is_axial(self) -> bool {
        !matches!(
            self,
            ConnKind::StudM | ConnKind::TubeF | ConnKind::BallM | ConnKind::SocketF
        )
    }

    /// Balls and sockets mate by position alone: the stem may point anywhere.
    pub fn is_point(self) -> bool {
        matches!(self, ConnKind::BallM | ConnKind::SocketF)
    }

    /// The part that goes into the other (stud, pin, axle, bar, ball, odd hinge half).
    pub fn is_male(self) -> bool {
        matches!(
            self,
            ConnKind::StudM
                | ConnKind::PinM
                | ConnKind::AxleM
                | ConnKind::BarM
                | ConnKind::BallM
                | ConnKind::HingeO
        )
    }

    /// Studs and receptacles sit on the stud grid; the others do not have to.
    pub fn is_stud_kind(self) -> bool {
        matches!(self, ConnKind::StudM | ConnKind::TubeF)
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
        if self.kind.is_point() {
            true
        } else if self.kind.is_axial() {
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
    /// The occupancy mask of the body ([`crate::mask`]): the narrowphase of collision for a part
    /// whose box is not its shape. `None` is the box. Never serialised; read from the pack.
    #[serde(skip)]
    pub mask: Option<crate::mask::SharedMask>,
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
