//! Engine behaviour: snapping on every face, collision, the connection graph,
//! commands with undo and redo.

mod common;

use common::*;
use openbrick_engine::{Command, EngineError, Model, Ray};
use openbrick_units::Rot24;

#[test]
fn a_brick_snaps_onto_the_baseplate_under_the_cursor() {
    let m = on_baseplate();
    let s = m
        .snap(part("ob.brick-2x4"), Rot24::IDENTITY, &down(3, 47))
        .unwrap()
        .unwrap();
    assert!(s.valid && !s.grounded, "{s:?}");
    assert_eq!(s.mates, 8, "all eight receptacles on studs");
    // Its bottom on the baseplate's top (Y = -8), 24 LDU tall.
    assert_eq!(s.pos[1], -8 - 24);
    // A 4 x 2 footprint centred on the stud lattice nearest the cursor.
    assert_eq!((s.pos[0], s.pos[2]), (0, 40));
    assert_eq!(s.target, Some(1));
}

#[test]
fn snapping_onto_a_brick_stacks_it() {
    let mut m = on_baseplate();
    let a = place(&mut m, "ob.brick-2x2", 0, &down(0, 0), 4).unwrap();
    let s = m
        .snap(part("ob.plate-1x1"), Rot24::IDENTITY, &down(5, 5))
        .unwrap()
        .unwrap();
    assert_eq!(s.target, Some(a));
    assert_eq!((s.pos, s.mates, s.valid), ([10, -8 - 24 - 8, 10], 1, true));
}

#[test]
fn a_part_goes_under_an_overhang_from_below() {
    // No baseplate: a 2x2 on the ground and a 2x4 across it, half overhanging.
    let mut m = Model::new(catalog());
    for (p, pos) in [
        ("ob.brick-2x2", [0, -24, 0]),
        ("ob.brick-2x4", [20, -48, 0]),
    ] {
        m.apply(&Command::Add {
            part: p.into(),
            pos,
            rot: 0,
            colour: 15,
        })
        .unwrap();
    }
    // Looking up at the overhang's bottom face from just above the ground.
    let up = Ray {
        origin: [50.0, -5.0, 10.0],
        dir: [0.0, -1.0, 0.0],
    };
    let hit = m.pick(&up).unwrap();
    assert_eq!((hit.instance, hit.normal), (Some(2), [0, 1, 0]));
    let s = m
        .snap(part("ob.plate-1x1"), Rot24::IDENTITY, &up)
        .unwrap()
        .unwrap();
    assert!(s.valid, "{s:?}");
    assert_eq!(s.pos, [50, -24, 10], "its top against the 2x4's bottom");
    assert_eq!(s.mates, 1);
    let id = m
        .apply(&Command::Add {
            part: "ob.plate-1x1".into(),
            pos: s.pos,
            rot: 0,
            colour: 4,
        })
        .unwrap()
        .unwrap();
    assert!(m.stats().floating.is_empty(), "held by the 2x4");
    assert_eq!(id, 3);
}

#[test]
fn beside_a_part_the_snap_lands_on_the_studs_next_to_it() {
    let mut m = on_baseplate();
    place(&mut m, "ob.brick-2x2", 0, &down(0, 0), 4).unwrap();
    // A ray from the side into the brick's +X face.
    let side = Ray {
        origin: [200.0, -20.0, 0.0],
        dir: [-1.0, 0.0, 0.0],
    };
    let hit = m.pick(&side).unwrap();
    assert_eq!(hit.normal, [1, 0, 0]);
    let s = m
        .snap(part("ob.brick-2x2"), Rot24::IDENTITY, &side)
        .unwrap()
        .unwrap();
    assert!(s.valid, "{s:?}");
    assert_eq!(s.pos, [40, -32, 0], "next to it, on the baseplate");
}

#[test]
fn on_the_bare_ground_parts_stand_on_it() {
    let m = Model::new(catalog());
    let s = m
        .snap(
            part("ob.brick-1x2"),
            Rot24::yaw_quarter_turns(1),
            &down(0, 0),
        )
        .unwrap()
        .unwrap();
    assert!(s.valid && s.grounded && s.mates == 0, "{s:?}");
    assert_eq!(s.pos[1], -24);
    let ray_up = Ray {
        origin: [0.0, -10.0, 0.0],
        dir: [0.0, -1.0, 0.0],
    };
    assert!(m
        .snap(part("ob.brick-1x1"), Rot24::IDENTITY, &ray_up)
        .unwrap()
        .is_none());
}

#[test]
fn overlapping_and_off_grid_and_underground_placements_are_refused() {
    let mut m = on_baseplate();
    let add = |pos: [i32; 3]| Command::Add {
        part: "ob.brick-2x2".into(),
        pos,
        rot: 0,
        colour: 4,
    };
    m.apply(&add([0, -32, 0])).unwrap();
    let e = m.apply(&add([20, -32, 0])).unwrap_err();
    assert!(matches!(e, EngineError::Collides { with: 2, .. }), "{e}");
    assert!(e.to_string().contains("only touch"), "a fix hint: {e}");
    // Touching on a face is fine.
    m.apply(&add([40, -32, 0])).unwrap();
    assert!(matches!(
        m.apply(&add([5, -56, 0])),
        Err(EngineError::OffGrid { .. })
    ));
    assert!(matches!(
        m.apply(&add([0, -4, 200])),
        Err(EngineError::OffGrid { .. })
    ));
    assert!(matches!(
        m.apply(&add([200, 8, 200])),
        Err(EngineError::BelowGround { .. })
    ));
    assert!(matches!(
        m.apply(&Command::Add {
            part: "ob.brick-9x9".into(),
            pos: [0, -56, 0],
            rot: 0,
            colour: 4
        }),
        Err(EngineError::UnknownPart(_))
    ));
    assert!(matches!(
        m.apply(&Command::Recolour {
            id: 2,
            colour: 9999
        }),
        Err(EngineError::UnknownColour(9999))
    ));
    assert_eq!(m.len(), 3, "refused edits change nothing");
}

#[test]
fn removing_a_supporting_part_leaves_the_parts_above_floating() {
    let mut m = on_baseplate();
    let a = place(&mut m, "ob.brick-2x2", 0, &down(0, 0), 4).unwrap();
    let b = place(&mut m, "ob.brick-2x2", 0, &down(0, 0), 15).unwrap();
    let c = place(&mut m, "ob.plate-2x2", 0, &down(0, 0), 14).unwrap();
    let s = m.stats();
    assert_eq!((s.parts, s.components, s.grounded), (4, 1, 1));
    assert_eq!(s.connections, 4 + 4 + 4, "each 2x2 sits on four studs");
    assert!(s.floating.is_empty());
    m.apply(&Command::Remove { id: a }).unwrap();
    let s = m.stats();
    assert_eq!(s.components, 2);
    assert_eq!(s.floating, vec![b, c]);
    m.undo().unwrap();
    assert!(m.stats().floating.is_empty());
}

#[test]
fn commands_undo_and_redo() {
    let mut m = on_baseplate();
    let start = m.instances().clone();
    let id = place(&mut m, "ob.brick-1x2", 0, &down(0, 0), 4).unwrap();
    let placed = m.instances().clone();
    m.apply(&Command::Recolour { id, colour: 14 }).unwrap();
    m.apply(&Command::Turn {
        id,
        quarter_turns: 1,
    })
    .unwrap();
    let turned = m.instance(id).unwrap();
    assert_eq!(turned.pose.rot, Rot24::yaw_quarter_turns(1));
    assert_eq!(turned.colour, 14);
    // Turned in place, its studs are back on the grid.
    assert_eq!(m.mates(turned.part, &turned.pose, Some(id)).unwrap(), 2);
    m.apply(&Command::Move {
        id,
        pos: [100, -32, 110],
        rot: 0,
    })
    .unwrap();
    m.apply(&Command::Clear).unwrap();
    assert!(m.is_empty() && m.indexes_are_empty());
    let after = m.instances().clone();
    for _ in 0..4 {
        m.undo().unwrap();
    }
    assert_eq!(m.instances(), &placed);
    m.undo().unwrap();
    assert_eq!(m.instances(), &start);
    for _ in 0..5 {
        m.redo().unwrap();
    }
    assert_eq!(m.instances(), &after);
    assert!(matches!(m.redo(), Err(EngineError::NothingToRedo)));
    // A new command drops the redo list.
    m.undo().unwrap();
    m.apply(&Command::Remove { id: 1 }).unwrap();
    assert!(!m.history().can_redo());
}

#[test]
fn ids_are_not_reused_after_an_undo() {
    let mut m = on_baseplate();
    let a = place(&mut m, "ob.plate-1x1", 0, &down(0, 0), 4).unwrap();
    m.undo().unwrap();
    let b = place(&mut m, "ob.plate-1x1", 0, &down(0, 0), 4).unwrap();
    assert_ne!(
        a, b,
        "a file saved before the undo never meets a new part with its id"
    );
}

#[test]
fn clear_built_keeps_the_baseplate_and_undoes_in_one_step() {
    let mut m = on_baseplate();
    place(&mut m, "ob.brick-2x2", 0, &down(0, 0), 4).unwrap();
    place(&mut m, "ob.plate-1x2", 0, &down(0, 0), 15).unwrap();
    let before = m.instances().clone();
    m.apply(&Command::ClearBuilt).unwrap();
    assert_eq!(m.len(), 1);
    assert!(m.stats().floating.is_empty());
    m.undo().unwrap();
    assert_eq!(m.instances(), &before);
    // Nothing to clear is still a command, and harmless.
    let mut empty = on_baseplate();
    empty.apply(&Command::ClearBuilt).unwrap();
    assert_eq!(empty.len(), 1);
}

#[test]
fn clear_built_leaves_no_baseplate_in_mid_air() {
    // A second baseplate laid on top of bricks is a placed part like any other: Clear
    // removes it with the bricks under it, and keeps the one on the ground.
    let mut m = on_baseplate();
    let a = place(&mut m, "ob.brick-2x2", 0, &down(0, 0), 4).unwrap();
    let base = m.catalog().find("ob.baseplate-16x16").unwrap();
    let top = m.snap(base, Rot24::IDENTITY, &down(0, 0)).unwrap().unwrap();
    assert!(top.valid && top.pos[1] < -8, "{top:?}");
    m.apply(&Command::Add {
        part: "ob.baseplate-16x16".into(),
        pos: top.pos,
        rot: top.rot,
        colour: 2,
    })
    .unwrap();
    assert_eq!(m.len(), 3);
    m.apply(&Command::ClearBuilt).unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(
        m.instance(1).unwrap().pose.pos,
        [0, -8, 0],
        "the ground baseplate stays"
    );
    assert!(m.stats().floating.is_empty());
    m.undo().unwrap();
    assert_eq!(m.len(), 3);
    // Any baseplate can be removed, the ground one included: what stood on it is
    // reported as floating, and undo brings everything back.
    m.apply(&Command::Remove { id: 1 }).unwrap();
    assert_eq!(m.stats().floating, vec![a, 3]);
    m.undo().unwrap();
    assert!(m.stats().floating.is_empty());
}
