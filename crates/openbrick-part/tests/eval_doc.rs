//! Evaluating whole documents: the example parts in parts/, parameter resolution
//! and bounds, `if` and `visibleWhen`, unsupported ops, content hashes, and the
//! derived connectors of docs/plan/06 section 6.

mod common;

use common::*;
use openbrick_part::eval::{resolve_params, MAX_BODY_PLATES};
use openbrick_part::{EvalError, ParamValues, PartDoc};
use openbrick_print::{example_fit_profile, Offsets};
use openbrick_units::{BRICK_LDU, PLATE_LDU, STUD_PITCH_LDU};
use serde_json::json;

fn logo_tile() -> PartDoc {
    PartDoc::parse(include_str!("../../../parts/ndn.logo-tile.obpart.json")).expect("parses")
}

fn keychain() -> PartDoc {
    PartDoc::parse(include_str!(
        "../../../parts/ndn.nameplate-keychain.obpart.json"
    ))
    .expect("parses")
}

fn unsupported(r: Result<openbrick_part::Built, EvalError>) -> (String, String, String) {
    match r {
        Err(e @ EvalError::Unsupported { .. }) => {
            let text = e.to_string();
            let EvalError::Unsupported { feature, op, .. } = e else {
                unreachable!()
            };
            (feature, op, text)
        }
        Err(other) => panic!("expected Unsupported, got {other}"),
        Ok(_) => panic!("expected Unsupported, got a part"),
    }
}

#[test]
fn logo_tile_reports_its_svg_relief_as_unsupported() {
    let p = process("fdm-0.4");
    let (feature, op, text) = unsupported(run(&logo_tile(), &ParamValues::new(), &p, None, false));
    assert_eq!((feature.as_str(), op.as_str()), ("art", "relief.svg"));
    assert!(
        text.contains("relief.svg") && text.contains("skip_unsupported"),
        "{text}"
    );
}

#[test]
fn logo_tile_builds_without_its_relief_for_a_preview() {
    // docs/plan/06 section 7: a 15.8 x 31.8 x 3.2 mm tile with three tubes, a
    // 0.3 mm top chamfer (`round`), and eight female sockets underneath.
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let b = run(&logo_tile(), &ParamValues::new(), &p, Some(&fit), true).expect("builds");
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("`art`") && w.contains("relief.svg")),
        "{:?}",
        b.warnings
    );
    let m = &b.measurements;
    assert!(m.is_manifold && m.components == 1 && m.genus == 0);
    let shape = Shape {
        size: [2, 4],
        plates: 1,
        studs: false,
        tubes: true,
        top_chamfer: 0.3,
    };
    let v = analytic_volume(shape, &p, &fit.offsets_mm);
    assert!(
        ((m.volume_mm3 - v) / v).abs() < 1e-6,
        "{} vs {v}",
        m.volume_mm3
    );
    assert!(
        (m.bounds_max_mm[2] - mm("plate_height")).abs() < 1e-4,
        "a tile has no studs"
    );
    assert_eq!(b.connectors.len(), 8);
    assert!(b
        .connectors
        .iter()
        .all(|c| c.kind == "tubeF" && c.fit_critical));
    assert_eq!(m.fit_critical_connectors, vec!["tubeF".to_string()]);
    assert_eq!(
        m.allowed_processes,
        vec!["fdm-0.4".to_string(), "fdm-0.2".into(), "resin-msla".into()]
    );

    // Parameters reach the geometry: a 4x4 brick-height tile has nine tubes.
    let params = ParamValues::new()
        .with("size", json!([4, 4]))
        .with("height", json!("brick"));
    let b = run(&logo_tile(), &params, &p, Some(&fit), true).expect("builds");
    assert_eq!(b.connectors.len(), 16);
    assert!(b.connectors.iter().all(|c| c.pos[1] == BRICK_LDU));
    assert!((b.measurements.bounds_max_mm[2] - mm("brick_height")).abs() < 1e-4);
}

#[test]
fn keychain_label_and_loop_are_unsupported_until_turned_off() {
    let p = process("fdm-0.4");
    let doc = keychain();
    // Defaults: no studs, so the text label is active (and the loop after it).
    let (feature, op, _) = unsupported(run(&doc, &ParamValues::new(), &p, None, false));
    assert_eq!((feature.as_str(), op.as_str()), ("label", "relief.text"));
    // Studs on top turns the label off; the loop is still on.
    let studs = ParamValues::new().with("studsOnTop", json!(true));
    let (feature, op, _) = unsupported(run(&doc, &studs, &p, None, false));
    assert_eq!((feature.as_str(), op.as_str()), ("ring", "loop.keyring"));
    // With no loop the part is a plain 2x8 plate with studs and tubes.
    let plain = studs.clone().with("loop", json!("none"));
    let b = run(&doc, &plain, &p, None, false).expect("builds");
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    let kinds = |k: &str| b.connectors.iter().filter(|c| c.kind == k).count();
    assert_eq!((kinds("studM"), kinds("tubeF")), (16, 16));
    // The 1-wide variant has pins instead of tubes.
    let narrow = plain.with("width", json!(1)).with("length", json!(4));
    let b = run(&doc, &narrow, &p, None, false).expect("builds");
    assert!(b.measurements.pin_diameter_mm.is_some());
    assert!(b.measurements.tube_outer_diameter_mm.is_none());
    assert_eq!(b.connectors.len(), 8);
}

#[test]
fn skipping_the_loop_keeps_the_offgrid_loop_out_of_the_connectors() {
    let p = process("fdm-0.4");
    let params = ParamValues::new().with("studsOnTop", json!(true));
    let b = run(&keychain(), &params, &p, None, true).expect("builds");
    assert!(
        b.warnings.iter().any(|w| w.contains("`ring`")),
        "{:?}",
        b.warnings
    );
    assert!(b.connectors.iter().all(|c| c.feature != "ring"));
    assert_eq!(b.connectors.len(), 32);
}

#[test]
fn parameter_bounds_are_enforced_with_clear_messages() {
    let p = process("fdm-0.4");
    let err = |doc: &PartDoc, name: &str, v: serde_json::Value| -> String {
        let params = ParamValues::new().with(name, v);
        match run(doc, &params, &p, None, true) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("{name} should be rejected"),
        }
    };
    let tile = logo_tile();
    let kc = keychain();
    let m = err(&tile, "size", json!([17, 4]));
    assert!(
        m.contains("`size`") && m.contains("maximum 16") && m.contains("axis 0"),
        "{m}"
    );
    let m = err(&tile, "size", json!([2, 0]));
    assert!(m.contains("at least 1"), "{m}");
    let m = err(&tile, "size", json!([2.5, 4]));
    assert!(m.contains("whole number"), "{m}");
    let m = err(&tile, "relief", json!("chrome"));
    assert!(m.contains("not one of the options"), "{m}");
    let m = err(&tile, "reliefHeight", json!("2mm"));
    assert!(m.contains("above the maximum 1.5"), "{m}");
    let m = err(&tile, "reliefHeight", json!("1stud"));
    assert!(m.contains("above the maximum"), "1 stud is 8 mm: {m}");
    let m = err(&kc, "width", json!(3));
    assert!(m.contains("not one of the options [1,2]"), "{m}");
    let m = err(&kc, "length", json!(3));
    assert!(m.contains("below the minimum 4"), "{m}");
    let m = err(&kc, "text", json!("A".repeat(15)));
    assert!(m.contains("maximum is 14"), "{m}");
    let m = err(&kc, "studsOnTop", json!("yes"));
    assert!(m.contains("true or false"), "{m}");
    // `step` counts from `min`: ringHole is 3 to 8 mm in 0.5 mm steps.
    let m = err(&kc, "ringHole", json!(5.3));
    assert!(
        m.contains("`ringHole`") && m.contains("0.5 steps from 3") && m.contains("5 and 5.5"),
        "{m}"
    );
    let m = err(&tile, "reliefHeight", json!("0.65mm"));
    assert!(m.contains("0.1 steps from 0.2"), "{m}");
    match run(
        &tile,
        &ParamValues::new().with("colour", json!("red")),
        &p,
        None,
        true,
    ) {
        Err(EvalError::UnknownParam(name)) => assert_eq!(name, "colour"),
        other => panic!("expected UnknownParam, got {:?}", other.err()),
    }
}

#[test]
fn quantities_convert_into_the_parameter_unit() {
    let p = process("fdm-0.4");
    let params = ParamValues::new().with("reliefHeight", json!("0.8mm"));
    let r = resolve_params(&logo_tile(), &params, &p).expect("resolves");
    assert_eq!(r.values["reliefHeight"], json!(0.8));
    let params = ParamValues::new().with("length", json!("120ldu"));
    let r = resolve_params(&keychain(), &params, &p).expect("resolves");
    assert_eq!(r.values["length"], json!(120 / STUD_PITCH_LDU));
}

#[test]
fn values_on_the_step_grid_are_accepted() {
    // Decimal steps that binary floating point cannot hold exactly still pass:
    // (0.7 - 0.2) / 0.1 is 4.999999999999999.
    let p = process("fdm-0.4");
    for v in ["0.2mm", "0.7mm", "1.5mm"] {
        let params = ParamValues::new().with("reliefHeight", json!(v));
        resolve_params(&logo_tile(), &params, &p).expect(v);
    }
    for v in [3.0, 5.5, 8.0] {
        let params = ParamValues::new().with("ringHole", json!(v));
        resolve_params(&keychain(), &params, &p).expect("on the grid");
    }
}

#[test]
fn the_tallest_body_builds_with_its_connectors_in_range() {
    let p = process("fdm-0.4");
    let doc = PartDoc::parse(&format!(
        r#"{{ "openbrick": "1.0", "id": "t.t", "version": "0.1.0", "name": "t",
              "license": "CC0-1.0", "credit": {{ "author": "t" }},
              "features": [
                {{ "id": "body", "op": "body.brick", "size": [1, 1], "height": "{MAX_BODY_PLATES}plate" }},
                {{ "id": "studs", "op": "stud.field" }},
                {{ "id": "bottom", "op": "tube.field" }} ] }}"#
    ))
    .expect("parses");
    let b = build(&doc, &p, None);
    let bricks = MAX_BODY_PLATES / (BRICK_LDU / PLATE_LDU) as u32;
    let height = f64::from(bricks) * mm("brick_height");
    assert!((b.measurements.bounds_max_mm[2] - height - mm("stud_height")).abs() < 1e-3);
    let socket = b
        .connectors
        .iter()
        .find(|c| c.kind == "tubeF")
        .expect("socket");
    assert_eq!(socket.pos[1], MAX_BODY_PLATES as i32 * PLATE_LDU);
    assert!(b.measurements.is_manifold && b.measurements.components == 1);
}

#[test]
fn visible_when_follows_the_other_parameters() {
    let p = process("fdm-0.4");
    let r = resolve_params(&logo_tile(), &ParamValues::new(), &p).expect("resolves");
    assert!(r.visible["reliefHeight"] && r.visible["size"]);
    let inlay = ParamValues::new().with("relief", json!("inlay"));
    let r = resolve_params(&logo_tile(), &inlay, &p).expect("resolves");
    assert!(
        !r.visible["reliefHeight"],
        "relief != 'inlay' hides the height"
    );
    let r = resolve_params(
        &keychain(),
        &ParamValues::new().with("loop", json!("none")),
        &p,
    )
    .expect("resolves");
    assert!(!r.visible["ringHole"]);
}

#[test]
fn feature_errors_name_the_feature_and_field() {
    let p = process("fdm-0.4");
    let parse = |features: &str| {
        PartDoc::parse(&format!(
            r#"{{ "openbrick": "1.0", "id": "t.t", "version": "0.1.0", "name": "t",
                  "license": "CC0-1.0", "credit": {{ "author": "t" }},
                  "params": {{ "on": {{ "type": "bool", "default": false }} }},
                  "features": [{features}] }}"#
        ))
        .expect("parses")
    };
    let fails = |features: &str| -> EvalError {
        run(&parse(features), &ParamValues::new(), &p, None, false).expect_err("must fail")
    };
    let body = r#"{ "id": "body", "op": "body.plate", "size": [2, 2] }"#;

    let e = fails(
        r#"{ "id": "body", "op": "body.tile", "size": [2, 2] },
                     { "id": "s", "op": "stud.field", "on": "body.top" }"#,
    );
    assert!(e.to_string().contains("tile has a smooth top"), "{e}");
    let e = fails(&format!(
        r#"{body}, {{ "id": "s", "op": "stud.field", "studType": "hollow" }}"#
    ));
    assert!(
        matches!(&e, EvalError::Unsupported { detail, .. } if detail.contains("hollow")),
        "{e}"
    );
    let e = fails(&format!(
        r#"{body}, {{ "id": "s", "op": "stud.field", "grid": [[0, 0]] }}"#
    ));
    assert!(e.to_string().contains("explicit grid"), "{e}");
    let e = fails(&format!(
        r#"{body}, {{ "id": "s", "op": "stud.field", "on": "body.bottom" }}"#
    ));
    assert!(e.to_string().contains("only `top`"), "{e}");
    let e = fails(&format!(
        r#"{body}, {{ "id": "s", "op": "stud.field", "logo": "x" }}"#
    ));
    assert!(
        e.to_string()
            .contains("never puts text or marks on stud tops"),
        "{e}"
    );
    let e = fails(r#"{ "id": "body", "op": "body.plate", "size": [2, 65] }"#);
    assert!(
        matches!(&e, EvalError::Field { field, .. } if field == "size"),
        "{e}"
    );
    let e = fails(r#"{ "id": "body", "op": "body.plate", "size": [2, 2], "height": "0.5plate" }"#);
    assert!(
        matches!(&e, EvalError::Field { field, .. } if field == "height"),
        "{e}"
    );
    // Heights are capped like footprints: 300,000,000 plates would overflow the
    // i32 LDU coordinates (x 8 = 2.4e9 > 2^31 - 1), and much less is absurd.
    for plates in [MAX_BODY_PLATES + 1, 300_000_000] {
        let e = fails(&format!(
            r#"{{ "id": "body", "op": "body.plate", "size": [2, 2], "height": "{plates}plate" }}"#
        ));
        assert!(
            matches!(&e, EvalError::Field { field, .. } if field == "height"),
            "{e}"
        );
        assert!(e.to_string().contains("taller than"), "{e}");
    }
    let e = fails(r#"{ "id": "body", "op": "body.plate", "size": [2, 2], "height": 2 }"#);
    assert!(
        e.to_string().contains("\"2plate\""),
        "a bare number is ambiguous: {e}"
    );
    let e = fails(r#"{ "id": "body", "op": "body.plate", "size": [2, 2], "if": "$on" }"#);
    assert!(matches!(e, EvalError::NoBody), "{e}");
    let e = fails(r#"{ "id": "body", "op": "body.plate", "size": [2, 2], "if": "$missing" }"#);
    assert!(
        matches!(&e, EvalError::Field { field, .. } if field == "if"),
        "{e}"
    );
    assert!(e.to_string().contains("no parameter `missing`"), "{e}");
    let e = fails(&format!(
        r#"{body}, {{ "id": "b2", "op": "body.brick", "size": [1, 1] }}"#
    ));
    assert!(e.to_string().contains("second body"), "{e}");
    let e = fails(
        r#"{ "id": "body", "op": "body.plate", "size": [2, 2], "round": { "tier": "minkowski", "radius": "1mm" } }"#,
    );
    assert!(e.to_string().contains("round tier `minkowski`"), "{e}");
    let e = fails(&format!(
        r#"{body}, {{ "id": "t", "op": "tube.field" }}, {{ "id": "t2", "op": "tube.field" }}"#
    ));
    assert!(
        e.to_string().contains("already has the tube field `t`"),
        "{e}"
    );
}

#[test]
fn unused_fields_and_offgrid_fields_are_handled() {
    let p = process("fdm-0.4");
    let doc = PartDoc::parse(
        r#"{ "openbrick": "1.0", "id": "t.t", "version": "0.1.0", "name": "t",
             "license": "CC0-1.0", "credit": { "author": "t" },
             "features": [
               { "id": "body", "op": "body.plate", "size": [2, 2], "colour": "red" },
               { "id": "studs", "op": "stud.field", "offGrid": true },
               { "id": "bottom", "op": "tube.field" } ] }"#,
    )
    .expect("parses");
    let b = build(&doc, &p, None);
    assert!(
        b.warnings.iter().any(|w| w.contains("`colour`")),
        "{:?}",
        b.warnings
    );
    assert!(
        b.connectors.iter().all(|c| c.kind == "tubeF"),
        "offGrid studs emit no connectors"
    );
    assert_eq!(b.connectors.len(), 4);
    assert!(
        b.measurements.stud_diameter_mm.is_some(),
        "the studs are still built"
    );
}

#[test]
fn content_hash_covers_inputs_and_ignores_spelling() {
    let p = process("fdm-0.4");
    let fit = example_fit_profile();
    let doc = keychain();
    let base = ParamValues::new()
        .with("studsOnTop", json!(true))
        .with("loop", json!("none"));
    let hash = |params: &ParamValues, fit| {
        run(&doc, params, &p, fit, false)
            .expect("builds")
            .content_hash
    };
    let h0 = hash(&base, None);
    assert_eq!(h0.len(), 64);
    assert!(h0.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(h0, hash(&base, None), "deterministic");
    // Naming the default value, or writing it with a unit, is the same input.
    assert_eq!(h0, hash(&base.clone().with("length", json!(8)), None));
    assert_eq!(h0, hash(&base.clone().with("length", json!("8stud")), None));
    assert_ne!(h0, hash(&base.clone().with("length", json!(9)), None));
    assert_ne!(h0, hash(&base, Some(&fit)));
    // A fit profile with the same id but other offsets is another input.
    let mut tweaked = fit.clone();
    tweaked.offsets_mm.stud_diameter += 0.05;
    assert_ne!(hash(&base, Some(&fit)), hash(&base, Some(&tweaked)));
    // Another process.
    let b = run(&doc, &base, &process("fdm-0.2"), None, false).expect("builds");
    assert_ne!(h0, b.content_hash);
    // Turning skipping on changes nothing when nothing is skipped.
    let skip = run(&doc, &base, &p, None, true).expect("builds");
    assert_eq!(h0, skip.content_hash);
}

#[test]
fn connectors_are_centred_on_the_stud_group() {
    // For every footprint up to 3 x 3: one stud and one socket per cell, on the
    // 20 LDU lattice, symmetric about the origin, sockets one plate below.
    let p = process("fdm-0.4");
    for nx in 1..=3u32 {
        for ny in 1..=3u32 {
            let doc = part("body.plate", [nx, ny], None, true, true);
            let b = build(&doc, &p, None);
            assert!(
                b.measurements.is_manifold && b.measurements.components == 1,
                "{nx}x{ny}"
            );
            for kind in ["studM", "tubeF"] {
                let cs: Vec<_> = b.connectors.iter().filter(|c| c.kind == kind).collect();
                assert_eq!(cs.len(), (nx * ny) as usize, "{nx}x{ny} {kind}");
                let sum = cs
                    .iter()
                    .fold([0, 0], |s, c| [s[0] + c.pos[0], s[1] + c.pos[2]]);
                assert_eq!(sum, [0, 0], "{nx}x{ny} {kind} symmetric");
                let mut seen: Vec<_> = cs.iter().map(|c| c.pos).collect();
                seen.sort();
                seen.dedup();
                assert_eq!(seen.len(), cs.len(), "distinct positions");
                for c in &cs {
                    let half = STUD_PITCH_LDU / 2;
                    assert_eq!((c.pos[0] + half * (nx as i32 - 1)) % STUD_PITCH_LDU, 0);
                    assert_eq!((c.pos[2] + half * (ny as i32 - 1)) % STUD_PITCH_LDU, 0);
                    assert_eq!(c.pos[1], if kind == "studM" { 0 } else { PLATE_LDU });
                }
            }
            // Analytic volume across footprints too (no top chamfer, nominal).
            let shape = Shape {
                size: [nx, ny],
                plates: 1,
                studs: true,
                tubes: true,
                top_chamfer: 0.0,
            };
            let v = analytic_volume(shape, &p, &Offsets::default());
            let got = b.measurements.volume_mm3;
            assert!(((got - v) / v).abs() < 1e-6, "{nx}x{ny}: {got} vs {v}");
        }
    }
}

#[test]
fn built_parts_serialise_for_the_cli_and_the_browser() {
    let p = process("fdm-0.4");
    let b = build(&part("body.plate", [1, 2], None, true, true), &p, None);
    let v = serde_json::to_value(&b).expect("serialises");
    assert_eq!(v["objects"][0]["role"], "body");
    assert_eq!(v["connectors"][0]["kind"], "studM");
    assert_eq!(v["connectors"][0]["sections"][0]["shape"], "R");
    assert!(
        v["measurements"]["minTubeWallMm"].is_null(),
        "1-wide parts have pins, no tubes"
    );
    assert!(v["measurements"]["pinDiameterMm"].is_number());
    assert_eq!(v["kernelId"], "manifold-rust 0.19.1");
    assert!(v["contentHash"].is_string());
}
