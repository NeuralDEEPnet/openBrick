//! The joint table: how the clip, hinge, ball and bar primitives of the LDraw library turn into
//! connectors (docs/plan/07 section 3, the Q04 report's calibration table).
//!
//! The table is data, `standards/joints-v1.json`, derived from the library's own primitives by
//! `scripts/derive-joint-table.py` (each entry cites the cylinder, cone or sphere it comes from)
//! and validated by `cargo xtask data` against `schemas/joints.schema.json`. This module reads it;
//! no joint number is typed into the code. Every entry is in the primitive's own frame, so an
//! instance's matrix places it (position and axis) in the part's frame.

use std::sync::OnceLock;

use openbrick_units::Credit;

use crate::shape::ConnKind;

const SOURCE: &str = include_str!("../../../standards/joints-v1.json");

/// One joint primitive.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JointPrimitive {
    /// The primitive's file name without `.dat`: `clip2`, `clh4`, `joint8ball`.
    pub primitive: String,
    pub description: String,
    /// The connector kind an instance of it makes.
    pub kind: ConnKind,
    /// The joint's axis in the primitive's frame: a lattice axis (positive).
    pub axis: [i32; 3],
    /// A point on that axis, in the primitive's frame, LDU.
    pub origin: [f64; 3],
    /// How it was derived: `bore-cylinder`, `sphere-centre`, `concentric-with-ball`, `hinge-pin`.
    pub method: String,
    /// The primitive geometry it was read from.
    pub evidence: String,
}

/// The rule that finds bars (a bar has no primitive).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BarRule {
    pub radius: f64,
    pub radius_tolerance: f64,
    pub min_length: f64,
    /// Lower-case words; a part whose description holds one may have a bar.
    pub description_words: Vec<String>,
    pub source: String,
}

/// The whole table.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JointTable {
    pub openbrick: String,
    pub kind: String,
    pub version: String,
    pub license: String,
    pub credit: Credit,
    pub library: String,
    pub notes: String,
    pub bars: BarRule,
    pub primitives: Vec<JointPrimitive>,
}

impl JointTable {
    /// The embedded table, parsed once.
    pub fn bundled() -> &'static JointTable {
        static T: OnceLock<JointTable> = OnceLock::new();
        T.get_or_init(|| {
            serde_json::from_str(SOURCE).expect("standards/joints-v1.json is a valid joint table")
        })
    }

    /// The entry of a primitive (file name without `.dat`, lower case).
    pub fn get(&self, primitive: &str) -> Option<&JointPrimitive> {
        self.primitives.iter().find(|p| p.primitive == primitive)
    }

    /// What the schema cannot say: unique primitives, lattice axes, finite origins, joint kinds
    /// that are joints, a bar radius that is positive. One line per problem.
    pub fn check(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for p in &self.primitives {
            if !seen.insert(p.primitive.as_str()) {
                problems.push(format!("primitive {} appears twice", p.primitive));
            }
            let unit =
                p.axis.iter().map(|v| v.abs()).sum::<i32>() == 1 && p.axis.iter().all(|&v| v >= 0);
            if !unit {
                problems.push(format!(
                    "{}: the axis {:?} is not a positive lattice axis",
                    p.primitive, p.axis
                ));
            }
            if p.origin.iter().any(|v| !v.is_finite() || v.abs() > 1000.0) {
                problems.push(format!(
                    "{}: the origin {:?} is not a finite point within 1000 LDU",
                    p.primitive, p.origin
                ));
            }
            if p.kind.is_stud_kind()
                || matches!(
                    p.kind,
                    ConnKind::PinM | ConnKind::PinF | ConnKind::AxleM | ConnKind::AxleF
                )
            {
                problems.push(format!(
                    "{}: {} is not a joint kind",
                    p.primitive,
                    p.kind.name()
                ));
            }
            if p.evidence.trim().is_empty() {
                problems.push(format!(
                    "{}: cite the geometry it was read from",
                    p.primitive
                ));
            }
        }
        if !(self.bars.radius > 0.0
            && self.bars.radius_tolerance >= 0.0
            && self.bars.min_length > 0.0)
        {
            problems.push("bars: radius, tolerance and minimum length must be positive".into());
        }
        if self.bars.description_words.is_empty()
            || self
                .bars
                .description_words
                .iter()
                .any(|w| w.is_empty() || w.to_lowercase() != *w)
        {
            problems.push("bars: descriptionWords must be non-empty lower-case words".into());
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_table_is_sound_and_covers_the_joint_families() {
        let t = JointTable::bundled();
        assert_eq!(t.check(), Vec::<String>::new());
        for (name, kind) in [
            ("clip2", ConnKind::ClipF),
            ("joint8ball", ConnKind::BallM),
            ("joint8socket1", ConnKind::SocketF),
            ("clh1", ConnKind::HingeO),
            ("clh4", ConnKind::HingeE),
            ("arm1", ConnKind::HingeE),
        ] {
            assert_eq!(t.get(name).map(|p| p.kind), Some(kind), "{name}");
        }
        // A clip's bore is a bar's radius: the table and the standard agree.
        let bar = openbrick_units::STANDARD
            .get()
            .feature("bar_diameter")
            .unwrap()
            .ldraw_ldu
            .unwrap();
        assert!(
            (t.bars.radius * 2.0 - bar).abs() < 0.2,
            "{} vs {bar}",
            t.bars.radius
        );
    }
}
