//! Builds the calibration coupon (docs/plan/05 section 4) and writes it as a 3MF,
//! a binary STL and a JSON manifest of the cell layout into a directory:
//!
//! ```text
//! cargo run --release -p openbrick-part --example coupon -- target/coupon
//! cargo run --release -p openbrick-part --example coupon -- --pockets full --bed 256x256 target/coupon
//! python3 -I scripts/validate-3mf.py target/coupon/coupon-fdm-0.4-cross.3mf
//! ```
//!
//! Options: `--process <id>` (a bundled process profile, default fdm-0.4),
//! `--pockets cross|cross-and-diagonal|full` (default cross), `--bed <x>x<y>` in
//! mm (default 180x180), `--copies <n>` (default 2). The files are named
//! `coupon-<process>-<pockets>.{3mf,stl,json}`. The manifest lists every cell
//! (row, copy, cell id, value, label, centre in mm and LDU), the sizes, the
//! print estimate, the content hash and the mesh fingerprints.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_io::{write_3mf, write_stl_binary, ExportMeta, ExportObject};
use openbrick_part::coupon::{build_coupon, CouponOptions, PocketSet};
use openbrick_print::default_process_profiles;
use serde_json::json;

const USAGE: &str = "usage: coupon [--process ID] [--pockets cross|cross-and-diagonal|full] [--bed XxY] [--copies N] OUT_DIR";

struct Args {
    process: String,
    opts: CouponOptions,
    pockets_name: String,
    out: PathBuf,
}

fn parse() -> Result<Args, String> {
    let mut process = "fdm-0.4".to_string();
    let mut opts = CouponOptions::default();
    let mut pockets_name = "cross".to_string();
    let mut out = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--process" => process = value()?,
            "--pockets" => {
                pockets_name = value()?;
                opts.pockets = match pockets_name.as_str() {
                    "cross" => PocketSet::Cross,
                    "cross-and-diagonal" => PocketSet::CrossAndDiagonal,
                    "full" => PocketSet::Full,
                    other => return Err(format!("unknown pocket set {other:?}")),
                };
            }
            "--bed" => {
                let v = value()?;
                let (x, y) = v.split_once('x').ok_or(format!("bad bed size {v:?}"))?;
                let num = |s: &str| {
                    s.parse::<f64>()
                        .map_err(|e| format!("bad bed size {v:?}: {e}"))
                };
                opts.bed_mm = [num(x)?, num(y)?];
            }
            "--copies" => {
                let v = value()?;
                opts.copies = v.parse().map_err(|e| format!("bad copies {v:?}: {e}"))?;
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => {
                if out.replace(PathBuf::from(s)).is_some() {
                    return Err("give one output directory".into());
                }
            }
        }
    }
    Ok(Args {
        process,
        opts,
        pockets_name,
        out: out.ok_or("give an output directory")?,
    })
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(process) = default_process_profiles()
        .into_iter()
        .find(|p| p.id == args.process)
    else {
        eprintln!("no bundled process profile {:?}", args.process);
        return ExitCode::from(2);
    };
    let kernel = ManifoldRust;
    let started = Instant::now();
    let coupon = match build_coupon(&kernel, &process, &args.opts) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let build_ms = started.elapsed().as_secs_f64() * 1e3;
    let s = &coupon.summary;
    let e = &coupon.estimate;
    let m = &coupon.built.measurements;
    let mesh = &coupon.built.objects[0].mesh;

    let stem = format!("coupon-{}-{}", process.id, args.pockets_name);
    let objects = [ExportObject::new("calibration coupon", "body", None, mesh)];
    let meta = ExportMeta {
        part_id: "openbrick.calibration-coupon".into(),
        version: format!("{}.{}.0", s.coupon_version, s.coupon_revision),
        content_hash: coupon.built.content_hash.clone(),
        // The coupon measures the printer and is never compensated.
        fit_profile_id: "none".into(),
        process_id: process.id.clone(),
        generator: format!(
            "{} {} (coupon example)",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ),
        kernel_id: coupon.built.kernel_id.clone(),
        title: format!(
            "Calibration coupon v{} ({}, {} pockets)",
            s.coupon_version, process.id, args.pockets_name
        ),
        serial: None,
    };
    let threemf = match write_3mf(&objects, &meta) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("3MF: {e}");
            return ExitCode::FAILURE;
        }
    };
    let stl = match write_stl_binary(&objects) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("STL: {e}");
            return ExitCode::FAILURE;
        }
    };
    let names = [
        format!("{stem}.3mf"),
        format!("{stem}.stl"),
        format!("{stem}.json"),
    ];
    let manifest = json!({
        "generator": "cargo run -p openbrick-part --example coupon",
        "files": { "threemf": names[0], "stl": names[1] },
        "kernel": coupon.built.kernel_id,
        "contentHash": coupon.built.content_hash,
        "fingerprint": format!("{:016x}", mesh.fingerprint()),
        "canonicalFingerprint": format!("{:016x}", mesh.canonical_fingerprint()),
        "triangles": mesh.triangle_count(),
        "manifold": m.is_manifold,
        "components": m.components,
        "genus": m.genus,
        "boundsMm": { "min": m.bounds_min_mm, "max": m.bounds_max_mm },
        "summary": s,
        "estimate": e,
        "warnings": coupon.built.warnings,
        "cells": coupon.cells,
    });
    let mut text = serde_json::to_string_pretty(&manifest).expect("manifest serialises");
    text.push('\n');

    if let Err(e) = std::fs::create_dir_all(&args.out) {
        eprintln!("cannot create {}: {e}", args.out.display());
        return ExitCode::FAILURE;
    }
    for (name, bytes) in names.iter().zip([threemf, stl, text.into_bytes()]) {
        let path = args.out.join(name);
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!("cannot write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("{:>9} bytes  {}", bytes.len(), path.display());
    }
    println!(
        "{} cells ({} lines per copy, {} copies, {:?}); footprint {:.1} x {:.1} mm, height {:.2} mm",
        coupon.cells.len(),
        s.rows_per_copy,
        s.copies,
        s.arrangement,
        s.footprint_mm[0],
        s.footprint_mm[1],
        s.height_mm
    );
    println!(
        "volume {:.0} mm3, {:.1} g PLA, about {:.0} min at {:.1} mm3/s; {} triangles; built in {build_ms:.0} ms ({})",
        e.volume_mm3,
        e.mass_g_pla,
        e.time_s / 60.0,
        e.flow_mm3_s,
        mesh.triangle_count(),
        coupon.built.kernel_id
    );
    ExitCode::SUCCESS
}
