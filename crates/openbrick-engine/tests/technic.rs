//! Technic parts in the assembler: pins and axles in holes (occupancy masks as the narrowphase,
//! snapping by connector pairs, the connection graph, undo and the indexes), on the synthetic
//! mini Technic library of the catalog's tests (`tests/fixtures/technic`, written for these
//! tests, CC0). The same checks run on the real LDraw library when `OPENBRICK_LDRAW_DIR` names
//! one (`tests/technic_real.rs`).

mod common;

use std::path::Path;
use std::sync::OnceLock;

use common::*;
use openbrick_catalog::ldraw::{import, ImportOptions};
use openbrick_catalog::{Catalog, Link, Mask, Pack};
use openbrick_engine::{Command, EngineError, Model, Ray};
use openbrick_units::{LatticePose, Rot24};

pub fn technic_pack() -> Pack {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../openbrick-catalog/tests/fixtures/technic");
    let opts = ImportOptions {
        categories: Some(["Brick", "Technic"].map(String::from).to_vec()),
        ..ImportOptions::default()
    };
    let (p, _) = import(&dir, &opts, &mut |_, _, _| {}).expect("fixture imports");
    Pack::from_bytes(&p.to_bytes().unwrap()).unwrap()
}

pub fn cat() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| Catalog::builtin().unwrap().with_pack(&technic_pack()))
}

fn add(part: &str, pos: [i32; 3], rot: u8) -> Command {
    Command::Add {
        part: part.into(),
        pos,
        rot,
        colour: 15,
    }
}

fn id(name: &str) -> u32 {
    cat().find(name).unwrap_or_else(|| panic!("part {name}"))
}

/// A ray straight at a point from the +X side.
fn from_plus_x(y: i32, z: i32) -> Ray {
    Ray {
        origin: [500.0, f64::from(y), f64::from(z)],
        dir: [-1.0, 0.0, 0.0],
    }
}

#[test]
fn the_mini_pack_has_masks_for_the_technic_parts_and_none_for_the_plain_brick() {
    let c = cat();
    for (n, mask) in [
        ("ld.3701", true),
        ("ld.3704", true),
        ("ld.3673", true),
        ("ld.3705", true),
        ("ld.3005", false),
    ] {
        let p = &c.parts[id(n) as usize];
        assert_eq!(p.shape.mask.is_some(), mask, "{n}");
    }
}

#[test]
fn a_pin_enters_the_hole_of_a_beam_that_its_box_would_forbid() {
    let c = cat();
    let mut m = Model::new(c);
    m.apply(&add("ld.3701", [0, -24, 0], 0)).unwrap();
    // The pin's connectors are at plus and minus 10 from its origin along X; origin at x = 10
    // puts one in the hole at the centre of the middle cell (0, -12, 0).
    let pin = id("ld.3673");
    let pose = LatticePose {
        pos: [10, -12, 0],
        rot: Rot24::IDENTITY,
    };
    // The boxes of the two overlap.
    let beam_body = m
        .body_box(
            id("ld.3701"),
            &LatticePose {
                pos: [0, -24, 0],
                rot: Rot24::IDENTITY,
            },
        )
        .unwrap();
    let pin_body = m.body_box(pin, &pose).unwrap();
    assert!(beam_body.overlaps(&pin_body));
    m.check_pose(pin, &pose, None)
        .expect("through the hole: no shared volume");
    m.apply(&add("ld.3673", pose.pos, 0)).unwrap();
    let s = m.stats();
    assert_eq!(
        (s.parts, s.connections, s.floating.len()),
        (2, 1, 0),
        "{s:?}"
    );
    // The same pin 6 LDU sideways cuts the wall of the hole.
    let off = LatticePose {
        pos: [10, -12, 6],
        rot: Rot24::IDENTITY,
    };
    let e = Model::new(c);
    let mut e = e;
    e.apply(&add("ld.3701", [0, -24, 0], 0)).unwrap();
    assert!(matches!(
        e.check_pose(pin, &off, None),
        Err(EngineError::Collides { .. })
    ));
}

#[test]
fn snapping_a_pin_to_a_hole_lies_it_down_and_says_a_pin_holds_it() {
    let c = cat();
    let mut m = Model::new(c);
    m.apply(&add("ld.3701", [0, -24, 0], 0)).unwrap();
    let pin = id("ld.3673");
    // A pointer ray at the middle hole of the +X face of the beam; the pin as it comes (standing up
    // on the page's default turn: its axis along X in its own frame, so the turn is identity).
    let s = m
        .snap(pin, Rot24::IDENTITY, &from_plus_x(-12, 0))
        .unwrap()
        .unwrap();
    assert!(s.valid && s.mates >= 1, "{s:?}");
    assert_eq!(s.links, Link::Pin.bit(), "{s:?}");
    assert_eq!((s.pos, s.rot), ([10, -12, 0], 0), "{s:?}");
    // Turned a quarter about Z so its axis points up, the snap turns it back onto the hole's axis.
    let up = Rot24::from_matrix([[0, -1, 0], [1, 0, 0], [0, 0, 1]]).unwrap();
    let t = m.snap(pin, up, &from_plus_x(-12, 0)).unwrap().unwrap();
    assert!(t.valid && t.links == Link::Pin.bit(), "{t:?}");
    let placed = LatticePose {
        pos: t.pos,
        rot: Rot24(t.rot),
    };
    assert!(m.mates_links(pin, &placed, None).unwrap().0 >= 1);
    m.apply(&add("ld.3673", t.pos, t.rot)).unwrap();
}

#[test]
fn an_axle_fits_the_cross_hole_in_four_quarter_turns_and_a_round_pin_does_not() {
    let c = cat();
    let mut m = Model::new(c);
    m.apply(&add("ld.3704", [0, -24, 0], 0)).unwrap();
    let axle = id("ld.3705");
    for k in 0..4 {
        // A quarter turn of the axle about its own axis (X).
        let roll = Rot24::from_matrix([[1, 0, 0], [0, 0, -1], [0, 1, 0]]).unwrap();
        let rot = (0..k).fold(Rot24::IDENTITY, |r, _| roll.compose(r));
        let pose = LatticePose {
            pos: [10, -12, 0],
            rot,
        };
        m.check_pose(axle, &pose, None)
            .unwrap_or_else(|e| panic!("roll {k}: {e}"));
        assert_eq!(
            m.mates_links(axle, &pose, None).unwrap(),
            (1, Link::Axle.bit()),
            "roll {k}"
        );
    }
    // The round pin has the same reach but a rounder, bigger shaft: it cuts the cross hole's wall.
    let pin = id("ld.3673");
    let pose = LatticePose {
        pos: [10, -12, 0],
        rot: Rot24::IDENTITY,
    };
    assert!(matches!(
        m.check_pose(pin, &pose, None),
        Err(EngineError::Collides { .. })
    ));
    // And it does not mate: a pin is not an axle.
    assert_eq!(m.mates(pin, &pose, None).unwrap(), 0);
    // The snap for the axle says "axle".
    let s = m
        .snap(axle, Rot24::IDENTITY, &from_plus_x(-12, 0))
        .unwrap()
        .unwrap();
    assert!(s.valid && s.links == Link::Axle.bit(), "{s:?}");
}

#[test]
fn a_plain_brick_stacks_on_a_technic_brick_by_its_studs() {
    let c = cat();
    let mut m = Model::new(c);
    // A 1-wide part's studs are at x = 0 in its own frame; put them on the stud grid (x = 10).
    m.apply(&add("ld.3701", [10, -24, 0], 0)).unwrap();
    let brick = id("ld.3005");
    let s = m
        .snap(brick, Rot24::IDENTITY, &down(10, 10))
        .unwrap()
        .unwrap();
    assert!(
        s.valid && s.links == Link::Stud.bit() && s.mates == 1,
        "{s:?}"
    );
    assert_eq!(s.pos[1], -48);
    m.apply(&add("ld.3005", s.pos, s.rot)).unwrap();
}

/// The solids of every pair of placed parts, from scratch with the masks: none shares volume.
fn assert_no_shared_volume(m: &Model<'_>, at: &str) {
    let c = m.catalog();
    let all: Vec<_> = m.instances().iter().collect();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            let (pa, pb) = (
                &c.parts[a.part as usize].shape,
                &c.parts[b.part as usize].shape,
            );
            let (ba, bb) = (
                pa.body.transformed(a.pose.rot, a.pose.pos),
                pb.body.transformed(b.pose.rot, b.pose.pos),
            );
            if !ba.overlaps(&bb) {
                continue;
            }
            let region = openbrick_catalog::Aabb {
                min: [0, 1, 2].map(|k| ba.min[k].max(bb.min[k])),
                max: [0, 1, 2].map(|k| ba.max[k].min(bb.max[k])),
            };
            assert!(
                !Mask::solids_overlap(
                    pa.mask.as_deref(),
                    &pa.body,
                    (a.pose.rot, a.pose.pos),
                    pb.mask.as_deref(),
                    &pb.body,
                    (b.pose.rot, b.pose.pos),
                    &region
                ),
                "{at}: parts {} and {} share volume",
                a.id,
                b.id
            );
        }
    }
}

/// Technic and plain parts through random edits with undo and redo: the incremental indexes equal
/// ones built again, the cached statistics equal fresh ones, no two solids share volume, a snap is
/// deterministic and the same after moving everything by a lattice step, and undoing everything
/// restores the model text.
#[test]
fn technic_parts_keep_every_index_equal_to_a_computation_from_scratch() {
    let c = cat();
    let names: Vec<String> = c
        .parts
        .iter()
        .filter(|p| p.origin == openbrick_catalog::Origin::Ldraw)
        .map(|p| p.id.clone())
        .collect();
    let mut pins_placed = 0;
    for seed in 0..10u64 {
        let mut rng = Rng(seed * 7_919 + 5);
        let mut m = Model::new(c);
        let empty = m.to_text("t", None);
        m.apply(&add("ob.baseplate-16x16", [0, -8, 0], 0)).unwrap();
        let mut edits = 0;
        for step in 0..120 {
            let part = rng.pick(&names).clone();
            let ray = ray_towards(&mut rng, 100);
            match rng.range(0, 6) {
                0..=3 => {
                    let p = id(&part);
                    let rot = Rot24(rng.range(0, 23) as u8);
                    if let Some(s) = m.snap(p, rot, &ray).unwrap() {
                        if s.valid {
                            // Snapping is deterministic.
                            assert_eq!(Some(s), m.snap(p, rot, &ray).unwrap());
                            m.apply(&add(&part, s.pos, s.rot))
                                .unwrap_or_else(|e| panic!("{part} {s:?}: {e}"));
                            edits += 1;
                            if s.links & !Link::Stud.bit() != 0 {
                                pins_placed += 1;
                            }
                        }
                    }
                }
                4 => {
                    let _ = m.apply(&Command::Remove {
                        id: rng.range(1, m.next_id().max(2) as i32) as u32,
                    });
                }
                5 => {
                    let _ = m.undo();
                }
                _ => {
                    let _ = m.redo();
                }
            }
            let at = format!("seed {seed} step {step}");
            assert_eq!(m.index_snapshot(), m.index_snapshot_from_scratch(), "{at}");
            let fresh = m.stats();
            assert_eq!(m.stats_cached(), &fresh, "{at}");
            if step % 10 == 0 {
                assert_no_shared_volume(&m, &at);
            }
        }
        assert_no_shared_volume(&m, &format!("seed {seed} end"));
        let _ = edits;
        // Undo to the start restores the text (ids are never reused, the text is the same).
        while m.undo().is_ok() {}
        assert_eq!(m.to_text("t", None), empty, "seed {seed}");
        assert!(m.indexes_are_empty());
    }
    assert!(
        pins_placed > 0,
        "the random edits never made a pin or axle connection"
    );
}

#[test]
fn snapping_a_pin_is_the_same_after_moving_the_beam_by_a_lattice_step() {
    let c = cat();
    let pin = id("ld.3673");
    let at = |d: [i32; 3]| {
        let mut m = Model::new(c);
        m.apply(&add("ld.3701", [d[0], -24 + d[1], d[2]], 0))
            .unwrap();
        let ray = Ray {
            origin: [
                500.0 + f64::from(d[0]),
                -12.0 + f64::from(d[1]) + 3.0,
                f64::from(d[2]) + 4.0,
            ],
            dir: [-1.0, 0.0, 0.0],
        };
        m.snap(pin, Rot24::IDENTITY, &ray).unwrap().unwrap()
    };
    let base = at([0, 0, 0]);
    for d in [[20, 0, 0], [0, -8, 0], [0, 0, -40], [60, -24, 100]] {
        let s = at(d);
        assert_eq!(
            (s.pos, s.rot, s.valid, s.mates, s.links),
            (
                [base.pos[0] + d[0], base.pos[1] + d[1], base.pos[2] + d[2]],
                base.rot,
                base.valid,
                base.mates,
                base.links
            ),
            "shift {d:?}"
        );
    }
}
