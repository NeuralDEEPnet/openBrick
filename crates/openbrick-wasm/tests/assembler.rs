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
    assert_eq!(c["maxModelBytes"], 16 * 1024 * 1024);
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
