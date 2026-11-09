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
    assert_eq!(j["report"]["counts"]["scanned"], 16);
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

/// A fake `curl` on the PATH that writes to the `--output` file forever (100 KB every 10 ms, 30 MB at
/// most) without ever stating a length, and records its arguments.
#[cfg(unix)]
fn fake_curl(bin: &Path, log: &Path, marker: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        r#"#!/bin/sh
echo "$@" > '{log}'
while [ $# -gt 0 ]; do
  case "$1" in --output) out="$2"; shift;; esac
  shift
done
i=0
while [ $i -lt 300 ]; do head -c 100000 /dev/zero >> "$out"; i=$((i+1)); sleep 0.01; done
touch '{marker}'
"#,
        log = log.display(),
        marker = marker.display()
    );
    let path = bin.join("curl");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn fetch_stops_a_download_that_never_says_how_long_it_is() {
    let d = dir("fetch-cap");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let (log, marker) = (d.join("args.txt"), d.join("finished"));
    fake_curl(&bin, &log, &marker);
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let dest = d.join("dl");
    let t = std::time::Instant::now();
    let o = Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .args(["catalog", "fetch", "--dest", dest.to_str().unwrap()])
        .args(["--url", "https://example.invalid/complete.zip"])
        .args(["--max-bytes", "1000000"])
        .env("PATH", path)
        .current_dir(repo())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o.stderr));
    let err = text(&o.stderr);
    assert!(
        err.contains("passed the 1000000-byte cap") && err.contains("stopped and deleted"),
        "{err}"
    );
    assert!(!marker.exists(), "the fake download was not stopped");
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    assert!(
        !dest.join("complete.zip.part").exists(),
        "the partial file is deleted"
    );
    assert!(!dest.join("complete.zip").exists());
    // Still https only, with curl's own cap too, and no shell: the arguments are curl's.
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(
        args.contains("--proto =https") && args.contains("--proto-redir =https"),
        "{args}"
    );
    assert!(args.contains("--max-filesize 1000000"), "{args}");
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn text_from_a_hostile_library_never_reaches_the_terminal_raw() {
    // A library whose one part carries an escape sequence and a right-to-left override in its
    // version tag, description and author (the tag is printed in the report and kept in the pack).
    let d = dir("hostile-lib");
    let lib = d.join("lib");
    std::fs::create_dir_all(lib.join("parts")).unwrap();
    std::fs::create_dir_all(lib.join("p")).unwrap();
    std::fs::write(
        lib.join("p/tri.dat"),
        "0 tri\n0 !LDRAW_ORG Primitive\n0 BFC CERTIFY CCW\n3 16 0 0 0 1 0 0 0 0 1\n",
    )
    .unwrap();
    std::fs::write(
        lib.join("parts/1001.dat"),
        "0 Brick \u{1b}[2J\u{1b}]0;owned\u{7} \u{202e}1 x 1\n0 Name: 1001.dat\n0 Author: \u{1b}[31mred\u{202e}\n0 !LDRAW_ORG Part UPDATE 20\u{1b}[31m26-01\u{202e}\n0 !LICENSE CC\n0 !CATEGORY Brick\n0 BFC CERTIFY CCW\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 tri.dat\n",
    )
    .unwrap();
    let out = d.join("h.obpack");
    let o = run(&[
        "catalog",
        "import",
        lib.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--include-unconnected",
    ]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let shown = format!("{}{}", text(&o.stdout), text(&o.stderr));
    let info = run(&["catalog", "info", out.to_str().unwrap()]);
    assert!(
        info.status.success(),
        "the pack reads back: {}",
        text(&info.stderr)
    );
    let shown = format!("{shown}{}{}", text(&info.stdout), text(&info.stderr));
    for bad in ['\u{1b}', '\u{7}', '\u{202e}', '\u{9b}'] {
        assert!(
            !shown.contains(bad),
            "{bad:?} reached the terminal: {shown:?}"
        );
    }
    assert!(
        shown.contains("library 20\u{FFFD}[31m26-01\u{FFFD}"),
        "{shown}"
    );
    // The pack itself holds printable text, so the page's palette gets nothing hostile either.
    let json = run(&["catalog", "info", out.to_str().unwrap(), "--json"]);
    let v: Value = serde_json::from_str(&text(&json.stdout)).unwrap();
    assert_eq!(v["library"], "20\u{FFFD}[31m26-01\u{FFFD}");
    std::fs::remove_dir_all(d).unwrap();
}
