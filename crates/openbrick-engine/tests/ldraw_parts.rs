//! The engine with parts of an LDraw pack: snapping, stacking, collision, the
//! connection graph, `.obrick` round trips and the error for a missing pack.
//! The pack is imported from the synthetic mini library of the catalog's tests
//! (written for the tests, none of the LDraw library's content).

mod common;

use std::path::Path;

use common::*;
use openbrick_catalog::ldraw::{import, ImportOptions};
use openbrick_catalog::{Catalog, ConnKind, Pack};
use openbrick_engine::{Command, DocError, EngineError, Model, Ray};
use openbrick_units::Rot24;

fn pack() -> Pack {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../openbrick-catalog/tests/fixtures/ldraw");
    let opts = ImportOptions {
        categories: Some(
            ["Brick", "Plate", "Tile", "Technic"]
                .map(String::from)
                .to_vec(),
        ),
        ..ImportOptions::default()
    };
    let (p, _) = import(&dir, &opts, &mut |_, _, _| {}).expect("fixture imports");
    // Through the file format, as the page loads it.
    Pack::from_bytes(&p.to_bytes().unwrap()).unwrap()
}

fn with_pack() -> &'static Catalog {
    use std::sync::OnceLock;
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| Catalog::builtin().unwrap().with_pack(&pack()))
}

fn add(part: &str, pos: [i32; 3], rot: u8) -> Command {
    Command::Add {
        part: part.into(),
        pos,
        rot,
        colour: 15,
    }
}

#[test]
fn imported_bricks_stack_with_all_their_studs_connected() {
    let c = with_pack();
    let mut m = Model::new(c);
    // A 2 x 2 on the ground (its studs on the stud grid: x and z at 0 puts them at plus and minus 10).
    m.apply(&add("ld.3003", [0, -24, 0], 0)).unwrap();
    let p = c.find("ld.3003").unwrap();
    let s = m.snap(p, Rot24::IDENTITY, &down(3, 3)).unwrap().unwrap();
    assert!(s.valid && !s.grounded, "{s:?}");
    assert_eq!((s.pos, s.mates), ([0, -48, 0], 4));
    m.apply(&add("ld.3003", s.pos, s.rot)).unwrap();
    let stats = m.stats();
    assert_eq!(
        (stats.parts, stats.connections, stats.floating.len()),
        (2, 4, 0)
    );
    // The same two parts overlapping are refused, with the engine's reason.
    let e = m.apply(&add("ld.3003", [0, -36, 0], 0)).unwrap_err();
    assert!(matches!(e, EngineError::Collides { .. }), "{e}");
}

#[test]
fn an_imported_plate_sits_on_a_builtin_brick_and_a_tile_on_the_plate() {
    let c = with_pack();
    let mut m = Model::new(c);
    m.apply(&add("ob.brick-2x2", [0, -24, 0], 0)).unwrap();
    let plate = c.find("ld.3024").unwrap();
    let s = m
        .snap(plate, Rot24::IDENTITY, &down(5, 5))
        .unwrap()
        .unwrap();
    assert!(s.valid, "{s:?}");
    assert_eq!(s.mates, 1);
    m.apply(&add("ld.3024", s.pos, s.rot)).unwrap();
    let tile = c.find("ld.3070b").unwrap();
    let t = m.snap(tile, Rot24::IDENTITY, &down(5, 5)).unwrap().unwrap();
    assert!(t.valid && t.mates == 1, "{t:?}");
    m.apply(&add("ld.3070b", t.pos, t.rot)).unwrap();
    // A tile has no studs: nothing stacks on it.
    let again = m
        .snap(plate, Rot24::IDENTITY, &down(5, 5))
        .unwrap()
        .unwrap();
    assert!(
        !again.valid || again.mates == 0 && again.grounded,
        "{again:?}"
    );
}

#[test]
fn a_part_with_studs_half_a_stud_off_is_still_placed_on_the_lattice() {
    // The 1 x 2 in the fixture is symmetric, so use a yaw turn: a 1 x 4 turned a quarter has its studs
    // at x = 0 and z = -30..30; snapping keeps every stud on the half-stud lattice.
    let c = with_pack();
    let m = Model::new(c);
    let long = c.find("ld.3010").unwrap();
    for turns in 0..4u8 {
        let rot = Rot24::yaw_quarter_turns(turns);
        let s = m.snap(long, rot, &down(7, 13)).unwrap().unwrap();
        assert!(s.valid && s.grounded, "turn {turns}: {s:?}");
        let mut m2 = Model::new(c);
        m2.apply(&add("ld.3010", s.pos, s.rot)).unwrap();
    }
}

#[test]
fn removing_and_undoing_imported_parts_restores_the_model() {
    let c = with_pack();
    let mut m = Model::new(c);
    let before = m.to_text("t", None);
    m.apply(&add("ld.3003", [0, -24, 0], 0)).unwrap();
    m.apply(&add("ld.3005", [10, -48, 10], 0)).unwrap();
    m.undo().unwrap();
    m.undo().unwrap();
    assert_eq!(m.to_text("t", None), before);
    assert!(m.indexes_are_empty());
}

#[test]
fn a_model_with_pack_parts_says_which_pack_it_needs_and_loads_with_it() {
    let c = with_pack();
    let mut m = Model::new(c);
    m.apply(&add("ob.brick-2x4", [0, -24, 0], 0)).unwrap();
    m.apply(&add("ld.3003", [0, -48, 0], 0)).unwrap();
    let text = m.to_text("two", None);
    assert!(text.contains("\"requires\""), "{text}");
    assert!(text.contains(&format!(
        "\"fingerprint\":\"{}\"",
        c.pack.as_ref().unwrap().fingerprint
    )));
    assert!(text.contains("\"library\":\"2026-01\""));
    // With the pack: byte for byte round trip.
    let back = Model::from_text(c, &text).unwrap();
    assert_eq!(back.to_text("two", None), text);
    // Without it: an error that names the pack and what to do, never a crash.
    let e = Model::from_text(catalog(), &text).unwrap_err();
    match &e {
        DocError::MissingPack {
            part,
            pack,
            library,
            ..
        } => assert_eq!(
            (part.as_str(), pack.as_str(), library.as_str()),
            ("ld.3003", "ldraw", "2026-01")
        ),
        other => panic!("{other:?}"),
    }
    assert!(
        e.to_string().contains("LDraw pack") && e.to_string().contains("not loaded"),
        "{e}"
    );
    // A model of built-in parts only has no `requires` and loads anywhere.
    let mut plain = Model::new(c);
    plain.apply(&add("ob.brick-2x4", [0, -24, 0], 0)).unwrap();
    assert!(!plain.to_text("p", None).contains("requires"));
}

#[test]
fn a_part_id_that_is_not_in_any_pack_is_an_unknown_part() {
    let text = Model::new(catalog()).to_text("e", None).replace("\"instances\": []", "\"instances\": [{\"id\":1,\"part\":\"zz.nothing\",\"pose\":{\"pos\":[0,0,0],\"rot\":0},\"colour\":15}]");
    assert!(matches!(
        Model::from_text(catalog(), &text),
        Err(DocError::UnknownPart { .. })
    ));
}

#[test]
fn pins_and_axles_mate_by_position_and_axis_not_by_facing() {
    use openbrick_catalog::Conn;
    let pin = Conn {
        kind: ConnKind::PinM,
        pos: [0, 10, 0],
        dir: [0, 0, 1],
    };
    let hole = Conn {
        kind: ConnKind::PinF,
        pos: [0, 10, 0],
        dir: [0, 0, 1],
    };
    assert!(pin.mates(&hole) && hole.mates(&pin));
    let flipped = Conn {
        dir: [0, 0, -1],
        ..hole
    };
    assert!(pin.mates(&flipped), "an axis has no sign");
    let across = Conn {
        dir: [1, 0, 0],
        ..hole
    };
    assert!(!pin.mates(&across));
    let axle = Conn {
        kind: ConnKind::AxleM,
        ..pin
    };
    assert!(!axle.mates(&hole), "an axle does not fit a round hole");
    // A part turned to a pose still has its pins on the hole's axis.
    let r = Rot24::from_matrix([[1, 0, 0], [0, 0, 1], [0, -1, 0]]).unwrap();
    assert_eq!(pin.transformed(r, [0, 0, 0]).dir.map(i32::abs), [0, 1, 0]);
}

#[test]
fn engine_snaps_a_pack_part_wherever_the_ray_goes_without_panicking() {
    // Seeded random rays and rotations over every pack part: a snap is either none or an answer
    // whose pose the engine accepts when it says valid.
    let c = with_pack();
    let mut rng = Rng(0xC0FFEE);
    let mut m = Model::new(c);
    m.apply(&add("ob.baseplate-16x16", [0, -8, 0], 0)).unwrap();
    for _ in 0..300 {
        let part = rng.range(0, c.len() as i32 - 1) as u32;
        let rot = Rot24(rng.range(0, 23) as u8);
        let ray = ray_towards(&mut rng, 150);
        if let Some(s) = m.snap(part, rot, &ray).unwrap() {
            if s.valid {
                // A valid snap is a pose the model accepts: snapping and adding agree.
                let cmd = add(&c.parts[part as usize].id, s.pos, s.rot);
                m.apply(&cmd)
                    .unwrap_or_else(|e| panic!("{}: {s:?}: {e}", c.parts[part as usize].id));
            }
        }
    }
    let st = m.stats();
    assert_eq!(st.parts, m.len());
    let _ = Ray {
        origin: [0.0; 3],
        dir: [0.0, 1.0, 0.0],
    };
}

#[test]
fn a_pack_part_whose_studs_are_off_the_stud_grid_is_placed_where_it_snaps() {
    // A brick with something hanging two units below it is 26 high, so on the ground its studs are
    // at Y = -26, off the plate grid. A real library has many such parts (clips, brackets): the
    // snap must not call a pose valid that the model then refuses.
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../openbrick-catalog/tests/fixtures/ldraw/ldraw");
    let dir = std::env::temp_dir().join(format!("openbrick-engine-offgrid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["p", "parts"] {
        std::fs::create_dir_all(dir.join("ldraw").join(sub)).unwrap();
    }
    for f in ["cube.dat", "stud.dat"] {
        std::fs::copy(src.join("p").join(f), dir.join("ldraw/p").join(f)).unwrap();
    }
    let part = "0 Brick  1 x  1 with Hanging Lug\n0 Name: 9001.dat\n0 !LDRAW_ORG Part UPDATE 2026-01\n0 BFC CERTIFY CCW\n\
        1 16 0 0 0 10 0 0 0 24 0 0 0 10 cube.dat\n1 16 0 24 0 3 0 0 0 2 0 0 0 3 cube.dat\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 stud.dat\n";
    std::fs::write(dir.join("ldraw/parts/9001.dat"), part).unwrap();
    let opts = ImportOptions {
        categories: Some(vec!["Brick".into()]),
        ..ImportOptions::default()
    };
    let (p, _) = import(&dir, &opts, &mut |_, _, _| {}).expect("imports");
    let c = Catalog::builtin().unwrap().with_pack(&p);
    let _ = std::fs::remove_dir_all(&dir);
    let id = c.find("ld.9001").unwrap();
    assert_eq!(c.parts[id as usize].shape.body.max[1], 26);
    let m = Model::new(&c);
    let s = m.snap(id, Rot24::IDENTITY, &down(3, 3)).unwrap().unwrap();
    assert!(s.valid && s.grounded, "{s:?}");
    let mut m = Model::new(&c);
    m.apply(&add("ld.9001", s.pos, s.rot)).unwrap();
    // A built-in part is still held to the stud grid.
    let e = Model::new(&c)
        .apply(&add("ob.brick-1x1", [10, -4, 10], 0))
        .unwrap_err();
    assert!(matches!(e, EngineError::OffGrid { .. }), "{e}");
}

/// The same pack with another library version (so another fingerprint) or another resolution.
fn other_pack(library: Option<&str>) -> Pack {
    let base = pack();
    let mut header = base.header.clone();
    match library {
        Some(l) => header.library = l.into(),
        None => header.generator = "another build".into(),
    }
    let meshes: Vec<_> = (0..header.parts.len())
        .map(|i| base.mesh(i).unwrap())
        .collect();
    Pack::build(header, &meshes).unwrap()
}

#[test]
fn a_pack_of_another_version_opens_the_model_with_a_notice() {
    let c = with_pack();
    let mut m = Model::new(c);
    m.apply(&add("ld.3003", [0, -24, 0], 0)).unwrap();
    let text = m.to_text("one", None);
    // The same pack: no notice.
    let (back, notices) = Model::from_text_with_notices(c, &text).unwrap();
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(back.len(), 1);
    // Another library version: the model opens and the notice says both versions.
    let newer = Catalog::builtin().unwrap().with_pack(&other_pack(Some("2027-02")));
    let (back, notices) = Model::from_text_with_notices(&newer, &text).unwrap();
    assert_eq!(back.len(), 1, "a pack of another version still opens the model");
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("2026-01") && notices[0].contains("2027-02"), "{}", notices[0]);
    assert!(notices[0].contains("fingerprint"), "{}", notices[0]);
    // The same library version, another build of the pack.
    let rebuilt = Catalog::builtin().unwrap().with_pack(&other_pack(None));
    let (_, notices) = Model::from_text_with_notices(&rebuilt, &text).unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("another build"), "{}", notices[0]);
    // A model of built-in parts only, whatever it says it requires, has nothing to differ in.
    let mut plain = Model::new(c);
    plain.apply(&add("ob.brick-2x4", [0, -24, 0], 0)).unwrap();
    let doc = plain.to_text("p", None);
    assert!(Model::from_text_with_notices(&newer, &doc).unwrap().1.is_empty());
    // `from_text` is unchanged: it opens with another pack and says nothing.
    assert!(Model::from_text(&newer, &text).is_ok());
}

#[test]
fn requires_is_checked_in_the_core_like_the_schema() {
    let c = with_pack();
    let mut m = Model::new(c);
    m.apply(&add("ob.brick-2x4", [0, -24, 0], 0)).unwrap();
    let base = m.to_text("p", None);
    let good = r#"{"pack":"ldraw","library":"2026-01","fingerprint":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
    let with = |requires: &str| base.replacen("\"palette\"", &format!("\"requires\": [{requires}],\n  \"palette\""), 1);
    assert!(Model::from_text(c, &with(good)).is_ok());
    let many = vec![good; 17].join(",");
    let bad_fp = good.replace("0000000000000000000000000000000000000000000000000000000000000000", "ABC");
    let upper = good.replace("00000000", "ABCDEF00");
    let long_lib = good.replace("2026-01", &"9".repeat(33));
    let bidi_lib = good.replace("2026-01", "20\\u202e26");
    let empty_pack = good.replace("\"ldraw\"", "\"\"");
    let odd_pack = good.replace("\"ldraw\"", "\"LD/RAW\"");
    for (what, requires) in [
        ("17 entries", many.as_str()),
        ("short fingerprint", bad_fp.as_str()),
        ("upper-case fingerprint", upper.as_str()),
        ("long library", long_lib.as_str()),
        ("bidi library", bidi_lib.as_str()),
        ("empty pack", empty_pack.as_str()),
        ("odd pack", odd_pack.as_str()),
    ] {
        let e = Model::from_text(c, &with(requires)).map(|_| ()).unwrap_err();
        assert!(matches!(e, DocError::Requires(_)), "{what}: {e}");
        assert!(e.to_string().starts_with("`requires`"), "{what}: {e}");
        assert!(e.to_string().len() < 400, "{what}: {e}");
    }
    // 16 entries are read.
    assert!(Model::from_text(c, &with(&vec![good; 16].join(","))).is_ok());
}

#[test]
fn error_messages_repeat_only_a_short_printable_part_of_the_document() {
    let long = "x".repeat(100_000);
    let doc = format!(
        r#"{{"openbrick":"1.0","kind":"model","name":"x","units":"ldu","frame":"ldraw-y-down","catalog":"openbrick-builtin@1","palette":[],"bom":[],
        "submodels":[{{"id":"main","instances":[{{"id":1,"part":"ld.\u202e{long}","pose":{{"pos":[10,-24,10],"rot":0}},"colour":4}}]}}]}}"#
    );
    let e = Model::from_text(catalog(), &doc).map(|_| ()).unwrap_err();
    assert!(e.to_string().len() < 800, "{} bytes", e.to_string().len());
    assert!(!e.to_string().contains('\u{202e}'), "{e}");
    let e = Model::from_text(catalog(), &doc.replace("\"model\"", &format!("\"{long}\"")))
        .map(|_| ())
        .unwrap_err();
    assert!(e.to_string().len() < 400, "{} bytes", e.to_string().len());
}
