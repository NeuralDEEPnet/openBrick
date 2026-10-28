//! The device render harness (backlog item 3 in docs/plan/14) loads brick meshes
//! that `examples/export_lods.rs` writes into `bench/harness/public/meshes`. These
//! tests rebuild them with the kernel and compare with the committed files, so a
//! kernel or standard change cannot leave the harness meshes stale unnoticed.
//!
//! The example is included as a module so both use the same `generate()`.

// main() belongs to the example binary and is not called here.
#[allow(dead_code)]
#[path = "../examples/export_lods.rs"]
mod export_lods;

use export_lods::{default_dir, generate, parse_args, Command};
use serde_json::Value;

/// Regenerate in memory and read the committed file of the same name.
fn regenerated_and_committed() -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let dir = default_dir();
    generate()
        .into_iter()
        .map(|(name, fresh)| {
            let path = dir.join(&name);
            let committed = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "{} is missing ({e}); regenerate with: cargo run --release -p openbrick-geom --example export_lods",
                    path.display()
                )
            });
            (name, fresh, committed)
        })
        .collect()
}

/// Byte-for-byte equality. The Q01 spike verified bit-identical kernel output on
/// x86-64 Linux (native debug, native release, wasm32); the committed files were
/// written there. Polygon::circle uses the platform's sin and cos, which other C
/// libraries may round differently in the last bit, so elsewhere only the
/// structural test below runs.
#[test]
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux")),
    ignore = "bit-exact output is verified on x86-64 Linux only (docs/spikes/q01-kernel-conformance.md)"
)]
fn committed_harness_meshes_are_byte_identical_to_the_kernel_output() {
    for (name, fresh, committed) in regenerated_and_committed() {
        assert!(
            fresh == committed,
            "bench/harness/public/meshes/{name} differs from the kernel output; regenerate with: cargo run --release -p openbrick-geom --example export_lods"
        );
    }
}

/// Drop the values that depend on the last bits of floating point (fingerprints,
/// micrometre-rounded volumes and bounds) and return them separately.
fn split_float_fields(manifest: &mut Value) -> Vec<(String, f64)> {
    let mut floats = Vec::new();
    for lod in manifest["lods"].as_array_mut().expect("lods array") {
        let level = lod["lod"].as_u64().expect("lod number");
        let lod = lod.as_object_mut().expect("lod object");
        lod.remove("fingerprint").expect("fingerprint");
        let volume = lod.remove("volume_mm3").expect("volume_mm3");
        floats.push((format!("LOD{level} volume_mm3"), volume.as_f64().unwrap()));
        let bounds = lod.remove("bounds_mm").expect("bounds_mm");
        for side in ["min", "max"] {
            for (axis, v) in bounds[side].as_array().unwrap().iter().enumerate() {
                floats.push((
                    format!("LOD{level} bounds {side}[{axis}]"),
                    v.as_f64().unwrap(),
                ));
            }
        }
    }
    floats
}

/// On every platform: same files, sizes, vertex and triangle counts and dimensions,
/// and volumes and bounds within one micrometre rounding step.
#[test]
fn committed_harness_meshes_match_the_kernel_structure() {
    let files = regenerated_and_committed();
    let names: Vec<&str> = files.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "brick-2x4-lod0.bin",
            "brick-2x4-lod1.bin",
            "brick-2x4-lod2.bin",
            "manifest.json"
        ]
    );
    for (name, fresh, committed) in &files[..3] {
        assert_eq!(fresh.len(), committed.len(), "{name}: size");
        assert_eq!(fresh[..32], committed[..32], "{name}: OBMESH header");
    }
    let (_, fresh, committed) = &files[3];
    let mut fresh: Value = serde_json::from_slice(fresh).expect("fresh manifest");
    let mut committed: Value = serde_json::from_slice(committed).expect("committed manifest");
    let fresh_floats = split_float_fields(&mut fresh);
    let committed_floats = split_float_fields(&mut committed);
    assert_eq!(
        fresh, committed,
        "manifest without fingerprints, volumes and bounds"
    );
    for ((what, a), (_, b)) in fresh_floats.iter().zip(&committed_floats) {
        // Both are rounded to 0.001 mm (mm³), so one last-bit difference before
        // rounding can move a value by one step.
        assert!((a - b).abs() <= 1.5e-3, "{what}: {a} vs committed {b}");
    }
}

#[test]
fn export_lods_rejects_unknown_options() {
    let args = |list: &[&str]| parse_args(list.iter().map(|s| s.to_string()));
    assert_eq!(
        args(&[]),
        Ok(Command::Run {
            check: false,
            dir: default_dir()
        })
    );
    assert_eq!(
        args(&["--check", "out"]),
        Ok(Command::Run {
            check: true,
            dir: "out".into()
        })
    );
    assert_eq!(args(&["--help"]), Ok(Command::Help));
    // A typo must not turn into an output directory named "--chek".
    let err = args(&["--chek"]).unwrap_err();
    assert!(err.starts_with("unknown option --chek"), "{err}");
    assert!(args(&["-c"]).is_err());
    assert!(args(&["a", "b"])
        .unwrap_err()
        .starts_with("more than one output directory"));
    assert_eq!(
        args(&["./-odd"]),
        Ok(Command::Run {
            check: false,
            dir: "./-odd".into()
        })
    );
}
