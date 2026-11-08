//! `openbrick bom`: the bill of materials of an assembler model, its warning
//! for floating parts and its exit codes.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bom(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .arg("bom")
        .args(args)
        .current_dir(repo())
        .output()
        .expect("the openbrick binary runs")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// A model with one baseplate and one 1x1 brick; `y` is the brick's height.
fn model(y: i32) -> String {
    format!(
        r#"{{"openbrick":"1.0","kind":"model","name":"t","units":"ldu","frame":"ldraw-y-down",
        "catalog":"openbrick-builtin@1","palette":[],"bom":[],
        "submodels":[{{"id":"main","instances":[
          {{"id":1,"part":"ob.baseplate-16x16","pose":{{"pos":[0,-8,0],"rot":0}},"colour":2}},
          {{"id":2,"part":"ob.brick-1x1","pose":{{"pos":[10,{y},10],"rot":0}},"colour":4}}]}}]}}"#
    )
}

fn temp(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("openbrick-bom-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn the_sample_house_has_a_bill_of_materials() {
    let o = bom(&["models/house.obrick.json"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("49 parts"), "{out}");
    assert!(
        out.contains("Brick 1 x 4") && out.contains("Neuraldeep blue"),
        "{out}"
    );
    assert!(!text(&o.stderr).contains("FLOATING"));
}

#[test]
fn json_mode_prints_one_object_with_the_bom() {
    let o = bom(&["models/staircase.obrick.json", "--json"]);
    assert_eq!(o.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&o.stdout).expect("one JSON object");
    assert_eq!(v["command"], "bom");
    assert_eq!(v["stats"]["parts"], 8);
    let total: u64 = v["bom"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["count"].as_u64().unwrap())
        .sum();
    assert_eq!(total, 8);
}

#[test]
fn a_floating_part_is_a_warning_not_a_blocker() {
    let p = temp("floating.obrick", &model(-56));
    let o = bom(&[p.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0));
    let err = text(&o.stderr);
    assert!(
        err.contains("warning FLOATING") && err.contains("threshold") && err.contains("fix:"),
        "{err}"
    );
    // And a brick pressed on the baseplate is attached.
    let p = temp("attached.obrick", &model(-32));
    let o = bom(&[p.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0));
    assert!(!text(&o.stderr).contains("FLOATING"));
}

#[test]
fn invalid_and_missing_files_exit_2() {
    // A brick through the baseplate.
    let p = temp("overlap.obrick", &model(-24));
    let o = bom(&[p.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).contains("only touch"),
        "{}",
        text(&o.stderr)
    );
    let o = bom(&["nope.obrick"]);
    assert_eq!(o.status.code(), Some(2));
    let o = bom(&["nope.obrick", "--json"]);
    assert_eq!(o.status.code(), Some(2));
    let v: Value = serde_json::from_slice(&o.stdout).expect("one JSON object");
    assert_eq!(v["ok"], false);
}

/// A debug build of the binary (the test profile) panics on any unchecked overflow, so exit
/// codes here prove the reader refuses hostile numbers instead of computing with them.
#[test]
fn hostile_models_exit_2_or_0_never_3() {
    let one = |id: u64, pos: [i64; 3]| {
        format!(
            r#"{{"openbrick":"1.0","kind":"model","name":"t","units":"ldu","frame":"ldraw-y-down",
            "catalog":"openbrick-builtin@1","palette":[],"bom":[],
            "submodels":[{{"id":"main","instances":[
              {{"id":{id},"part":"ob.brick-1x1","pose":{{"pos":[{},{},{}],"rot":5}},"colour":4}}]}}]}}"#,
            pos[0], pos[1], pos[2]
        )
    };
    for (name, id, pos, want) in [
        ("id-zero", 0, [10, -24, 10], 2),
        ("id-max", 4_294_967_295, [10, -24, 10], 0),
        ("y-min", 1, [10, i64::from(i32::MIN), 10], 2),
        ("x-max", 1, [i64::from(i32::MAX), -24, 10], 2),
        ("x-beyond", 1, [100_000_010, -24, 10], 2),
        ("z-neg-beyond", 1, [10, -24, -100_000_010], 2),
    ] {
        let p = temp(&format!("hostile-{name}.obrick"), &one(id, pos));
        let o = bom(&[p.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(want), "{name}: {}", text(&o.stderr));
    }
}
