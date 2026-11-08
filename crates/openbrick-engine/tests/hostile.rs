//! Hostile input: poses, ids and rays a file or a page could send. Nothing may panic
//! (the test profile has overflow checks, so an unchecked add fails here), and what is
//! out of bounds is refused with a message that says what to change.

mod common;

use common::*;
use openbrick_engine::{Command, DocError, EngineError, Model, Ray, MAX_COORD};
use openbrick_units::Rot24;

fn doc(instances: &str) -> String {
    format!(
        r#"{{"openbrick":"1.0","kind":"model","name":"x","units":"ldu","frame":"ldraw-y-down",
        "catalog":"openbrick-builtin@1","palette":[],"bom":[],
        "submodels":[{{"id":"main","instances":[{instances}]}}]}}"#
    )
}

fn brick(id: u64, pos: [i64; 3]) -> String {
    format!(
        r#"{{"id":{id},"part":"ob.brick-1x1","pose":{{"pos":[{},{},{}],"rot":0}},"colour":4}}"#,
        pos[0], pos[1], pos[2]
    )
}

#[test]
fn id_zero_is_refused_when_reading() {
    let e = Model::from_text(catalog(), &doc(&brick(0, [10, -24, 10])))
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(
            e,
            DocError::Instance {
                id: 0,
                error: EngineError::ZeroId
            }
        ),
        "{e}"
    );
    assert!(e.to_string().contains("start at 1"), "{e}");
}

#[test]
fn the_largest_id_loads_and_then_adding_says_the_ids_are_used_up() {
    let mut m =
        Model::from_text(catalog(), &doc(&brick(4_294_967_295, [10, -24, 10]))).expect("loads");
    assert_eq!(m.stats().parts, 1);
    let e = m
        .apply(&Command::Add {
            part: "ob.brick-1x1".into(),
            pos: [30, -24, 10],
            rot: 0,
            colour: 4,
        })
        .unwrap_err();
    assert!(matches!(e, EngineError::IdsExhausted(_)), "{e}");
    assert_eq!(m.len(), 1);
    // Saving and loading it again still works.
    let text = m.to_text("x", None);
    assert!(Model::from_text(catalog(), &text).is_ok());
}

#[test]
fn coordinates_beyond_the_schema_bound_are_refused() {
    for pos in [
        [10, i64::from(i32::MIN), 10],
        [i64::from(i32::MAX) - 10 + 10 - 10, -24, 10],
        [10, -24, 100_000_010],
        [-100_000_010, -24, 10],
    ] {
        let e = Model::from_text(catalog(), &doc(&brick(1, pos)))
            .map(|_| ())
            .unwrap_err();
        assert!(
            matches!(
                &e,
                DocError::Instance {
                    error: EngineError::OutOfRange { .. },
                    ..
                } | DocError::Instance {
                    error: EngineError::OffGrid { .. },
                    ..
                }
            ),
            "{pos:?}: {e}"
        );
    }
    // Exactly at the bound on the stud grid is accepted (x = 10 + 20 k).
    let edge = MAX_COORD - MAX_COORD.rem_euclid(20) + 10 - 20; // 99,999,990
    assert!(Model::from_text(catalog(), &doc(&brick(1, [i64::from(edge), -24, 10]))).is_ok());
    let m = on_baseplate();
    let mut m = m;
    for cmd in [
        Command::Add {
            part: "ob.brick-1x1".into(),
            pos: [i32::MIN, i32::MIN, i32::MIN],
            rot: 0,
            colour: 4,
        },
        Command::Add {
            part: "ob.brick-1x1".into(),
            pos: [i32::MAX, -24, 10],
            rot: 0,
            colour: 4,
        },
        Command::Move {
            id: 1,
            pos: [i32::MAX, i32::MIN, i32::MAX],
            rot: 3,
        },
    ] {
        let e = m.apply(&cmd).unwrap_err();
        assert!(matches!(e, EngineError::OutOfRange { .. }), "{cmd:?}: {e}");
    }
    assert_eq!(m.len(), 1);
}

#[test]
fn absurd_rays_hit_nothing_and_do_not_panic() {
    let m = on_baseplate();
    let p = part("ob.brick-1x1");
    let rays = [
        Ray {
            origin: [1e12, -1e12, 1e12],
            dir: [0.0, 1.0, 0.0],
        },
        Ray {
            origin: [1e300, -1.0, 1e300],
            dir: [0.0, 1.0, 0.0],
        },
        Ray {
            origin: [f64::NAN, -100.0, 0.0],
            dir: [0.0, 1.0, 0.0],
        },
        Ray {
            origin: [0.0, -100.0, 0.0],
            dir: [f64::INFINITY, 1.0, 0.0],
        },
        Ray {
            origin: [0.0, -100.0, 0.0],
            dir: [1e-300, 1e-300, 1e-300],
        },
        Ray {
            origin: [0.0, -100.0, 0.0],
            dir: [0.0, 0.0, 0.0],
        },
    ];
    for r in rays {
        assert!(m.snap(p, Rot24::IDENTITY, &r).unwrap().is_none(), "{r:?}");
        assert!(m.pick(&r).is_none(), "{r:?}");
    }
    // Inside MAX_RAY, hitting the ground far beyond MAX_COORD: pick reports the
    // hit, but nothing can be placed there.
    let far = Ray {
        origin: [5e8, -1000.0, 5e8],
        dir: [0.0, 1.0, 0.0],
    };
    assert!(m.pick(&far).is_some());
    assert!(m.snap(p, Rot24::IDENTITY, &far).unwrap().is_none());
}

/// Random combinations of extreme values must come back as Ok or Err, never a panic.
#[test]
fn hostile_poses_never_panic() {
    let extremes_i32 = [
        i32::MIN,
        i32::MIN + 1,
        -100_000_001,
        -100_000_000,
        -20,
        -1,
        0,
        1,
        10,
        100_000_000,
        100_000_001,
        i32::MAX - 1,
        i32::MAX,
    ];
    let extremes_f64 = [
        0.0,
        -0.0,
        1.0,
        -1e9,
        1e9,
        1e9 + 1.0,
        1e300,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        5e-324,
        1e-300,
        1e8,
        -1e8,
    ];
    let parts = [
        "ob.brick-1x1",
        "ob.baseplate-16x16",
        "ob.tile-2x2",
        "ob.plate-2x6",
    ];
    for seed in 0..40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9) ^ 0xD00D);
        let mut m = on_baseplate();
        for _ in 0..150 {
            let pick = |rng: &mut Rng| *rng.pick(&extremes_i32);
            let pos = if rng.range(0, 2) == 0 {
                [pick(&mut rng), pick(&mut rng), pick(&mut rng)]
            } else {
                [
                    rng.range(-200, 200),
                    rng.range(-200, 8),
                    rng.range(-200, 200),
                ]
            };
            let rot = *rng.pick(&[0u8, 1, 5, 23, 24, 100, 255]);
            let part = (*rng.pick(&parts)).to_string();
            let id = *rng.pick(&[0u32, 1, 2, 3, u32::MAX]);
            let cmd = match rng.range(0, 6) {
                0 => Command::Add {
                    part,
                    pos,
                    rot,
                    colour: *rng.pick(&[0u16, 4, 15, 9999, u16::MAX]),
                },
                1 => Command::Move { id, pos, rot },
                2 => Command::Turn {
                    id,
                    quarter_turns: *rng.pick(&[i32::MIN, -1, 0, 1, 3, i32::MAX]),
                },
                3 => Command::Recolour {
                    id,
                    colour: *rng.pick(&[4u16, 70, u16::MAX]),
                },
                4 => Command::Remove { id },
                _ => Command::ClearBuilt,
            };
            let _ = m.apply(&cmd);
            let ray = Ray {
                origin: [
                    *rng.pick(&extremes_f64),
                    *rng.pick(&extremes_f64),
                    *rng.pick(&extremes_f64),
                ],
                dir: [
                    *rng.pick(&extremes_f64),
                    *rng.pick(&extremes_f64),
                    *rng.pick(&extremes_f64),
                ],
            };
            let p = m.catalog().find(&parts[rng.range(0, 3) as usize]).unwrap();
            let _ = m.snap(p, Rot24(*rng.pick(&[0u8, 5, 23])), &ray);
            let _ = m.pick(&ray);
            let _ = m.stats();
            if rng.range(0, 4) == 0 {
                let _ = m.undo();
            }
        }
        // Whatever survived still saves and loads.
        let text = m.to_text("hostile", None);
        Model::from_text(catalog(), &text).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    }
}
