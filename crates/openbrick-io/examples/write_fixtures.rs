//! Writes the 3MF and STL fixtures that CI checks with lib3mf, the reference 3MF
//! implementation (`scripts/validate-3mf.py`; backlog items 12 and 13 in
//! docs/plan/14, decision D18 in docs/plan/03).
//!
//! Fixtures:
//!
//! - `brick-2x4-with-cube`: a nominal 2x4 brick built with the default kernel
//!   (role `body`) and, one module to its side, a cube as wide as a stud (role
//!   `accent`). Written as a 3MF, one merged STL and one STL per role. The brick is
//!   built exactly as LOD0 of `crates/openbrick-geom/examples/export_lods.rs`, so
//!   its mesh fingerprint equals the one in `bench/harness/public/meshes/manifest.json`.
//! - `three-roles`: hand-built solids, one per role (a cube as `body`, two
//!   tetrahedra as `accent` and `serial`, the last without a colour), with a
//!   serial number in the metadata.
//! - `fixtures.json`: what every file must contain (objects, counts, colours,
//!   volumes, bounds, stored mesh fingerprints, Production UUIDs and metadata),
//!   which the validator compares with what lib3mf reads.
//!
//! Every brick dimension comes from `standards/dimensions-v1.json` through
//! `openbrick-units`. Run from the repository root:
//!
//! ```text
//! cargo run -p openbrick-io --example write_fixtures                 # target/io-fixtures
//! cargo run -p openbrick-io --example write_fixtures -- some/dir
//! python3 -I scripts/validate-3mf.py target/io-fixtures/fixtures.json
//! ```
//!
//! `tests/fixtures.rs` includes this file as a module and checks the same files
//! with independent Rust readers.

use std::path::PathBuf;
use std::process::ExitCode;

use openbrick_geom::manifold_rust::{ManifoldRust, MrSolid};
use openbrick_geom::{CrossSection, Kernel, Mesh, Polygon, Solid};
use openbrick_io::threemf::{package_uuids, stored_fingerprint, OPENBRICK_NAMESPACE};
use openbrick_io::{
    write_3mf, write_stl_binary, write_stl_binary_per_role, ExportMeta, ExportObject, GENERATOR,
};
use openbrick_units::STANDARD;
use serde_json::json;

const USAGE: &str = "usage: write_fixtures [output-dir]";
/// Footprint of the brick in studs (x, y).
pub const STUDS: [u32; 2] = [2, 4];
// `brick` places one row of tubes between the two stud columns, which is the
// layout of a 2-wide brick only.
const _: () = assert!(STUDS[0] == 2, "brick() builds 2-wide bricks only");
/// Display colours (sRGB, alpha) chosen for the fixtures; any distinct pair works.
const BODY_COLOUR: [u8; 4] = [0xD0, 0x30, 0x20, 0xFF];
const ACCENT_COLOUR: [u8; 4] = [0xF2, 0xCD, 0x37, 0xFF];

/// One object of a fixture, owning its mesh.
pub struct Part {
    pub name: String,
    pub role: String,
    pub colour: Option<[u8; 4]>,
    pub mesh: Mesh,
}

/// One fixture: the objects of one export and its metadata.
pub struct Fixture {
    pub name: &'static str,
    pub parts: Vec<Part>,
    pub meta: ExportMeta,
}

impl Fixture {
    pub fn objects(&self) -> Vec<ExportObject<'_>> {
        self.parts
            .iter()
            .map(|p| ExportObject::new(p.name.clone(), p.role.clone(), p.colour, &p.mesh))
            .collect()
    }
}

/// The nominal 2x4 brick, built step for step as LOD0 of export_lods.rs (stud,
/// tube, shell, roof, then the placed copies): the creation order decides the
/// vertex order, and with it the fingerprint.
fn brick(k: &ManifoldRust) -> MrSolid {
    let s = STANDARD.get();
    let (pitch, gap, wall) = (
        s.mm("module"),
        s.mm("undersize_per_side"),
        s.mm("side_wall"),
    );
    let (roof, height) = (s.mm("roof"), s.mm("brick_height"));
    let segments = 64;
    let w = STUDS[0] as f64 * pitch - 2.0 * gap;
    let depth = STUDS[1] as f64 * pitch - 2.0 * gap;
    let cavity = height - roof;
    let outer = Polygon::rect([0.0, 0.0], [w, depth]);
    let inner = Polygon::rect([wall, wall], [w - 2.0 * wall, depth - 2.0 * wall]).reversed();
    let stud = k
        .extrude(
            &CrossSection::from(vec![Polygon::circle(
                [0.0, 0.0],
                s.mm("stud_diameter") / 2.0,
                segments,
            )]),
            s.mm("stud_height"),
            1.0,
        )
        .expect("stud");
    let tube = k
        .extrude(
            &CrossSection::from(vec![
                Polygon::circle([0.0, 0.0], s.mm("tube_outer_diameter") / 2.0, segments),
                Polygon::circle([0.0, 0.0], s.mm("tube_inner_diameter") / 2.0, segments).reversed(),
            ]),
            cavity,
            1.0,
        )
        .expect("tube");
    let mut parts = vec![
        k.extrude(&CrossSection::from(vec![outer.clone(), inner]), cavity, 1.0)
            .expect("shell"),
        k.translate(
            &k.extrude(&CrossSection::from(vec![outer]), roof, 1.0)
                .expect("roof"),
            [0.0, 0.0, cavity],
        ),
    ];
    for ix in 0..STUDS[0] {
        for iy in 0..STUDS[1] {
            parts.push(k.translate(
                &stud,
                [
                    ix as f64 * pitch + pitch / 2.0 - gap,
                    iy as f64 * pitch + pitch / 2.0 - gap,
                    height,
                ],
            ));
        }
    }
    // Tubes sit between stud columns; a 2-wide brick has one row of them.
    for iy in 1..STUDS[1] {
        parts.push(k.translate(&tube, [pitch - gap, iy as f64 * pitch - gap, 0.0]));
    }
    k.union_all(&parts).expect("brick union")
}

/// A cube as wide as a stud, one module beyond the brick's +x face.
fn accent_cube(k: &ManifoldRust) -> MrSolid {
    let s = STANDARD.get();
    let side = s.mm("stud_diameter");
    let x = STUDS[0] as f64 * s.mm("module") - 2.0 * s.mm("undersize_per_side") + s.mm("module");
    k.extrude(
        &CrossSection::from(vec![Polygon::rect([x, 0.0], [side, side])]),
        side,
        1.0,
    )
    .expect("cube")
}

/// Abstract size of the hand-built solids in millimetres; not a brick dimension.
const UNIT: f32 = 10.0;

/// Axis-aligned cube with outward, counter-clockwise triangles.
fn cube_mesh(origin: [f32; 3], side: f32) -> Mesh {
    let [x, y, z] = origin;
    let (a, b, c) = (x + side, y + side, z + side);
    Mesh {
        positions: vec![
            [x, y, z],
            [a, y, z],
            [a, b, z],
            [x, b, z],
            [x, y, c],
            [a, y, c],
            [a, b, c],
            [x, b, c],
        ],
        indices: vec![
            0, 2, 1, 0, 3, 2, // bottom, -z
            4, 5, 6, 4, 6, 7, // top, +z
            0, 1, 5, 0, 5, 4, // front, -y
            3, 7, 6, 3, 6, 2, // back, +y
            0, 4, 7, 0, 7, 3, // left, -x
            1, 2, 6, 1, 6, 5, // right, +x
        ],
    }
}

/// Right-angled tetrahedron with outward, counter-clockwise triangles.
fn tetra_mesh(origin: [f32; 3], side: f32) -> Mesh {
    let [x, y, z] = origin;
    Mesh {
        positions: vec![
            [x, y, z],
            [x + side, y, z],
            [x, y + side, z],
            [x, y, z + side],
        ],
        indices: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2],
    }
}

/// Signed volume by the divergence theorem, in f64 from the f32 positions. Exact
/// for closed meshes up to rounding.
pub fn mesh_volume(m: &Mesh) -> f64 {
    let p = |i: u32| m.positions[i as usize].map(f64::from);
    let (triangles, _) = m.indices.as_chunks::<3>();
    triangles
        .iter()
        .map(|&[i, j, k]| {
            let (a, b, c) = (p(i), p(j), p(k));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

/// Both fixtures, with their meshes.
pub fn fixtures() -> Vec<Fixture> {
    let k = ManifoldRust;
    let body = brick(&k);
    let cube = accent_cube(&k);
    assert!(body.is_manifold() && cube.is_manifold());
    let (body, cube) = (body.to_mesh(), cube.to_mesh());
    // A stand-in for the part document hash, which the evaluator will provide:
    // the canonical fingerprints of both meshes, labelled as such.
    let stand_in_hash = format!(
        "fixture-canonical-fnv1a64:{:016x}-{:016x}",
        body.canonical_fingerprint(),
        cube.canonical_fingerprint()
    );
    let brick_fixture = Fixture {
        name: "brick-2x4-with-cube",
        parts: vec![
            Part {
                name: "2x4 brick".into(),
                role: "body".into(),
                colour: Some(BODY_COLOUR),
                mesh: body,
            },
            Part {
                name: "accent cube".into(),
                role: "accent".into(),
                colour: Some(ACCENT_COLOUR),
                mesh: cube,
            },
        ],
        meta: ExportMeta {
            part_id: "fixture.brick-2x4-with-cube".into(),
            version: "0.1.0".into(),
            content_hash: stand_in_hash,
            // Nominal geometry: no fit offsets are applied.
            fit_profile_id: "nominal".into(),
            process_id: "fdm-0.4".into(),
            generator: GENERATOR.into(),
            kernel_id: k.id(),
            title: "2x4 brick with an accent cube (fixture)".into(),
            serial: None,
        },
    };
    let three_roles = Fixture {
        name: "three-roles",
        parts: vec![
            Part {
                name: "cube".into(),
                role: "body".into(),
                colour: Some(BODY_COLOUR),
                mesh: cube_mesh([0.0; 3], UNIT),
            },
            Part {
                name: "tetrahedron".into(),
                role: "accent".into(),
                colour: Some(ACCENT_COLOUR),
                mesh: tetra_mesh([2.0 * UNIT, 0.0, 0.0], UNIT),
            },
            Part {
                name: "serial tag".into(),
                role: "serial".into(),
                colour: None,
                mesh: tetra_mesh([4.0 * UNIT, 0.0, 0.0], UNIT / 2.0),
            },
        ],
        meta: ExportMeta {
            part_id: "fixture.three-roles".into(),
            version: "0.1.0".into(),
            content_hash: "fixture:hand-built-solids-v1".into(),
            fit_profile_id: "nominal".into(),
            process_id: "fdm-0.4".into(),
            generator: GENERATOR.into(),
            kernel_id: "none (hand-built meshes)".into(),
            title: "Cube, tetrahedron & serial tag <fixture>".into(),
            serial: Some("NDN-0001".into()),
        },
    };
    vec![brick_fixture, three_roles]
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
}

/// Expected model metadata, as (name, value) with the `openbrick:` prefix.
fn expected_metadata(meta: &ExportMeta) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    if !meta.title.is_empty() {
        m.insert("Title".into(), json!(meta.title));
    }
    m.insert("Application".into(), json!(meta.generator));
    m.insert("openbrick:partId".into(), json!(meta.part_id));
    m.insert("openbrick:version".into(), json!(meta.version));
    m.insert("openbrick:contentHash".into(), json!(meta.content_hash));
    m.insert("openbrick:fitProfile".into(), json!(meta.fit_profile_id));
    m.insert("openbrick:process".into(), json!(meta.process_id));
    m.insert("openbrick:generator".into(), json!(meta.generator));
    m.insert("openbrick:kernel".into(), json!(meta.kernel_id));
    if let Some(serial) = &meta.serial {
        m.insert("openbrick:serial".into(), json!(serial));
    }
    serde_json::Value::Object(m)
}

/// Write every fixture in memory and return (file name, bytes), manifest last.
pub fn generate() -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for f in fixtures() {
        let objects = f.objects();
        let uuids = package_uuids(&objects, &f.meta);
        let threemf = format!("{}.3mf", f.name);
        let stl = format!("{}.stl", f.name);
        files.push((threemf.clone(), write_3mf(&objects, &f.meta).expect("3MF")));
        files.push((stl.clone(), write_stl_binary(&objects).expect("STL")));
        let mut per_role = serde_json::Map::new();
        for (role, bytes) in write_stl_binary_per_role(&objects).expect("STL per role") {
            let file = format!("{}.{role}.stl", f.name);
            let triangles: usize = f
                .parts
                .iter()
                .filter(|p| p.role == role)
                .map(|p| p.mesh.triangle_count())
                .sum();
            per_role.insert(role, json!({ "file": file, "triangles": triangles }));
            files.push((file, bytes));
        }
        let objects_json: Vec<_> = f
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let (lo, hi) = p.mesh.bounds();
                json!({
                    "name": p.name,
                    "role": p.role,
                    "color": p.colour.map(hex),
                    "vertices": p.mesh.positions.len(),
                    "triangles": p.mesh.triangle_count(),
                    "volumeMm3": mesh_volume(&p.mesh),
                    "boundsMm": { "min": lo, "max": hi },
                    "storedFingerprint": format!("{:016x}", stored_fingerprint(&p.mesh)),
                    "uuid": uuids.objects[i].to_string(),
                    "itemUuid": uuids.items[i].to_string(),
                })
            })
            .collect();
        entries.push(json!({
            "name": f.name,
            "threemf": threemf,
            "stl": stl,
            "stlTriangles": f.parts.iter().map(|p| p.mesh.triangle_count()).sum::<usize>(),
            "stlPerRole": per_role,
            "metadata": expected_metadata(&f.meta),
            "buildUuid": uuids.build.to_string(),
            "objects": objects_json,
        }));
    }
    let manifest = json!({
        "generator": "cargo run -p openbrick-io --example write_fixtures",
        "writer": GENERATOR,
        "openbrickNamespace": OPENBRICK_NAMESPACE,
        "unit": "millimeter",
        "fixtures": entries,
    });
    let mut text = serde_json::to_string_pretty(&manifest).expect("manifest");
    text.push('\n');
    files.push(("fixtures.json".into(), text.into_bytes()));
    files
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = match args.as_slice() {
        [] => PathBuf::from("target/io-fixtures"),
        [d] if !d.starts_with('-') => PathBuf::from(d),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    for (name, bytes) in generate() {
        let path = dir.join(&name);
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!("cannot write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("{:>9} bytes  {}", bytes.len(), path.display());
    }
    ExitCode::SUCCESS
}
