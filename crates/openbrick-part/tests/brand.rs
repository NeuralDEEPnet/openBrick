//! The Neuraldeep.net demo tiles: `parts/ndn.brand-tile.obpart.json` (the
//! NEURALDEEP.NET wordmark on a 2x6 tile) and `parts/ndn.brand-badge.obpart.json`
//! (the wordmark in three lines on a 4x4 tile), with the four wordmark
//! drawings in `parts/assets/brand/` (made by `scripts/brand-text.py` from
//! Inter, SIL Open Font License 1.1). Those drawings are Neural Deep Network
//! Ltd's trademarks, all rights reserved, and not under this repository's
//! licences (`parts/assets/brand/NOTICE.md`); the part documents are CC0.
//!
//! Each drawing is built at the tile it was drawn for (2x6, 2x8, 2x10 and
//! 4x4; the 2x6 and the 4x4 drawings are the documents' defaults, built with
//! no parameters but the relief) in every
//! relief: emboss, deboss, the filament-swap inlay and the multi-material
//! (parts) inlay, on fdm-0.4 with the example fit profile. Every build may be
//! exported with no finding at all, so no FEATURE.MIN: the opening removes at
//! most maxRemovedAreaPct of the artwork and the thinnest stroke is at least
//! minFeatureMm.
//!
//! Goldens: the content hash of every build (it covers the document, the
//! parameters, the profiles, the kernel id and the artwork bytes, so any change
//! to a drawing or its header changes it; it is the same on every target
//! because `.gitattributes` turns off line-ending conversion for the artwork,
//! so a Windows checkout with `core.autocrlf` reads the same LF bytes, which
//! `every_checkout_reads_the_same_artwork_bytes` checks), and
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
const SAMPLE_MARK: &str = include_str!("../../../parts/assets/mark.svg");
const GITATTRIBUTES: &str = include_str!("../../../.gitattributes");

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

const WORDMARK_2X6: Mark = Mark {
    asset: "assets/brand/neuraldeep-wordmark-2x6.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-wordmark-2x6.svg"),
    doc: BRAND_TILE,
    size: [2, 6],
    hashes: [
        "be192d1b14200df74eac6cfdd38791cd28ef243240b3d8d74c1722ff1a7c77f0",
        "04a3a1f0045694dbbdb03ef9a757be949fe6e9a9fce96e6e99ebdc7375b893be",
        "33187fd7815a0c52e946a8e5071cb7baeb6a55c4909752c2f4d37f14a75b814f",
        "a62202076ec9a9a78bc946c17fe3d7b8646daefff7893a22bec9aef49ea9784f",
    ],
    emboss_components: 15,
    emboss_accent: 0xc700_15f4_d59c_f72d,
};

const WORDMARK_2X8: Mark = Mark {
    asset: "assets/brand/neuraldeep-wordmark-2x8.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-wordmark-2x8.svg"),
    doc: BRAND_TILE,
    size: [2, 8],
    hashes: [
        "5b48f995ba067e83ca3fade91dbd2e2c360e88c3833737af741480344963eb77",
        "793274c7d001cd83c87dae39632d70b9d462dce505693f8a63ce49efbdf56b44",
        "fa7fa2e8fdbdcde7dd8a30b5d9701dd7d91a76650b70f0dcdc639882d424284e",
        "7d5471fc25f613590c184c43f3bac5debab8994dfd4bccc822aec142f185e6b9",
    ],
    emboss_components: 15,
    emboss_accent: 0x6941_fbf7_b601_61ed,
};

const WORDMARK_2X10: Mark = Mark {
    asset: "assets/brand/neuraldeep-wordmark-2x10.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-wordmark-2x10.svg"),
    doc: BRAND_TILE,
    size: [2, 10],
    hashes: [
        "210e3177e855a0f52b31d56dd9b5d023215ba8bd9c2bd1d0faf8ea73e4f75e60",
        "f5ab53658b15c8601ac8383ea9e3428688423b53813e226663e47bca8ec43d9d",
        "3d56b3407c77c1173eef1130c059fb68e9a5d237e461d4488188e43481225017",
        "297b2919312f6a398b7e77dba1315efd80648a0cdddc4e7ce1cd6d28c65bce82",
    ],
    emboss_components: 15,
    emboss_accent: 0x2094_75a8_7858_0999,
};

const WORDMARK_4X4: Mark = Mark {
    asset: "assets/brand/neuraldeep-wordmark-4x4.svg",
    svg: include_str!("../../../parts/assets/brand/neuraldeep-wordmark-4x4.svg"),
    doc: BRAND_BADGE,
    size: [4, 4],
    hashes: [
        "cf93bc5d6ed9c39e4b592f47ad3890089db7360eb7a24ee303c4efff2d637aec",
        "29be6e4ec3351ab47064eee9dd7ad8ddfcddc8e1cc255e86e1f91759659ab582",
        "5940ce139e776bffe4ea8c8db2a391f260c387bce67398f25de993c0b43ab039",
        "ed9ad771d1682c840699cebaa81f371dbd5317baa40fdf026ca30ae2656bbe06",
    ],
    emboss_components: 15,
    emboss_accent: 0xf52a_0bab_8571_7dd1,
};

const MARKS: [&Mark; 4] = [&WORDMARK_2X6, &WORDMARK_2X8, &WORDMARK_2X10, &WORDMARK_4X4];

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

        // The validator: export allowed, and nothing to report but, for the
        // swap inlay, the short bridges over the artwork's recesses: printed
        // face down, the face slab leaves the artwork open for the first
        // layers and the body closes it over at the slab's depth
        // (OVERHANG.BRIDGED, a warning).
        let report = validate(&b, &p, Some(&fit));
        assert!(report.export_allowed(), "{what}: {:#?}", report.findings);
        assert!(
            report.findings.iter().all(|f| f.rule != "FEATURE.MIN"),
            "{what}: {:#?}",
            report.findings
        );
        if mode == ("inlay", "swap") {
            assert!(!report.findings.is_empty(), "{what}");
            assert!(
                report.findings.iter().all(|f| f.rule == "OVERHANG.BRIDGED"),
                "{what}: {:#?}",
                report.findings
            );
        } else {
            assert!(report.findings.is_empty(), "{what}: {:#?}", report.findings);
        }
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
fn the_brand_tile_builds_its_default_wordmark_at_2x6() {
    assert!(is_default(&WORDMARK_2X6), "the brand tile's default");
    check_mark(&WORDMARK_2X6);
}

#[test]
fn the_two_line_wordmark_builds_at_2x8() {
    check_mark(&WORDMARK_2X8);
}

#[test]
fn the_one_line_wordmark_builds_at_2x10() {
    check_mark(&WORDMARK_2X10);
}

#[test]
fn the_brand_badge_builds_its_default_wordmark_at_4x4() {
    assert!(is_default(&WORDMARK_4X4), "the brand badge's default");
    check_mark(&WORDMARK_4X4);
}

#[test]
fn every_checkout_reads_the_same_artwork_bytes() {
    // The content hash covers the artwork's bytes, so a CRLF checkout (Git for
    // Windows with core.autocrlf, as on the Windows CI runners) would change
    // every golden hash above. `.gitattributes` turns off line-ending
    // conversion for SVGs and everything under parts/assets/.
    let attributes: Vec<&str> = GITATTRIBUTES.lines().map(str::trim).collect();
    for rule in ["*.svg -text", "parts/assets/** -text"] {
        assert!(attributes.contains(&rule), ".gitattributes lacks `{rule}`");
    }
    for (name, text) in MARKS.iter().map(|m| (m.asset, m.svg)).chain([
        ("assets/mark.svg", SAMPLE_MARK),
        ("assets/brand/NOTICE.md", NOTICE),
    ]) {
        assert!(
            !text.contains('\r'),
            "{name} has CR line endings: the checkout converted it; see .gitattributes"
        );
    }
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

        // The header comment: the copyright line, the text, its target
        // tile, the typeface and its licence, and the script that made it.
        let svg = mark.svg;
        assert!(svg.starts_with("<?xml"), "{file}");
        let header = &svg[..svg.find("<svg").expect("an svg element")];
        assert!(header.contains(&format!("<!--\n  {COPYRIGHT}\n")), "{file}");
        let target = format!("Target tile: {}x{}.", mark.size[0], mark.size[1]);
        assert!(header.contains(&target), "{file}: {target}");
        assert!(header.contains("The NEURALDEEP.NET wordmark: "), "{file}");
        assert!(
            header.contains("Letterforms: Inter (") && header.contains("SIL Open Font License 1.1"),
            "{file}"
        );
        assert!(
            header.contains("Generated by scripts/brand-text.py"),
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
            WORDMARK_2X6.asset,
            [2, 6],
        ),
        (
            BRAND_BADGE,
            "ndn.brand-badge",
            "Neuraldeep.net badge",
            WORDMARK_4X4.asset,
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
