//! The LDraw importer on synthetic libraries: `tests/fixtures/ldraw` (a mini
//! library written for these tests, with LDraw-style names and none of the
//! LDraw library's content) and small libraries made in a temp folder.
//!
//! A last test runs the importer on a real library when `OPENBRICK_LDRAW_DIR`
//! names one (a folder or complete.zip); it is skipped otherwise, so CI does
//! not need the library.

#![cfg(feature = "import")]

use std::path::{Path, PathBuf};

use openbrick_catalog::ldraw::connect::{self, classify, Prim};
use openbrick_catalog::ldraw::flatten::{Collect, IndexedMesh, Limits, Mat, WalkError, Walker};
use openbrick_catalog::ldraw::library::{
    crc32, normalise_name, zip_entry_path, Library, LibraryError, Resolution,
};
use openbrick_catalog::ldraw::{import, ImportOptions};
use openbrick_catalog::{Catalog, ConnKind, Connectivity, Pack};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ldraw")
}

fn quiet(_: &str, _: usize, _: usize) {}

fn pack_of(opts: &ImportOptions) -> (Pack, openbrick_catalog::ldraw::ImportReport) {
    import(&fixture(), opts, &mut quiet).expect("the fixture imports")
}

fn all_categories() -> ImportOptions {
    ImportOptions {
        categories: Some(
            ["Brick", "Plate", "Tile", "Technic"]
                .map(String::from)
                .to_vec(),
        ),
        ..ImportOptions::default()
    }
}

fn part<'a>(p: &'a Pack, id: &str) -> &'a openbrick_catalog::PackPart {
    p.header
        .parts
        .iter()
        .find(|x| x.id == id)
        .unwrap_or_else(|| panic!("{id} is in the pack"))
}

/// A temp folder removed on drop.
struct Tmp(PathBuf);

impl Tmp {
    fn new(name: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "openbrick-ldraw-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Signed volume of a closed triangle mesh: positive when the faces look outwards.
fn volume(m: &openbrick_catalog::builtin::PartMesh) -> f64 {
    let mut v = 0.0;
    for t in m.indices.chunks(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.positions[i as usize].map(f64::from));
        v += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    v
}

#[test]
fn the_fixture_imports_with_the_right_selection() {
    let (pack, report) = pack_of(&all_categories());
    assert_eq!(report.counts.scanned, 16);
    // 3023 is an alias ("~Moved to"), 973 a minifigure part, 3003a a Duplo-compatible part,
    // 3003p01 a pattern, 9999 another category.
    assert_eq!(report.excluded.not_a_part, 1);
    assert_eq!(report.excluded.minifig, 1);
    assert_eq!(report.excluded.other_systems, 1);
    assert_eq!(report.excluded.prints_and_stickers, 1);
    assert_eq!(report.excluded.other_categories, 1);
    assert_eq!(report.counts.eligible, 12);
    assert_eq!(report.counts.imported, 11);
    assert_eq!(report.library, "2026-01");
    let ids: Vec<&str> = pack.header.parts.iter().map(|p| p.id.as_str()).collect();
    for banned in ["ld.973", "ld.3003p01", "ld.3023", "ld.3003a", "ld.9999"] {
        assert!(!ids.contains(&banned), "{banned} must be left out: {ids:?}");
    }
    assert!(pack
        .header
        .licences
        .iter()
        .any(|l| l.text.contains("Synthetic test library")));
    assert!(pack.header.attribution.contains("LDraw Parts Library"));
    for p in &pack.header.parts {
        assert!(
            !p.name.to_ascii_lowercase().contains("minifig"),
            "{}",
            p.name
        );
        assert_eq!(p.author, "OpenBrick3D tests");
    }
}

#[test]
fn a_two_by_two_brick_gets_its_studs_and_receptacles_from_the_geometry() {
    let (pack, _) = pack_of(&all_categories());
    let b = part(&pack, "ld.3003");
    assert_eq!(b.category, "Brick");
    assert_eq!(b.connectivity, Connectivity::Derived);
    assert_eq!((b.body.min, b.body.max), ([-20, 0, -20], [20, 24, 20]));
    // The pick box holds the studs (4 LDU high), the body box does not.
    assert_eq!(b.pick.min, [-20, -4, -20]);
    assert_eq!(b.studs, [2, 2]);
    assert_eq!(b.height_ldu, 24);
    let studs: Vec<_> = b
        .connectors
        .iter()
        .filter(|c| c.kind == ConnKind::StudM)
        .collect();
    let tubes: Vec<_> = b
        .connectors
        .iter()
        .filter(|c| c.kind == ConnKind::TubeF)
        .collect();
    assert_eq!((studs.len(), tubes.len()), (4, 4));
    for sx in [-10, 10] {
        for sz in [-10, 10] {
            assert!(
                studs
                    .iter()
                    .any(|c| c.pos == [sx, 0, sz] && c.dir == [0, -1, 0]),
                "stud {sx} {sz}"
            );
            assert!(
                tubes
                    .iter()
                    .any(|c| c.pos == [sx, 24, sz] && c.dir == [0, 1, 0]),
                "receptacle {sx} {sz}"
            );
        }
    }
    // A stud group primitive (stug-1x4) and the plain stud lines give the same kind of answer.
    let long = part(&pack, "ld.3010");
    assert_eq!(
        long.connectors
            .iter()
            .filter(|c| c.kind == ConnKind::StudM)
            .count(),
        4
    );
    assert_eq!(
        long.connectors
            .iter()
            .filter(|c| c.kind == ConnKind::TubeF)
            .count(),
        4
    );
}

#[test]
fn plates_and_tiles_get_receptacles_and_tiles_have_no_studs() {
    let (pack, _) = pack_of(&all_categories());
    for (id, studs, tubes, height) in [
        ("ld.3024", 1, 1, 8),
        ("ld.3070b", 0, 1, 8),
        ("ld.3068b", 0, 4, 8),
    ] {
        let p = part(&pack, id);
        let s = p
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::StudM)
            .count();
        let t = p
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::TubeF)
            .count();
        assert_eq!((s, t, p.height_ldu), (studs, tubes, height), "{id}");
    }
}

#[test]
fn every_mesh_is_closed_and_faces_outwards_whatever_the_bfc_flips() {
    // The stud is drawn with a negative Y scale (a mirror), the cube is certified CCW:
    // only the determinant rule makes the studs come out right.
    let (pack, _) = pack_of(&all_categories());
    for (i, p) in pack.header.parts.iter().enumerate() {
        if p.id == "ld.3673" || p.id == "ld.3705" || p.id == "ld.3706" || p.id == "ld.4081b" {
            continue; // these include a one-triangle stand-in for a primitive the scanner never reads
        }
        let m = pack.mesh(i).unwrap();
        assert!(volume(&m) > 0.0, "{} has volume {}", p.id, volume(&m));
    }
}

#[test]
fn technic_pins_axles_and_holes_come_from_their_primitives() {
    let (pack, _) = pack_of(&all_categories());
    // Two peg-hole ends facing away from each other, 20 LDU apart, make one pin hole at the midpoint.
    let t = part(&pack, "ld.3700");
    let holes: Vec<_> = t
        .connectors
        .iter()
        .filter(|c| c.kind == ConnKind::PinF)
        .collect();
    assert_eq!(holes.len(), 1);
    assert_eq!(
        (holes[0].pos, holes[0].dir.map(i32::abs)),
        ([0, 10, 0], [0, 0, 1])
    );
    // A pin of two modules (two connect.dat) is two connectors; one module each.
    let pin = part(&pack, "ld.3673");
    let m: Vec<_> = pin
        .connectors
        .iter()
        .filter(|c| c.kind == ConnKind::PinM)
        .map(|c| c.pos)
        .collect();
    assert_eq!(m, vec![[0, -30, 0], [0, -10, 0]]);
    // An axle of four modules is four connectors along X.
    let axle = part(&pack, "ld.3705");
    let xs: Vec<_> = axle
        .connectors
        .iter()
        .filter(|c| c.kind == ConnKind::AxleM)
        .map(|c| c.pos[0])
        .collect();
    assert_eq!(xs, vec![-30, -10, 10, 30]);
    // An axle hole one module long is one connector.
    assert_eq!(
        part(&pack, "ld.3706")
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::AxleF)
            .count(),
        1
    );
}

#[test]
fn clip_parts_are_flagged_not_modelled() {
    let (pack, report) = pack_of(&all_categories());
    let c = part(&pack, "ld.4081b");
    assert_eq!(c.joints, vec!["clip".to_string()]);
    // Its stud still snaps; the clip is not a connector.
    assert!(c.connectors.iter().any(|x| x.kind == ConnKind::StudM));
    assert_eq!(report.counts.with_joints, 1);
}

#[test]
fn resolution_picks_the_primitive_variant() {
    let tri = |r: Resolution| {
        let opts = ImportOptions {
            resolution: r,
            ..all_categories()
        };
        let (pack, _) = pack_of(&opts);
        part(&pack, "ld.3005").triangles
    };
    let (low, std, high) = (
        tri(Resolution::Low),
        tri(Resolution::Standard),
        tri(Resolution::High),
    );
    assert!(low < std && std < high, "{low} {std} {high}");
}

#[test]
fn limit_and_category_filter_and_determinism() {
    let one = ImportOptions {
        limit: Some(2),
        categories: Some(vec!["brick".into()]),
        ..ImportOptions::default()
    };
    // A small limit over several categories takes some of each, not only the first category's parts.
    let spread = ImportOptions {
        limit: Some(4),
        categories: Some(["Brick", "Plate", "Tile"].map(String::from).to_vec()),
        ..ImportOptions::default()
    };
    let (s, _) = pack_of(&spread);
    let cats: Vec<&str> = s.header.parts.iter().map(|p| p.category.as_str()).collect();
    assert_eq!(cats.len(), 4);
    assert!(
        cats.contains(&"Brick") && cats.contains(&"Plate") && cats.contains(&"Tile"),
        "{cats:?}"
    );
    assert!(
        cats.windows(2).all(|w| w[0] <= w[1] || w[0] != "Tile"),
        "palette order: {cats:?}"
    );
    let (a, _) = pack_of(&one);
    assert_eq!(a.header.parts.len(), 2);
    assert!(a.header.parts.iter().all(|p| p.category == "Brick"));
    let (b, _) = pack_of(&one);
    assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap());
    assert_eq!(a.fingerprint(), b.fingerprint());
    // Unmatched categories import nothing: an error that says what to change.
    let none = ImportOptions {
        categories: Some(vec!["Nothing".into()]),
        ..ImportOptions::default()
    };
    assert!(import(&fixture(), &none, &mut quiet).is_err());
}

#[test]
fn an_imported_pack_gives_the_engine_a_catalog_with_stable_builtin_ids() {
    let (pack, _) = pack_of(&all_categories());
    let bytes = pack.to_bytes().unwrap();
    let pack = Pack::from_bytes(&bytes).unwrap();
    let base = Catalog::builtin().unwrap();
    let c = base.with_pack(&pack);
    assert_eq!(c.len(), base.len() + pack.header.parts.len());
    for (i, p) in base.parts.iter().enumerate() {
        assert_eq!(c.parts[i].id, p.id, "built-in part numbers do not move");
    }
    assert!(c.find("ld.3003").is_some() && c.find("ob.brick-2x4").is_some());
    assert_eq!(c.pack.as_ref().unwrap().fingerprint, pack.fingerprint());
}

// ------------------------------------------------------------ BFC and matrices

fn lib_with(t: &Tmp, files: &[(&str, &str)]) -> Library {
    for (rel, text) in files {
        t.write(&format!("ldraw/{rel}"), text);
    }
    t.write("ldraw/p/.keep.dat", "");
    Library::open(&t.0, Resolution::Standard).unwrap()
}

fn soup_of(lib: &Library, path: &str) -> (Vec<[[f64; 3]; 3]>, usize) {
    let mut c = Collect::new(|_| false);
    let mut w = Walker::new(lib, Limits::default());
    w.walk(&mut c, path, &Mat::IDENTITY).unwrap();
    (c.soup.tris, w.stats.uncertified_triangles)
}

fn normal(t: &[[f64; 3]; 3]) -> [f64; 3] {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

const TRI: &str = "3 16 0 0 0 1 0 0 0 0 1\n"; // normal +Y for CCW
const CERT: &str = "0 BFC CERTIFY CCW\n";

#[test]
fn winding_follows_certify_cw_mirror_and_invertnext() {
    let t = Tmp::new("bfc");
    let lib = lib_with(
        &t,
        &[
            ("parts/ccw.dat", &format!("0 ccw\n0 !LDRAW_ORG Part\n{CERT}{TRI}")),
            ("parts/cw.dat", &format!("0 cw\n0 !LDRAW_ORG Part\n0 BFC CERTIFY CW\n{TRI}")),
            ("parts/mirror.dat", &format!("0 m\n0 !LDRAW_ORG Part\n{CERT}1 16 0 0 0 -1 0 0 0 1 0 0 0 1 ccw.dat\n")),
            ("parts/invert.dat", &format!("0 i\n0 !LDRAW_ORG Part\n{CERT}0 BFC INVERTNEXT\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 ccw.dat\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 ccw.dat\n")),
            ("parts/both.dat", &format!("0 b\n0 !LDRAW_ORG Part\n{CERT}1 16 0 0 0 -1 0 0 0 1 0 0 0 1 invert.dat\n")),
            ("parts/open.dat", &format!("0 o\n0 !LDRAW_ORG Part\n{TRI}")),
        ],
    );
    // The triangle (0,0,0), (1,0,0), (0,0,1) written counter-clockwise has its normal along -Y.
    let (base, _) = soup_of(&lib, "parts/ccw.dat");
    let n0 = normal(&base[0]);
    assert_eq!(n0, [0.0, -1.0, 0.0]);
    let same = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2] > 0.0;
    // CERTIFY CW: the vertices are reversed to counter-clockwise, so the normal turns round.
    let (cw, _) = soup_of(&lib, "parts/cw.dat");
    assert!(!same(normal(&cw[0]), n0));
    // A mirror (negative determinant) inverts the winding rule: the copy mirrored in X still
    // faces -Y, which is what BFC is for.
    let (m, _) = soup_of(&lib, "parts/mirror.dat");
    assert_eq!(m.len(), 1);
    assert!(same(normal(&m[0]), n0), "mirrored copy faces the same way");
    // INVERTNEXT applies to the first reference only.
    let (inv, _) = soup_of(&lib, "parts/invert.dat");
    assert_eq!(inv.len(), 2);
    assert!(!same(normal(&inv[0]), n0) && same(normal(&inv[1]), n0));
    // A mirror and an INVERTNEXT cancel on the first reference (written as is, so it faces +Y in
    // the mirrored world) and add up to the mirror alone on the second.
    let (both, _) = soup_of(&lib, "parts/both.dat");
    assert_eq!(both.len(), 2);
    assert!(!same(normal(&both[0]), n0) && same(normal(&both[1]), n0));
    // A file that is not certified is written in both orders and counted.
    let (open, uncertified) = soup_of(&lib, "parts/open.dat");
    assert_eq!((open.len(), uncertified), (2, 2));
    assert!(!same(normal(&open[0]), normal(&open[1])));
}

#[test]
fn matrices_compose_parent_after_child() {
    let t = Tmp::new("mat");
    let lib = lib_with(
        &t,
        &[
            (
                "parts/leaf.dat",
                &format!("0 leaf\n0 !LDRAW_ORG Part\n{CERT}3 16 0 0 0 1 0 0 0 0 1\n"),
            ),
            // Rotate a quarter turn about Y then move: x -> z.
            (
                "parts/mid.dat",
                &format!(
                    "0 mid\n0 !LDRAW_ORG Part\n{CERT}1 16 5 0 0 0 0 1 0 1 0 -1 0 0 leaf.dat\n"
                ),
            ),
            (
                "parts/top.dat",
                &format!("0 top\n0 !LDRAW_ORG Part\n{CERT}1 16 0 7 0 2 0 0 0 2 0 0 0 2 mid.dat\n"),
            ),
        ],
    );
    let (tris, _) = soup_of(&lib, "parts/top.dat");
    let p = tris[0][1];
    // Check against a direct evaluation of the two matrices (the parent's applied after the child's).
    let m_mid = Mat {
        m: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
        t: [5.0, 0.0, 0.0],
    };
    let m_top = Mat {
        m: [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
        t: [0.0, 7.0, 0.0],
    };
    let want = m_top.mul(&m_mid).apply([1.0, 0.0, 0.0]);
    assert_eq!(p, want);
}

// ------------------------------------------------------------ hostile input

#[test]
fn a_part_that_refers_to_itself_is_an_error() {
    let t = Tmp::new("cycle");
    let lib = lib_with(
        &t,
        &[(
            "parts/loop.dat",
            "0 l\n0 !LDRAW_ORG Part\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 loop.dat\n",
        )],
    );
    let mut c = Collect::new(|_| false);
    let e = Walker::new(&lib, Limits::default())
        .walk(&mut c, "parts/loop.dat", &Mat::IDENTITY)
        .unwrap_err();
    assert!(matches!(e, WalkError::Cycle(_)), "{e}");
}

#[test]
fn deep_chains_and_exponential_expansion_are_cut_off() {
    let t = Tmp::new("bombs");
    // A chain 40 deep.
    let mut files: Vec<(String, String)> = (0..40)
        .map(|i| {
            (
                format!("parts/c{i}.dat"),
                format!(
                    "0 c\n0 !LDRAW_ORG Part\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 c{}.dat\n",
                    i + 1
                ),
            )
        })
        .collect();
    files.push(("parts/c40.dat".into(), format!("0 end\n{CERT}{TRI}")));
    // Ten levels each referencing the next ten times: 10^10 triangles if expanded.
    for i in 0..10 {
        let next = if i == 9 {
            "leaf.dat".to_string()
        } else {
            format!("e{}.dat", i + 1)
        };
        files.push((
            format!("parts/e{i}.dat"),
            format!(
                "0 e\n0 !LDRAW_ORG Part\n{CERT}{}",
                format!("1 16 0 0 0 1 0 0 0 1 0 0 0 1 {next}\n").repeat(10)
            ),
        ));
    }
    files.push(("parts/leaf.dat".into(), format!("0 leaf\n{CERT}{TRI}")));
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let lib = lib_with(&t, &refs);
    let mut c = Collect::new(|_| false);
    let e = Walker::new(&lib, Limits::default())
        .walk(&mut c, "parts/c0.dat", &Mat::IDENTITY)
        .unwrap_err();
    assert!(matches!(e, WalkError::TooDeep(..)), "{e}");
    let mut c = Collect::new(|_| false);
    let e = Walker::new(&lib, Limits::default())
        .walk(&mut c, "parts/e0.dat", &Mat::IDENTITY)
        .unwrap_err();
    assert!(
        matches!(
            e,
            WalkError::TooManyRefs(_) | WalkError::TooManyTriangles(_)
        ),
        "{e}"
    );
}

#[test]
fn an_oversized_file_is_refused() {
    let t = Tmp::new("big");
    let big = format!(
        "0 big\n{}",
        "0 padding line of text to make the file large\n".repeat(400_000)
    );
    assert!(big.len() > 16 * 1024 * 1024);
    t.write("ldraw/parts/big.dat", &big);
    t.write("ldraw/p/x.dat", "0 x\n");
    let lib = Library::open(&t.0, Resolution::Standard).unwrap();
    assert!(matches!(
        lib.load("parts/big.dat"),
        Err(LibraryError::TooBig { .. })
    ));
}

#[test]
fn unsafe_names_never_resolve() {
    let t = Tmp::new("names");
    t.write("secret.dat", "0 secret\n");
    let lib = lib_with(&t, &[("parts/a.dat", "0 a\n")]);
    for bad in [
        "../secret.dat",
        "..\\secret.dat",
        "/etc/passwd",
        "parts/../../secret.dat",
        "c:\\x.dat",
    ] {
        assert!(lib.candidates(bad).is_empty(), "{bad}");
        assert!(lib.resolve(bad).unwrap().is_none(), "{bad}");
    }
    assert_eq!(normalise_name("a/./b"), None);
    assert_eq!(lib.candidates("S\\x.dat")[0], "parts/s/x.dat");
    assert_eq!(lib.candidates("48\\x.dat"), vec!["p/48/x.dat".to_string()]);
}

#[test]
fn symlinks_in_a_library_folder_are_not_followed() {
    #[cfg(unix)]
    {
        let t = Tmp::new("symlink");
        t.write("outside/secret.dat", "0 secret\n");
        let lib_dir = t.0.join("ldraw/parts");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::create_dir_all(t.0.join("ldraw/p")).unwrap();
        std::os::unix::fs::symlink(t.0.join("outside/secret.dat"), lib_dir.join("link.dat"))
            .unwrap();
        t.write("ldraw/parts/real.dat", "0 real\n");
        let lib = Library::open(&t.0, Resolution::Standard).unwrap();
        let names = lib.part_paths();
        assert_eq!(names, vec!["parts/real.dat".to_string()]);
    }
}

// ------------------------------------------------------------ zip

/// A zip with the given (name, content, deflate) entries, written by hand.
fn zip_bytes(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for (name, data, deflate) in entries {
        let comp = if *deflate {
            miniz_oxide::deflate::compress_to_vec(data, 6)
        } else {
            data.to_vec()
        };
        let offset = out.len() as u32;
        let method: u16 = if *deflate { 8 } else { 0 };
        let crc = crc32(data);
        out.extend([0x50, 0x4b, 3, 4, 20, 0, 0, 0]);
        out.extend(method.to_le_bytes());
        out.extend([0, 0, 0x21, 0]);
        out.extend(crc.to_le_bytes());
        out.extend((comp.len() as u32).to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend([0, 0]);
        out.extend(name.as_bytes());
        out.extend(&comp);
        cd.extend([0x50, 0x4b, 1, 2, 20, 0, 20, 0, 0, 0]);
        cd.extend(method.to_le_bytes());
        cd.extend([0, 0, 0x21, 0]);
        cd.extend(crc.to_le_bytes());
        cd.extend((comp.len() as u32).to_le_bytes());
        cd.extend((data.len() as u32).to_le_bytes());
        cd.extend((name.len() as u16).to_le_bytes());
        cd.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.extend(offset.to_le_bytes());
        cd.extend(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend(&cd);
    out.extend([0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((cd.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend([0, 0]);
    out
}

#[test]
fn a_zip_library_reads_like_a_folder_and_ignores_traversal_entries() {
    let t = Tmp::new("zip");
    let brick = std::fs::read_to_string(fixture().join("ldraw/parts/3005.dat")).unwrap();
    let stud = std::fs::read_to_string(fixture().join("ldraw/p/stud.dat")).unwrap();
    let cube = std::fs::read_to_string(fixture().join("ldraw/p/cube.dat")).unwrap();
    let zip = zip_bytes(&[
        ("ldraw/parts/3005.dat", brick.as_bytes(), true),
        ("ldraw/p/stud.dat", stud.as_bytes(), false),
        ("ldraw/p/cube.dat", cube.as_bytes(), true),
        ("ldraw/../evil.dat", b"0 evil\n", false),
        ("../../evil2.dat", b"0 evil\n", false),
        ("/abs/parts/evil3.dat", b"0 evil\n", false),
        ("ldraw/parts/s/../x.dat", b"0 x\n", false),
    ]);
    std::fs::write(t.0.join("lib.zip"), zip).unwrap();
    let lib = Library::open(&t.0.join("lib.zip"), Resolution::Standard).unwrap();
    assert_eq!(lib.part_paths(), vec!["parts/3005.dat".to_string()]);
    assert!(lib.resolve("evil.dat").unwrap().is_none());
    assert!(lib.resolve("evil2.dat").unwrap().is_none());
    let d = connect::derive(&lib, "parts/3005.dat", Limits::default())
        .unwrap()
        .unwrap();
    assert_eq!(d.studs, 1);
    assert_eq!(d.body.max, [10, 24, 10]);
    assert_eq!(zip_entry_path("ldraw/parts/../../x.dat"), None);
    // A pack made from the zip equals one made from the same files in a folder.
    let opts = ImportOptions {
        categories: Some(vec!["Brick".into()]),
        ..ImportOptions::default()
    };
    let (from_zip, _) = import(&t.0.join("lib.zip"), &opts, &mut quiet).unwrap();
    assert_eq!(from_zip.header.parts.len(), 1);
    assert_eq!(from_zip.header.parts[0].id, "ld.3005");
}

#[test]
fn a_damaged_zip_is_an_error() {
    let t = Tmp::new("badzip");
    std::fs::write(t.0.join("bad.zip"), b"PK\x03\x04 this is not a zip").unwrap();
    assert!(matches!(
        Library::open(&t.0.join("bad.zip"), Resolution::Standard),
        Err(LibraryError::Zip(_))
    ));
    let good = zip_bytes(&[
        ("ldraw/parts/a.dat", b"0 a\n", true),
        ("ldraw/p/b.dat", b"0 b\n", false),
    ]);
    for cut in [10usize, 30, good.len() - 30, good.len() - 5] {
        std::fs::write(t.0.join("cut.zip"), &good[..cut]).unwrap();
        assert!(Library::open(&t.0.join("cut.zip"), Resolution::Standard).is_err());
    }
    // A deflate bomb: the entry claims 10 bytes and inflates to 20 MiB.
    let mut zip = zip_bytes(&[
        ("ldraw/parts/bomb.dat", &vec![b'0'; 20 * 1024 * 1024], true),
        ("ldraw/p/b.dat", b"0 b\n", false),
    ]);
    // Rewrite the claimed size in the central directory to something small.
    let cd = zip
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 1, 2])
        .unwrap();
    zip[cd + 24..cd + 28].copy_from_slice(&10u32.to_le_bytes());
    std::fs::write(t.0.join("bomb.zip"), zip).unwrap();
    let lib = Library::open(&t.0.join("bomb.zip"), Resolution::Standard).unwrap();
    assert!(lib.load("parts/bomb.dat").is_err());
}

/// One entry of a hand-made hostile zip: what the local header, the directory and the data say.
struct Spec<'a> {
    central_name: &'a str,
    local_name: &'a str,
    data: &'a [u8],
    deflate: bool,
    /// Point the directory record at the local header of this entry (by index) instead of its own.
    share_with: Option<usize>,
    claimed_size: Option<u32>,
    crc: Option<u32>,
    /// Flip the first byte of the stored data after writing.
    corrupt: bool,
}

impl<'a> Spec<'a> {
    fn new(name: &'a str, data: &'a [u8]) -> Spec<'a> {
        Spec {
            central_name: name,
            local_name: name,
            data,
            deflate: false,
            share_with: None,
            claimed_size: None,
            crc: None,
            corrupt: false,
        }
    }
}

fn hostile_zip(specs: &[Spec]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut offsets = Vec::new();
    let mut comps = Vec::new();
    for sp in specs {
        let comp = if sp.deflate {
            miniz_oxide::deflate::compress_to_vec(sp.data, 6)
        } else {
            sp.data.to_vec()
        };
        offsets.push(out.len() as u32);
        let method: u16 = if sp.deflate { 8 } else { 0 };
        out.extend([0x50, 0x4b, 3, 4, 20, 0, 0, 0]);
        out.extend(method.to_le_bytes());
        out.extend([0, 0, 0x21, 0]);
        out.extend(crc32(sp.data).to_le_bytes());
        out.extend((comp.len() as u32).to_le_bytes());
        out.extend((sp.data.len() as u32).to_le_bytes());
        out.extend((sp.local_name.len() as u16).to_le_bytes());
        out.extend([0, 0]);
        out.extend(sp.local_name.as_bytes());
        let at = out.len();
        out.extend(&comp);
        if sp.corrupt {
            out[at] ^= 0xFF;
        }
        comps.push(comp.len() as u32);
    }
    let mut cd = Vec::new();
    for (i, sp) in specs.iter().enumerate() {
        let method: u16 = if sp.deflate { 8 } else { 0 };
        let off = sp.share_with.map_or(offsets[i], |j| offsets[j]);
        cd.extend([0x50, 0x4b, 1, 2, 20, 0, 20, 0, 0, 0]);
        cd.extend(method.to_le_bytes());
        cd.extend([0, 0, 0x21, 0]);
        cd.extend(sp.crc.unwrap_or_else(|| crc32(sp.data)).to_le_bytes());
        cd.extend(comps[i].to_le_bytes());
        cd.extend(
            sp.claimed_size
                .unwrap_or(sp.data.len() as u32)
                .to_le_bytes(),
        );
        cd.extend((sp.central_name.len() as u16).to_le_bytes());
        cd.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.extend(off.to_le_bytes());
        cd.extend(sp.central_name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend(&cd);
    out.extend([0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
    out.extend((specs.len() as u16).to_le_bytes());
    out.extend((specs.len() as u16).to_le_bytes());
    out.extend((cd.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend([0, 0]);
    out
}

fn open_zip(t: &Tmp, name: &str, zip: Vec<u8>) -> Result<Library, LibraryError> {
    std::fs::write(t.0.join(name), zip).unwrap();
    Library::open(&t.0.join(name), Resolution::Standard)
}

#[test]
fn zip_entries_that_share_data_are_refused_at_once() {
    // 200 directory records, one local header and one 16 MiB deflate stream: the importer would
    // inflate the same stream 200 times (32 s in a release build before the fix).
    let t = Tmp::new("overlap");
    let big = vec![b'0'; 64 * 1024];
    let names: Vec<String> = (0..200).map(|i| format!("ldraw/parts/{i}.dat")).collect();
    let mut specs: Vec<Spec> = names
        .iter()
        .map(|n| Spec {
            share_with: Some(0),
            local_name: "ldraw/parts/0.dat",
            deflate: true,
            ..Spec::new(n, &big)
        })
        .collect();
    specs.push(Spec::new("ldraw/p/b.dat", b"0 b\n"));
    let zip = hostile_zip(&specs);
    let t0 = std::time::Instant::now();
    let e = open_zip(&t, "share.zip", zip)
        .err()
        .expect("refused");
    assert!(e.to_string().contains("overlap"), "{e}");
    assert!(t0.elapsed().as_millis() < 1000, "{:?}", t0.elapsed());
    // Two entries, one of them inside the other's data.
    let specs = [
        Spec::new("ldraw/parts/a.dat", b"0 a\n"),
        Spec {
            share_with: Some(0),
            ..Spec::new("ldraw/parts/b.dat", b"0 a\n")
        },
    ];
    assert!(open_zip(&t, "two.zip", hostile_zip(&specs)).is_err());
    // Entries in a different order in the directory than in the file are fine.
    let ok = [
        Spec::new("ldraw/parts/a.dat", b"0 a\n"),
        Spec::new("ldraw/parts/b.dat", b"0 b\n"),
    ];
    assert!(open_zip(&t, "ok.zip", hostile_zip(&ok)).is_ok());
}

#[test]
fn a_local_header_that_names_another_file_is_refused() {
    let t = Tmp::new("localname");
    let specs = [
        Spec {
            local_name: "ldraw/parts/b.dat",
            ..Spec::new("ldraw/parts/a.dat", b"0 a\n")
        },
        Spec::new("ldraw/p/p.dat", b"0 p\n"),
    ];
    let lib = open_zip(&t, "name.zip", hostile_zip(&specs)).unwrap();
    let e = lib.load("parts/a.dat").err().expect("refused");
    assert!(e.to_string().contains("local header"), "{e}");
}

#[test]
fn zip_entries_that_claim_too_much_are_refused_when_indexed() {
    let t = Tmp::new("declared");
    // Five entries claiming 400 MB each: over 1 GiB in all, whatever they hold.
    let specs: Vec<Spec> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| Spec {
            claimed_size: Some(400_000_000),
            ..Spec::new(Box::leak(format!("ldraw/parts/{n}.dat").into_boxed_str()), b"0 x\n")
        })
        .collect();
    let e = open_zip(&t, "claims.zip", hostile_zip(&specs))
        .err()
        .expect("refused");
    assert!(e.to_string().contains("claim"), "{e}");
}

#[test]
fn the_crc_and_the_size_of_an_entry_are_checked() {
    let t = Tmp::new("crc");
    let brick = b"0 Brick\n0 Name: a.dat\n";
    for (what, spec, want) in [
        (
            "flipped byte, stored",
            Spec {
                corrupt: true,
                ..Spec::new("ldraw/parts/a.dat", brick)
            },
            "CRC-32",
        ),
        (
            "wrong crc",
            Spec {
                crc: Some(0x1234_5678),
                ..Spec::new("ldraw/parts/a.dat", brick)
            },
            "CRC-32",
        ),
        (
            "wrong size, deflated",
            Spec {
                deflate: true,
                claimed_size: Some(5),
                ..Spec::new("ldraw/parts/a.dat", brick)
            },
            "size",
        ),
    ] {
        let specs = [spec, Spec::new("ldraw/p/p.dat", b"0 p\n")];
        let lib = open_zip(&t, "crc.zip", hostile_zip(&specs)).unwrap();
        let e = lib.load("parts/a.dat").err().unwrap_or_else(|| panic!("{what}"));
        assert!(e.to_string().contains(want), "{what}: {e}");
    }
    // An undamaged entry loads, and crc32 is the zip one.
    let specs = [
        Spec::new("ldraw/parts/a.dat", brick),
        Spec::new("ldraw/p/p.dat", b"0 p\n"),
    ];
    let lib = open_zip(&t, "good.zip", hostile_zip(&specs)).unwrap();
    assert!(lib.load("parts/a.dat").is_ok());
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn a_damaged_deflate_stream_error_is_short() {
    let t = Tmp::new("inflate");
    // A valid stream that stops half way inflates to a large partial output; the error must not
    // carry it (63 MB of message before the fix).
    let data = vec![b'0'; 8 * 1024 * 1024];
    let mut comp = miniz_oxide::deflate::compress_to_vec(&data, 6);
    let cut = comp.len() / 2;
    comp.truncate(cut);
    let mut zip = hostile_zip(&[
        Spec::new("ldraw/parts/a.dat", b"x"),
        Spec::new("ldraw/p/p.dat", b"0 p\n"),
    ]);
    // Replace entry a's stored data by the cut deflate stream (method 8) in both headers.
    let local = 30 + "ldraw/parts/a.dat".len();
    let mut patched = zip[..local].to_vec();
    patched[8..10].copy_from_slice(&8u16.to_le_bytes());
    patched[18..22].copy_from_slice(&(comp.len() as u32).to_le_bytes());
    patched[22..26].copy_from_slice(&(data.len() as u32).to_le_bytes());
    patched.extend(&comp);
    let shift = comp.len() as i64 - 1;
    let rest = zip.split_off(local + 1);
    let mut rest = rest;
    // Fix the directory: offsets, sizes and method of both records, and the end record.
    let cd = rest.windows(4).position(|w| w == [0x50, 0x4b, 1, 2]).unwrap();
    let mut i = cd;
    for rec in 0..2 {
        if rec == 0 {
            rest[i + 10..i + 12].copy_from_slice(&8u16.to_le_bytes());
            rest[i + 20..i + 24].copy_from_slice(&(comp.len() as u32).to_le_bytes());
            rest[i + 24..i + 28].copy_from_slice(&(data.len() as u32).to_le_bytes());
        } else {
            let off = u32::from_le_bytes(rest[i + 42..i + 46].try_into().unwrap());
            rest[i + 42..i + 46].copy_from_slice(&((i64::from(off) + shift) as u32).to_le_bytes());
        }
        let n = u16::from_le_bytes([rest[i + 28], rest[i + 29]]) as usize;
        i += 46 + n;
    }
    let eocd = rest.windows(4).rposition(|w| w == [0x50, 0x4b, 5, 6]).unwrap();
    let cd_off = u32::from_le_bytes(rest[eocd + 16..eocd + 20].try_into().unwrap());
    rest[eocd + 16..eocd + 20].copy_from_slice(&((i64::from(cd_off) + shift) as u32).to_le_bytes());
    patched.extend(rest);
    let lib = open_zip(&t, "cut-stream.zip", patched).unwrap();
    let e = lib.load("parts/a.dat").err().expect("does not inflate");
    let text = e.to_string();
    assert!(text.contains("cannot inflate"), "{text}");
    assert!(text.len() < 400, "{} bytes of error", text.len());
}

// ------------------------------------------------------------ import budget

/// A folder library of `n` brick parts, each referencing a one-triangle primitive `refs` times.
fn heavy_library(t: &Tmp, n: usize, refs: usize) {
    t.write("ldraw/p/tri.dat", &format!("0 tri\n0 !LDRAW_ORG Primitive\n{CERT}{TRI}"));
    for i in 0..n {
        let name = 1000 + i;
        t.write(
            &format!("ldraw/parts/{name}.dat"),
            &format!(
                "0 Brick {name}\n0 Name: {name}.dat\n0 Author: t\n0 !LDRAW_ORG Part UPDATE 2026-01\n0 !LICENSE CC\n0 !CATEGORY Brick\n{CERT}{}",
                "1 16 0 0 0 1 0 0 0 1 0 0 0 1 tri.dat\n".repeat(refs)
            ),
        );
    }
}

#[test]
fn one_work_budget_covers_the_whole_import() {
    let t = Tmp::new("budget");
    // 300 parts of 1,000 references: each is inside every per-part limit, together they are
    // 900,000 units of work.
    heavy_library(&t, 300, 1000);
    let opts = ImportOptions {
        include_unconnected: true,
        limit: None,
        ..ImportOptions::default()
    };
    let (pack, report) = import(&t.0, &opts, &mut quiet).expect("within the default budget");
    assert_eq!(pack.header.parts.len(), 300);
    // Per part: the scan walk expands the references, the tessellating walk expands them and
    // produces a triangle for each.
    assert_eq!(report.work, 300 * 1000 * 3);
    let small = ImportOptions {
        max_work: 100_000,
        ..opts.clone()
    };
    let e = import(&t.0, &small, &mut quiet).err().expect("over budget");
    assert!(
        matches!(e, openbrick_catalog::ldraw::ImportError::Library(LibraryError::WorkBudget(100_000))),
        "{e}"
    );
    assert!(e.to_string().contains("100000"), "{e}");
    // Exactly the work done is allowed.
    let exact = ImportOptions {
        max_work: report.work,
        ..opts
    };
    assert!(import(&t.0, &exact, &mut quiet).is_ok());
}

#[test]
fn when_every_part_fails_the_error_says_why() {
    let t = Tmp::new("allfail");
    // Parts that refer to themselves: each fails in the walk.
    for i in 0..5 {
        let name = 2000 + i;
        t.write(
            &format!("ldraw/parts/{name}.dat"),
            &format!(
                "0 Brick {name}\n0 Name: {name}.dat\n0 Author: t\n0 !LDRAW_ORG Part UPDATE 2026-01\n0 !LICENSE CC\n0 !CATEGORY Brick\n{CERT}1 16 0 0 0 1 0 0 0 1 0 0 0 1 {name}.dat\n"
            ),
        );
    }
    t.write("ldraw/p/.keep.dat", "");
    let e = import(&t.0, &ImportOptions::default(), &mut quiet)
        .err()
        .expect("nothing to write");
    let text = e.to_string();
    assert!(text.contains("5 failed to read"), "{text}");
    assert!(text.contains("parts/2000.dat"), "{text}");
    assert!(text.contains("refers to itself"), "{text}");
    assert!(!text.contains("category filter"), "the filter is not the problem: {text}");
    // An empty category list result still points at the filter.
    let opts = ImportOptions {
        categories: Some(vec!["Nope".into()]),
        ..ImportOptions::default()
    };
    let e = import(&t.0, &opts, &mut quiet).err().unwrap();
    assert!(e.to_string().contains("category filter"), "{e}");
}

#[test]
fn hostile_descriptions_are_made_printable_and_short_in_the_pack() {
    let t = Tmp::new("hostile-text");
    let desc = format!("Brick \u{202E}gnirts \u{1b}[31m{}", "long ".repeat(400));
    t.write(
        "ldraw/parts/3001.dat",
        &format!(
            "0 {desc}\n0 Name: 3001.dat\n0 Author: A\u{200B}B\u{202E} {}\n0 !LDRAW_ORG Part UPDATE 2026-01\n0 !LICENSE CC\n0 !CATEGORY Brick\n{CERT}1 16 0 0 0 10 0 0 0 4 0 0 0 10 cube.dat\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 stud.dat\n",
            "x".repeat(2000)
        ),
    );
    t.write("ldraw/p/cube.dat", &std::fs::read_to_string(fixture().join("ldraw/p/cube.dat")).unwrap());
    t.write("ldraw/p/stud.dat", &std::fs::read_to_string(fixture().join("ldraw/p/stud.dat")).unwrap());
    t.write("ldraw/CAreadme.txt", "agreement\u{202E}\nline two\ttab\u{1b}[2J\n");
    let opts = ImportOptions {
        include_unconnected: true,
        ..ImportOptions::default()
    };
    let (pack, _) = import(&t.0, &opts, &mut quiet).expect("imports");
    let p = &pack.header.parts[0];
    assert!(p.name.chars().count() <= 256 && p.name.ends_with('…'), "{}", p.name);
    assert!(p.author.chars().count() <= 512);
    for text in [&p.name, &p.author] {
        assert!(!openbrick_catalog::text::has_unsafe(text), "{text:?}");
    }
    assert!(p.name.contains('\u{FFFD}'));
    let licence = &pack.header.licences[0].text;
    assert_eq!(licence, "agreement\u{FFFD}\nline two\ttab\u{FFFD}[2J\n");
    // And the file reads back through every check.
    Pack::from_bytes(&pack.to_bytes().unwrap()).expect("the pack reads back");
}

#[test]
fn asking_for_a_figure_category_by_name_imports_no_figure_part() {
    let t = Tmp::new("figures");
    t.write("ldraw/p/tri.dat", &format!("0 tri\n0 !LDRAW_ORG Primitive\n{CERT}{TRI}"));
    for (n, desc, cat) in [
        (1001, "Brick 1 x 1", "Brick"),
        (1002, "Figure Friends Girl Torso", "Figure"),
        (1003, "Hair Accessory", "_Figure Accessory"),
        (1004, "Construction Hand", "~Constraction"),
        (1005, "Doll Hair", "Brick"),
    ] {
        let desc = if n == 1005 { "Mini-Doll Hair" } else { desc };
        t.write(
            &format!("ldraw/parts/{n}.dat"),
            &format!("0 {desc}\n0 Name: {n}.dat\n0 Author: t\n0 !LDRAW_ORG Part UPDATE 2026-01\n0 !LICENSE CC\n0 !CATEGORY {cat}\n{CERT}1 16 0 0 0 1 0 0 0 1 0 0 0 1 tri.dat\n"),
        );
    }
    let opts = ImportOptions {
        categories: Some(
            ["Brick", "Figure", "Figure Accessory", "_Figure Accessory", "Constraction", "~Constraction"]
                .map(String::from)
                .to_vec(),
        ),
        include_unconnected: true,
        ..ImportOptions::default()
    };
    let (pack, report) = import(&t.0, &opts, &mut quiet).expect("the brick imports");
    let ids: Vec<&str> = pack.header.parts.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["ld.1001"]);
    assert_eq!(report.excluded.minifig, 4);
}

// ------------------------------------------------------------ properties

#[test]
fn random_libraries_never_panic() {
    // Random type-1 soups over a few names, with random matrices: the walker either
    // finishes or returns an error, and whatever mesh comes out is finite and indexed in range.
    let mut s = 0xD1B5_4A32_D192_ED03_u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for round in 0..40 {
        let t = Tmp::new(&format!("rand{round}"));
        let names = ["a", "b", "c", "d"];
        let mut files = Vec::new();
        for n in names {
            let mut text = format!("0 {n}\n0 !LDRAW_ORG Part\n");
            if next() % 2 == 0 {
                text.push_str(CERT);
            }
            for _ in 0..(next() % 12) {
                let pick = |next: &mut dyn FnMut() -> u64| (next() % 7) as f64 - 3.0;
                match next() % 5 {
                    0 => text.push_str(&format!(
                        "1 16 {} {} {} {} {} {} {} {} {} {} {} {} {}.dat\n",
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next),
                        names[(next() % 4) as usize]
                    )),
                    1 => text.push_str("0 BFC INVERTNEXT\n"),
                    2 => text.push_str(&format!(
                        "4 16 {} 0 0 1 0 {} 1 1 1 0 1 {}\n",
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next)
                    )),
                    _ => text.push_str(&format!(
                        "3 16 {} {} 0 1 0 0 0 {} 1\n",
                        pick(&mut next),
                        pick(&mut next),
                        pick(&mut next)
                    )),
                }
            }
            files.push((format!("parts/{n}.dat"), text));
        }
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let lib = lib_with(&t, &refs);
        for n in names {
            let path = format!("parts/{n}.dat");
            let limits = Limits {
                max_depth: 6,
                max_refs: 5_000,
                max_triangles: 20_000,
            };
            let mut c = Collect::new(connect::is_stud);
            if Walker::new(&lib, limits)
                .walk(&mut c, &path, &Mat::IDENTITY)
                .is_ok()
            {
                let m = IndexedMesh::from_soup(&c.soup);
                assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
                assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
                let _ = connect::boxes(&c.body, &c.studs);
                let _ = connect::synth_studf(&c.soup, None);
            }
            let _ = connect::derive(&lib, &path, limits);
        }
    }
}

#[test]
fn primitive_names_classify_as_the_q04_table_says() {
    for (name, want) in [
        ("p/stud.dat", Some(Prim::Stud)),
        ("p/48/stud.dat", Some(Prim::Stud)),
        ("p/studel.dat", Some(Prim::Stud)),
        ("p/stud-logo4.dat", Some(Prim::Stud)),
        ("p/stud4.dat", Some(Prim::UnderTube)),
        ("p/peghole.dat", Some(Prim::PegHole)),
        ("p/peghole2.dat", Some(Prim::PegHole)),
        ("p/beamhole.dat", Some(Prim::BeamHole)),
        ("p/axlehole.dat", Some(Prim::AxleHole)),
        ("p/axlehol8.dat", Some(Prim::AxleShaft)),
        ("p/axl3hol8.dat", Some(Prim::AxleHole)),
        ("p/connect.dat", Some(Prim::Pin)),
        ("p/confric3.dat", Some(Prim::Pin)),
        ("p/clip3.dat", Some(Prim::Joint("clip"))),
        ("p/joint8ball.dat", Some(Prim::Joint("ball"))),
        ("p/4-4cyli.dat", None),
        ("parts/stud.dat", None),
        ("p/studs.dat", None),
    ] {
        assert_eq!(classify(name), want, "{name}");
    }
}

#[test]
fn lattice_snapping_of_axes_and_points() {
    assert_eq!(connect::lattice_axis([0.0, -3.0, 0.0]), Some([0, -1, 0]));
    assert_eq!(connect::lattice_axis([0.5, -3.0, 0.0]), None);
    assert_eq!(connect::lattice_axis([0.0, 0.0, 0.0]), None);
    assert_eq!(
        connect::lattice_point([10.01, 4.0, -9.99]),
        Some([10, 4, -10])
    );
    assert_eq!(connect::lattice_point([10.5, 4.0, 0.0]), None);
}

// ------------------------------------------------------------ a real library, when there is one

#[test]
fn a_real_library_imports_when_one_is_given() {
    let Some(dir) = std::env::var_os("OPENBRICK_LDRAW_DIR") else {
        eprintln!("skipped: set OPENBRICK_LDRAW_DIR to an LDraw library folder or complete.zip to run this test");
        return;
    };
    let opts = ImportOptions {
        limit: Some(60),
        categories: Some(["Brick", "Plate", "Tile"].map(String::from).to_vec()),
        ..ImportOptions::default()
    };
    let (pack, report) = import(Path::new(&dir), &opts, &mut quiet).expect("the library imports");
    assert!(report.counts.scanned > 10_000, "{report:?}");
    assert!(report.excluded.minifig > 0, "minifigure parts are left out");
    assert!(!pack.header.parts.is_empty());
    let id = |s: &str| pack.header.parts.iter().find(|p| p.id == s);
    // Plain bricks and plates: studs and receptacles are exactly the footprint.
    for (part, studs) in [
        ("ld.3001", 8),
        ("ld.3003", 4),
        ("ld.3005", 1),
        ("ld.3020", 8),
        ("ld.3022", 4),
    ] {
        let Some(p) = id(part) else { continue };
        let s = p
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::StudM)
            .count();
        let f = p
            .connectors
            .iter()
            .filter(|c| c.kind == ConnKind::TubeF)
            .count();
        assert_eq!((s, f), (studs, studs), "{part}");
    }
    for p in &pack.header.parts {
        assert!(
            !p.name.to_ascii_lowercase().contains("minifig"),
            "{}",
            p.name
        );
    }
}
