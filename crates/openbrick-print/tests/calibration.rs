//! How fit profiles are read and written, so they agree with
//! `schemas/fit-profile.schema.json`.

use openbrick_print::{example_fit_profile, FitProfile};

fn with_coupon_version(v: &str) -> String {
    let mut doc: serde_json::Value = serde_json::from_str(include_str!(
        "../../../profiles/fit/example-mk4s-petg-0.4.json"
    ))
    .expect("example fit profile is JSON");
    doc["calibration"]["couponVersion"] = serde_json::from_str(v).expect("a JSON number");
    doc.to_string()
}

#[test]
fn coupon_version_accepts_whole_numbers_written_as_floats() {
    // JSON Schema (draft 2020-12) counts 1.0 as an integer, so the reader must too.
    for (text, want) in [("1", 1), ("1.0", 1), ("4294967295", u32::MAX)] {
        let f: FitProfile = serde_json::from_str(&with_coupon_version(text))
            .unwrap_or_else(|e| panic!("couponVersion {text}: {e}"));
        assert_eq!(f.calibration.expect("calibrated").coupon_version, want);
    }
    for text in ["1.5", "-1", "4294967296"] {
        let r: Result<FitProfile, _> = serde_json::from_str(&with_coupon_version(text));
        assert!(r.is_err(), "couponVersion {text} must be rejected");
    }
}

#[test]
fn written_profiles_keep_credit_and_omit_what_is_absent() {
    let mut f = example_fit_profile();
    let written = serde_json::to_value(&f).expect("serialises");
    assert_eq!(written["credit"]["author"], "OpenBrick3D contributors");
    assert!(
        written["credit"].get("url").is_none(),
        "empty url is omitted"
    );
    f.calibration = None;
    let written = serde_json::to_value(&f).expect("serialises");
    assert!(
        written.get("calibration").is_none(),
        "an uncalibrated profile has no calibration key, not null"
    );
}
