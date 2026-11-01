//! The browser wizard and `openbrick calibrate record` write the same fit
//! profile (docs/plan/05 section 4, docs/plan/06 section 11).
//!
//! For each case the CLI runs as a user runs it, with its flags, and
//! `openbrick_wasm::record_fit` (the function the playground's worker calls,
//! run natively here) gets the same choices as the page sends them, in JSON.
//! The file the CLI writes and the `text` the wizard downloads must be
//! byte-identical: the same id derivation, the same rounding, the same date
//! (the wizard sends the date as given, the CLI is given `--date`), the same
//! defaults and the same encoding. Every case gives a date: without `--date`
//! the CLI writes today's, which WebAssembly cannot know, so the page always
//! sends its date field (today unless the person changes it). A choice the CLI refuses, the wizard refuses
//! with the same message, naming the field it concerns. Every cell value comes
//! from the coupon data, as the coupon's labels show it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use openbrick_print::coupon_v1;
use openbrick_wasm::record_fit;
use serde_json::{json, Value};

/// A fresh folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "openbrick-record-parity-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the scratch folder");
        Scratch(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn openbrick(dir: &Path, args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openbrick"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the openbrick binary runs")
}

/// The `i`-th value of a coupon sweep.
fn value(cell: &str, i: usize) -> f64 {
    coupon_v1().cell(cell).expect("a coupon cell").values()[i]
}

/// The pockets a pocket set prints, tube OD and rib depth.
fn pockets(set: &str) -> Vec<(f64, f64)> {
    coupon_v1().printed_pockets(set).expect("a pocket set")
}

/// A value as the coupon label shows it (two decimals), the way a person
/// types it on the command line.
fn label(v: f64) -> String {
    format!("{v:.2}")
}

/// The pocket set as the CLI spells it (`cross-and-diagonal`).
fn kebab(name: &str) -> String {
    name.chars()
        .flat_map(|c| {
            if c.is_ascii_uppercase() {
                vec!['-', c.to_ascii_lowercase()]
            } else {
                vec![c]
            }
        })
        .collect()
}

/// One set of choices: the wizard's JSON, and the CLI flags for the same.
struct Case {
    label: &'static str,
    wizard: Value,
}

impl Case {
    /// The CLI flags for the wizard's choices.
    fn flags(&self, out: &Path) -> Vec<String> {
        let w = &self.wizard;
        let mut args: Vec<String> = vec!["calibrate".into(), "record".into()];
        let mut flag = |name: &str, v: String| {
            args.push(format!("--{name}"));
            args.push(v);
        };
        for (key, name) in [
            ("process", "process"),
            ("printer", "printer"),
            ("material", "material"),
            ("brand", "brand"),
            ("slicer", "slicer"),
            ("intent", "intent"),
            ("id", "id"),
            ("author", "author"),
            ("date", "date"),
            ("notes", "notes"),
        ] {
            if let Some(v) = w[key].as_str() {
                flag(name, v.to_string());
            }
        }
        if let Some(n) = w["nozzleMm"].as_f64() {
            flag("nozzle", n.to_string());
        }
        if let Some(set) = w["pocketSet"].as_str() {
            flag("pocket-set", kebab(set));
        }
        let cells = &w["cells"];
        for (key, name) in [
            ("stud", "stud"),
            ("tube", "tube"),
            ("technicH", "technic-h"),
            ("pin", "pin"),
            ("axle", "axle"),
            ("bar", "bar"),
            ("clip", "clip"),
        ] {
            if let Some(v) = cells[key].as_f64() {
                flag(name, label(v));
            }
        }
        match (cells["pocketTubeOD"].as_f64(), cells["pocketRib"].as_f64()) {
            (Some(od), Some(rib)) => flag("pocket", format!("{}/{}", label(od), label(rib))),
            (Some(od), None) => flag("pocket-tube-od", label(od)),
            (None, Some(rib)) => flag("pocket-rib", label(rib)),
            (None, None) => {}
        }
        flag("out", out.to_string_lossy().into_owned());
        args
    }
}

fn cases() -> Vec<Case> {
    let cross = pockets("cross");
    let diagonal_only: Vec<(f64, f64)> = pockets("crossAndDiagonal")
        .into_iter()
        .filter(|p| !cross.contains(p))
        .collect();
    let (od, rib) = *cross.last().expect("a pocket");
    let (dod, drib) = diagonal_only[0];
    vec![
        Case {
            // The smoke test's choices (apps/web/smoke.mjs, native_eval).
            label: "wizard defaults with a pocket",
            wizard: json!({
                "process": "fdm-0.4",
                "pocketSet": "cross",
                "printer": "Prusa MINI+",
                "material": "PLA",
                "nozzleMm": 0.4,
                "date": "2026-11-12",
                "cells": {
                    "stud": value("stud", 4),
                    "tube": value("tube", 3),
                    "pocketTubeOD": od,
                    "pocketRib": rib,
                },
            }),
        },
        Case {
            label: "every text field, a diagonal pocket, fdm-0.2",
            wizard: json!({
                "process": "fdm-0.2",
                "pocketSet": "crossAndDiagonal",
                "printer": "Bambu Lab A1",
                "material": "PETG",
                "brand": "Prusament",
                "nozzleMm": 0.2,
                "slicer": "Bambu Studio 2.2",
                "intent": "fits-itself",
                "id": "my-a1-petg",
                "author": "A. Maker",
                "date": "2026-02-28",
                "notes": "0.12 mm layers, \"3 walls\"\nsecond line, Ünïcode",
                "cells": {
                    "stud": value("stud", 0),
                    "pocketTubeOD": dod,
                    "pocketRib": drib,
                },
            }),
        },
        Case {
            label: "no pocket, resin, a printer name to slug",
            wizard: json!({
                "process": "resin-msla",
                "printer": "Elegoo Saturn 4 Ultra",
                "material": "tough resin",
                "date": "2027-01-01",
                "cells": {
                    "tube": value("tube", 0),
                },
            }),
        },
        Case {
            label: "an empty note is written as given",
            wizard: json!({
                "process": "fdm-0.4",
                "printer": "Prusa XL",
                "material": "ASA",
                "notes": "",
                "date": "2026-10-31",
                "cells": { "stud": value("stud", 2), "tube": value("tube", 5) },
            }),
        },
    ]
}

#[test]
fn the_wizard_writes_the_cli_file_byte_for_byte() {
    let dir = Scratch::new("ok");
    for (i, case) in cases().iter().enumerate() {
        let out = dir.path(&format!("fit-{i}.json"));
        let o = openbrick(&dir.0, &case.flags(&out));
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}: {}",
            case.label,
            String::from_utf8_lossy(&o.stderr)
        );
        let cli = std::fs::read(&out).expect("the CLI wrote the profile");
        let wizard: Value =
            serde_json::from_str(&record_fit(&case.wizard.to_string()).expect("valid input"))
                .expect("JSON");
        assert_eq!(wizard["ok"], true, "{}: {wizard}", case.label);
        let text = wizard["text"].as_str().expect("the file text");
        assert_eq!(
            text.as_bytes(),
            cli.as_slice(),
            "{}: the wizard's file differs from the CLI's:\n{text}\n---\n{}",
            case.label,
            String::from_utf8_lossy(&cli)
        );
        let profile: Value = serde_json::from_slice(&cli).unwrap();
        assert_eq!(
            wizard["fileName"],
            format!("{}.json", profile["id"].as_str().unwrap()),
            "{}",
            case.label
        );
        assert_eq!(wizard["fitProfile"], profile, "{}", case.label);
    }
}

#[test]
fn the_wizard_refuses_what_the_cli_refuses() {
    let dir = Scratch::new("refused");
    let cross = pockets("cross");
    let unprinted = pockets("full")
        .into_iter()
        .find(|p| !cross.contains(p))
        .expect("the full cross has pockets the cross does not");
    let (od, _) = cross[0];
    let base = json!({
        "process": "fdm-0.4",
        "printer": "Prusa MINI+",
        "material": "PLA",
        "date": "2026-11-12",
    });
    let with = |cells: Value, extra: &[(&str, Value)]| {
        let mut w = base.clone();
        w["cells"] = cells;
        for (k, v) in extra {
            w[*k] = v.clone();
        }
        w
    };
    let refused = [
        (
            "a pocket the cross did not print",
            with(
                json!({ "pocketTubeOD": unprinted.0, "pocketRib": unprinted.1 }),
                &[("pocketSet", json!("cross"))],
            ),
            "pocket",
        ),
        (
            "half a pocket",
            with(json!({ "pocketTubeOD": od }), &[]),
            "pocket",
        ),
        (
            "an empty printer",
            with(
                json!({ "stud": value("stud", 1) }),
                &[("printer", json!(" "))],
            ),
            "printer",
        ),
        (
            "a date that is not one",
            with(
                json!({ "stud": value("stud", 1) }),
                &[("date", json!("2026-13-01"))],
            ),
            "date",
        ),
        (
            "an unknown process",
            with(
                json!({ "stud": value("stud", 1) }),
                &[("process", json!("fdm-04"))],
            ),
            "process",
        ),
    ];
    for (i, (label, wizard, field)) in refused.iter().enumerate() {
        let case = Case {
            label,
            wizard: wizard.clone(),
        };
        let out = dir.path(&format!("bad-{i}.json"));
        let o = openbrick(&dir.0, &case.flags(&out));
        assert_eq!(o.status.code(), Some(2), "{label}: the CLI accepted it");
        assert!(!out.exists(), "{label}: the CLI wrote a file");
        let answer: Value =
            serde_json::from_str(&record_fit(&wizard.to_string()).expect("valid input")).unwrap();
        assert_eq!(answer["ok"], false, "{label}: {answer}");
        assert_eq!(answer["error"]["field"], *field, "{label}: {answer}");
        let message = answer["error"]["message"].as_str().unwrap();
        let stderr = String::from_utf8_lossy(&o.stderr);
        assert!(
            stderr.contains(&format!("error: {message}")),
            "{label}: the CLI said {stderr}, the wizard {message}"
        );
    }
}
