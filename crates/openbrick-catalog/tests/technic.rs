//! The occupancy masks and Technic connectors of the synthetic mini Technic library
//! (`tests/fixtures/technic`, written for these tests, CC0).
#![cfg(feature = "import")]

use std::path::{Path, PathBuf};

use openbrick_catalog::ldraw::{import, ImportOptions, Resolution};
use openbrick_catalog::{ConnKind, Pack};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/technic")
}

fn pack(resolution: Resolution, categories: &[&str]) -> Pack {
    let opts = ImportOptions {
        resolution,
        categories: Some(categories.iter().map(|s| (*s).to_string()).collect()),
        include_unconnected: true,
        ..ImportOptions::default()
    };
    import(&dir(), &opts, &mut |_, _, _| {}).expect("imports").0
}

#[test]
fn technic_is_off_by_default_and_on_when_asked() {
    let default = import(&dir(), &ImportOptions::default(), &mut |_, _, _| {});
    // The default categories hold the plain brick and the plates; none of the Technic category.
    let (p, _) = default.expect("the plain parts import");
    let ids: Vec<&str> = p.header.parts.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["ld.3005", "ld.4085", "ld.9103"]);
    assert!(p.header.parts.iter().all(|p| p.category != "Technic"));
    let both = pack(Resolution::Standard, &["Brick", "Technic"]);
    assert_eq!(
        both.header.parts.len(),
        7,
        "the plain brick, the pin, two beams, the axle, the ball and the socket"
    );
}

#[test]
fn the_holes_pins_and_axles_are_derived_with_their_axes() {
    let p = pack(Resolution::Standard, &["Brick", "Technic"]);
    let get = |id: &str| p.header.parts.iter().find(|x| x.id == id).unwrap();
    let kinds = |id: &str, k: ConnKind| -> Vec<([i32; 3], [i32; 3])> {
        get(id)
            .connectors
            .iter()
            .filter(|c| c.kind == k)
            .map(|c| (c.pos, c.dir))
            .collect()
    };
    assert_eq!(
        kinds("ld.3701", ConnKind::PinF),
        vec![
            ([0, 12, -20], [1, 0, 0]),
            ([0, 12, 0], [1, 0, 0]),
            ([0, 12, 20], [1, 0, 0])
        ]
    );
    assert_eq!(
        kinds("ld.3704", ConnKind::AxleF),
        vec![([0, 12, 0], [1, 0, 0])]
    );
    assert_eq!(
        kinds("ld.3673", ConnKind::PinM),
        vec![([-10, 0, 0], [1, 0, 0]), ([10, 0, 0], [1, 0, 0])]
    );
    assert_eq!(
        kinds("ld.3705", ConnKind::AxleM)
            .iter()
            .map(|c| c.0[0])
            .collect::<Vec<_>>(),
        vec![-30, -10, 10, 30]
    );
}

#[test]
fn masks_are_the_same_whatever_the_render_resolution() {
    // The importer chooses the library's p/8, p or p/48 primitives for the render mesh; the
    // collision mask is built at the standard resolution whatever was chosen (docs/plan/07
    // section 6: collision never derives from the level of detail).
    let low = pack(Resolution::Low, &["Brick", "Technic"]);
    let std_ = pack(Resolution::Standard, &["Brick", "Technic"]);
    let high = pack(Resolution::High, &["Brick", "Technic"]);
    assert_ne!(
        low.mesh(1).unwrap().indices.len(),
        high.mesh(1).unwrap().indices.len(),
        "the render meshes differ"
    );
    for i in 0..std_.header.parts.len() {
        let (a, b, c) = (low.mask(i), std_.mask(i), high.mask(i));
        assert_eq!(a, b, "part {i}");
        assert_eq!(b, c, "part {i}");
        assert_eq!(
            std_.header.parts[i].mask, low.header.parts[i].mask,
            "part {i}"
        );
    }
    assert!(std_.mask(1).is_some());
}

#[test]
fn a_pack_with_masks_is_byte_for_byte_reproducible() {
    let a = pack(Resolution::Standard, &["Brick", "Technic"])
        .to_bytes()
        .unwrap();
    let b = pack(Resolution::Standard, &["Brick", "Technic"])
        .to_bytes()
        .unwrap();
    assert_eq!(a, b);
    let back = Pack::from_bytes(&a).unwrap();
    assert!(back.mask(0).is_some() || back.mask(1).is_some());
}

#[test]
fn a_clip_the_table_does_not_calibrate_only_flags_the_part() {
    // clip4 has no cylinder of the bar radius: it is not in standards/joints-v1.json, so the part
    // keeps its studs and is flagged with the joint kind, as before the table existed.
    let p = pack(Resolution::Standard, &["Plate"]);
    let c = p.header.parts.iter().find(|x| x.id == "ld.9103").unwrap();
    assert_eq!(c.joints, vec!["clip".to_string()]);
    assert!(c.connectors.iter().all(|k| k.kind != ConnKind::ClipF));
    assert!(c.connectors.iter().any(|k| k.kind == ConnKind::StudM));
    // The plate with a clip the table does know has the connector and no flag.
    let ok = p.header.parts.iter().find(|x| x.id == "ld.4085").unwrap();
    assert!(ok.joints.is_empty());
    let clip: Vec<_> = ok
        .connectors
        .iter()
        .filter(|k| k.kind == ConnKind::ClipF)
        .collect();
    assert_eq!(clip.len(), 1);
    // The table's point (0, 2, 0) in clip3's frame, placed by the part's instance at (0, -4, -20).
    assert_eq!((clip[0].pos, clip[0].dir), ([0, -2, -20], [1, 0, 0]));
}

#[test]
fn bars_come_from_a_cylinder_of_the_bar_radius_in_a_part_that_names_a_bar() {
    let p = pack(Resolution::Standard, &["Bar", "Plate"]);
    let bar = p.header.parts.iter().find(|x| x.id == "ld.87994").unwrap();
    assert_eq!(
        bar.connectors
            .iter()
            .map(|c| (c.kind, c.pos, c.dir))
            .collect::<Vec<_>>(),
        vec![(ConnKind::BarM, [0, 0, 0], [1, 0, 0])]
    );
    // The plate's clip bore is a cylinder of the same radius, but a plate is not a bar: no barM.
    let plate = p.header.parts.iter().find(|x| x.id == "ld.4085").unwrap();
    assert!(plate.connectors.iter().all(|c| c.kind != ConnKind::BarM));
}
