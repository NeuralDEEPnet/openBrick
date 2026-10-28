//! The coupon sweep data (`standards/coupon-v1.json`, docs/plan/05 section 4)
//! agrees with the plan's table, the Dimensional Standard and the fit profile
//! rules that use it.

use openbrick_print::{coupon_v1, required_cells, CouponSpec, Offsets};
use openbrick_units::STANDARD;

/// Every `a to b` range written in a print target note, as numbers.
fn ranges(text: &str) -> Vec<(f64, f64)> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ';' || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    words
        .windows(3)
        .filter(|w| w[1] == "to")
        .filter_map(|w| Some((w[0].parse().ok()?, w[2].parse().ok()?)))
        .collect()
}

#[test]
fn the_bundled_coupon_loads_and_checks() {
    let spec = coupon_v1();
    assert_eq!(spec.kind, "coupon");
    assert_eq!(spec.version, 1, "couponVersion 1");
    assert!(spec.check().is_empty(), "{:?}", spec.check());
    assert_eq!(
        spec.copies, 2,
        "two identical rows (docs/plan/05 section 4)"
    );
    for c in &spec.cells {
        assert!(!c.source.is_empty(), "{} cites a source", c.id);
        assert!(c.values().len() >= 4, "{}: a sweep, not a point", c.id);
    }
}

#[test]
fn every_chosen_cell_key_is_a_recordable_coupon_cell() {
    // required_cells() names the chosenCells keys FIT.MISSING asks for; each must
    // be a coupon cell with an offset, or no coupon could satisfy the rule.
    let spec = coupon_v1();
    for connector in [
        "studM", "tubeF", "studF", "pinF", "pinM", "axleM", "axleF", "barM", "clipF",
    ] {
        for cell in required_cells(connector) {
            let c = spec
                .cell(cell)
                .unwrap_or_else(|| panic!("{connector} needs `{cell}`, which coupon v1 lacks"));
            assert!(c.offset_field.is_some(), "{cell} records an offset");
        }
    }
    // And every recordable cell sets a different offset.
    let mut fields: Vec<&str> = spec
        .cells
        .iter()
        .filter_map(|c| c.offset_field.as_deref())
        .collect();
    fields.sort_unstable();
    let mut keys = Offsets::KEYS.to_vec();
    keys.sort_unstable();
    assert_eq!(fields, keys, "one coupon cell per diameter or depth offset");
}

#[test]
fn sweeps_match_the_print_targets_of_the_standard() {
    // Where a row of the standard says "by coupon" (or "coupon sweeps"), one of
    // the cells measured from that row sweeps exactly that range.
    let spec = coupon_v1();
    let mut checked = 0;
    for f in &STANDARD.get().features {
        let Some(target) = f.print_target.get("fdm-0.4") else {
            continue;
        };
        if !target.contains("coupon") {
            continue;
        }
        let cells: Vec<_> = spec
            .cells
            .iter()
            .filter(|c| c.standard_id == f.id)
            .collect();
        assert!(
            !cells.is_empty(),
            "{}: no coupon cell for \"{target}\"",
            f.id
        );
        let found = ranges(target).iter().any(|&(a, b)| {
            cells
                .iter()
                .any(|c| (c.start_mm - a).abs() < 1e-9 && (c.end_mm - b).abs() < 1e-9)
        });
        assert!(found, "{}: no cell sweeps a range of \"{target}\"", f.id);
        checked += 1;
    }
    // stud_diameter, rib_depth, tube_outer_diameter, bar_diameter,
    // hollow_stud_bore, pin_shaft_diameter, axle_across_tips.
    assert_eq!(checked, 7);
}

/// The one place the coupon numbers are written outside the data file: a
/// transcription check of `standards/coupon-v1.json` against the table in
/// docs/plan/05 section 4, like the anchor test of the Dimensional Standard
/// (`derived_relations_hold` in openbrick-units). Code reads the numbers from
/// the data through `coupon_v1()`.
#[test]
#[allow(clippy::approx_constant)] // 3.14 mm is the first bar and clip cell, not pi
fn coupon_v1_transcribes_the_plan_table() {
    // docs/plan/05 section 4, row by row: (cell, start, end, step).
    let table = [
        ("stud", 4.60, 4.95, 0.05),
        ("tube", 4.80, 5.05, 0.05),
        ("pocketTubeOD", 6.40, 6.65, 0.05),
        ("pocketRib", 0.20, 0.50, 0.05),
        ("technicH", 4.80, 5.10, 0.05),
        ("pin", 4.65, 4.90, 0.05),
        ("axleTips", 4.60, 4.85, 0.05),
        ("axle", 4.75, 5.00, 0.05),
        ("bar", 3.14, 3.26, 0.04),
        ("clip", 3.14, 3.30, 0.04),
    ];
    let spec = coupon_v1();
    assert_eq!(spec.cells.len(), table.len());
    for (id, start, end, step) in table {
        let c = spec.cell(id).unwrap_or_else(|| panic!("cell {id}"));
        assert_eq!(
            (c.start_mm, c.end_mm, c.step_mm),
            (start, end, step),
            "{id}"
        );
    }
    // The pocket row crosses tube OD with rib depth.
    assert_eq!(
        spec.cell("pocketTubeOD").map(|c| &c.row),
        spec.cell("pocketRib").map(|c| &c.row)
    );
    // Cells differ by the row step: 4.60, 4.65, ..., 4.95 is 8 studs.
    assert_eq!(spec.cell("stud").map(|c| c.values().len()), Some(8));
}

#[test]
fn nominal_values_lie_inside_their_sweep() {
    // Uncalibrated geometry must not raise CLUTCH.RISK, so every nominal the
    // validator compares (stud, bore, tube OD) is inside its sweep.
    let spec = coupon_v1();
    for id in ["stud", "tube", "pocketTubeOD", "pocketRib"] {
        let c = spec.cell(id).expect("cell");
        assert!(c.covers(c.nominal_mm()), "{id}: nominal {}", c.nominal_mm());
    }
}

#[test]
fn broken_sweeps_are_reported() {
    let mut spec: CouponSpec = coupon_v1().clone();
    spec.cells[0].end_mm = 4.97; // 0.37 is 7.4 steps of 0.05
    spec.cells[1].id = spec.cells[0].id.clone();
    spec.cells[2].standard_id = "no_such_row".into();
    spec.cells[3].offset_field = Some("xyScale".into());
    spec.cells[4].step_mm = 0.0;
    spec.cells[5].offset_field = spec.cells[0].offset_field.clone();
    let problems = spec.check();
    assert_eq!(problems.len(), 6, "{problems:#?}");
    assert!(
        problems[0].contains("whole number of 0.05 mm steps"),
        "{problems:#?}"
    );
    assert!(problems.iter().any(|p| p.contains("used twice")));
    assert!(problems.iter().any(|p| p.contains("no_such_row")));
    assert!(problems.iter().any(|p| p.contains("`xyScale`")));
    assert!(problems.iter().any(|p| p.contains("positive step")));
    assert!(problems.iter().any(|p| p.contains("already set")));
    // A broken step gives no endless sweep.
    assert_eq!(spec.cells[4].values().len(), 1);
}
