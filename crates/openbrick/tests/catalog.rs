//! `openbrick catalog import | info | fetch` on the synthetic mini library of
//! `crates/openbrick-catalog/tests/fixtures/ldraw` (written for the tests; the
//! real library is never needed), the pack in `openbrick bom --pack`, and the
//! refusals of `fetch` that need no network.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture() -> PathBuf {
    repo().join("crates/openbrick-catalog/tests/fixtures/ldraw")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .args(args)
        .current_dir(repo())
        .output()
        .expect("the openbrick binary runs")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "openbrick-catalog-cli-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn import(out: &Path, extra: &[&str]) -> Output {
    let fixture = fixture();
    let mut args = vec![
        "catalog",
        "import",
        fixture.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--category-filter",
        "Brick,Plate,Tile,Technic",
        "--json",
    ];
    args.extend(extra);
    run(&args)
}

#[test]
fn import_writes_a_pack_and_reports_the_coverage() {
    let d = dir("import");
    let out = d.join("mini.obpack");
    let o = import(&out, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let j: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(j["ok"], true);
    assert_eq!(j["report"]["counts"]["scanned"], 17);
    assert_eq!(j["report"]["counts"]["imported"], 11);
    assert_eq!(j["report"]["excluded"]["minifig"], 1);
    assert!(out.exists());
    // The attribution travels in the file: the CLI says so and the pack carries the text.
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..8], b"OBPACK1\n");
    assert_eq!(j["bytes"], bytes.len());
    // Importing again refuses to overwrite, and --force replaces.
    let again = import(&out, &[]);
    assert_eq!(again.status.code(), Some(2));
    assert!(
        text(&again.stderr).contains("--force"),
        "{}",
        text(&again.stderr)
    );
    let forced = import(&out, &["--force"]);
    assert_eq!(forced.status.code(), Some(0));
    let j2: Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(
        j["fingerprint"], j2["fingerprint"],
        "the same library gives the same pack"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn info_prints_counts_and_coverage() {
    let d = dir("info");
    let out = d.join("mini.obpack");
    assert_eq!(import(&out, &[]).status.code(), Some(0));
    let o = run(&["catalog", "info", out.to_str().unwrap(), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let j: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(j["parts"], 11);
    assert_eq!(j["connectivity"]["derived"], 11);
    assert_eq!(j["connectivity"]["none"], 0);
    assert_eq!(j["withJoints"], 1);
    assert_eq!(j["library"], "2026-01");
    assert!(j["attribution"]
        .as_str()
        .unwrap()
        .contains("LDraw Parts Library"));
    let human = run(&["catalog", "info", out.to_str().unwrap()]);
    let t = text(&human.stdout);
    assert!(
        t.contains("11 parts") && t.contains("categories:") && t.contains("LDraw Parts Library"),
        "{t}"
    );
    // A file that is not a pack, and a damaged pack, are input errors (exit 2), not crashes.
    let junk = d.join("junk.obpack");
    std::fs::write(&junk, b"not a pack").unwrap();
    assert_eq!(
        run(&["catalog", "info", junk.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    let mut bytes = std::fs::read(&out).unwrap();
    bytes[30] ^= 0xFF;
    std::fs::write(&junk, bytes).unwrap();
    let o = run(&["catalog", "info", junk.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("checksum"), "{}", text(&o.stderr));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn import_of_a_missing_or_wrong_folder_is_an_input_error() {
    let d = dir("bad");
    let o = run(&[
        "catalog",
        "import",
        d.join("nowhere").to_str().unwrap(),
        "--out",
        d.join("x.obpack").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2));
    let o = run(&[
        "catalog",
        "import",
        d.to_str().unwrap(),
        "--out",
        d.join("x.obpack").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).contains("LDraw library"),
        "{}",
        text(&o.stderr)
    );
    let o = run(&[
        "catalog",
        "import",
        fixture().to_str().unwrap(),
        "--out",
        d.join("y.obpack").to_str().unwrap(),
        "--category-filter",
        "Nothing",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        !d.join("y.obpack").exists(),
        "nothing is written when nothing qualifies"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn fetch_refuses_before_it_touches_the_network() {
    let d = dir("fetch");
    // Not https.
    let o = run(&[
        "catalog",
        "fetch",
        "--dest",
        d.to_str().unwrap(),
        "--url",
        "http://example.invalid/complete.zip",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("https"), "{}", text(&o.stderr));
    // A bad hash.
    let o = run(&[
        "catalog",
        "fetch",
        "--dest",
        d.to_str().unwrap(),
        "--sha256",
        "abc",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).contains("64 hexadecimal"),
        "{}",
        text(&o.stderr)
    );
    // An existing complete.zip is never overwritten.
    std::fs::write(d.join("complete.zip"), b"precious").unwrap();
    let o = run(&["catalog", "fetch", "--dest", d.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).contains("not overwriting"),
        "{}",
        text(&o.stderr)
    );
    assert_eq!(std::fs::read(d.join("complete.zip")).unwrap(), b"precious");
    // A file kept aside by an earlier run is accepted when its hash is the one given, without the network.
    let e = dir("fetch-aside");
    std::fs::write(e.join("complete.zip.unverified"), b"hello").unwrap();
    // SHA-256 of "hello".
    let sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    let o = run(&[
        "catalog",
        "fetch",
        "--dest",
        e.to_str().unwrap(),
        "--sha256",
        sha,
        "--json",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let j: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(j["sha256"], sha);
    assert_eq!(std::fs::read(e.join("complete.zip")).unwrap(), b"hello");
    let _ = std::fs::remove_dir_all(d);
    let _ = std::fs::remove_dir_all(e);
}

#[test]
fn bom_reads_a_model_with_pack_parts_only_with_the_pack() {
    let d = dir("bom");
    let pack = d.join("mini.obpack");
    assert_eq!(import(&pack, &[]).status.code(), Some(0));
    // The pack's fingerprint goes into the model, as the assembler writes it.
    let info: Value =
        serde_json::from_slice(&run(&["catalog", "info", pack.to_str().unwrap(), "--json"]).stdout)
            .unwrap();
    let fp = info["fingerprint"].as_str().unwrap();
    let model = format!(
        r#"{{"openbrick":"1.0","kind":"model","name":"t","units":"ldu","frame":"ldraw-y-down",
        "catalog":"openbrick-builtin@1",
        "requires":[{{"pack":"ldraw","library":"2026-01","fingerprint":"{fp}"}}],
        "palette":[],"bom":[],
        "submodels":[{{"id":"main","instances":[
          {{"id":1,"part":"ld.3003","pose":{{"pos":[0,-24,0],"rot":0}},"colour":4}},
          {{"id":2,"part":"ld.3003","pose":{{"pos":[0,-48,0],"rot":0}},"colour":2}}]}}]}}"#
    );
    let f = d.join("m.obrick.json");
    std::fs::write(&f, model).unwrap();
    let without = run(&["bom", f.to_str().unwrap()]);
    assert_eq!(without.status.code(), Some(2));
    assert!(
        text(&without.stderr).contains("LDraw pack"),
        "{}",
        text(&without.stderr)
    );
    let with = run(&[
        "bom",
        f.to_str().unwrap(),
        "--pack",
        pack.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(with.status.code(), Some(0), "{}", text(&with.stderr));
    let j: Value = serde_json::from_slice(&with.stdout).unwrap();
    assert_eq!(j["stats"]["parts"], 2);
    assert_eq!(j["stats"]["connections"], 4);
    assert_eq!(j["bom"][0]["name"], "Brick 2 x 2");
    let _ = std::fs::remove_dir_all(d);
}
