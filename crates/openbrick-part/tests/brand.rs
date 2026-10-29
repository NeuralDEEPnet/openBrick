//! The Neuraldeep.net demo tiles: `parts/ndn.brand-tile.obpart.json` (the
//! portrait mark on a 2x4 tile) and `parts/ndn.brand-badge.obpart.json` (the
//! hexagon badge on a 4x4 tile), with the four drawings in
//! `parts/assets/brand/`. Those drawings are Neural Deep Network Ltd's
//! trademarks, all rights reserved, and not under this repository's licences
//! (`parts/assets/brand/NOTICE.md`); the part documents are CC0.
//!
//! Each drawing is built at the tile it was drawn for (the portrait mark at
//! 2x4 and 4x6, the badge at 4x4 and 2x2; the 2x4 mark and the 4x4 badge are
//! the documents' defaults, built with no parameters but the relief) in every
//! relief: emboss, deboss, the filament-swap inlay and the multi-material
//! (parts) inlay, on fdm-0.4 with the example fit profile. Every build may be
//! exported with no finding at all, so no FEATURE.MIN: the opening removes at
//! most maxRemovedAreaPct of the artwork and the thinnest stroke is at least
//! minFeatureMm.
//!
//! Goldens: the content hash of every build (the same on every target: it
//! covers the document, the parameters, the profiles, the kernel id and the
//! artwork bytes, so any change to a drawing or its header changes it), and
//! for the emboss build the body volume against the analytic tile model, the
//! relief's volume, place and height, the number of separate pieces, and the
//! canonical fingerprint of the accent mesh (asserted on x86-64 Linux only,
//! like the other golden tests; recorded for `manifold-rust 0.19.1`).

mod common;

use common::*;
use openbrick_geom::Mesh;
use openbrick_part::eval::EVALUATOR_REVISION;
use openbrick_part::{validate, Built, MemoryAssets, ParamValues, PartDoc};
use openbrick_print::{example_fit_profile, Offsets};
use serde_json::{json, Value};

const BRAND_TILE: &str = include_str!("../../../parts/ndn.brand-tile.obpart.json");
const BRAND_BADGE: &str = include_str!("../../../parts/ndn.brand-badge.obpart.json");
const LOGO_TILE: &str = include_str!("../../../parts/ndn.logo-tile.obpart.json");
const NOTICE: &str = include_str!("../../../parts/assets/brand/NOTICE.md");

/// The notice the owner wrote, word for word (parts/assets/brand/NOTICE.md).
const OWNER_NOTICE: &str = "Copyright Neural Deep Network Ltd. All rights reserved. \
The Neuraldeep.net marks are trademarks of Neural Deep Network Ltd and are not covered by \
the licences of this repository (MIT for code, CC0 for parts and data). You may use these \
files to build, print and show the OpenBrick3D demo tiles; any other use needs written \
permission.";

/// The copyright line every drawing's header comment starts with.
const COPYRIGHT: &str = "Copyright Neural Deep Network Ltd. All rights reserved.";

/// One drawing in parts/assets/brand/ and the tile it was drawn for.
struct Mark {
    /// The asset name, relative to parts/ as the part documents name it.
    asset: &'static str,
    svg: &'static str,
    /// The part document it is built with.
    doc: &'static str,
    /// The tile it was drawn for.
    size: [u32; 2],
    /// Content hashes of the emboss, deboss, swap inlay and parts inlay builds.
    hashes: [&'static str; 4],
    /// Body plus the separate pieces of the embossed drawing.
    emboss_components: usize,
    /// Canonical fingerprint of the embossed drawing (the accent mesh).
    emboss_accent: u64,
}

const MARK_2X4: Mark = Mark {
    asset: "assets/brand/neuraldeep-mark-2x4.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-mark-2x4.svg"),
    doc: BRAND_TILE,
    size: [2, 4],
    hashes: [
        "7eed249de2c40d5a130d2b35b1ad12fb9a8185e19cc89ee39c3446d4a6f588ba",
        "3426dbb3141f9f489a83fde0e2d014a56864a36bf354daac89c803951ffd80c8",
        "ee6e2edad78063a76002b501d953438223e9a484d595e653eec0994947ad4284",
        "1208c021dfc9c9754fc22d8ec6bb15a4d3f810e00a93f2ecf99fa23df6c6e087",
    ],
    emboss_components: 9,
    emboss_accent: 0x5e64_74d0_3786_9a4d,
};

const MARK_4X6: Mark = Mark {
    asset: "assets/brand/neuraldeep-mark-4x6.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-mark-4x6.svg"),
    doc: BRAND_TILE,
    size: [4, 6],
    hashes: [
        "6bdd71ec528a51f17d92d5a6302d6f21d75a41ab610cf56bf29c1cd346bc9064",
        "5d3ec1efa192d7628b06fbc07fed364e994e63ec60418654de49421f3a3f1c24",
        "c29296b6e1588676a2e5c0e75e452f28a7cd5e1966a1bed8f33f5fe9e16e756a",
        "14b01a24c8ac298b6bd457b4e76a7311360a8624044bd77a47758c97d09066e2",
    ],
    emboss_components: 11,
    emboss_accent: 0xc1ea_78cb_fc36_16a5,
};

const BADGE_4X4: Mark = Mark {
    asset: "assets/brand/neuraldeep-badge-4x4.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-badge-4x4.svg"),
    doc: BRAND_BADGE,
    size: [4, 4],
    hashes: [
        "cb914b448292c6c655969c86a220a76fadc939004d8074c03815ddda651a0726",
        "34540f282624ba2c591448abb984819b93d9eac8a30e4d87dd3c93fe4678175b",
        "170b8a92ac414a19970ac502cc9c9c4e790ee865c8451df689b7036f6d92f9b2",
        "112cb9d611ddd7fac23acc62d42a9ad2e7a0c1a5ddc769feedacb2d95d0b9c0d",
    ],
    emboss_components: 8,
    emboss_accent: 0xbff6_ffa7_2b50_6e91,
};

const BADGE_2X2: Mark = Mark {
    asset: "assets/brand/neuraldeep-badge-2x2.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-badge-2x2.svg"),
    doc: BRAND_BADGE,
    size: [2, 2],
    hashes: [
        "eb9c71abf3d2ef9c81a2c754f2208444feaa890f74fa74cf4e2546434e6201a8",
        "428de2283b122b28799229f4e1c4a7d8872aeebec2daafc9f9b175f191fae519",
        "fa40ec7e2b3a90bea1aa2134ebc1f652162aa28921154c695d2b8e08a9e7ea6d",
        "2cfb0d38246fac3f295251416768f6da55dd521e2c5bb6ebb3da8bc948206ccd",
    ],
    emboss_components: 5,
    emboss_accent: 0xab64_f406_9e9b_6d55,
};

const MARKS: [&Mark; 4] = [&MARK_2X4, &MARK_4X6, &BADGE_4X4, &BADGE_2X2];

/// The relief modes, as (relief, inlayStyle), in the order of `Mark::hashes`.
const MODES: [(&str, &str); 4] = [
    ("emboss", "swap"),
    ("deboss", "swap"),
    ("inlay", "swap"),
    ("inlay", "parts"),
];

fn parse(text: &str) -> PartDoc {
    PartDoc::parse(text).expect("the part document parses")
}

/// The document's default for a parameter.
fn default_of(doc: &PartDoc, name: &str) -> Value {
    doc.params[name].default.clone()
}

/// Whether `mark` is its document's default artwork at its default size.
fn is_default(mark: &Mark) -> bool {
    let doc = parse(mark.doc);
    default_of(&doc, "logo") == json!(mark.asset) && default_of(&doc, "size") == json!(mark.size)
}

/// The parameters for `mark` in one relief mode: only what differs from the
/// document's defaults, so the defaults are built as shipped.
fn params_for(mark: &Mark, (relief, style): (&str, &str)) -> ParamValues {
    let mut params = ParamValues::new();
    if !is_default(mark) {
        params = params
            .with("logo", json!(mark.asset))
            .with("size", json!(mark.size));
    }
    if relief != "emboss" {
        params = params.with("relief", json!(relief));
    }
    if relief == "inlay" {
        params = params.with("inlayStyle", json!(style));
    }
    params
}

fn build_mark(mark: &Mark, mode: (&str, &str)) -> Built {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let assets = MemoryAssets::new().with(mark.asset, mark.svg.as_bytes().to_vec());
    run_with_assets(
        &parse(mark.doc),
        &params_for(mark, mode),
        &p,
        Some(&fit),
        &assets,
        false,
    )
    .unwrap_or_else(|e| panic!("{} {mode:?}: {e}", mark.asset))
}

/// The `viewBox` of a drawing: min x, min y, width, height.
fn view_box(svg: &str) -> [f64; 4] {
    let start = svg.find("viewBox=\"").expect("a viewBox") + "viewBox=\"".len();
    let end = start + svg[start..].find('"').expect("closed");
    let v: Vec<f64> = svg[start..end]
        .split_whitespace()
        .map(|x| x.parse().expect("a number"))
        .collect();
    v.try_into().expect("four numbers")
}

/// The tile's top chamfer from its `round` field ("0.3mm" in the documents).
fn top_chamfer(doc: &str) -> f64 {
    let v: Value = serde_json::from_str(doc).expect("JSON");
    v["features"][0]["round"]["radius"]
        .as_str()
        .and_then(|s| s.strip_suffix("mm"))
        .and_then(|s| s.parse().ok())
        .expect("a radius in mm")
}

/// Pieces of a mesh connected through shared vertices (one closed shell is one).
fn shells(mesh: &Mesh) -> usize {
    let mut parent: Vec<usize> = (0..mesh.positions.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let a = find(&mut parent, t[0] as usize);
        for &v in &t[1..] {
            let b = find(&mut parent, v as usize);
            parent[b] = a;
        }
    }
    let mut roots: Vec<usize> = mesh
        .indices
        .iter()
        .map(|&i| find(&mut parent, i as usize))
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots.len()
}

/// Build `mark` in every relief mode and check the rules and the goldens.
fn check_mark(mark: &Mark) {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let max_removed = p
        .max_removed_area_pct
        .expect("fdm-0.4 sets maxRemovedAreaPct");
    for (i, mode) in MODES.into_iter().enumerate() {
        let what = format!(
            "{} at {}x{} {mode:?}",
            mark.asset, mark.size[0], mark.size[1]
        );
        let b = build_mark(mark, mode);
        eprintln!("{what}: content hash {}", b.content_hash);
        assert!(
            b.warnings.iter().all(|w| !w.contains("skipped")),
            "{what}: {:?}",
            b.warnings
        );
        let m = &b.measurements;
        assert!(m.is_manifold, "{what}");
        let roles: Vec<&str> = b.objects.iter().map(|o| o.role.as_str()).collect();
        let want: &[&str] = if mode.0 == "deboss" {
            &["body"]
        } else {
            &["body", "accent"]
        };
        assert_eq!(roles, want, "{what}");

        // The validator: export allowed, and nothing to report at all.
        let report = validate(&b, &p, Some(&fit));
        assert!(report.export_allowed(), "{what}: {:#?}", report.findings);
        assert!(
            report.findings.iter().all(|f| f.rule != "FEATURE.MIN"),
            "{what}: {:#?}",
            report.findings
        );
        assert!(report.findings.is_empty(), "{what}: {:#?}", report.findings);
        let pct = m.removed_area_pct.expect("measured");
        eprintln!(
            "{what}: removed {pct:.3} %, thinnest stroke {:?} mm",
            m.min_stroke_mm
        );
        assert!(
            pct <= max_removed,
            "{what}: removed {pct} % > {max_removed} %"
        );
        let stroke = m.min_stroke_mm.expect("measured");
        assert!(
            stroke >= p.min_feature_mm,
            "{what}: thinnest stroke {stroke} mm < {} mm",
            p.min_feature_mm
        );
        let art = b.artwork.as_ref().expect("an artwork report");
        assert_eq!(art.strokes_ignored, 0, "{what}: fills only");
        assert_eq!(art.mode, mode.0, "{what}");

        if mode.0 == "emboss" {
            check_emboss(mark, &b, &fit.offsets_mm, &what);
        }
        assert_eq!(
            b.content_hash, mark.hashes[i],
            "{what}: the content hash changed (evaluator revision {EVALUATOR_REVISION}); \
             a drawing, a part document or the evaluator changed"
        );
    }
}

/// The emboss build against the analytic tile and the drawing's fitted frame.
fn check_emboss(mark: &Mark, b: &Built, offsets: &Offsets, what: &str) {
    let p = process("fdm-0.4");
    let doc = parse(mark.doc);
    let (body, accent) = (&b.objects[0], &b.objects[1]);

    // The body is the plain tile with the fit offsets, as the analytic model
    // of tests/common builds it: the relief is its own object.
    let shape = Shape {
        size: mark.size,
        plates: 1,
        studs: false,
        tubes: true,
        top_chamfer: top_chamfer(mark.doc),
    };
    let tile_v = analytic_volume(shape, &p, offsets);
    assert!(
        ((body.volume_mm3 - tile_v) / tile_v).abs() < 1e-6,
        "{what}: body {} vs analytic {tile_v}",
        body.volume_mm3
    );

    // The relief: the opened drawing raised by reliefHeight on the face.
    let h = default_of(&doc, "reliefHeight")
        .as_f64()
        .expect("a numeric reliefHeight");
    let art = b.artwork.as_ref().expect("report");
    assert_eq!(art.height_mm, h);
    assert!(
        (art.area_after_mm2 * h - accent.volume_mm3).abs() < 1e-6 * accent.volume_mm3,
        "{what}: accent {} vs area {} x {h}",
        accent.volume_mm3,
        art.area_after_mm2
    );
    assert!(art.area_after_mm2 <= art.area_before_mm2);

    // The face box: the body footprint inside the top chamfer and the margin.
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let footprint = mark.size.map(|n| f64::from(n) * pitch - 2.0 * gap);
    let margin = default_of(&doc, "margin").as_f64().expect("a margin");
    let inset = top_chamfer(mark.doc) + margin;
    for (k, side) in footprint.iter().enumerate() {
        assert!((art.box_min_mm[k] - inset).abs() < 1e-9, "{what}");
        assert!((art.box_max_mm[k] - (side - inset)).abs() < 1e-9, "{what}");
    }
    // keepAspect fits the viewBox into the face box, centred: k mm per unit.
    // The drawing lies inside that frame (it need not touch it), and covers
    // at most its area.
    let [_, _, vw, vh] = view_box(mark.svg);
    let box_size = [footprint[0] - 2.0 * inset, footprint[1] - 2.0 * inset];
    let k = (box_size[0] / vw).min(box_size[1] / vh);
    let frame = [vw * k, vh * k];
    let centre = footprint.map(|f| f / 2.0);
    let face_z = art.face_z_mm;
    assert_eq!(face_z, mm("plate_height"), "{what}: a plate-height tile");
    let (lo, hi) = accent.mesh.bounds();
    for a in 0..2 {
        let (l, u) = (f64::from(lo[a]), f64::from(hi[a]));
        assert!(
            l >= centre[a] - frame[a] / 2.0 - 1e-4 && u <= centre[a] + frame[a] / 2.0 + 1e-4,
            "{what}: axis {a} spans {l} to {u}, outside the {}-mm frame",
            frame[a]
        );
    }
    assert!((f64::from(lo[2]) - face_z).abs() < 1e-4, "{what}");
    assert!((f64::from(hi[2]) - (face_z + h)).abs() < 1e-4, "{what}");
    assert!(art.area_before_mm2 <= frame[0] * frame[1]);

    // Topology: the body and the drawing's separate pieces, each a closed
    // shell; fused, they are one solid.
    let m = &b.measurements;
    eprintln!(
        "{what}: {} components, genus {}, accent {} triangles, canonical 0x{:016x}",
        m.components,
        m.genus,
        accent.mesh.triangle_count(),
        accent.mesh.canonical_fingerprint()
    );
    assert_eq!(m.components, mark.emboss_components, "{what}");
    assert_eq!(
        shells(&body.mesh) + shells(&accent.mesh),
        mark.emboss_components,
        "{what}"
    );
    let fused = b.fused.as_ref().expect("fused");
    assert_eq!(shells(&fused.mesh), 1, "{what}: one solid");
    assert!((fused.volume_mm3 - m.volume_mm3).abs() < 1e-6 * m.volume_mm3);
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        assert_eq!(
            accent.mesh.canonical_fingerprint(),
            mark.emboss_accent,
            "{what}: the embossed drawing changed (evaluator revision {EVALUATOR_REVISION})"
        );
    }
}

#[test]
fn the_brand_tile_builds_its_default_mark_at_2x4() {
    assert!(is_default(&MARK_2X4), "the brand tile's default");
    check_mark(&MARK_2X4);
}

#[test]
fn the_portrait_mark_for_4x6_builds_at_4x6() {
    check_mark(&MARK_4X6);
}

#[test]
fn the_brand_badge_builds_its_default_badge_at_4x4() {
    assert!(is_default(&BADGE_4X4), "the brand badge's default");
    check_mark(&BADGE_4X4);
}

#[test]
fn the_badge_for_2x2_builds_at_2x2() {
    check_mark(&BADGE_2X2);
}

#[test]
fn the_marks_carry_their_notice_and_stay_fill_only() {
    // The owner's notice, word for word, and a line for every file.
    assert!(NOTICE.contains(OWNER_NOTICE), "NOTICE.md lacks the notice");
    for mark in MARKS {
        let file = mark.asset.rsplit('/').next().expect("a file name");
        assert!(
            NOTICE.contains(&format!("`{file}`")),
            "NOTICE.md lacks {file}"
        );

        // The header comment: the copyright line, the drawing, its target
        // tile and where it comes from.
        let svg = mark.svg;
        assert!(svg.starts_with("<?xml"), "{file}");
        let header = &svg[..svg.find("<svg").expect("an svg element")];
        assert!(header.contains(&format!("<!--\n  {COPYRIGHT}\n")), "{file}");
        let target = format!("Target tile: {}x{}.", mark.size[0], mark.size[1]);
        assert!(header.contains(&target), "{file}: {target}");
        assert!(
            header.contains("Derived from the Neural Deep Network Ltd website illustration."),
            "{file}"
        );
        assert!(header.contains("NOTICE.md"), "{file}");

        // Fill-only: outside comments, no text, images, CSS or strokes, and
        // every path is filled black with the even-odd rule.
        let mut body = String::new();
        let mut rest = &svg[header.len()..];
        while let Some(open) = rest.find("<!--") {
            body.push_str(&rest[..open]);
            let close = rest[open..].find("-->").expect("closed comment");
            rest = &rest[open + close + 3..];
        }
        body.push_str(rest);
        for banned in [
            "<text", "<image", "<style", "stroke", "style=", "class=", "<use",
        ] {
            assert!(!body.contains(banned), "{file} contains {banned}");
        }
        let paths = body.matches("<path").count();
        assert!(paths > 0, "{file}");
        assert_eq!(
            body.matches(r##"<path fill="#000" fill-rule="evenodd" d=""##)
                .count(),
            paths,
            "{file}: every path is a black even-odd fill"
        );
    }

    // The part documents: CC0 themselves, crediting the drawing and its notice.
    for (text, id, name, logo, size) in [
        (
            BRAND_TILE,
            "ndn.brand-tile",
            "Neuraldeep.net tile",
            MARK_2X4.asset,
            [2, 4],
        ),
        (
            BRAND_BADGE,
            "ndn.brand-badge",
            "Neuraldeep.net badge",
            BADGE_4X4.asset,
            [4, 4],
        ),
    ] {
        let doc = parse(text);
        assert_eq!((doc.id.as_str(), doc.name.as_str()), (id, name));
        assert_eq!(doc.license, "CC0-1.0", "{id}");
        assert_eq!(default_of(&doc, "logo"), json!(logo), "{id}");
        assert_eq!(default_of(&doc, "size"), json!(size), "{id}");
        assert!(
            doc.credit.author.contains("parts/assets/brand/NOTICE.md"),
            "{id}: the credit names the notice"
        );
        let help = doc.params["logo"].help.as_deref().unwrap_or_default();
        assert!(help.contains("assets/brand/NOTICE.md"), "{id}: {help}");
        assert!(
            doc.provenance
                .history
                .iter()
                .any(|h| h.contains("parts/assets/brand/NOTICE.md")),
            "{id}"
        );
        // Everything else is the logo tile's design.
        let logo_tile = parse(LOGO_TILE);
        assert_eq!(
            serde_json::to_value(&doc.features).unwrap(),
            serde_json::to_value(&logo_tile.features).unwrap(),
            "{id}: the logo tile's features"
        );
    }
    // The CC0 sample stays the logo tile's default.
    assert_eq!(
        default_of(&parse(LOGO_TILE), "logo"),
        json!("assets/mark.svg")
    );
}
