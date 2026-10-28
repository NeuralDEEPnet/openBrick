//! Exports a nominal 2x4 brick at three levels of detail for the device render
//! harness (`bench/harness`, backlog item 3 in docs/plan/14, decision D10 in
//! docs/plan/03, rendering budgets in docs/plan/07 section 9).
//!
//! The harness never builds geometry itself (CLAUDE.md: UI packages call the core);
//! it loads the files written here.
//!
//! - LOD0: the full brick, as in `kernel_timing.rs`: shell, roof, studs and tubes
//!   with 64-segment cylinders.
//! - LOD1: shell, roof and 16-segment studs, no tubes (underside detail is
//!   invisible from typical views).
//! - LOD2: the stud-less hull box of the body.
//!
//! Every dimension comes from `standards/dimensions-v1.json` through
//! `openbrick-units`; the sources of each number are cited in that file.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run --release -p openbrick-geom --example export_lods            # write
//! cargo run --release -p openbrick-geom --example export_lods -- --check # verify
//! ```
//!
//! An optional directory argument replaces the default output directory
//! `bench/harness/public/meshes`. `--check` rebuilds everything in memory and
//! exits with status 1 if a committed file differs from the kernel output.
//!
//! # Binary mesh format (`.bin`, "OBMESH" version 1), all little-endian
//!
//! | offset | type        | field                                              |
//! |--------|-------------|----------------------------------------------------|
//! | 0      | 8 bytes     | magic `OBMESH\0\0`                                 |
//! | 8      | u32         | format version (1)                                 |
//! | 12     | u32         | vertex count V                                     |
//! | 16     | u32         | index count I (3 per triangle)                     |
//! | 20     | u32         | position units: 1 = millimetres, 2 = metres        |
//! | 24     | u32         | up axis: 0 = +X, 1 = +Y, 2 = +Z                    |
//! | 28     | u32         | reserved (0)                                       |
//! | 32     | f32 x 3V    | positions x, y, z                                  |
//! | 32+12V | u32 x I     | triangle indices, counter-clockwise seen from outside |
//!
//! The header is 32 bytes so both arrays start on a 4-byte boundary and can be
//! viewed in place as `Float32Array` and `Uint32Array`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use openbrick_geom::manifold_rust::{ManifoldRust, MrSolid};
use openbrick_geom::{CrossSection, Kernel, Mesh, Polygon, Solid};
use openbrick_units::STANDARD;
use serde_json::json;

const MAGIC: &[u8; 8] = b"OBMESH\0\0";
const FORMAT_VERSION: u32 = 1;
const HEADER_BYTES: usize = 32;
const UNITS_MM: u32 = 1;
const UP_Z: u32 = 2;
/// Footprint of the exported part in studs (x, y).
const STUDS: [u32; 2] = [2, 4];

/// Nominal dimensions read once from the standard.
struct Dims {
    pitch: f64,
    gap: f64,
    wall: f64,
    roof: f64,
    height: f64,
    stud_d: f64,
    stud_h: f64,
    tube_od: f64,
    tube_id: f64,
}

impl Dims {
    fn from_standard() -> Dims {
        let s = STANDARD.get();
        Dims {
            pitch: s.mm("module"),
            gap: s.mm("undersize_per_side"),
            wall: s.mm("side_wall"),
            roof: s.mm("roof"),
            height: s.mm("brick_height"),
            stud_d: s.mm("stud_diameter"),
            stud_h: s.mm("stud_height"),
            tube_od: s.mm("tube_outer_diameter"),
            tube_id: s.mm("tube_inner_diameter"),
        }
    }

    /// Body size (x, y) of an n x m brick: n studs minus the undersize on both sides.
    fn body(&self) -> [f64; 2] {
        [
            STUDS[0] as f64 * self.pitch - 2.0 * self.gap,
            STUDS[1] as f64 * self.pitch - 2.0 * self.gap,
        ]
    }
}

/// One level of detail.
struct Lod {
    level: u32,
    description: &'static str,
    /// Cylinder segments of the studs, or `None` for a stud-less hull.
    stud_segments: Option<u32>,
    tubes: bool,
}

const LODS: [Lod; 3] = [
    Lod {
        level: 0,
        description: "full brick: shell, roof, 64-segment studs and tubes",
        stud_segments: Some(64),
        tubes: true,
    },
    Lod {
        level: 1,
        description: "shell, roof and 16-segment studs; no tubes (underside detail is invisible from typical views)",
        stud_segments: Some(16),
        tubes: false,
    },
    Lod {
        level: 2,
        description: "stud-less hull box of the body",
        stud_segments: None,
        tubes: false,
    },
];

/// Build one LOD. LOD0 repeats the construction of `kernel_timing.rs` step for step,
/// so its fingerprint equals the 2x4 row printed there.
fn build(k: &ManifoldRust, d: &Dims, lod: &Lod) -> MrSolid {
    let [w, depth] = d.body();
    let outer = Polygon::rect([0.0, 0.0], [w, depth]);
    let Some(segments) = lod.stud_segments else {
        return k
            .extrude(&CrossSection::from(vec![outer]), d.height, 1.0)
            .expect("hull box");
    };
    let cavity = d.height - d.roof;
    let inner =
        Polygon::rect([d.wall, d.wall], [w - 2.0 * d.wall, depth - 2.0 * d.wall]).reversed();
    // Keep the creation order of kernel_timing.rs (stud, tube, shell, roof): the
    // vertex order of a batch union follows the order in which the kernel created
    // its inputs, so a different order gives the same solid with another fingerprint.
    let stud = k
        .extrude(
            &CrossSection::from(vec![Polygon::circle([0.0, 0.0], d.stud_d / 2.0, segments)]),
            d.stud_h,
            1.0,
        )
        .expect("stud");
    // Tubes sit between stud columns; a 2-wide brick has one row of them.
    let tube = lod.tubes.then(|| {
        k.extrude(
            &CrossSection::from(vec![
                Polygon::circle([0.0, 0.0], d.tube_od / 2.0, segments),
                Polygon::circle([0.0, 0.0], d.tube_id / 2.0, segments).reversed(),
            ]),
            cavity,
            1.0,
        )
        .expect("tube")
    });
    let mut parts = vec![
        k.extrude(&CrossSection::from(vec![outer.clone(), inner]), cavity, 1.0)
            .expect("shell"),
        k.translate(
            &k.extrude(&CrossSection::from(vec![outer]), d.roof, 1.0)
                .expect("roof"),
            [0.0, 0.0, cavity],
        ),
    ];
    for ix in 0..STUDS[0] {
        for iy in 0..STUDS[1] {
            parts.push(k.translate(
                &stud,
                [
                    ix as f64 * d.pitch + d.pitch / 2.0 - d.gap,
                    iy as f64 * d.pitch + d.pitch / 2.0 - d.gap,
                    d.height,
                ],
            ));
        }
    }
    if let Some(tube) = tube {
        for iy in 1..STUDS[1] {
            parts.push(k.translate(&tube, [d.pitch - d.gap, iy as f64 * d.pitch - d.gap, 0.0]));
        }
    }
    k.union_all(&parts).expect("brick union")
}

/// Serialise a mesh in the OBMESH v1 layout documented at the top of this file.
fn encode(mesh: &Mesh) -> Vec<u8> {
    let v = mesh.positions.len();
    let i = mesh.indices.len();
    let mut out = Vec::with_capacity(HEADER_BYTES + 12 * v + 4 * i);
    out.extend_from_slice(MAGIC);
    for word in [
        FORMAT_VERSION,
        u32::try_from(v).expect("vertex count fits u32"),
        u32::try_from(i).expect("index count fits u32"),
        UNITS_MM,
        UP_Z,
        0,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    debug_assert_eq!(out.len(), HEADER_BYTES);
    for p in &mesh.positions {
        for c in p {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
    for idx in &mesh.indices {
        out.extend_from_slice(&idx.to_le_bytes());
    }
    out
}

/// Round to micrometres for the manifest so it stays readable and stable.
fn um(x: f64) -> f64 {
    (x * 1e3).round() / 1e3
}

fn bounds_json(mesh: &Mesh) -> serde_json::Value {
    let (lo, hi) = mesh.bounds();
    json!({
        "min": lo.map(|c| um(c as f64)),
        "max": hi.map(|c| um(c as f64)),
    })
}

/// Build every LOD and return (file name, bytes) pairs, manifest last.
fn generate() -> Vec<(String, Vec<u8>)> {
    let s = STANDARD.get();
    let d = Dims::from_standard();
    let k = ManifoldRust;
    let mut files = Vec::new();
    let mut lods = Vec::new();
    for lod in &LODS {
        let solid = build(&k, &d, lod);
        assert!(solid.is_manifold(), "LOD{} is not manifold", lod.level);
        let mesh = solid.to_mesh();
        let name = format!("brick-{}x{}-lod{}.bin", STUDS[0], STUDS[1], lod.level);
        let bytes = encode(&mesh);
        lods.push(json!({
            "lod": lod.level,
            "file": name,
            "description": lod.description,
            "stud_segments": lod.stud_segments,
            "tubes": lod.tubes,
            "vertices": mesh.positions.len(),
            "triangles": mesh.triangle_count(),
            "bytes": bytes.len(),
            "fingerprint": format!("0x{:016x}", mesh.fingerprint()),
            "volume_mm3": um(solid.volume_mm3()),
            "bounds_mm": bounds_json(&mesh),
        }));
        files.push((name, bytes));
    }
    let body = d.body();
    let manifest = json!({
        "format": "OBMESH",
        "format_version": FORMAT_VERSION,
        "generator": "cargo run --release -p openbrick-geom --example export_lods",
        "kernel": k.id(),
        "standard": format!("dimensions-v1 {}", s.version),
        "part": format!("brick {}x{}", STUDS[0], STUDS[1]),
        "units": "mm",
        "frame": "kernel frame: right-handed, +Z up; body minimum corner at the origin, body from z = 0 to brick_height_mm, studs on top",
        "to_threejs": "three.js is right-handed with +Y up: (x, y, z) -> (x, z, -y), a -90 degree rotation about +X (determinant +1, so triangle winding is kept)",
        "studs": STUDS,
        "pitch_mm": d.pitch,
        "undersize_per_side_mm": d.gap,
        "brick_height_mm": d.height,
        "stud_height_mm": d.stud_h,
        "body_mm": [um(body[0]), um(body[1]), um(d.height)],
        "lods": lods,
    });
    let mut text = serde_json::to_string_pretty(&manifest).expect("manifest serialises");
    text.push('\n');
    files.push(("manifest.json".to_string(), text.into_bytes()));
    files
}

/// `bench/harness/public/meshes` under the workspace root (two levels above this crate).
fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the workspace root")
        .join("bench/harness/public/meshes")
}

fn main() -> ExitCode {
    let mut check = false;
    let mut dir = default_dir();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "-h" | "--help" => {
                println!("usage: export_lods [--check] [output-dir]");
                return ExitCode::SUCCESS;
            }
            other => dir = PathBuf::from(other),
        }
    }
    let files = generate();
    let mut stale = 0;
    for (name, bytes) in &files {
        let path = dir.join(name);
        if check {
            match std::fs::read(&path) {
                Ok(old) if old == *bytes => println!("ok      {}", path.display()),
                Ok(_) => {
                    eprintln!("differs {}", path.display());
                    stale += 1;
                }
                Err(e) => {
                    eprintln!("missing {} ({e})", path.display());
                    stale += 1;
                }
            }
        } else {
            std::fs::create_dir_all(&dir).expect("create output directory");
            std::fs::write(&path, bytes).expect("write mesh file");
            println!("wrote   {} ({} bytes)", path.display(), bytes.len());
        }
    }
    if stale > 0 {
        eprintln!(
            "{stale} file(s) out of date; regenerate with: cargo run --release -p openbrick-geom --example export_lods"
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
