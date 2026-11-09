//! Properties of the indexes that make big models cheap (docs/plan/07 section 14): everything the
//! engine keeps incrementally (the spatial hash, the connector hash, the bill of materials, the
//! cached statistics, the change list, the table order) must equal what a computation from scratch
//! gives, after random edit sequences with undo and redo; and a ray walk must pick what testing
//! every part picks. Seeded random cases, like properties.rs: a failure names the seed.

mod common;

use common::*;
use openbrick_catalog::Aabb;
use openbrick_engine::{Command, DocError, InstanceId, Model, ModelDoc, MAX_INSTANCES};
use openbrick_units::Rot24;
use std::collections::BTreeSet;

const SEEDS: u64 = 16;
const PARTS: [&str; 8] = [
    "ob.brick-1x1",
    "ob.brick-2x4",
    "ob.plate-1x4",
    "ob.plate-2x2",
    "ob.tile-1x2",
    "ob.brick-1x2",
    "ob.plate-2x6",
    "ob.baseplate-16x16",
];

/// One random edit: an add by snapping, a removal, a move, a turn, a recolour, an undo, a redo, now
/// and then a clear. Returns false when the edit was refused (that is fine, it changed nothing).
fn random_edit(m: &mut Model<'static>, rng: &mut Rng) -> bool {
    let max_id = m.next_id().max(2) as i32;
    match rng.range(0, 11) {
        0..=4 => {
            let part = *rng.pick(&PARTS);
            let ray = ray_towards(rng, 160);
            place(
                m,
                part,
                *rng.pick(&yaws()),
                &ray,
                *rng.pick(&[2u16, 4, 14, 15]),
            )
            .is_some()
        }
        5 | 6 => m
            .apply(&Command::Remove {
                id: rng.range(1, max_id) as u32,
            })
            .is_ok(),
        7 => {
            let id = rng.range(1, max_id) as u32;
            m.instance(id).is_some_and(|i| {
                let pos = [
                    i.pose.pos[0] + 20 * rng.range(-3, 3),
                    i.pose.pos[1] - 8 * rng.range(0, 2),
                    i.pose.pos[2] + 20 * rng.range(-3, 3),
                ];
                m.apply(&Command::Move {
                    id,
                    pos,
                    rot: i.pose.rot.0,
                })
                .is_ok()
            })
        }
        8 => m
            .apply(&Command::Turn {
                id: rng.range(1, max_id) as u32,
                quarter_turns: rng.range(1, 3),
            })
            .is_ok(),
        9 => m
            .apply(&Command::Recolour {
                id: rng.range(1, max_id) as u32,
                colour: *rng.pick(&[4u16, 14, 70, 25]),
            })
            .is_ok(),
        10 => m.undo().is_ok(),
        _ => {
            if rng.range(0, 9) == 0 {
                m.apply(&Command::ClearBuilt).is_ok()
            } else {
                m.redo().is_ok()
            }
        }
    }
}

fn ids(m: &Model) -> Vec<InstanceId> {
    m.instances().iter().map(|i| i.id).collect()
}

#[test]
fn the_incremental_indexes_equal_a_computation_from_scratch() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed * 7919 + 5);
        let mut m = on_baseplate();
        for step in 0..120 {
            random_edit(&mut m, &mut rng);
            let at = format!("seed {seed} step {step}");
            // Spatial hash, connector hash and bill of materials against indexes built again.
            assert_eq!(m.index_snapshot(), m.index_snapshot_from_scratch(), "{at}");
            assert_eq!(
                m.bom_counts_from_scratch().len(),
                m.bom().len(),
                "{at}: bill of materials lines"
            );
            assert_eq!(
                m.bom().iter().map(|l| l.count as usize).sum::<usize>(),
                m.len(),
                "{at}: the bill of materials counts every part"
            );
            // Cached statistics against a fresh computation, twice (the second is a cache hit).
            let fresh = m.stats();
            assert_eq!(m.stats_cached(), &fresh, "{at}");
            assert_eq!(m.stats_cached(), &fresh, "{at} (cache hit)");
            // The table stays in placement order: ids ascend (adds take a new highest id and undo
            // puts a part back where it was).
            let order = ids(&m);
            assert!(order.windows(2).all(|w| w[0] < w[1]), "{at}: {order:?}");
            // The hash gives every overlapping part as a candidate, and collision finds the lowest id.
            for _ in 0..3 {
                let c = [
                    rng.range(-200, 200),
                    rng.range(-120, 0),
                    rng.range(-200, 200),
                ];
                let b = Aabb {
                    min: c,
                    max: [
                        c[0] + 20 * rng.range(1, 6),
                        c[1] + 8 * rng.range(1, 4),
                        c[2] + 20 * rng.range(1, 6),
                    ],
                };
                let overlapping: Vec<InstanceId> = m
                    .instances()
                    .iter()
                    .filter(|i| m.body_box(i.part, &i.pose).unwrap().overlaps(&b))
                    .map(|i| i.id)
                    .collect();
                let near = m.candidates_near(&b).unwrap();
                assert!(
                    overlapping.iter().all(|id| near.contains(id)),
                    "{at}: candidates {near:?} miss an overlapping part {overlapping:?}"
                );
                assert_eq!(
                    m.collision(&b, None).unwrap(),
                    overlapping.first().copied(),
                    "{at}"
                );
            }
        }
        // After removing everything the indexes are empty again.
        m.apply(&Command::Clear).ok();
        assert!(m.indexes_are_empty(), "seed {seed}");
    }
}

#[test]
fn a_ray_walk_picks_what_testing_every_part_picks() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed * 104_729 + 3);
        let mut m = on_baseplate();
        for _ in 0..80 {
            random_edit(&mut m, &mut rng);
        }
        for _ in 0..200 {
            let ray = match rng.range(0, 3) {
                0 => ray_towards(&mut rng, 200),
                // Along the ground, through the model, and at odd angles.
                1 => openbrick_engine::Ray {
                    origin: [
                        -1000.0,
                        f64::from(rng.range(-60, -1)),
                        f64::from(rng.range(-160, 160)),
                    ],
                    dir: [1.0, 0.0, 0.0],
                },
                2 => openbrick_engine::Ray {
                    origin: [
                        f64::from(rng.range(-300, 300)),
                        -800.0,
                        f64::from(rng.range(-300, 300)),
                    ],
                    dir: [
                        f64::from(rng.range(-9, 9)),
                        f64::from(rng.range(1, 9)),
                        f64::from(rng.range(-9, 9)),
                    ],
                },
                // Exactly on cell borders: the walk must not lose a box it only grazes.
                _ => openbrick_engine::Ray {
                    origin: [
                        f64::from(160 * rng.range(-2, 2)),
                        f64::from(-96 * rng.range(1, 2)),
                        -1000.0,
                    ],
                    dir: [0.0, 0.0, 1.0],
                },
            };
            assert_eq!(m.pick(&ray), m.pick_linear(&ray), "seed {seed}: {ray:?}");
        }
    }
}

#[test]
fn the_changes_since_the_last_read_are_exactly_what_differs() {
    for seed in 0..SEEDS {
        let mut rng = Rng(seed * 31 + 99);
        let mut m = on_baseplate();
        m.take_changes();
        let mut before: Vec<_> = m.instances().iter().collect();
        for step in 0..60 {
            let revision = m.revision();
            random_edit(&mut m, &mut rng);
            let after: Vec<_> = m.instances().iter().collect();
            let changes = m.take_changes();
            assert!(!changes.overflow);
            assert!(
                changes.ids.windows(2).all(|w| w[0] < w[1]),
                "seed {seed} step {step}: every id appears once, ascending: {:?}",
                changes.ids
            );
            if m.revision() == revision {
                assert!(
                    changes.ids.is_empty(),
                    "seed {seed} step {step}: no revision change, no changes"
                );
            }
            // Everything that differs between the two tables is in the change list.
            let touched: BTreeSet<InstanceId> = changes.ids.iter().copied().collect();
            let differs: BTreeSet<InstanceId> = before
                .iter()
                .filter(|b| !after.contains(b))
                .chain(after.iter().filter(|a| !before.contains(a)))
                .map(|i| i.id)
                .collect();
            assert!(
                differs.is_subset(&touched),
                "seed {seed} step {step}: {differs:?} vs {touched:?}"
            );
            before = after;
        }
    }
}

#[test]
fn the_streamed_text_is_the_document_text_and_the_loader_reads_it_back() {
    for seed in 0..4u64 {
        let mut rng = Rng(seed + 400);
        let mut m = on_baseplate();
        for _ in 0..80 {
            random_edit(&mut m, &mut rng);
        }
        let streamed = m.to_text("t", None);
        assert_eq!(streamed, m.to_doc("t", None).to_text(), "seed {seed}");
        // In steps of 7 parts, the loader gives the model the one-shot reader gives.
        let mut loader = Model::loader(catalog(), &streamed).unwrap();
        let total = loader.total();
        let mut done = 0;
        while done < total {
            let next = loader.step(7).unwrap();
            assert!(next > done);
            done = next;
        }
        let (loaded, notices) = loader.finish().unwrap();
        assert!(notices.is_empty());
        assert_eq!(loaded.to_text("t", None), streamed, "seed {seed}");
        assert_eq!(&loaded, &Model::from_text(catalog(), &streamed).unwrap());
        assert_eq!(loaded.stats(), m.stats());
    }
}

#[test]
fn a_document_over_the_part_limit_is_refused_before_any_part_is_read() {
    let mut doc = ModelDoc::parse(&on_baseplate().to_text("t", None)).unwrap();
    let one = doc.submodels[0].instances[0].clone();
    doc.submodels[0].instances = vec![one; MAX_INSTANCES + 1];
    let t = std::time::Instant::now();
    let Err(e) = Model::from_doc(catalog(), &doc) else {
        panic!("{} parts must be refused", MAX_INSTANCES + 1);
    };
    assert!(
        matches!(e, DocError::TooManyInstances { count, limit } if count == MAX_INSTANCES + 1 && limit == MAX_INSTANCES),
        "{e}"
    );
    assert!(
        e.to_string().contains("150000") || e.to_string().contains("150,000"),
        "{e}"
    );
    assert!(t.elapsed().as_millis() < 2000, "refused quickly");
}

#[test]
fn a_model_at_the_limit_opens_and_one_more_part_is_refused() {
    // A baseplate floor with 1x1 plates side by side, one part per 20 LDU; 150,000 of them.
    let mut doc = ModelDoc::parse(&on_baseplate().to_text("t", None)).unwrap();
    let plate = |id: u32, x: i32, z: i32| {
        let mut i = doc.submodels[0].instances[0].clone();
        i.id = id;
        i.part = "ob.plate-1x1".into();
        i.pose.pos = [x, -8, z];
        i.colour = 4;
        i
    };
    let side = 400; // plates per row
    let instances: Vec<_> = (0..MAX_INSTANCES as u32)
        .map(|n| {
            plate(
                n + 1,
                -3990 + 20 * (n as i32 % side),
                -3990 + 20 * (n as i32 / side),
            )
        })
        .collect();
    doc.submodels[0].instances = instances;
    let t = std::time::Instant::now();
    let mut m = Model::from_doc(catalog(), &doc).expect("the limit itself is allowed");
    assert_eq!(m.len(), MAX_INSTANCES);
    let seconds = t.elapsed().as_secs_f64();
    let e = m
        .apply(&Command::Add {
            part: "ob.plate-1x1".into(),
            pos: [20_010, -8, 10],
            rot: 0,
            colour: 4,
        })
        .unwrap_err();
    assert!(e.to_string().contains("150000"), "{e}");
    // Open time of 150,000 parts (debug build: generous), and the model works.
    assert!(seconds < 120.0, "{seconds} s");
    assert!(m.stats_cached().parts == MAX_INSTANCES);
    let _ = Rot24::IDENTITY;
}

/// Rays aimed exactly at the corners and edges of hash cells (80 x 48 x 80 LDU), started on the ray's line
/// at integer distances, inside boxes, on a cell border and along the axes at boundaries: the walk of the
/// spatial hash must equal testing every part, tie-breaks included, however the ray meets a cell's corner.
#[test]
fn the_ray_walk_equals_testing_every_part_through_cell_corners_edges_and_borders() {
    use openbrick_engine::Ray;
    for seed in 0..SEEDS {
        let mut rng = Rng(seed * 6_700_417 + 11);
        let mut m = on_baseplate();
        for _ in 0..120 {
            random_edit(&mut m, &mut rng);
        }
        let mut checked = 0;
        for k in 0..1500 {
            let corner = [
                80 * rng.range(-3, 3),
                -48 * rng.range(0, 2),
                80 * rng.range(-3, 3),
            ];
            // Sometimes only an edge: one coordinate off the grid.
            let aim = match k % 3 {
                0 => corner,
                1 => [corner[0], corner[1] - rng.range(0, 47), corner[2]],
                _ => [corner[0] + rng.range(0, 79), corner[1], corner[2]],
            };
            let dir = [rng.range(-9, 9), rng.range(-9, 9), rng.range(-9, 9)];
            if dir == [0, 0, 0] {
                continue;
            }
            let back = rng.range(0, 40); // 0: starts on the point itself
            let origin = [
                aim[0] - dir[0] * back,
                aim[1] - dir[1] * back,
                aim[2] - dir[2] * back,
            ];
            let ray = Ray {
                origin: origin.map(f64::from),
                dir: dir.map(f64::from),
            };
            assert_eq!(m.pick(&ray), m.pick_linear(&ray), "seed {seed}: {ray:?}");
            checked += 1;
        }
        assert!(checked > 1000);
        // Along an axis, on the plane of a cell border and on the line of a cell edge.
        for _ in 0..400 {
            let on = [
                80 * rng.range(-3, 3),
                -48 * rng.range(0, 2),
                80 * rng.range(-3, 3),
            ];
            let axis = rng.range(0, 2) as usize;
            let mut origin = on.map(f64::from);
            origin[axis] = -1500.0;
            let mut dir = [0.0; 3];
            dir[axis] = 1.0;
            let ray = Ray { origin, dir };
            assert_eq!(m.pick(&ray), m.pick_linear(&ray), "seed {seed}: {ray:?}");
        }
    }
}

/// Bigger worlds: far from the origin (the hash cells far out), spread wide, with parts of a pack whose
/// connectors and boxes differ from the built-ins'. The indexes still equal ones built again.
#[test]
fn the_indexes_hold_in_a_big_world_far_from_the_origin() {
    for seed in 0..6u64 {
        let mut rng = Rng(seed * 977 + 1);
        let mut m = Model::new(catalog());
        let far = [
            rng.range(-90_000_000, 90_000_000),
            0,
            rng.range(-90_000_000, 90_000_000),
        ];
        let far = [
            far[0] - far[0].rem_euclid(80),
            0,
            far[2] - far[2].rem_euclid(80),
        ];
        // Baseplates on the ground far away, bricks and plates stacked and moved about on and beside them.
        for b in 0..6 {
            let _ = m.apply(&Command::Add {
                part: "ob.baseplate-16x16".into(),
                pos: [far[0] + 320 * (b % 3), -8, far[2] + 320 * (b / 3)],
                rot: 0,
                colour: 2,
            });
        }
        for _ in 0..150 {
            let part = *rng.pick(&PARTS);
            let ray = openbrick_engine::Ray {
                origin: [
                    f64::from(far[0] + rng.range(-100, 900)),
                    -600.0,
                    f64::from(far[2] + rng.range(-100, 600)),
                ],
                dir: [0.1, 1.0, 0.05],
            };
            place(&mut m, part, *rng.pick(&yaws()), &ray, 4);
            random_edit_far(&mut m, &mut rng);
        }
        assert_eq!(
            m.index_snapshot(),
            m.index_snapshot_from_scratch(),
            "seed {seed}"
        );
        let fresh = m.stats();
        assert_eq!(m.stats_cached(), &fresh, "seed {seed}");
        for _ in 0..300 {
            let ray = openbrick_engine::Ray {
                origin: [
                    f64::from(far[0] + rng.range(-200, 1000)),
                    -800.0,
                    f64::from(far[2] + rng.range(-200, 700)),
                ],
                dir: [
                    f64::from(rng.range(-3, 3)),
                    1.0,
                    f64::from(rng.range(-3, 3)),
                ],
            };
            assert_eq!(m.pick(&ray), m.pick_linear(&ray), "seed {seed}: {ray:?}");
        }
        let _ = m.apply(&Command::Clear);
        assert!(m.indexes_are_empty(), "seed {seed}");
    }
}

fn random_edit_far(m: &mut Model<'static>, rng: &mut Rng) {
    let max_id = m.next_id().max(2) as i32;
    let _ = match rng.range(0, 3) {
        0 => m.apply(&Command::Remove {
            id: rng.range(1, max_id) as u32,
        }),
        1 => m.apply(&Command::Recolour {
            id: rng.range(1, max_id) as u32,
            colour: 14,
        }),
        2 => m.undo().map(|_| None),
        _ => m.redo().map(|_| None),
    };
}
