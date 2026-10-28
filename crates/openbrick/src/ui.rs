//! Output conventions shared by the commands (docs/plan/05 section 9): human
//! text, the `--json` object, exit codes and the validator finding layout.
//!
//! - Human text goes to stdout, or to stderr when `--json` is set, so that
//!   stdout then carries exactly one JSON object.
//! - Exit codes: 0 success, 1 the validator found blockers (nothing written),
//!   2 a usage or input error the user can fix, 3 an internal error.

use openbrick_print::{Finding, Report, Severity};
use serde_json::Value;

/// Exit code when the validator found blockers.
pub const EXIT_BLOCKED: u8 = 1;
/// Exit code for a usage or input error.
pub const EXIT_INPUT: u8 = 2;
/// Exit code for an internal error (a bug: please report it).
pub const EXIT_INTERNAL: u8 = 3;

/// The generator string written into every export and JSON object.
pub const GENERATOR: &str = concat!("openbrick ", env!("CARGO_PKG_VERSION"));

/// Why a command stopped before it produced its result.
#[derive(Debug)]
pub enum Fail {
    /// Something the user can fix: a missing file, a bad parameter, an op the
    /// evaluator does not build yet (exit 2).
    Input(String),
    /// A bug or an environment failure the user cannot fix from the command
    /// line (exit 3).
    Internal(String),
}

impl Fail {
    pub fn code(&self) -> u8 {
        match self {
            Fail::Input(_) => EXIT_INPUT,
            Fail::Internal(_) => EXIT_INTERNAL,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Fail::Input(m) | Fail::Internal(m) => m,
        }
    }
}

/// What a command produced: its exit code and its `--json` object.
pub struct Outcome {
    pub code: u8,
    pub json: Value,
}

/// Where human text goes.
#[derive(Clone, Copy)]
pub struct Ui {
    pub json: bool,
}

impl Ui {
    /// One line of human text: stdout, or stderr in `--json` mode.
    pub fn say(&self, line: impl AsRef<str>) {
        if self.json {
            eprintln!("{}", line.as_ref());
        } else {
            println!("{}", line.as_ref());
        }
    }

    /// One line of an error or a blocker: always stderr.
    pub fn err(&self, line: impl AsRef<str>) {
        eprintln!("{}", line.as_ref());
    }
}

/// A number for a message: three decimals at most, without trailing zeros.
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// A signed offset with three decimals, as the record table shows it.
pub fn signed(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v:+.3}")
}

/// One finding as text: rule, severity and feature, the message, the measured
/// value and threshold, and the fix hint (CLAUDE.md: every validator message
/// carries all four).
pub fn finding_text(f: &Finding) -> String {
    let severity = match f.severity {
        Severity::Blocker => "blocker",
        Severity::Warning => "warning",
    };
    let feature = f
        .feature
        .as_deref()
        .map(|x| format!(" (feature `{x}`)"))
        .unwrap_or_default();
    let value = f.value.map(num).unwrap_or_else(|| "n/a".into());
    let threshold = f.threshold.map(num).unwrap_or_else(|| "n/a".into());
    format!(
        "{severity} {}{feature}: {}\n    measured {value}, threshold {threshold}\n    fix: {}",
        f.rule, f.message, f.fix
    )
}

/// Print every finding of a report with `print`, blockers first (the report
/// is sorted that way).
pub fn print_findings(report: &Report, mut print: impl FnMut(String)) {
    for f in &report.findings {
        print(finding_text(f));
    }
}

/// The JSON object printed for a command that failed before it had a result.
pub fn failure_json(command: &str, fail: &Fail) -> Value {
    serde_json::json!({
        "command": command,
        "generator": GENERATOR,
        "ok": false,
        "exitCode": fail.code(),
        "error": fail.message(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short() {
        assert_eq!(num(0.85), "0.85");
        assert_eq!(num(0.7), "0.7");
        assert_eq!(num(6.514), "6.514");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(signed(0.15000000000000036), "+0.150");
        assert_eq!(signed(-0.1), "-0.100");
        assert_eq!(signed(-0.0), "+0.000");
    }

    #[test]
    fn findings_carry_rule_value_threshold_and_fix() {
        let f = Finding {
            rule: "TUBE.WALL".into(),
            severity: Severity::Blocker,
            message: "tube wall 0.65 mm is below 0.7 mm".into(),
            value: Some(0.65),
            threshold: Some(0.7),
            fix: "lower the tube bore offset".into(),
            feature: Some("bottom".into()),
        };
        let t = finding_text(&f);
        for part in [
            "blocker TUBE.WALL",
            "feature `bottom`",
            "measured 0.65",
            "threshold 0.7",
            "fix: lower",
        ] {
            assert!(t.contains(part), "{t}");
        }
    }
}
