//! Property tests (docs/plan/10 section 3, Engine row). `proptest` is not in
//! the dependency tree, so these run seeded random cases from a small
//! SplitMix64 generator; a failure message names the seed to rerun.

mod common;

use common::*;
use openbrick_engine::{on_stud_grid, Command, Model};
use openbrick_units::{LatticePose, Rot24};

const SEEDS: u64 = 24;
const PARTS: [&str; 7] = [
    "ob.brick-1x1",
    "ob.brick-2x4",
    "ob.plate-1x4",
    "ob.plate-2x2",
    "ob.tile-1x2",
    "ob.brick-1x2",
    "ob.plate-2x6",
];

/// Place `n` random parts by snapping random rays; returns the model.
fn build(seed: u64, n: usize) -> Model<'static> {
    let mut rng = Rng(seed);
    let mut m = on_baseplate();
    for _ in 0..n {
        let part = *rng.pick(&PARTS);
        let rot = *rng.pick(&yaws());
        let colour = *rng.pick(&[2u16, 4, 14, 15]);
        let ray = ray_towards(&mut rng, 120);
        place(&mut m, part, rot, &ray, colour);
    }
    m
}

#[test]
fn every_snapped_pose_is_on_the_lattice_and_collides_with_nothing() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed ^ 0xA5);
        let mut m = build(seed, 30);
        for _ in 0..40 {
            let part = *rng.pick(&PARTS);
            let rot = Rot24(*rng.pick(&yaws()));
            let ray = ray_towards(&mut rng, 140);
            let p = m.catalog().find(part).unwrap();
            let Some(s) = m.snap(p, rot, &ray).unwrap() else {
                continue;
            };
            let pose = LatticePose {
                pos: s.pos,
                rot: Rot24(s.rot),
            };
            // Origins are stud-group centres, so odd widths sit on multiples of
            // half a stud: the 2 LDU fine lattice (docs/plan/04 section 3).
            assert!(
                pose.pos
                    .iter()
                    .all(|c| c % openbrick_units::FINE_LATTICE_LDU == 0),
                "seed {seed}: {s:?}"
            );
            for c in m.placed_connectors(p, &pose).unwrap() {
                if c.dir[1] != 0 {
                    assert!(on_stud_grid(c.pos), "seed {seed}: {c:?}");
                }
            }
            let b = m.body_box(p, &pose).unwrap();
            if s.valid {
                assert!(
                    m.collision(&b, None).is_none(),
                    "seed {seed}: a valid snap collides: {s:?}"
                );
                assert!(b.max[1] <= 0);
                assert!(s.mates > 0 || s.grounded, "seed {seed}: attached");
                // And the engine accepts it as a command.
                m.apply(&Command::Add {
                    part: part.into(),
                    pos: s.pos,
                    rot: s.rot,
                    colour: 4,
                })
                .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            } else {
                assert!(s.collides.is_some() || (s.mates == 0 && !s.grounded) || b.max[1] > 0);
            }
        }
        // Whatever was built never overlaps.
        let all: Vec<_> = m.instances().iter().collect();
        for (i, a) in all.iter().enumerate() {
            let ba = m.body_box(a.part, &a.pose).unwrap();
            for b in &all[i + 1..] {
                assert!(
                    !ba.overlaps(&m.body_box(b.part, &b.pose).unwrap()),
                    "seed {seed}"
                );
            }
        }
    }
}

#[test]
fn snapping_is_deterministic() {
    for seed in 0..SEEDS {
        let m = build(seed, 20);
        let mut rng = Rng(seed + 99);
        for _ in 0..10 {
            let ray = ray_towards(&mut rng, 100);
            let p = part(PARTS[seed as usize % PARTS.len()]);
            assert_eq!(
                m.snap(p, Rot24::IDENTITY, &ray).unwrap(),
                m.snap(p, Rot24::IDENTITY, &ray).unwrap()
            );
        }
    }
}

#[test]
fn snapping_is_translation_invariant() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed + 7);
        let (dx, dz) = (20 * rng.range(-6, 6), 20 * rng.range(-6, 6));
        let a = build(seed, 25);
        // The same model shifted by a whole number of studs.
        let mut b = Model::new(a.catalog());
        for i in a.instances().iter() {
            let c = &a.catalog().parts[i.part as usize];
            b.apply(&Command::Add {
                part: c.id.clone(),
                pos: [i.pose.pos[0] + dx, i.pose.pos[1], i.pose.pos[2] + dz],
                rot: i.pose.rot.0,
                colour: i.colour,
            })
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        }
        for _ in 0..20 {
            let ray = ray_towards(&mut rng, 120);
            let mut shifted = ray;
            shifted.origin[0] += f64::from(dx);
            shifted.origin[2] += f64::from(dz);
            let p = part(rng.pick(&PARTS));
            let rot = Rot24(*rng.pick(&yaws()));
            let sa = a.snap(p, rot, &ray).unwrap();
            let sb = b.snap(p, rot, &shifted).unwrap();
            match (sa, sb) {
                (None, None) => {}
                (Some(sa), Some(sb)) => {
                    assert_eq!(
                        [sa.pos[0] + dx, sa.pos[1], sa.pos[2] + dz],
                        sb.pos,
                        "seed {seed}"
                    );
                    assert_eq!(
                        (sa.rot, sa.valid, sa.mates, sa.grounded),
                        (sb.rot, sb.valid, sb.mates, sb.grounded)
                    );
                }
                other => panic!("seed {seed}: {other:?}"),
            }
        }
    }
}

#[test]
fn placing_then_removing_restores_the_table() {
    for seed in 0..SEEDS {
        let mut m = build(seed, 25);
        let before = m.instances().clone();
        let mut rng = Rng(seed + 31);
        for _ in 0..10 {
            let ray = ray_towards(&mut rng, 120);
            if let Some(id) = place(&mut m, rng.pick(&PARTS), 0, &ray, 4) {
                m.apply(&Command::Remove { id }).unwrap();
                assert_eq!(m.instances(), &before, "seed {seed}");
            }
        }
        // Removing everything empties the indexes too.
        m.apply(&Command::Clear).unwrap();
        assert!(m.indexes_are_empty(), "seed {seed}");
    }
}

#[test]
fn undoing_any_sequence_restores_the_document() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed + 1000);
        let mut m = on_baseplate();
        let start = m.to_text("t", None);
        let mut applied = 0;
        for _ in 0..40 {
            let cmd = match rng.range(0, 5) {
                0..=2 => {
                    let ray = ray_towards(&mut rng, 100);
                    let part = *rng.pick(&PARTS);
                    let p = m.catalog().find(part).unwrap();
                    match m.snap(p, Rot24(*rng.pick(&yaws())), &ray).unwrap() {
                        Some(s) if s.valid => Command::Add {
                            part: part.into(),
                            pos: s.pos,
                            rot: s.rot,
                            colour: 15,
                        },
                        _ => continue,
                    }
                }
                3 => Command::Remove {
                    id: rng.range(1, 30) as u32,
                },
                4 => Command::Turn {
                    id: rng.range(1, 30) as u32,
                    quarter_turns: rng.range(1, 3),
                },
                _ => Command::Recolour {
                    id: rng.range(1, 30) as u32,
                    colour: *rng.pick(&[4u16, 14, 70]),
                },
            };
            if m.apply(&cmd).is_ok() {
                applied += 1;
            }
        }
        let end = m.to_text("t", None);
        for _ in 0..applied {
            m.undo().unwrap();
        }
        assert_eq!(m.to_text("t", None), start, "seed {seed}");
        for _ in 0..applied {
            m.redo().unwrap();
        }
        assert_eq!(m.to_text("t", None), end, "seed {seed}");
    }
}

#[test]
fn documents_round_trip_byte_for_byte() {
    for seed in 0..SEEDS {
        let m = build(seed, 30);
        let text = m.to_text("Round trip", None);
        let back =
            Model::from_text(m.catalog(), &text).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(back.instances(), m.instances(), "seed {seed}");
        assert_eq!(back.to_text("Round trip", None), text, "seed {seed}");
        assert_eq!(back.stats(), m.stats());
        assert_eq!(back.bom(), m.bom());
    }
}

/// The footprint centre of `part` at `pose` in X and Z (twice, to stay in integers).
fn centre2(m: &Model, part: openbrick_catalog::PartId, pose: &LatticePose) -> [i32; 2] {
    let b = m.body_box(part, pose).unwrap();
    [b.min[0] + b.max[0], b.min[2] + b.max[2]]
}

#[test]
fn over_a_free_baseplate_the_snap_is_centred_within_half_a_stud_of_the_hit() {
    // The baseplate is 320 LDU wide; hits at least 80 inside it leave every nearby
    // candidate on the plate, so the nearest lattice pose is always valid.
    for seed in 0..SEEDS {
        let mut rng = Rng(seed + 4242);
        let m = on_baseplate();
        for _ in 0..40 {
            let (x, z) = (rng.range(-80, 80), rng.range(-80, 80));
            let p = part(rng.pick(&PARTS));
            let rot = Rot24(*rng.pick(&yaws()));
            let s = m.snap(p, rot, &down(x, z)).unwrap().expect("a hit");
            assert!(s.valid, "seed {seed}: {s:?}");
            let c = centre2(&m, p, &s.pose());
            // Centre within 10 LDU (half a stud) of the hit point on each axis.
            assert!((c[0] - 2 * x).abs() <= 20 && (c[1] - 2 * z).abs() <= 20, "seed {seed}: part {p} at ({x},{z}) snapped to {s:?}, centre*2 {c:?}");
        }
    }
}

#[test]
fn no_valid_pose_at_the_same_height_is_nearer_than_the_snap() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed + 777);
        let m = build(seed, 25);
        for _ in 0..25 {
            let ray = ray_towards(&mut rng, 100);
            let Some(hit) = m.pick(&ray) else { continue };
            if hit.normal != [0, -1, 0] {
                continue; // top faces: the height is fixed, so only X and Z compete
            }
            let p = part(rng.pick(&PARTS));
            let rot = Rot24(*rng.pick(&yaws()));
            let Some(s) = m.snap(p, rot, &ray).unwrap() else { continue };
            if !s.valid {
                continue;
            }
            let dist = |c: [i32; 2]| {
                let dx = f64::from(c[0]) / 2.0 - hit.point[0];
                let dz = f64::from(c[1]) / 2.0 - hit.point[2];
                dx * dx + dz * dz
            };
            let best = dist(centre2(&m, p, &s.pose()));
            for i in -3..=3 {
                for j in -3..=3 {
                    let other = LatticePose {
                        pos: [s.pos[0] + 20 * i, s.pos[1], s.pos[2] + 20 * j],
                        rot,
                    };
                    let ok = m.check_pose(p, &other, None).is_ok() && m.mates(p, &other, None).unwrap() > 0;
                    if ok {
                        assert!(
                            best <= dist(centre2(&m, p, &other)) + 1e-9,
                            "seed {seed}: {s:?} is farther from the hit {:?} than {other:?}",
                            hit.point
                        );
                    }
                }
            }
        }
    }
}
