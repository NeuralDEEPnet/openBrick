//! Picking and snapping (docs/plan/07 section 5).
//!
//! The page sends the pointer ray in the model's frame (LDU, LDraw frame). The
//! engine finds what the ray hits first (a placed part's pick box or the ground
//! plane) and then the best pose for the part being placed near that point:
//!
//! - on a part's top face: the new part's receptacles on the studs there;
//! - on a part's bottom face: the new part's studs in the receptacles there;
//! - on a side face: beside it, at any height where its connectors mate;
//! - on the ground: standing on the ground, which needs no mates.
//!
//! Candidates are the lattice poses in a small window (plus or minus
//! [`WINDOW`] studs, and plates for side faces) around the point, all with
//! their studs on the stud grid. They are ranked by: valid first (no overlap,
//! above the ground, and at least one mate unless on the ground), then the
//! distance from the cursor to the new part's contact face, then more mates,
//! then the offset from the cursor, axis by axis. The order is total, so the
//! answer is deterministic, and it depends only on positions relative to the
//! cursor, so moving the model and the ray by a lattice step moves the answer
//! by the same step (the property tests check both).

use openbrick_catalog::{Aabb, PartId};
use openbrick_units::{LatticePose, Rot24, HALF_STUD_LDU, PLATE_LDU, STUD_PITCH_LDU};

use crate::model::{EngineError, InstanceId, Model};

/// Candidates span this many lattice steps on each side of the cursor.
pub const WINDOW: i32 = 2;

/// A pointer ray in the model frame (LDU; minus Y up). `dir` need not be unit.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ray {
    pub origin: [f64; 3],
    pub dir: [f64; 3],
}

impl Ray {
    fn at(&self, t: f64) -> [f64; 3] {
        [
            self.origin[0] + t * self.dir[0],
            self.origin[1] + t * self.dir[1],
            self.origin[2] + t * self.dir[2],
        ]
    }
}

/// What a ray hits first.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hit {
    /// The placed part, or `None` for the ground.
    pub instance: Option<InstanceId>,
    pub point: [f64; 3],
    /// Outward lattice normal of the face hit (the ground's is up, `[0, -1, 0]`).
    pub normal: [i32; 3],
    pub t: f64,
}

/// The pose the engine proposes for a part, and whether it can go there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub pos: [i32; 3],
    pub rot: u8,
    /// No overlap, above the ground, and attached (mates, or on the ground).
    pub valid: bool,
    /// Connectors that would mate with placed parts.
    pub mates: u32,
    /// The classes of those connections, as [`openbrick_catalog::Link`] bits (a stud in a
    /// receptacle, a pin in a hole, an axle in a hole, a clip on a bar, a ball in a socket, hinge
    /// halves): what the page's hover line says the part would be held by.
    pub links: u32,
    /// On the ground (its bottom at Y = 0).
    pub grounded: bool,
    /// A placed part it would overlap, if any.
    pub collides: Option<InstanceId>,
    /// The placed part the ray hit, if any.
    pub target: Option<InstanceId>,
}

impl Snap {
    pub fn pose(&self) -> LatticePose {
        LatticePose {
            pos: self.pos,
            rot: Rot24(self.rot),
        }
    }
}

/// Slab test: entry distance and the axis/sign of the face entered.
fn ray_box(ray: &Ray, b: &Aabb) -> Option<(f64, [i32; 3])> {
    let mut t0 = f64::NEG_INFINITY;
    let mut t1 = f64::INFINITY;
    let mut normal = [0; 3];
    for k in 0..3 {
        let (lo, hi) = (f64::from(b.min[k]), f64::from(b.max[k]));
        let (o, d) = (ray.origin[k], ray.dir[k]);
        if d.abs() < 1e-12 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (mut a, mut c) = ((lo - o) / d, (hi - o) / d);
        // Entering through the min face means the face normal points to minus.
        let mut n = -1;
        if a > c {
            std::mem::swap(&mut a, &mut c);
            n = 1;
        }
        if a > t0 {
            t0 = a;
            normal = [0; 3];
            normal[k] = n;
        }
        t1 = t1.min(c);
        if t0 > t1 {
            return None;
        }
    }
    (t0 >= 0.0 && normal != [0; 3]).then_some((t0, normal))
}

/// The value nearest `v` that is `phase` modulo `step` (ties go down).
///
/// `v` must be within [`crate::MAX_COORD`] (the callers check), so the result and
/// the candidate arithmetic around it stay far inside `i32`; the clamp is a second line
/// of defence against a value that is not.
fn nearest_phase(v: f64, phase: i32, step: i32) -> i32 {
    let limit = f64::from(crate::MAX_COORD);
    let v = if v.is_finite() {
        v.clamp(-limit, limit)
    } else {
        0.0
    };
    let k = ((v - f64::from(phase)) / f64::from(step)).round();
    let candidate = phase + step * k as i32;
    // round() sends .5 away from zero; make ties go down for symmetry.
    if (f64::from(candidate) - v - f64::from(step) / 2.0).abs() < 1e-9 {
        candidate - step
    } else {
        candidate
    }
}

impl Model<'_> {
    /// The first thing `ray` hits: a placed part (lowest id on a tie) or the
    /// ground plane Y = 0 seen from above. The ray walks the spatial hash cell by cell and stops
    /// at the first cell that cannot hold anything nearer, so the cost depends on what is near the
    /// ray, not on how many parts the model has.
    pub fn pick(&self, ray: &Ray) -> Option<Hit> {
        self.pick_with(ray, true)
    }

    /// [`Model::pick`] testing every part's box one by one: the answer the walk must equal (the
    /// property tests compare them), and what the walk falls back to for a ray that would cross
    /// more cells than the hash holds entries.
    pub fn pick_linear(&self, ray: &Ray) -> Option<Hit> {
        self.pick_with(ray, false)
    }

    fn pick_with(&self, ray: &Ray, walk: bool) -> Option<Hit> {
        // A ray that is not finite, or starts absurdly far away, hits nothing.
        let sane = ray
            .origin
            .iter()
            .all(|c| c.is_finite() && c.abs() <= crate::MAX_RAY)
            && ray
                .dir
                .iter()
                .all(|c| c.is_finite() && c.abs() <= crate::MAX_RAY)
            && ray.dir != [0.0; 3];
        if !sane {
            return None;
        }
        let mut best: Option<Hit> = None;
        let test = |id: InstanceId, b: &Aabb, best: &mut Option<Hit>| -> f64 {
            let Some((t, normal)) = ray_box(ray, b) else {
                return f64::INFINITY;
            };
            let better = match best {
                None => true,
                Some(h) => t < h.t || (t == h.t && h.instance.is_some_and(|other| id < other)),
            };
            if better {
                *best = Some(Hit {
                    instance: Some(id),
                    point: ray.at(t),
                    normal,
                    t,
                });
            }
            t
        };
        let walked = walk
            && self.spatial().walk_ray(ray.origin, ray.dir, |slot| {
                test(
                    self.instances.id_of(slot),
                    self.instances.pick_at(slot),
                    &mut best,
                )
            });
        if !walked {
            for &slot in self.instances.slots() {
                test(
                    self.instances.id_of(slot),
                    self.instances.pick_at(slot),
                    &mut best,
                );
            }
        }
        if ray.dir[1] > 1e-12 && ray.origin[1] < 0.0 {
            let t = -ray.origin[1] / ray.dir[1];
            if best.is_none_or(|b| t < b.t) {
                let mut point = ray.at(t);
                point[1] = 0.0;
                best = Some(Hit {
                    instance: None,
                    point,
                    normal: [0, -1, 0],
                    t,
                });
            }
        }
        best
    }

    /// The best pose for `part` turned by `rot` where `ray` points, or `None`
    /// when the ray hits nothing. See the module documentation.
    pub fn snap(&self, part: PartId, rot: Rot24, ray: &Ray) -> Result<Option<Snap>, EngineError> {
        let shape = &self.part(part)?.shape;
        if rot.0 >= 24 {
            return Err(EngineError::UnknownRotation(rot.0));
        }
        let Some(hit) = self.pick(ray) else {
            return Ok(None);
        };
        // A hit beyond the coordinate bound can place nothing (check_pose refuses it).
        if hit
            .point
            .iter()
            .any(|c| !c.is_finite() || c.abs() > f64::from(crate::MAX_COORD))
        {
            return Ok(None);
        }
        let local = shape.body.transformed(rot, [0, 0, 0]);
        // The phase that puts the part's studs on the grid.
        let phase = shape
            .connectors
            .iter()
            .map(|c| c.transformed(rot, [0, 0, 0]))
            .find(|c| c.kind.is_stud_kind() && c.dir[1] != 0)
            .map(|c| {
                [
                    (HALF_STUD_LDU - c.pos[0]).rem_euclid(STUD_PITCH_LDU),
                    (HALF_STUD_LDU - c.pos[2]).rem_euclid(STUD_PITCH_LDU),
                ]
            })
            .unwrap_or([0, 0]);
        let centre2 = |k: usize| f64::from(local.min[k] + local.max[k]) / 2.0;
        let target_box = hit.instance.map(|id| {
            let i = self.instance(id).expect("picked");
            self.body_box(i.part, &i.pose).expect("placed")
        });

        // Candidate heights (pos.y) and the anchor of the contact face.
        enum Anchor {
            Bottom,
            Top,
            Centre,
        }
        let (ys, anchor, cursor): (Vec<i32>, Anchor, [f64; 3]) = match (target_box, hit.normal) {
            (None, _) => (vec![-local.max[1]], Anchor::Bottom, hit.point),
            (Some(b), [0, -1, 0]) => (vec![b.min[1] - local.max[1]], Anchor::Bottom, hit.point),
            (Some(b), [0, 1, 0]) => (vec![b.max[1] - local.min[1]], Anchor::Top, hit.point),
            (Some(_), n) => {
                // Beside the face: the new part's centre half its size out.
                let mut c = hit.point;
                for k in [0, 2] {
                    c[k] += f64::from(n[k]) * f64::from(local.max[k] - local.min[k]) / 2.0;
                }
                let base = nearest_phase(c[1] - centre2(1), 0, PLATE_LDU);
                let ys = (-3 * WINDOW..=3 * WINDOW)
                    .map(|i| base + i * PLATE_LDU)
                    .collect();
                (ys, Anchor::Centre, c)
            }
        };
        let x0 = nearest_phase(cursor[0] - centre2(0), phase[0], STUD_PITCH_LDU);
        let z0 = nearest_phase(cursor[2] - centre2(2), phase[1], STUD_PITCH_LDU);

        let mut best: Option<(Key, Snap)> = None;
        // Evaluate one candidate pose; `face_dist` is the ranking distance, `anchor` the point of
        // the part its offset from the cursor is measured at.
        let consider = |pos: [i32; 3],
                        rot: Rot24,
                        dist: Option<f64>,
                        best: &mut Option<(Key, Snap)>|
         -> Result<(), EngineError> {
            let pose = LatticePose { pos, rot };
            let b = shape.body.transformed(rot, pos);
            let a = [
                f64::from(b.min[0] + b.max[0]) / 2.0,
                match anchor {
                    Anchor::Bottom => f64::from(b.max[1]),
                    Anchor::Top => f64::from(b.min[1]),
                    Anchor::Centre => f64::from(b.min[1] + b.max[1]) / 2.0,
                },
                f64::from(b.min[2] + b.max[2]) / 2.0,
            ];
            let d = [a[0] - cursor[0], a[1] - cursor[1], a[2] - cursor[2]];
            let dist = dist.unwrap_or_else(|| d.iter().map(|v| v * v).sum::<f64>());
            let (mates, links) = self.mates_links(part, &pose, None)?;
            let grounded = b.max[1] == 0;
            let collides = self.collision_of(part, &pose, &b, None)?;
            let valid = collides.is_none() && b.max[1] <= 0 && (mates > 0 || grounded);
            let key = Key {
                invalid: !valid,
                dist,
                mates,
                d,
            };
            if best.as_ref().is_none_or(|(k, _)| key.before(k)) {
                *best = Some((
                    key,
                    Snap {
                        pos,
                        rot: rot.0,
                        valid,
                        mates,
                        links,
                        grounded,
                        collides,
                        target: hit.instance,
                    },
                ));
            }
            Ok(())
        };
        for &y in &ys {
            for i in -WINDOW..=WINDOW {
                for j in -WINDOW..=WINDOW {
                    let pos = [x0 + i * STUD_PITCH_LDU, y, z0 + j * STUD_PITCH_LDU];
                    consider(pos, rot, None, &mut best)?;
                }
            }
        }
        // Poses that put one of the part's pins, axles, clips, bars, balls or hinge halves on the
        // matching connector of a part near the cursor. They are not on the stud grid: a pin's hole
        // is half a stud from a brick's studs, and a hole two plates above a beam's bottom is not
        // on the plate grid, so no lattice window finds them. Parts without such connectors, and
        // targets without them, add no candidate (stud and receptacle mates are found by the window).
        if shape.connectors.iter().any(|c| !c.kind.is_stud_kind()) {
            for (pos, rot, _) in self.mate_poses(part, rot, hit.point)? {
                consider(pos, rot, None, &mut best)?;
            }
        }
        Ok(best.map(|(_, s)| s))
    }

    /// The lattice poses that mate a non-stud connector of `part` (turned by `rot`) with a
    /// placed one near `point`, nearest target first: at most [`MATE_TARGETS`] targets and
    /// [`MATE_CONNECTORS`] connectors of the part, each with the distance from the target to
    /// `point` squared. The rotation is the given one when the connector's axis already lies on the
    /// target's; otherwise it is turned a quarter or a half about the axis that takes one onto the
    /// other (so a pin dropped on a hole in a side face lies down to fit, whichever way the user
    /// had turned it). A pin spins freely about its axis and an axle fits in four quarter turns of
    /// it; the roll kept is the one the given rotation implies, and R chooses another.
    pub fn mate_poses(
        &self,
        part: PartId,
        rot: Rot24,
        point: [f64; 3],
    ) -> Result<Vec<([i32; 3], Rot24, f64)>, EngineError> {
        let shape = &self.part(part)?.shape;
        let reach = MATE_REACH;
        let probe = Aabb {
            min: [0, 1, 2].map(|k| (point[k] - f64::from(reach)).floor() as i32),
            max: [0, 1, 2].map(|k| (point[k] + f64::from(reach)).ceil() as i32 + 1),
        };
        let near = self.spatial().near(&probe)?;
        // Targets: placed connectors of an axial or point kind, with their distance to the point.
        let mut targets: Vec<(f64, InstanceId, openbrick_catalog::Conn)> = Vec::new();
        for slot in near {
            let inst = self.instances.at_slot(slot);
            let other = &self.catalog().parts[inst.part as usize].shape;
            for c in other
                .connectors
                .iter()
                .filter(|c| !c.kind.is_stud_kind())
                .map(|c| c.transformed(inst.pose.rot, inst.pose.pos))
            {
                let d: f64 = (0..3)
                    .map(|k| (f64::from(c.pos[k]) - point[k]).powi(2))
                    .sum();
                if d <= f64::from(reach) * f64::from(reach) {
                    targets.push((d, inst.id, c));
                }
            }
        }
        targets.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.kind.cmp(&b.2.kind))
                .then(a.2.pos.cmp(&b.2.pos))
        });
        targets.truncate(MATE_TARGETS);
        let mut out: Vec<([i32; 3], Rot24, f64)> = Vec::new();
        for (d, _, t) in targets {
            let mut used = 0;
            for c in shape
                .connectors
                .iter()
                .filter(|c| !c.kind.is_stud_kind() && c.kind.mate() == t.kind)
            {
                if used == MATE_CONNECTORS {
                    break;
                }
                let Some(r) = align(rot, c, &t) else { continue };
                used += 1;
                let p = r.apply(c.pos);
                let pos = [t.pos[0] - p[0], t.pos[1] - p[1], t.pos[2] - p[2]];
                if pos
                    .iter()
                    .any(|v| v.unsigned_abs() > crate::MAX_COORD as u32)
                {
                    continue;
                }
                if !out.iter().any(|o| o.0 == pos && o.1 == r) {
                    out.push((pos, r, d));
                }
            }
        }
        Ok(out)
    }
}

/// The nearest placed connectors considered when looking for pin, axle, clip, bar, ball and hinge
/// mates, and the connectors of the dragged part tried against each: bounds on the work of one
/// snap, so a beam of sixteen holes dropped among a hundred pins costs a fixed amount.
pub const MATE_TARGETS: usize = 8;
pub const MATE_CONNECTORS: usize = 8;
/// How far from the cursor (LDU) a target connector may be: two studs and a half.
pub const MATE_REACH: i32 = 50;

/// The rotation, derived from `rot`, under which connector `c` of a part mates with the placed
/// connector `t` in direction (and `None` when the two kinds do not mate).
fn align(rot: Rot24, c: &openbrick_catalog::Conn, t: &openbrick_catalog::Conn) -> Option<Rot24> {
    if c.kind.mate() != t.kind {
        return None;
    }
    if c.kind.is_point() {
        return Some(rot);
    }
    let a = rot.apply(c.dir);
    let dot = a[0] * t.dir[0] + a[1] * t.dir[1] + a[2] * t.dir[2];
    if dot != 0 {
        // Parallel or anti-parallel: an axis has no sign.
        return Some(rot);
    }
    // Perpendicular: a quarter turn about a x t takes a onto t.
    let n = [
        a[1] * t.dir[2] - a[2] * t.dir[1],
        a[2] * t.dir[0] - a[0] * t.dir[2],
        a[0] * t.dir[1] - a[1] * t.dir[0],
    ];
    // R v = n x v + n (n . v), the quarter turn about the unit lattice vector n.
    let col = |e: [i32; 3]| {
        let cross = [
            n[1] * e[2] - n[2] * e[1],
            n[2] * e[0] - n[0] * e[2],
            n[0] * e[1] - n[1] * e[0],
        ];
        let dn = n[0] * e[0] + n[1] * e[1] + n[2] * e[2];
        [
            cross[0] + n[0] * dn,
            cross[1] + n[1] * dn,
            cross[2] + n[2] * dn,
        ]
    };
    let (c0, c1, c2) = (col([1, 0, 0]), col([0, 1, 0]), col([0, 0, 1]));
    let quarter = Rot24::from_matrix([
        [c0[0], c1[0], c2[0]],
        [c0[1], c1[1], c2[1]],
        [c0[2], c1[2], c2[2]],
    ])?;
    Some(quarter.compose(rot))
}

/// The ranking of candidates (module documentation).
struct Key {
    invalid: bool,
    dist: f64,
    mates: u32,
    d: [f64; 3],
}

impl Key {
    fn before(&self, other: &Key) -> bool {
        use std::cmp::Ordering;
        let order = self
            .invalid
            .cmp(&other.invalid)
            .then(self.dist.total_cmp(&other.dist))
            .then(other.mates.cmp(&self.mates))
            .then(self.d[0].total_cmp(&other.d[0]))
            .then(self.d[1].total_cmp(&other.d[1]))
            .then(self.d[2].total_cmp(&other.d[2]));
        order == Ordering::Less
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_enters_the_top_face() {
        let b = Aabb {
            min: [-20, -24, -20],
            max: [20, 0, 20],
        };
        let ray = Ray {
            origin: [0.0, -100.0, 0.0],
            dir: [0.0, 1.0, 0.0],
        };
        let (t, n) = ray_box(&ray, &b).unwrap();
        assert_eq!((t, n), (76.0, [0, -1, 0]));
        let side = Ray {
            origin: [-100.0, -10.0, 0.0],
            dir: [1.0, 0.0, 0.0],
        };
        assert_eq!(ray_box(&side, &b).unwrap().1, [-1, 0, 0]);
        let away = Ray {
            origin: [0.0, -100.0, 0.0],
            dir: [0.0, -1.0, 0.0],
        };
        assert!(ray_box(&away, &b).is_none());
    }

    #[test]
    fn nearest_phase_rounds_ties_down() {
        assert_eq!(nearest_phase(0.0, 10, 20), -10);
        assert_eq!(nearest_phase(0.1, 10, 20), 10);
        assert_eq!(nearest_phase(33.0, 10, 20), 30);
        assert_eq!(nearest_phase(-3.0, 0, 8), 0);
        assert_eq!(nearest_phase(-4.0, 0, 8), -8);
    }
}
