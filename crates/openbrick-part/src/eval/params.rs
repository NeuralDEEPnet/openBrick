//! Parameter resolution (docs/plan/06 section 3) and feature field values.
//!
//! Defaults come from the document's `params`, overrides from [`ParamValues`];
//! every value is checked against its type, `min`, `max`, `step`, `options` and
//! (for strings) `maxLength`. Feature fields may then be literals with unit suffixes
//! (`"0.6mm"`, `"2stud"`, `"1plate"`, `"brick"`), references (`"$size"`), arrays of
//! those (`["$width", "$length"]`) or expressions (`"$length - 1"`).

use std::collections::BTreeMap;

use openbrick_print::ProcessProfile;
use openbrick_units::{BRICK_LDU, LDU_MM, PLATE_LDU, STANDARD, STUD_PITCH_LDU};
use serde_json::Value;

use super::expr::{self, Scalar};
use super::{EvalError, ParamValues};
use crate::{Param, PartDoc};

/// The unit a number carries (docs/plan/06 section 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    /// A bare number: a count, or the default unit of the field reading it.
    None,
    Mm,
    Stud,
    Plate,
    Brick,
    Ldu,
    Deg,
}

impl Unit {
    fn parse(s: &str) -> Option<Unit> {
        Some(match s {
            "mm" => Unit::Mm,
            "stud" | "studs" => Unit::Stud,
            "plate" | "plates" => Unit::Plate,
            "brick" | "bricks" => Unit::Brick,
            "ldu" => Unit::Ldu,
            "deg" => Unit::Deg,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Unit::None => "",
            Unit::Mm => "mm",
            Unit::Stud => "stud",
            Unit::Plate => "plate",
            Unit::Brick => "brick",
            Unit::Ldu => "ldu",
            Unit::Deg => "deg",
        }
    }

    /// Millimetres per unit for lengths; `None` for counts and angles.
    fn mm(self) -> Option<f64> {
        let s = STANDARD.get();
        match self {
            Unit::Mm => Some(1.0),
            Unit::Stud => Some(s.mm("module")),
            Unit::Plate => Some(s.mm("plate_height")),
            Unit::Brick => Some(s.mm("brick_height")),
            Unit::Ldu => Some(LDU_MM),
            Unit::None | Unit::Deg => None,
        }
    }

    /// LDU per unit for lattice units (exact integers).
    fn ldu(self) -> Option<i64> {
        match self {
            Unit::Stud => Some(STUD_PITCH_LDU.into()),
            Unit::Plate => Some(PLATE_LDU.into()),
            Unit::Brick => Some(BRICK_LDU.into()),
            Unit::Ldu => Some(1),
            _ => None,
        }
    }
}

/// `"0.6mm"`, `"2 stud"`, `"-1.5plate"`: a number with a unit suffix.
fn parse_quantity(s: &str) -> Option<(f64, Unit)> {
    let s = s.trim();
    let split = s
        .find(|c: char| c.is_ascii_alphabetic())
        .filter(|&i| i > 0)?;
    let (num, unit) = s.split_at(split);
    let num = num.trim();
    // Only plain decimals: no exponent, no inf or nan.
    if !num
        .trim_start_matches('-')
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    Some((num.parse().ok()?, Unit::parse(unit.trim())?))
}

/// A literal string as a value: a quantity, a bare unit word meaning one of it
/// (`"plate"`, `"brick"`), or a plain string.
pub(crate) fn literal(s: &str) -> FVal {
    if let Some((n, u)) = parse_quantity(s) {
        return FVal::Num(n, u);
    }
    match s {
        "stud" | "plate" | "brick" => FVal::Num(1.0, Unit::parse(s).unwrap_or(Unit::None)),
        _ => FVal::Str(s.to_string()),
    }
}

/// A feature field after references and expressions are resolved.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FVal {
    Null,
    Bool(bool),
    Num(f64, Unit),
    Str(String),
    List(Vec<FVal>),
    Obj(BTreeMap<String, FVal>),
}

impl FVal {
    fn describe(&self) -> String {
        match self {
            FVal::Null => "null".into(),
            FVal::Bool(b) => b.to_string(),
            FVal::Num(n, u) => format!("{n}{}", u.name()),
            FVal::Str(s) => format!("'{s}'"),
            FVal::List(l) => format!(
                "[{}]",
                l.iter().map(FVal::describe).collect::<Vec<_>>().join(", ")
            ),
            FVal::Obj(_) => "an object".into(),
        }
    }

    /// A whole number of studs (bare numbers count as studs).
    pub(crate) fn studs(&self) -> Result<u32, String> {
        let ldu = match self {
            FVal::Num(n, Unit::None) => n * f64::from(STUD_PITCH_LDU),
            FVal::Num(n, u) if u.ldu().is_some() => n * u.ldu().unwrap_or(1) as f64,
            other => {
                return Err(format!(
                    "expected a number of studs, found {}",
                    other.describe()
                ))
            }
        };
        let studs = ldu / f64::from(STUD_PITCH_LDU);
        if studs.fract() != 0.0 || studs < 1.0 || studs > f64::from(u32::MAX) {
            return Err(format!(
                "expected a whole number of studs, at least 1, found {}",
                self.describe()
            ));
        }
        Ok(studs as u32)
    }

    /// A footprint in studs, `[x, y]`.
    pub(crate) fn grid2(&self) -> Result<[u32; 2], String> {
        match self {
            FVal::List(l) if l.len() == 2 => Ok([l[0].studs()?, l[1].studs()?]),
            other => Err(format!(
                "expected a footprint [x, y] in studs, found {}",
                other.describe()
            )),
        }
    }

    /// A body height as a whole number of plates (`"plate"`, `"brick"`,
    /// `"2plate"`, `"24ldu"`; a bare number is ambiguous and rejected).
    pub(crate) fn plates(&self) -> Result<u32, String> {
        let ldu = match self {
            FVal::Num(n, u) if u.ldu().is_some() => n * u.ldu().unwrap_or(1) as f64,
            FVal::Num(n, Unit::Mm) => (n / LDU_MM * 1e6).round() / 1e6,
            other => {
                return Err(format!(
                    "expected a height such as \"plate\", \"brick\" or \"2plate\", found {}",
                    other.describe()
                ))
            }
        };
        let plates = ldu / f64::from(PLATE_LDU);
        if plates.fract() != 0.0 || plates < 1.0 || plates > f64::from(u32::MAX) {
            return Err(format!(
                "expected a whole number of plates, at least one, found {}",
                self.describe()
            ));
        }
        Ok(plates as u32)
    }

    /// A length in millimetres (bare numbers are millimetres).
    pub(crate) fn mm(&self) -> Result<f64, String> {
        match self {
            FVal::Num(n, Unit::None) => Ok(*n),
            FVal::Num(n, u) => u
                .mm()
                .map(|f| n * f)
                .ok_or_else(|| format!("expected a length, found {}", self.describe())),
            other => Err(format!("expected a length, found {}", other.describe())),
        }
    }

    pub(crate) fn str(&self) -> Result<&str, String> {
        match self {
            FVal::Str(s) => Ok(s),
            other => Err(format!("expected a string, found {}", other.describe())),
        }
    }

    pub(crate) fn obj(&self) -> Result<&BTreeMap<String, FVal>, String> {
        match self {
            FVal::Obj(o) => Ok(o),
            other => Err(format!("expected an object, found {}", other.describe())),
        }
    }
}

/// Parameter values after defaults, overrides and checks.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedParams {
    /// Every parameter's value, normalised: lengths and counts are plain numbers
    /// in the parameter's own unit.
    pub values: BTreeMap<String, Value>,
    /// Whether each parameter is shown, from its `visibleWhen` (true when absent).
    pub visible: BTreeMap<String, bool>,
}

fn param_err(name: &str, message: impl Into<String>) -> EvalError {
    EvalError::Param {
        name: name.to_string(),
        message: message.into(),
    }
}

fn option_values(p: &Param) -> Vec<Value> {
    p.options
        .iter()
        .flatten()
        .map(|o| o.get("value").cloned().unwrap_or_else(|| o.clone()))
        .collect()
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn show(v: &Value) -> String {
    v.to_string()
}

/// The unit a parameter's numbers carry.
fn param_unit(p: &Param) -> Unit {
    match p.unit.as_deref() {
        Some(u) => Unit::parse(u).unwrap_or(Unit::None),
        None => match p.ty.as_str() {
            "length" => Unit::Mm,
            "angle" => Unit::Deg,
            "grid2" => Unit::Stud,
            _ => Unit::None,
        },
    }
}

/// A number for a numeric parameter, from a JSON number or a string with a unit
/// suffix converted into the parameter's unit.
fn number_in_unit(name: &str, p: &Param, v: &Value) -> Result<f64, EvalError> {
    if let Some(n) = v.as_f64() {
        return Ok(n);
    }
    let Some(s) = v.as_str() else {
        return Err(param_err(
            name,
            format!("expected a number, found {}", show(v)),
        ));
    };
    let want = param_unit(p);
    let FVal::Num(n, have) = literal(s) else {
        return Err(param_err(
            name,
            format!("expected a number, found {}", show(v)),
        ));
    };
    if have == want || have == Unit::None {
        return Ok(n);
    }
    match (have.mm(), want.mm()) {
        (Some(a), Some(b)) => Ok(n * a / b),
        _ => Err(param_err(
            name,
            format!(
                "{} cannot be converted to {}",
                show(v),
                if want == Unit::None {
                    "a plain number"
                } else {
                    want.name()
                }
            ),
        )),
    }
}

/// A `min` or `max` bound, per axis for grid2 (`[1, 1]`) or one number for all.
fn bound(b: &Option<Value>, axis: Option<usize>) -> Option<f64> {
    let b = b.as_ref()?;
    match (axis, b) {
        (Some(i), Value::Array(a)) => a.get(i).and_then(Value::as_f64),
        _ => b.as_f64(),
    }
}

/// `step`: the value must be `min` (or zero without a minimum) plus a whole
/// number of steps (docs/plan/06 section 3: "bounds enforced by the UI and the
/// evaluator"). The tolerance absorbs decimal steps that binary floating point
/// cannot hold exactly ((0.6 - 0.2) / 0.1 is 3.9999999999999996).
fn check_step(name: &str, p: &Param, n: f64, axis: Option<usize>) -> Result<(), EvalError> {
    let Some(step) = p.step.filter(|s| *s > 0.0) else {
        return Ok(());
    };
    let base = bound(&p.min, axis).unwrap_or(0.0);
    let k = (n - base) / step;
    if (k - k.round()).abs() <= 1e-9 * k.abs().max(1.0) {
        return Ok(());
    }
    let at = axis.map(|i| format!(" (axis {i})")).unwrap_or_default();
    let (below, above) = (base + k.floor() * step, base + k.ceil() * step);
    Err(param_err(
        name,
        format!(
            "{n} is not a whole number of {step} steps from {base}{at}; the nearest allowed values are {} and {}",
            round_display(below),
            round_display(above)
        ),
    ))
}

/// A value for a message, without binary floating-point tails (5.0000000001).
fn round_display(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

fn check_bounds(name: &str, p: &Param, n: f64, axis: Option<usize>) -> Result<(), EvalError> {
    let bound = |b: &Option<Value>| bound(b, axis);
    let at = axis.map(|i| format!(" (axis {i})")).unwrap_or_default();
    if let Some(min) = bound(&p.min) {
        if n < min {
            return Err(param_err(
                name,
                format!("{n} is below the minimum {min}{at}"),
            ));
        }
    }
    if let Some(max) = bound(&p.max) {
        if n > max {
            return Err(param_err(
                name,
                format!("{n} is above the maximum {max}{at}"),
            ));
        }
    }
    check_step(name, p, n, axis)
}

fn check_options(name: &str, p: &Param, v: &Value) -> Result<(), EvalError> {
    let options = option_values(p);
    if p.options.is_some() && !options.iter().any(|o| same_value(o, v)) {
        return Err(param_err(
            name,
            format!(
                "{} is not one of the options {}",
                show(v),
                Value::Array(options)
            ),
        ));
    }
    Ok(())
}

/// Check one value against its parameter and return it normalised.
fn check_param(name: &str, p: &Param, v: &Value) -> Result<Value, EvalError> {
    let out = match p.ty.as_str() {
        "int" => {
            let n = number_in_unit(name, p, v)?;
            if n.fract() != 0.0 || !n.is_finite() {
                return Err(param_err(
                    name,
                    format!("expected a whole number, found {n}"),
                ));
            }
            check_bounds(name, p, n, None)?;
            serde_json::json!(n as i64)
        }
        "length" | "angle" => {
            let n = number_in_unit(name, p, v)?;
            if !n.is_finite() {
                return Err(param_err(name, "expected a finite number"));
            }
            check_bounds(name, p, n, None)?;
            serde_json::json!(n)
        }
        "bool" => match v {
            Value::Bool(_) => v.clone(),
            _ => {
                return Err(param_err(
                    name,
                    format!("expected true or false, found {}", show(v)),
                ))
            }
        },
        "string" => {
            let s = v
                .as_str()
                .ok_or_else(|| param_err(name, format!("expected a string, found {}", show(v))))?;
            if let Some(max) = p.extra.get("maxLength").and_then(Value::as_u64) {
                let len = s.chars().count() as u64;
                if len > max {
                    return Err(param_err(
                        name,
                        format!("is {len} characters long; the maximum is {max}"),
                    ));
                }
            }
            v.clone()
        }
        "enum" => {
            if p.options.is_none() {
                return Err(param_err(name, "an enum parameter needs options"));
            }
            v.clone()
        }
        "grid2" => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| {
                param_err(name, format!("expected [x, y] in studs, found {}", show(v)))
            })?;
            let mut out = Vec::new();
            for (i, x) in a.iter().enumerate() {
                let n = number_in_unit(name, p, x)?;
                if n.fract() != 0.0 || n < 1.0 {
                    return Err(param_err(
                        name,
                        format!("axis {i} must be a whole number of at least 1, found {n}"),
                    ));
                }
                check_bounds(name, p, n, Some(i))?;
                out.push(serde_json::json!(n as i64));
            }
            Value::Array(out)
        }
        "svg" | "image" | "font" | "color" => {
            if !v.is_string() {
                return Err(param_err(
                    name,
                    format!("expected a string, found {}", show(v)),
                ));
            }
            v.clone()
        }
        "list" => {
            if !v.is_array() {
                return Err(param_err(
                    name,
                    format!("expected a list, found {}", show(v)),
                ));
            }
            v.clone()
        }
        other => return Err(param_err(name, format!("unknown parameter type `{other}`"))),
    };
    check_options(name, p, &out)?;
    Ok(out)
}

/// Resolve parameters: defaults, then overrides, each checked against its type
/// and bounds; then evaluate every `visibleWhen`.
pub fn resolve_params(
    doc: &PartDoc,
    overrides: &ParamValues,
    process: &ProcessProfile,
) -> Result<ResolvedParams, EvalError> {
    if let Some(unknown) = overrides
        .values
        .keys()
        .find(|k| !doc.params.contains_key(*k))
    {
        return Err(EvalError::UnknownParam(unknown.clone()));
    }
    let mut values = BTreeMap::new();
    for (name, p) in &doc.params {
        let v = overrides.values.get(name).unwrap_or(&p.default);
        if v.is_null() {
            return Err(param_err(name, "has no default and no value was given"));
        }
        values.insert(name.clone(), check_param(name, p, v)?);
    }
    let mut resolved = ResolvedParams {
        values,
        visible: BTreeMap::new(),
    };
    let scope = Scope::new(doc, &resolved, process);
    let mut visible = BTreeMap::new();
    for (name, p) in &doc.params {
        let shown = match &p.visible_when {
            Some(e) => scope
                .condition(e)
                .map_err(|m| param_err(name, format!("visibleWhen `{e}`: {m}")))?,
            None => true,
        };
        visible.insert(name.clone(), shown);
    }
    resolved.visible = visible;
    Ok(resolved)
}

/// What names in fields and expressions resolve against.
pub(crate) struct Scope<'a> {
    doc: &'a PartDoc,
    params: &'a ResolvedParams,
    process: Value,
}

fn is_reference(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('$') else {
        return false;
    };
    let mut chars = rest.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl<'a> Scope<'a> {
    pub(crate) fn new(
        doc: &'a PartDoc,
        params: &'a ResolvedParams,
        process: &ProcessProfile,
    ) -> Scope<'a> {
        Scope {
            doc,
            params,
            process: serde_json::to_value(process).unwrap_or(Value::Null),
        }
    }

    /// Resolve a name for an expression: `$size` or `size` is a parameter,
    /// `profile.minFeature` a process profile field (the `Mm` suffix is optional).
    fn lookup(&self, name: &str) -> Result<Scalar, String> {
        if let Some(field) = name.strip_prefix("profile.") {
            let v = self
                .process
                .get(field)
                .or_else(|| self.process.get(format!("{field}Mm")))
                .ok_or_else(|| format!("the process profile has no field `{field}`"))?;
            return match v {
                Value::Number(n) => Ok(Scalar::Num(n.as_f64().unwrap_or(f64::NAN))),
                Value::Bool(b) => Ok(Scalar::Bool(*b)),
                Value::String(s) => Ok(Scalar::Str(s.clone())),
                _ => Err(format!(
                    "process profile field `{field}` is not a single value"
                )),
            };
        }
        let key = name.strip_prefix('$').unwrap_or(name);
        match self.params.values.get(key) {
            Some(Value::Number(n)) => Ok(Scalar::Num(n.as_f64().unwrap_or(f64::NAN))),
            Some(Value::Bool(b)) => Ok(Scalar::Bool(*b)),
            Some(Value::String(s)) => Ok(Scalar::Str(s.clone())),
            Some(_) => Err(format!(
                "parameter `{key}` is a list; use it as a whole field value (\"${key}\"), not in an expression"
            )),
            None => Err(format!("there is no parameter `{key}`")),
        }
    }

    /// Evaluate an `if` or `visibleWhen` expression.
    pub(crate) fn condition(&self, src: &str) -> Result<bool, String> {
        expr::eval_bool(src, &|n| self.lookup(n))
    }

    fn param(&self, name: &str) -> Result<FVal, String> {
        let v = self
            .params
            .values
            .get(name)
            .ok_or_else(|| format!("there is no parameter `{name}`"))?;
        let unit = self
            .doc
            .params
            .get(name)
            .map(param_unit)
            .unwrap_or(Unit::None);
        Ok(match v {
            Value::Number(n) => FVal::Num(n.as_f64().unwrap_or(f64::NAN), unit),
            Value::Array(a) => FVal::List(
                a.iter()
                    .map(|x| match x.as_f64() {
                        Some(n) => FVal::Num(n, unit),
                        None => from_json_literal(x),
                    })
                    .collect(),
            ),
            Value::String(s) => literal(s),
            other => from_json_literal(other),
        })
    }

    /// Resolve a feature field value.
    pub(crate) fn field(&self, v: &Value) -> Result<FVal, String> {
        Ok(match v {
            Value::String(s) if is_reference(s) => self.param(&s[1..])?,
            Value::String(s) if s.contains('$') || s.starts_with("profile.") => {
                match expr::eval(s, &|n| self.lookup(n))? {
                    Scalar::Num(n) => FVal::Num(n, Unit::None),
                    Scalar::Bool(b) => FVal::Bool(b),
                    Scalar::Str(s) => literal(&s),
                }
            }
            Value::String(s) => literal(s),
            Value::Array(a) => {
                FVal::List(a.iter().map(|x| self.field(x)).collect::<Result<_, _>>()?)
            }
            Value::Object(o) => FVal::Obj(
                o.iter()
                    .map(|(k, x)| Ok((k.clone(), self.field(x)?)))
                    .collect::<Result<_, String>>()?,
            ),
            other => from_json_literal(other),
        })
    }
}

fn from_json_literal(v: &Value) -> FVal {
    match v {
        Value::Null => FVal::Null,
        Value::Bool(b) => FVal::Bool(*b),
        Value::Number(n) => FVal::Num(n.as_f64().unwrap_or(f64::NAN), Unit::None),
        Value::String(s) => literal(s),
        Value::Array(a) => FVal::List(a.iter().map(from_json_literal).collect()),
        Value::Object(o) => FVal::Obj(
            o.iter()
                .map(|(k, x)| (k.clone(), from_json_literal(x)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities_and_unit_words() {
        assert_eq!(parse_quantity("0.6mm"), Some((0.6, Unit::Mm)));
        assert_eq!(parse_quantity("2stud"), Some((2.0, Unit::Stud)));
        assert_eq!(parse_quantity("2 studs"), Some((2.0, Unit::Stud)));
        assert_eq!(parse_quantity("1plate"), Some((1.0, Unit::Plate)));
        assert_eq!(parse_quantity("-0.5mm"), Some((-0.5, Unit::Mm)));
        assert_eq!(parse_quantity("plate"), None);
        assert_eq!(parse_quantity("2furlong"), None);
        assert_eq!(parse_quantity("1e3mm"), None);
        assert_eq!(literal("brick"), FVal::Num(1.0, Unit::Brick));
        assert_eq!(literal("emboss"), FVal::Str("emboss".into()));
    }

    #[test]
    fn heights_and_footprints() {
        let brick_plates = (BRICK_LDU / PLATE_LDU) as u32;
        assert_eq!(literal("plate").plates(), Ok(1));
        assert_eq!(literal("brick").plates(), Ok(brick_plates));
        assert_eq!(literal("2plate").plates(), Ok(2));
        assert_eq!(literal("24ldu").plates(), Ok(brick_plates));
        assert!(
            FVal::Num(2.0, Unit::None).plates().is_err(),
            "a bare number is ambiguous"
        );
        assert!(literal("0.5plate").plates().is_err());
        let size = FVal::List(vec![FVal::Num(2.0, Unit::None), literal("80ldu")]);
        assert_eq!(size.grid2(), Ok([2, 4]));
        assert!(
            FVal::List(vec![FVal::Num(0.0, Unit::None), FVal::Num(1.0, Unit::None)])
                .grid2()
                .is_err()
        );
        let pitch = STANDARD.get().mm("module");
        assert!((literal("2stud").mm().unwrap() - 2.0 * pitch).abs() < 1e-12);
    }
}
