//! Round-trip tests for the fixtures that `examples/write_fixtures.rs` writes and CI
//! checks with lib3mf (`scripts/validate-3mf.py`). Here the same files are read back
//! with independent Rust readers (the `zip` crate, `roxmltree` and `stl_io`, none
//! of which share code with the writers) and compared with the meshes that went in.
//!
//! The example is included as a module so both use the same `fixtures()`.

// main() belongs to the example binary and is not called here.
#[allow(dead_code)]
#[path = "../examples/write_fixtures.rs"]
mod write_fixtures;

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::OnceLock;

use openbrick_geom::{CrossSection, Mesh, Polygon};
use openbrick_io::threemf::{
    package_uuids, stored_fingerprint, CORE_NAMESPACE, MATERIAL_NAMESPACE, MODEL_PATH,
    OPENBRICK_NAMESPACE, PRODUCTION_NAMESPACE,
};
use openbrick_io::{stl_binary_len, ExportMeta};
use openbrick_units::STANDARD;
use write_fixtures::{fixtures, generate, mesh_volume, Fixture, STUDS};

fn all_fixtures() -> &'static [Fixture] {
    static F: OnceLock<Vec<Fixture>> = OnceLock::new();
    F.get_or_init(fixtures)
}

fn all_files() -> &'static [(String, Vec<u8>)] {
    static F: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    F.get_or_init(generate)
}

fn file(name: &str) -> &'static [u8] {
    all_files()
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, b)| b.as_slice())
        .unwrap_or_else(|| panic!("{name} was not generated"))
}

/// Unzip a package into (entry name, text) pairs, in archive order. The `zip`
/// crate checks every CRC while reading.
fn unzip(bytes: &[u8]) -> Vec<(String, String)> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip archive");
    (0..archive.len())
        .map(|i| {
            let mut entry = archive.by_index(i).expect("zip entry");
            let mut text = String::new();
            entry.read_to_string(&mut text).expect("UTF-8 entry");
            (entry.name().expect("entry name").to_string(), text)
        })
        .collect()
}

/// An object as read back from the model XML.
#[derive(Debug)]
struct ReadObject {
    id: String,
    name: String,
    role: String,
    colour: Option<[u8; 4]>,
    uuid: String,
    mesh: Mesh,
}

/// Everything the tests compare, parsed with roxmltree.
struct ReadModel {
    metadata: BTreeMap<String, String>,
    objects: Vec<ReadObject>,
    build_uuid: String,
    items: Vec<(String, String)>,
}

fn parse_colour(text: &str) -> [u8; 4] {
    assert!(text.len() == 9 && text.starts_with('#'), "colour {text}");
    let byte = |i: usize| u8::from_str_radix(&text[1 + 2 * i..3 + 2 * i], 16).unwrap();
    [byte(0), byte(1), byte(2), byte(3)]
}

/// The element children of a node.
fn children<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}

fn parse_model(xml: &str) -> ReadModel {
    let doc = roxmltree::Document::parse(xml).expect("well-formed model XML");
    let model = doc.root_element();
    assert_eq!(model.tag_name().namespace(), Some(CORE_NAMESPACE));
    assert_eq!(model.tag_name().name(), "model");
    assert_eq!(model.attribute("unit"), Some("millimeter"));
    assert_eq!(model.attribute("requiredextensions"), None);
    assert_eq!(
        model.lookup_namespace_uri(Some("m")),
        Some(MATERIAL_NAMESPACE)
    );
    assert_eq!(
        model.lookup_namespace_uri(Some("p")),
        Some(PRODUCTION_NAMESPACE)
    );
    assert_eq!(
        model.lookup_namespace_uri(Some("openbrick")),
        Some(OPENBRICK_NAMESPACE)
    );
    let core = |n: roxmltree::Node, name: &str| {
        n.tag_name().namespace() == Some(CORE_NAMESPACE) && n.tag_name().name() == name
    };
    let uuid_of = |n: roxmltree::Node| {
        n.attribute((PRODUCTION_NAMESPACE, "UUID"))
            .expect("p:UUID")
            .to_string()
    };

    let mut metadata = BTreeMap::new();
    let mut colour_groups: BTreeMap<String, Vec<[u8; 4]>> = BTreeMap::new();
    let mut objects = Vec::new();
    let mut build = None;
    for node in children(model) {
        if core(node, "metadata") {
            let name = node.attribute("name").expect("metadata name").to_string();
            let value = node.text().unwrap_or_default().to_string();
            assert!(metadata.insert(name, value).is_none(), "repeated metadata");
        } else if core(node, "resources") {
            for res in children(node) {
                if res.tag_name().namespace() == Some(MATERIAL_NAMESPACE)
                    && res.tag_name().name() == "colorgroup"
                {
                    let colours = children(res)
                        .iter()
                        .map(|c| parse_colour(c.attribute("color").expect("color")))
                        .collect();
                    colour_groups.insert(res.attribute("id").unwrap().to_string(), colours);
                } else {
                    assert!(
                        core(res, "object"),
                        "unexpected resource {:?}",
                        res.tag_name()
                    );
                    assert_eq!(res.attribute("type"), Some("model"));
                    let colour = res.attribute("pid").map(|pid| {
                        let index: usize = res.attribute("pindex").unwrap().parse().unwrap();
                        colour_groups[pid][index]
                    });
                    let mut role = None;
                    let mut mesh = Mesh::default();
                    for part in children(res) {
                        if core(part, "metadatagroup") {
                            for md in children(part) {
                                assert_eq!(md.attribute("name"), Some("openbrick:role"));
                                role = md.text().map(str::to_string);
                            }
                        } else {
                            assert!(core(part, "mesh"));
                            let lists = children(part);
                            assert!(core(lists[0], "vertices") && core(lists[1], "triangles"));
                            for v in children(lists[0]) {
                                let c = |a: &str| v.attribute(a).unwrap().parse::<f32>().unwrap();
                                mesh.positions.push([c("x"), c("y"), c("z")]);
                            }
                            for t in children(lists[1]) {
                                for a in ["v1", "v2", "v3"] {
                                    mesh.indices.push(t.attribute(a).unwrap().parse().unwrap());
                                }
                            }
                        }
                    }
                    objects.push(ReadObject {
                        id: res.attribute("id").unwrap().to_string(),
                        name: res.attribute("name").unwrap().to_string(),
                        role: role.expect("openbrick:role"),
                        colour,
                        uuid: uuid_of(res),
                        mesh,
                    });
                }
            }
        } else {
            assert!(core(node, "build"));
            let items = children(node)
                .iter()
                .map(|item| {
                    assert!(core(*item, "item"));
                    assert_eq!(item.attribute("transform"), None);
                    (
                        item.attribute("objectid").unwrap().to_string(),
                        uuid_of(*item),
                    )
                })
                .collect();
            build = Some((uuid_of(node), items));
        }
    }
    let (build_uuid, items) = build.expect("build element");
    ReadModel {
        metadata,
        objects,
        build_uuid,
        items,
    }
}

fn expected_metadata(meta: &ExportMeta) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: &str| m.insert(k.to_string(), v.to_string());
    put("Title", &meta.title);
    put("Application", &meta.generator);
    put("openbrick:partId", &meta.part_id);
    put("openbrick:version", &meta.version);
    put("openbrick:contentHash", &meta.content_hash);
    put("openbrick:fitProfile", &meta.fit_profile_id);
    put("openbrick:process", &meta.process_id);
    put("openbrick:generator", &meta.generator);
    put("openbrick:kernel", &meta.kernel_id);
    if let Some(serial) = &meta.serial {
        put("openbrick:serial", serial);
    }
    m
}

#[test]
fn every_3mf_package_has_the_opc_parts() {
    for f in all_fixtures() {
        let entries = unzip(file(&format!("{}.3mf", f.name)));
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["[Content_Types].xml", "_rels/.rels", MODEL_PATH]);

        let types = roxmltree::Document::parse(&entries[0].1).unwrap();
        let defaults: BTreeMap<&str, &str> = types
            .root_element()
            .children()
            .filter(|c| c.is_element())
            .map(|d| {
                assert_eq!(d.tag_name().name(), "Default");
                (
                    d.attribute("Extension").unwrap(),
                    d.attribute("ContentType").unwrap(),
                )
            })
            .collect();
        assert_eq!(
            defaults.get("model"),
            Some(&"application/vnd.ms-package.3dmanufacturing-3dmodel+xml")
        );
        assert_eq!(
            defaults.get("rels"),
            Some(&"application/vnd.openxmlformats-package.relationships+xml")
        );

        let rels = roxmltree::Document::parse(&entries[1].1).unwrap();
        let rel = rels
            .root_element()
            .children()
            .find(|c| c.is_element())
            .unwrap();
        assert_eq!(rel.attribute("Target"), Some("/3D/3dmodel.model"));
        assert_eq!(
            rel.attribute("Type"),
            Some("http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel")
        );
    }
}

#[test]
fn every_3mf_fixture_reads_back_exactly() {
    for f in all_fixtures() {
        let entries = unzip(file(&format!("{}.3mf", f.name)));
        let model = parse_model(&entries[2].1);
        assert_eq!(model.metadata, expected_metadata(&f.meta), "{}", f.name);
        assert_eq!(model.objects.len(), f.parts.len());
        for (read, part) in model.objects.iter().zip(&f.parts) {
            assert_eq!(read.name, part.name);
            assert_eq!(read.role, part.role);
            assert_eq!(read.colour, part.colour, "{}", part.name);
            assert_eq!(read.mesh.indices, part.mesh.indices, "{}", part.name);
            // Bit-exact coordinates: the decimal text parses back to the same f32.
            let bits = |m: &Mesh| -> Vec<[u32; 3]> {
                m.positions
                    .iter()
                    .map(|p| p.map(|c| (c + 0.0).to_bits()))
                    .collect()
            };
            assert_eq!(bits(&read.mesh), bits(&part.mesh), "{}", part.name);
            // The fingerprint the UUIDs are derived from is what a reader can
            // recompute from the file alone.
            assert_eq!(
                read.mesh.fingerprint(),
                stored_fingerprint(&part.mesh),
                "{}",
                part.name
            );
        }
        // The UUIDs are the documented derivation from metadata and objects.
        let expected = package_uuids(&f.objects(), &f.meta);
        let read_objects: Vec<String> = model.objects.iter().map(|o| o.uuid.clone()).collect();
        let read_items: Vec<String> = model.items.iter().map(|(_, u)| u.clone()).collect();
        let strings = |v: &[uuid::Uuid]| v.iter().map(|u| u.to_string()).collect::<Vec<_>>();
        assert_eq!(model.build_uuid, expected.build.to_string(), "{}", f.name);
        assert_eq!(read_objects, strings(&expected.objects), "{}", f.name);
        assert_eq!(read_items, strings(&expected.items), "{}", f.name);
        // One build item per object, in order, and every UUID distinct and v5.
        let ids: Vec<&str> = model.objects.iter().map(|o| o.id.as_str()).collect();
        let item_ids: Vec<&str> = model.items.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(item_ids, ids);
        let mut uuids: Vec<&str> = vec![model.build_uuid.as_str()];
        uuids.extend(model.objects.iter().map(|o| o.uuid.as_str()));
        uuids.extend(model.items.iter().map(|(_, u)| u.as_str()));
        for u in &uuids {
            let parsed = uuid::Uuid::parse_str(u).expect("well-formed UUID");
            assert_eq!(parsed.get_version_num(), 5, "{u}");
            assert_eq!(
                parsed.hyphenated().to_string(),
                *u,
                "lower-case hyphenated form"
            );
        }
        let count = uuids.len();
        uuids.sort_unstable();
        uuids.dedup();
        assert_eq!(uuids.len(), count, "repeated UUID in {}", f.name);
    }
}

/// Triangles of a binary STL as read by `stl_io`.
fn read_stl(bytes: &[u8]) -> Vec<[[f32; 3]; 3]> {
    let mut cursor = std::io::Cursor::new(bytes);
    let reader = stl_io::create_stl_reader(&mut cursor).expect("STL");
    reader
        .map(|t| {
            let t = t.expect("STL triangle");
            t.vertices.map(|v| [v[0], v[1], v[2]])
        })
        .collect()
}

fn triangles_of(meshes: &[&Mesh]) -> Vec<[[f32; 3]; 3]> {
    meshes
        .iter()
        .flat_map(|m| {
            let (triangles, _) = m.indices.as_chunks::<3>();
            triangles.iter().map(|t| t.map(|i| m.positions[i as usize]))
        })
        .collect()
}

fn soup_volume(tris: &[[[f32; 3]; 3]]) -> f64 {
    let mut m = Mesh::default();
    for t in tris {
        let base = m.positions.len() as u32;
        m.positions.extend_from_slice(t);
        m.indices.extend([base, base + 1, base + 2]);
    }
    mesh_volume(&m)
}

#[test]
fn stl_fixtures_hold_the_same_triangles() {
    for f in all_fixtures() {
        let bytes = file(&format!("{}.stl", f.name));
        let meshes: Vec<&Mesh> = f.parts.iter().map(|p| &p.mesh).collect();
        let expected = triangles_of(&meshes);
        assert_eq!(bytes.len(), stl_binary_len(expected.len()));
        assert_eq!(read_stl(bytes), expected, "{}", f.name);

        let mut roles: Vec<&str> = f.parts.iter().map(|p| p.role.as_str()).collect();
        roles.dedup();
        for role in roles {
            let bytes = file(&format!("{}.{role}.stl", f.name));
            let meshes: Vec<&Mesh> = f
                .parts
                .iter()
                .filter(|p| p.role == role)
                .map(|p| &p.mesh)
                .collect();
            assert_eq!(read_stl(bytes), triangles_of(&meshes), "{} {role}", f.name);
        }
    }
}

/// Golden values for the brick fixture: analytic volume and bounding box from the
/// standard, measured on the triangles read back from the STL file.
#[test]
fn brick_fixture_has_the_analytic_volume_and_bounds() {
    let s = STANDARD.get();
    let (pitch, gap, wall) = (
        s.mm("module"),
        s.mm("undersize_per_side"),
        s.mm("side_wall"),
    );
    let (roof, height) = (s.mm("roof"), s.mm("brick_height"));
    let [nx, ny] = STUDS.map(f64::from);
    let (w, d) = (nx * pitch - 2.0 * gap, ny * pitch - 2.0 * gap);
    let cavity = height - roof;
    let outer = Polygon::rect([0.0, 0.0], [w, d]);
    let inner = Polygon::rect([wall, wall], [w - 2.0 * wall, d - 2.0 * wall]).reversed();
    let disc = CrossSection::from(vec![Polygon::circle(
        [0.0, 0.0],
        s.mm("stud_diameter") / 2.0,
        64,
    )]);
    let annulus = CrossSection::from(vec![
        Polygon::circle([0.0, 0.0], s.mm("tube_outer_diameter") / 2.0, 64),
        Polygon::circle([0.0, 0.0], s.mm("tube_inner_diameter") / 2.0, 64).reversed(),
    ]);
    // Shell, roof, one stud per footprint cell and one row of tubes between the
    // two stud columns (STUDS[1] - 1 of them); the faceted polygon areas, so the
    // comparison checks the file, not the faceting.
    let studs = nx * ny;
    let tubes = ny - 1.0;
    let brick = CrossSection::from(vec![outer.clone(), inner]).area() * cavity
        + CrossSection::from(vec![outer]).area() * roof
        + studs * disc.area() * s.mm("stud_height")
        + tubes * annulus.area() * cavity;
    let side = s.mm("stud_diameter");
    let cube = side * side * side;

    let body = read_stl(file("brick-2x4-with-cube.body.stl"));
    let accent = read_stl(file("brick-2x4-with-cube.accent.stl"));
    let merged = read_stl(file("brick-2x4-with-cube.stl"));
    // f32 coordinates carry a relative rounding error of about 6e-8 per
    // coordinate, so 1e-5 leaves a wide margin. This checks the file against the
    // geometry the standard describes; exact triangle-by-triangle equality with
    // the kernel mesh is `stl_fixtures_hold_the_same_triangles` (a volume alone
    // cannot see a flipped triangle in a plane through the origin, whose
    // divergence-theorem term is zero).
    let rel = |a: f64, b: f64| ((a - b) / b).abs();
    assert!(
        rel(soup_volume(&body), brick) < 1e-5,
        "{} vs {brick}",
        soup_volume(&body)
    );
    assert!(
        rel(soup_volume(&accent), cube) < 1e-5,
        "{} vs {cube}",
        soup_volume(&accent)
    );
    assert!(rel(soup_volume(&merged), brick + cube) < 1e-5);

    let bounds = |tris: &[[[f32; 3]; 3]]| {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in tris.iter().flatten() {
            for k in 0..3 {
                lo[k] = lo[k].min(f64::from(v[k]));
                hi[k] = hi[k].max(f64::from(v[k]));
            }
        }
        (lo, hi)
    };
    let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5);
    let (lo, hi) = bounds(&body);
    assert!(close(lo, [0.0; 3]), "{lo:?}");
    assert!(close(hi, [w, d, height + s.mm("stud_height")]), "{hi:?}");
    let (lo, hi) = bounds(&accent);
    let x0 = w + pitch;
    assert!(close(lo, [x0, 0.0, 0.0]), "{lo:?}");
    assert!(close(hi, [x0 + side, side, side]), "{hi:?}");
}

/// Counts are the same everywhere; the fingerprints are recorded for x86-64 Linux,
/// where the Q01 spike verified bit-identical kernel output
/// (docs/spikes/q01-kernel-conformance.md). The body fingerprint equals LOD0 in
/// bench/harness/public/meshes/manifest.json, which is built the same way.
#[test]
fn brick_fixture_counts() {
    let f = &all_fixtures()[0];
    let counts: Vec<(usize, usize)> = f
        .parts
        .iter()
        .map(|p| (p.mesh.positions.len(), p.mesh.triangle_count()))
        .collect();
    assert_eq!(counts, [(1812, 3620), (8, 12)]);
    assert_eq!(file("brick-2x4-with-cube.stl").len(), stl_binary_len(3632));
}

#[test]
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux")),
    ignore = "bit-exact output is verified on x86-64 Linux only (docs/spikes/q01-kernel-conformance.md)"
)]
fn brick_fixture_fingerprints() {
    let f = &all_fixtures()[0];
    let prints: Vec<u64> = f.parts.iter().map(|p| p.mesh.fingerprint()).collect();
    assert_eq!(prints, [0xfa96_9a6c_0bf6_57e7, 0xd6d9_62aa_5ef1_e7ed]);
    // No negative zeros, so the stored fingerprints (from which the UUIDs are
    // derived and which the manifest records) are the same values.
    let stored: Vec<u64> = f
        .parts
        .iter()
        .map(|p| stored_fingerprint(&p.mesh))
        .collect();
    assert_eq!(stored, prints);
}

#[test]
fn fixtures_are_byte_identical_across_runs() {
    let again = generate();
    assert_eq!(again.len(), all_files().len());
    for ((name, a), (_, b)) in again.iter().zip(all_files()) {
        assert!(a == b, "{name} differs between two runs");
    }
}
