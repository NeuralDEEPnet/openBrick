//! The assembler exports, natively: the same functions the browser calls.

use openbrick_wasm::*;
use serde_json::{json, Value};

fn json_of(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn catalog_lists_parts_palette_and_rotations() {
    let c = json_of(&assembler_catalog_json().unwrap());
    assert_eq!(c["catalog"], "openbrick-builtin@1");
    let parts = c["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 14);
    assert!(parts.iter().any(|p| p["base"] == true));
    assert_eq!(c["rotations"].as_array().unwrap().len(), 24);
    assert_eq!(c["yaws"][0], 0);
    assert_eq!(c["lduMm"], 0.4);
    assert_eq!(
        (c["studPitchLdu"].clone(), c["plateLdu"].clone()),
        (json!(20), json!(8))
    );
    assert_eq!(c["maxModelBytes"], 64 * 1024 * 1024);
    assert_eq!(c["maxParts"], 150_000);
    assert_eq!(c["maxHistory"], 5_000);
    assert!(c["palette"]["colours"].as_array().unwrap().len() >= 8);
    assert_eq!(c["samples"].as_array().unwrap().len(), 2);
}

#[test]
fn meshes_come_in_two_levels_of_detail() {
    let (mut d, l) = (
        assembler_part_mesh(4, true).unwrap(),
        assembler_part_mesh(4, false).unwrap(),
    );
    assert!(l.triangle_count() < d.triangle_count());
    let tris = d.triangle_count();
    let idx = d.take_indices();
    let pos = d.take_positions();
    assert_eq!(idx.len() as u32, tris * 3);
    assert!(idx.iter().all(|&i| (i as usize) < pos.len() / 3));
    assert!(assembler_part_mesh(99, true).is_err());
}

#[test]
fn place_undo_save_load_bom_and_stats() {
    let mut a = Assembler::create().unwrap();
    let base = json!({"op":"add","part":"ob.baseplate-16x16","pos":[0,-8,0],"rot":0,"colour":2});
    a.apply(&base.to_string()).unwrap();
    let hit = json_of(&a.pick(0.0, -500.0, 0.0, 0.0, 1.0, 0.0).unwrap());
    assert_eq!(hit["instance"], 1);
    let snap = json_of(
        &a.snap("ob.brick-2x4", 0, 3.0, -500.0, 47.0, 0.0, 1.0, 0.0)
            .unwrap(),
    );
    assert_eq!(snap["valid"], true);
    assert_eq!(snap["pos"], json!([0, -32, 40]));
    let add =
        json!({"op":"add","part":"ob.brick-2x4","pos":snap["pos"],"rot":snap["rot"],"colour":4});
    let r = json_of(&a.apply(&add.to_string()).unwrap());
    assert_eq!(r["id"], 2);
    assert_eq!(a.instances().len(), 14);
    let stats = json_of(&a.stats());
    assert_eq!(stats["parts"], json!(2));
    assert_eq!(stats["floating"], json!([]));
    assert_eq!(json_of(&a.bom()).as_array().unwrap().len(), 2);

    let text = a.save("Test");
    let mut b = Assembler::create().unwrap();
    b.load(&text).unwrap();
    assert_eq!(b.instances(), a.instances());
    assert_eq!(b.save("Test"), text);

    a.undo().unwrap();
    assert!(a.can_redo());
    assert_eq!(a.instances().len(), 7);
    a.redo().unwrap();
    // A refused command is an error with a fix hint; the model is unchanged.
    let err = a.apply(&add.to_string()).unwrap_err();
    assert!(err.0.contains("only touch"), "{err}");
    assert_eq!(a.instances().len(), 14);
    assert!(a.apply("{\"op\":\"nope\"}").is_err());
    assert!(a.load("{}").is_err());
    assert_eq!(a.instances().len(), 14, "a failed load keeps the model");
}

#[test]
fn samples_load_and_stand() {
    for id in ["house", "staircase"] {
        let mut a = Assembler::create().unwrap();
        a.load(&assembler_sample(id).unwrap()).unwrap();
        let s = json_of(&a.stats());
        assert_eq!(s["floating"], json!([]), "{id}");
        assert_eq!(s["components"], 1, "{id}");
        assert!(s["parts"].as_u64().unwrap() >= 8);
    }
    assert!(assembler_sample("nope").is_err());
}

#[test]
fn yaw_rotation_composes_in_the_core() {
    assert_eq!(assembler_turn_rotation(0, 4).unwrap(), 0);
    let one = assembler_turn_rotation(0, 1).unwrap();
    assert_ne!(one, 0);
    assert_eq!(assembler_turn_rotation(one, 3).unwrap(), 0);
    assert!(assembler_turn_rotation(24, 1).is_err());
}

#[test]
fn loading_a_model_returns_its_notices_as_json() {
    let mut a = Assembler::create().unwrap();
    let notices = a.load(&assembler_sample("staircase").unwrap()).unwrap();
    assert_eq!(notices, "[]", "built-in parts only: nothing to say");
}

/// The page keeps its own copy of the table and patches it with deltas: after every edit, the copy
/// patched with the delta equals the table the core holds.
#[test]
fn deltas_patch_a_copy_into_the_table() {
    let mut a = Assembler::create().unwrap();
    let mut copy: std::collections::BTreeMap<i32, Vec<i32>> = Default::default();
    let mut order: Vec<i32> = Vec::new();
    let mut sync = |a: &mut Assembler| {
        let d = a.delta();
        if d[0] == 1 {
            copy.clear();
            order.clear();
            for r in d[1..].as_chunks::<7>().0.iter() {
                copy.insert(r[0], r.to_vec());
                order.push(r[0]);
            }
        } else {
            for r in d[1..].as_chunks::<7>().0.iter() {
                if r[1] < 0 {
                    copy.remove(&r[0]);
                } else {
                    copy.insert(r[0], r.to_vec());
                }
            }
        }
        let table = a.instances();
        let expected: Vec<Vec<i32>> = table
            .as_chunks::<7>()
            .0
            .iter()
            .map(|r| r.to_vec())
            .collect();
        let mut have: Vec<Vec<i32>> = copy.values().cloned().collect();
        have.sort_by_key(|r| r[0]);
        let mut want = expected.clone();
        want.sort_by_key(|r| r[0]);
        assert_eq!(have, want);
    };
    sync(&mut a);
    let add = |a: &mut Assembler, x: i32, y: i32| {
        a.apply(&format!(
            r#"{{"op":"add","part":"ob.plate-2x2","pos":[{x},{y},0],"rot":0,"colour":4}}"#
        ))
        .unwrap();
    };
    add(&mut a, 20, -8);
    add(&mut a, 60, -8);
    sync(&mut a);
    a.apply(r#"{"op":"recolour","id":1,"colour":15}"#).unwrap();
    sync(&mut a);
    a.apply(r#"{"op":"remove","id":1}"#).unwrap();
    sync(&mut a);
    a.undo().unwrap();
    sync(&mut a);
    a.undo().unwrap();
    a.undo().unwrap();
    sync(&mut a);
    a.redo().unwrap();
    sync(&mut a);
    add(&mut a, 100, -8);
    a.apply(r#"{"op":"clear"}"#).unwrap();
    let d = a.delta();
    assert_eq!(d[0], 0);
    assert!(
        d[1..].as_chunks::<7>().0.iter().all(|r| r[1] == -1),
        "a clear lists the parts gone: {d:?}"
    );
    // Nothing changed since: an empty delta.
    assert_eq!(a.delta(), vec![0]);
    // So many new parts that listing them costs more than the table: the whole table, mode 1.
    for n in 0..140 {
        add(&mut a, 40 * (n % 20), -8 * (1 + n / 20));
    }
    let d = a.delta();
    assert_eq!(d[0], 1);
    assert_eq!((d.len() - 1) / 7, 140);
}

#[test]
fn a_document_loads_in_steps_and_the_table_loads_from_rows() {
    let mut a = Assembler::create().unwrap();
    let text = assembler_sample("house").unwrap();
    let total = a.load_begin(&text).unwrap();
    assert_eq!(total, 49);
    assert!(
        a.is_empty(),
        "the model is untouched until the load finishes"
    );
    assert_eq!(a.load_step(10).unwrap(), 10);
    assert_eq!(a.load_step(1000).unwrap(), 49);
    a.load_finish().unwrap();
    assert_eq!(a.len(), 49);
    assert!(
        a.delta() == vec![0] || a.delta()[0] == 0,
        "an open leaves no change list"
    );
    let rows = a.instances();
    // A second assembler gets the same model from the table.
    let mut b = Assembler::create().unwrap();
    b.load_rows(&rows).unwrap();
    assert_eq!(b.save("x"), a.save("x"));
    assert_eq!(json_of(&b.stats()), json_of(&a.stats()));
    // Bad rows are refused with a message and change nothing.
    let mut bad = rows.clone();
    bad[5] = 99; // rotation
    let e = b.load_rows(&bad).unwrap_err();
    assert!(e.to_string().contains("rotation"), "{e}");
    assert!(b.load_rows(&rows[..6]).is_err());
    assert_eq!(b.len(), 49);
    // A cancelled load changes nothing, and a step without a load is an error.
    b.load_begin(&text).unwrap();
    b.load_cancel();
    assert!(b.load_step(1).is_err());
    assert_eq!(b.len(), 49);
    // A refused part ends the load.
    let broken =
        text.replacen("\"rot\": 0", "\"rot\": 77", 1)
            .replacen("\"rot\":0", "\"rot\":77", 1);
    let n = b.load_begin(&broken).unwrap();
    assert_eq!(n, 49);
    assert!(b.load_step(100).is_err());
    assert!(b.load_finish().is_err());
    assert_eq!(b.len(), 49);
}

#[test]
fn every_render_level_comes_from_the_core() {
    let hull = assembler_part_lod(4, 3).unwrap();
    assert_eq!(hull.triangle_count(), 12);
    let tris: Vec<u32> = (0..4)
        .map(|lod| assembler_part_lod(4, lod).unwrap().triangle_count())
        .collect();
    assert!(tris.windows(2).all(|w| w[0] > w[1]), "{tris:?}");
    assert!(assembler_part_lod(4, 4).is_err());
    // The levels never change what snaps and collides.
    let a = Assembler::create().unwrap();
    let before = a
        .snap("ob.brick-2x4", 0, 0.0, -300.0, 0.0, 0.0, 1.0, 0.0)
        .unwrap();
    let _ = assembler_part_lod(4, 3).unwrap();
    assert_eq!(
        before,
        a.snap("ob.brick-2x4", 0, 0.0, -300.0, 0.0, 0.0, 1.0, 0.0)
            .unwrap()
    );
}

/// Hostile input through the stepped open and through the table: refused with a message, quickly,
/// and the model on screen is untouched.
#[test]
fn hostile_documents_and_tables_are_refused_through_the_stepped_path_and_load_rows() {
    let mut a = Assembler::create().unwrap();
    let house = assembler_sample("house").unwrap();
    a.load(&house).unwrap();
    let before = a.save("x");
    let part = |id: u64, x: i64| {
        format!(
            r#"{{"id":{id},"part":"ob.plate-2x2","pose":{{"pos":[{x},-8,0],"rot":0}},"colour":4}}"#
        )
    };
    let doc = |instances: &str| {
        format!(
            r#"{{"openbrick":"1.0","kind":"model","name":"h","units":"ldu","frame":"ldraw-y-down","catalog":"openbrick-builtin@1","palette":[],"bom":[],"submodels":[{{"id":"main","instances":[{instances}]}}]}}"#
        )
    };
    let t = std::time::Instant::now();
    for (what, text, want) in [
        (
            "duplicate ids",
            doc(&format!("{},{}", part(1, 0), part(1, 40))),
            "used by two",
        ),
        ("id zero", doc(&part(0, 0)), "id 0"),
        ("far away", doc(&part(1, 2_000_000_000)), "beyond"),
        (
            "overlap",
            doc(&format!("{},{}", part(1, 0), part(2, 20))),
            "overlap",
        ),
    ] {
        let begun = a.load_begin(&text);
        let err = match begun {
            Err(e) => e,
            Ok(_) => a.load_step(1000).expect_err(what),
        };
        assert!(err.to_string().contains(want), "{what}: {err}");
        assert!(
            a.load_finish().is_err(),
            "{what}: nothing is left to finish"
        );
        assert_eq!(a.save("x"), before, "{what}: the model is untouched");
    }
    // Too many parts: refused before any is read.
    let many = (0..150_001u64)
        .map(|i| part(i + 1, 0))
        .collect::<Vec<_>>()
        .join(",");
    let err = a.load_begin(&doc(&many)).unwrap_err();
    assert!(err.to_string().contains("150000"), "{err}");
    // The same through the table.
    let ok_row = [1, 8, 0, -8, 0, 0, 4];
    let mut dup = ok_row.to_vec();
    dup.extend([1, 8, 40, -8, 0, 0, 4]);
    for (what, rows, want) in [
        ("duplicate ids", dup, "used by two"),
        ("id zero", vec![0, 8, 0, -8, 0, 0, 4], "id 0"),
        ("far away", vec![1, 8, 2_000_000_000, -8, 0, 0, 4], "beyond"),
        ("unknown part", vec![1, 9999, 0, -8, 0, 0, 4], "part"),
        ("unknown colour", vec![1, 8, 0, -8, 0, 0, 31337], "colour"),
    ] {
        let err = a.load_rows(&rows).expect_err(what);
        assert!(err.to_string().contains(want), "{what}: {err}");
        assert_eq!(a.save("x"), before, "{what}: the model is untouched");
    }
    assert!(t.elapsed().as_secs() < 30, "refused quickly");
}

/// The worst case the writer can produce for the most parts the engine holds still opens again: 150,000 parts with the
/// largest ids and coordinates saves to more than the old 16 MiB limit, and under the limit now.
#[test]
fn a_model_of_the_most_parts_with_the_longest_numbers_saves_and_opens_again() {
    use openbrick_wasm::assembler::MAX_MODEL_BYTES;
    let n = 150_000u64;
    let instances: Vec<String> = (0..n)
        .map(|i| {
            format!(
                r#"{{"id":{},"part":"ob.plate-1x1","pose":{{"pos":[{},-8,{}],"rot":0}},"colour":10000}}"#,
                4_294_967_295 - i,
                -99_999_990 + 20 * (i % 1000) as i64,
                -99_999_990 + 20 * (i / 1000) as i64
            )
        })
        .collect();
    let text = format!(
        r#"{{"openbrick":"1.0","kind":"model","name":"worst","units":"ldu","frame":"ldraw-y-down","catalog":"openbrick-builtin@1","palette":[],"bom":[],"submodels":[{{"id":"main","instances":[{}]}}]}}"#,
        instances.join(",")
    );
    let mut a = Assembler::create().unwrap();
    a.load(&text).unwrap();
    assert_eq!(a.len(), 150_000);
    let saved = a.save("worst");
    assert!(
        saved.len() > 16 * 1024 * 1024,
        "the old limit would have refused this file ({} bytes)",
        saved.len()
    );
    assert!(
        saved.len() <= MAX_MODEL_BYTES,
        "{} bytes against {MAX_MODEL_BYTES}",
        saved.len()
    );
    // The worst line of any part, times the most parts, is within the limit with room for the rest.
    assert!(
        openbrick_wasm::assembler::MAX_PART_TEXT_BYTES * 150_000 + 8 * 1024 * 1024
            <= MAX_MODEL_BYTES
    );
    let mut b = Assembler::create().unwrap();
    b.load(&saved).unwrap();
    assert_eq!(b.len(), 150_000);
    assert_eq!(b.save("worst"), saved, "byte for byte");
}
