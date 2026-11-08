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
fn nearest_phase(v: f64, phase: i32, step: i32) -> i32 {
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
    /// ground plane Y = 0 seen from above.
    pub fn pick(&self, ray: &Ray) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for inst in self.instances.iter() {
            if let Some((t, normal)) = ray_box(ray, &self.pick_box(&inst)) {
                if best.is_none_or(|b| t < b.t) {
                    best = Some(Hit {
                        instance: Some(inst.id),
                        point: ray.at(t),
                        normal,
                        t,
                    });
                }
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
        let local = shape.body.transformed(rot, [0, 0, 0]);
        // The phase that puts the part's studs on the grid.
        let phase = shape
            .connectors
            .iter()
            .map(|c| c.transformed(rot, [0, 0, 0]))
            .find(|c| c.dir[1] != 0)
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
        for &y in &ys {
            for i in -WINDOW..=WINDOW {
                for j in -WINDOW..=WINDOW {
                    let pos = [x0 + i * STUD_PITCH_LDU, y, z0 + j * STUD_PITCH_LDU];
                    let pose = LatticePose { pos, rot };
                    let b = local.transformed(Rot24::IDENTITY, pos);
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
                    let dist = d.iter().map(|v| v * v).sum::<f64>();
                    let mates = self.mates(part, &pose, None)?;
                    let grounded = b.max[1] == 0;
                    let collides = self.collision(&b, None);
                    let valid = collides.is_none() && b.max[1] <= 0 && (mates > 0 || grounded);
                    let key = Key {
                        invalid: !valid,
                        dist,
                        mates,
                        d,
                    };
                    if best.as_ref().is_none_or(|(k, _)| key.before(k)) {
                        best = Some((
                            key,
                            Snap {
                                pos,
                                rot: rot.0,
                                valid,
                                mates,
                                grounded,
                                collides,
                                target: hit.instance,
                            },
                        ));
                    }
                }
            }
        }
        Ok(best.map(|(_, s)| s))
    }
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
