//! Calibration of the occupancy masks and the Technic snapping on the real LDraw library, when
//! `OPENBRICK_LDRAW_DIR` names one (a folder or complete.zip); skipped otherwise, so CI does not
//! need the library. It never writes the library or a pack anywhere.
//!
//! For every part of the pack it mates the parts that must fit: a 1 x 1 plate's stud in every
//! receptacle, a pin in every pin hole and an axle in every axle hole, and counts the poses the
//! engine refuses (a part that legitimately mates must never be refused; the number is reported).

mod common;

use openbrick_catalog::ldraw::{import, ImportOptions, DEFAULT_CATEGORIES, TECHNIC_CATEGORIES};
use openbrick_catalog::{Catalog, Conn, ConnKind, Origin};
use openbrick_engine::{Command, Model};
use openbrick_units::{LatticePose, Rot24};

/// The library named by `OPENBRICK_LDRAW_DIR`, or `None` with a printed `SKIPPED` line. CI that wants
/// these checks to run sets `OPENBRICK_REQUIRE_LDRAW=1`, and then a missing library fails the test
/// instead of letting it pass by doing nothing.
fn real_library() -> Option<String> {
    match std::env::var("OPENBRICK_LDRAW_DIR") {
        Ok(d) => Some(d),
        Err(_) => {
            assert!(
                std::env::var("OPENBRICK_REQUIRE_LDRAW").is_err(),
                "OPENBRICK_REQUIRE_LDRAW is set but OPENBRICK_LDRAW_DIR is not"
            );
            eprintln!(
                "SKIPPED: OPENBRICK_LDRAW_DIR is not set; the real-library calibration did not run"
            );
            None
        }
    }
}

/// The turn that takes the pin's or axle's own axis (`from`) onto `to`.
fn turn_onto(from: [i32; 3], to: [i32; 3]) -> Rot24 {
    for r in Rot24::all() {
        let a = r.apply(from);
        if a == to || a == to.map(|v| -v) {
            return r;
        }
    }
    unreachable!("every axis maps onto every axis")
}

#[test]
fn pins_axles_and_studs_fit_the_real_parts_they_mate_with() {
    let Some(dir) = real_library() else {
        return;
    };
    let cats: Vec<String> = DEFAULT_CATEGORIES
        .iter()
        .chain(TECHNIC_CATEGORIES)
        .map(|s| (*s).to_string())
        .collect();
    let opts = ImportOptions {
        categories: Some(cats),
        limit: None,
        ..ImportOptions::default()
    };
    let (pack, report) =
        import(std::path::Path::new(&dir), &opts, &mut |_, _, _| {}).expect("imports");
    eprintln!(
        "imported {} parts, {} with masks ({} too fragmented)",
        report.counts.imported, report.counts.with_masks, report.masks_too_fragmented
    );
    let cat = Catalog::builtin().unwrap().with_pack(&pack);
    let find = |n: &str| cat.find(&format!("ld.{n}"));
    let plate = find("3024").expect("plate 1x1");
    let pin = find("3673").expect("pin");
    let fric = find("2780").expect("friction pin");
    let axle = find("3705").expect("axle 4 (the pack has no part with axle connectors of its own)");
    let mut counts = std::collections::BTreeMap::<&str, (u32, u32)>::new();
    let mut failed: Vec<String> = Vec::new();
    let mut refused_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for p in &cat.parts {
        if p.origin != Origin::Ldraw {
            continue;
        }
        // The part on its own, high above the ground.
        let mut m = Model::new(&cat);
        let base = LatticePose {
            pos: [0, -600, 0],
            rot: Rot24::IDENTITY,
        };
        if m.apply(&Command::Add {
            part: p.id.clone(),
            pos: base.pos,
            rot: 0,
            colour: 15,
        })
        .is_err()
        {
            continue;
        }
        let conns: Vec<Conn> = p
            .shape
            .connectors
            .iter()
            .map(|c| c.transformed(base.rot, base.pos))
            .collect();
        for c in conns.iter().take(40) {
            let (name, with, own_kind, rolls): (&str, Vec<u32>, ConnKind, bool) = match c.kind {
                ConnKind::TubeF => ("stud in receptacle", vec![plate], ConnKind::StudM, false),
                ConnKind::PinF => ("pin in hole", vec![pin, fric], ConnKind::PinM, false),
                ConnKind::AxleF => ("axle in hole", vec![axle], ConnKind::AxleM, true),
                _ => continue,
            };
            let entry = counts.entry(name).or_default();
            entry.0 += 1;
            // Does any of the fitting parts, in any of its poses, go in?
            let mut fits = false;
            for w in &with {
                let shape = &cat.parts[*w as usize].shape;
                for own in shape.connectors.iter().filter(|o| o.kind == own_kind) {
                    let rolls: Vec<Rot24> = if rolls {
                        let r = Rot24::from_matrix([[1, 0, 0], [0, 0, -1], [0, 1, 0]]).unwrap();
                        let along = turn_onto(own.dir, c.dir);
                        (0..4)
                            .map(|k| {
                                let about = (0..k).fold(Rot24::IDENTITY, |a, _| r.compose(a));
                                // a quarter turn about the hole's own axis: conjugate by the alignment
                                along.compose(about.compose(along.inverse()).compose(along))
                            })
                            .collect()
                    } else {
                        vec![turn_onto(own.dir, c.dir)]
                    };
                    for rot in rolls {
                        let o = rot.apply(own.pos);
                        let pos = [c.pos[0] - o[0], c.pos[1] - o[1], c.pos[2] - o[2]];
                        let pose = LatticePose { pos, rot };
                        // The stud's own opening faces down; it is the part's pin or axle that must lie on the axis.
                        if m.check_pose(*w, &pose, None).is_ok() {
                            fits = true;
                        }
                    }
                }
            }
            if std::env::var("OB_FOCUS")
                .is_ok_and(|f| f.split(',').any(|x| format!("ld.{x}") == p.id))
            {
                eprintln!(
                    "focus {} {name} at {:?} dir {:?}: {}",
                    p.id,
                    c.pos,
                    c.dir,
                    if fits { "fits" } else { "REFUSED" }
                );
            }
            if fits {
                entry.1 += 1;
            } else {
                refused_ids.insert(p.id.clone());
                if failed.len() < 40 {
                    failed.push(format!("{} {name} at {:?}", p.id, c.pos));
                }
            }
        }
    }
    for (k, (n, ok)) in &counts {
        eprintln!(
            "{k}: {ok} of {n} fit ({:.1} percent)",
            100.0 * f64::from(*ok) / f64::from((*n).max(1))
        );
    }
    for f in &failed {
        eprintln!("  refused: {f}");
    }
    // The test can fail: floors a few points under what was measured (the 2026-10 library, union-depth rule: studs 94.1,
    // pins 71.6, axles 76.3 percent), and the Technic bricks, beams and liftarms of the plan's list fit
    // every one of their holes (the first 40 connectors of each part are tried).
    let rate = |k: &str| {
        let (n, ok) = counts[k];
        100.0 * f64::from(ok) / f64::from(n.max(1))
    };
    assert!(
        rate("stud in receptacle") >= 94.0,
        "studs {:.1}",
        rate("stud in receptacle")
    );
    assert!(
        rate("pin in hole") >= 70.0,
        "pins {:.1}",
        rate("pin in hole")
    );
    assert!(
        rate("axle in hole") >= 75.0,
        "axles {:.1}",
        rate("axle in hole")
    );
    for n in ["3700", "3701", "3702", "32316", "32523", "32524", "41678"] {
        assert!(
            cat.find(&format!("ld.{n}")).is_some(),
            "ld.{n} is in the pack"
        );
        assert!(
            !refused_ids.contains(&format!("ld.{n}")),
            "ld.{n} refused a pin or axle in one of its holes: {failed:?}"
        );
    }
}

/// The clip, bar, ball, socket and hinge connectors of the joint table, against the real parts:
/// for pairs of parts that should join (a clip and a bar, a ball and a socket, an odd and an even
/// hinge half), mate their connectors where they coincide and count the poses with no shared
/// volume. A table entry whose point or axis is wrong makes parts collide at the mated pose.
#[test]
fn joints_fit_the_real_parts_they_mate_with() {
    use openbrick_catalog::Link;
    let Some(dir) = real_library() else {
        return;
    };
    let cats: Vec<String> = DEFAULT_CATEGORIES
        .iter()
        .chain(TECHNIC_CATEGORIES)
        .map(|s| (*s).to_string())
        .collect();
    let opts = ImportOptions {
        categories: Some(cats),
        limit: None,
        ..ImportOptions::default()
    };
    let (pack, _) = import(std::path::Path::new(&dir), &opts, &mut |_, _, _| {}).expect("imports");
    let cat = Catalog::builtin().unwrap().with_pack(&pack);
    let with = |kind: ConnKind| -> Vec<u32> {
        cat.parts
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.origin == Origin::Ldraw && p.shape.connectors.iter().any(|c| c.kind == kind)
            })
            .map(|(i, _)| i as u32)
            .collect()
    };
    let mut total = std::collections::BTreeMap::<&str, (u32, u32, u32)>::new();
    for (name, a_kind, b_kind) in [
        ("clip on bar", ConnKind::ClipF, ConnKind::BarM),
        ("ball in socket", ConnKind::BallM, ConnKind::SocketF),
        ("hinge halves", ConnKind::HingeO, ConnKind::HingeE),
    ] {
        let (a_parts, b_parts) = (with(a_kind), with(b_kind));
        eprintln!(
            "{name}: {} parts with {}, {} with {}",
            a_parts.len(),
            a_kind.name(),
            b_parts.len(),
            b_kind.name()
        );
        for &a in a_parts.iter().take(40) {
            for &b in b_parts.iter().take(40) {
                // The target part, high above the ground, and the pose of the other that mates.
                let mut m = Model::new(&cat);
                if m.apply(&Command::Add {
                    part: cat.parts[b as usize].id.clone(),
                    pos: [0, -600, 0],
                    rot: 0,
                    colour: 15,
                })
                .is_err()
                {
                    continue;
                }
                let targets: Vec<Conn> = cat.parts[b as usize]
                    .shape
                    .connectors
                    .iter()
                    .filter(|c| c.kind == b_kind)
                    .map(|c| c.transformed(Rot24::IDENTITY, [0, -600, 0]))
                    .collect();
                for t in targets.iter().take(4) {
                    let point = t.pos.map(f64::from);
                    let poses = m.mate_poses(a, Rot24::IDENTITY, point).unwrap();
                    let e = total.entry(name).or_default();
                    e.0 += 1;
                    let mut fits = false;
                    let mut mated = false;
                    for (pos, rot, _) in poses {
                        let pose = LatticePose { pos, rot };
                        let (n, links) = m.mates_links(a, &pose, None).unwrap();
                        if n > 0
                            && links
                                & Link::ALL
                                    .iter()
                                    .filter(|l| **l == a_kind.link())
                                    .map(|l| l.bit())
                                    .sum::<u32>()
                                != 0
                        {
                            mated = true;
                            if m.check_pose(a, &pose, None).is_ok() {
                                fits = true;
                            }
                        }
                    }
                    e.1 += u32::from(mated);
                    e.2 += u32::from(fits);
                }
            }
        }
    }
    for (k, (tried, mated, fit)) in &total {
        eprintln!("{k}: {tried} connector pairs tried, {mated} mate by position and axis, {fit} of those with no shared volume ({:.1} percent)", 100.0 * f64::from(*fit) / f64::from((*mated).max(1)));
    }
    // Floors a few points under the 2026-10 measurement (union-depth rule: clip 64.2, ball 36.0, hinge 53.1 percent), and every
    // pair mates by position and axis (a table entry whose axis were wrong would mate fewer).
    for (k, floor) in [
        ("clip on bar", 60.0),
        ("ball in socket", 30.0),
        ("hinge halves", 50.0),
    ] {
        let (tried, mated, fit) = total[k];
        assert!(
            tried > 0 && mated == tried,
            "{k}: {mated} of {tried} pairs mate"
        );
        assert!(
            100.0 * f64::from(fit) / f64::from(mated) >= floor,
            "{k}: {fit} of {mated}"
        );
    }
}

/// The connectors derived for the part types the plan names (friction pin, long pin, pin with axle end,
/// bush, half bush), printed, and checked where the library's own description says what they must be.
#[test]
fn named_technic_parts_have_the_connectors_their_descriptions_promise() {
    let Some(dir) = real_library() else {
        return;
    };
    let cats: Vec<String> = DEFAULT_CATEGORIES
        .iter()
        .chain(TECHNIC_CATEGORIES)
        .map(|s| (*s).to_string())
        .collect();
    let opts = ImportOptions {
        categories: Some(cats),
        limit: None,
        include_unconnected: true,
        ..ImportOptions::default()
    };
    let (pack, _) = import(std::path::Path::new(&dir), &opts, &mut |_, _, _| {}).expect("imports");
    let cat = Catalog::builtin().unwrap().with_pack(&pack);
    let kinds = |n: &str| -> Option<std::collections::BTreeMap<&'static str, usize>> {
        let p = &cat.parts[cat.find(&format!("ld.{n}"))? as usize];
        let mut m = std::collections::BTreeMap::new();
        for c in &p.shape.connectors {
            *m.entry(c.kind.name()).or_default() += 1;
        }
        eprintln!("{n} {}: {m:?}", p.name);
        Some(m)
    };
    // Friction pin (ribs are not modelled: a pin is a pin), long pin, pin with axle end, bush, half bush.
    for n in ["2780", "6558", "3749", "3713", "4265c"] {
        let m = kinds(n);
        eprintln!(
            "  {n}: {}",
            if m.is_some() {
                "in the pack"
            } else {
                "NOT in the pack"
            }
        );
    }
    assert_eq!(
        kinds("3713").unwrap().get("axleF"),
        Some(&1),
        "Technic Bush: its axle hole"
    );
    // 3749 (Technic Axle Pin) gets its pin module but not the axle end: a known gap, printed above.
    assert_eq!(kinds("3749").unwrap().get("pinM"), Some(&1));
    let pin = kinds("3673").unwrap();
    assert_eq!(pin.get("pinM"), Some(&2), "Technic Pin: two modules of pin");
    let fric = kinds("2780").unwrap();
    assert_eq!(
        fric.get("pinM"),
        Some(&2),
        "Technic Pin with Friction: two modules of pin"
    );
    if let Some(long) = kinds("6558") {
        assert_eq!(long.get("pinM"), Some(&3), "Technic Pin 3L: three modules");
    }
}
