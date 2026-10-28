//! `--param NAME=VALUE` on the command line, turned into the JSON value the
//! evaluator expects for the parameter's type (docs/plan/06 section 3).
//!
//! | Type | Accepted text | Becomes |
//! |------|---------------|---------|
//! | grid2 | `2x4`, `2,4`, `[2,4]` | `[2, 4]` |
//! | int, length, angle | `0.6`, or with a unit, `0.6mm` | a number, or the text for the evaluator to convert |
//! | bool | `true`/`false`, `yes`/`no`, `on`/`off`, `1`/`0` | `true` or `false` |
//! | enum | one of the options | the option (a number when the option is one) |
//! | svg, image, font | a file path, relative to the current directory | the file's name; the file is read now |
//! | list | JSON, `[1,2]` | the JSON |
//! | string, color, other | any text | the text |
//!
//! The evaluator then checks every value against the parameter's bounds,
//! steps and options, and its messages name what is allowed.

use std::collections::BTreeMap;

use openbrick_part::{Param, ParamValues, PartDoc};
use serde_json::Value;

/// Parameter values from the command line, plus the asset files they name.
#[derive(Debug, Default)]
pub struct CliParams {
    pub values: ParamValues,
    /// Asset parameters given on the command line (svg, image and font), as
    /// (parameter, path relative to the current directory). The caller reads
    /// each file and replaces the parameter's value with the name the
    /// evaluator finds it under (`FileAssets::add_given`).
    pub assets: Vec<(String, String)>,
}

/// Parse every `NAME=VALUE` pair against the part's parameters.
pub fn parse(doc: &PartDoc, pairs: &[String]) -> Result<CliParams, String> {
    let mut out = CliParams::default();
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    for pair in pairs {
        let Some((name, raw)) = pair.split_once('=') else {
            return Err(format!(
                "`{pair}` is not NAME=VALUE; for example --param size=2x4"
            ));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!(
                "`{pair}` has no parameter name; for example --param size=2x4"
            ));
        }
        let Some(param) = doc.params.get(name) else {
            let known: Vec<&str> = doc.params.keys().map(String::as_str).collect();
            return Err(format!(
                "part `{}` has no parameter `{name}`; its parameters are {}",
                doc.id,
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ));
        };
        if seen.insert(name.to_string(), ()).is_some() {
            return Err(format!("parameter `{name}` is given twice; give it once"));
        }
        let value = convert(param, raw.trim()).map_err(|m| format!("parameter `{name}`: {m}"))?;
        if matches!(param.ty.as_str(), "svg" | "image" | "font") {
            out.assets.push((name.to_string(), raw.trim().to_string()));
        }
        out.values.values.insert(name.to_string(), value);
    }
    Ok(out)
}

/// The JSON value for one parameter of `param`'s type.
fn convert(param: &Param, raw: &str) -> Result<Value, String> {
    Ok(match param.ty.as_str() {
        "grid2" => grid2(raw)?,
        "int" | "length" | "angle" => number_or_quantity(raw)?,
        "bool" => boolean(raw)?,
        "enum" => option(param, raw),
        "list" => serde_json::from_str(raw)
            .map_err(|e| format!("expected a JSON list such as [1, 2], found `{raw}` ({e})"))?,
        "svg" | "image" | "font" => {
            if raw.is_empty() {
                return Err("expected a file path, for example ./my-logo.svg".into());
            }
            Value::String(raw.to_string())
        }
        _ => Value::String(raw.to_string()),
    })
}

/// A footprint: `2x4`, `2X4`, `2,4` or JSON `[2,4]`.
fn grid2(raw: &str) -> Result<Value, String> {
    let bad = || format!("expected a footprint in studs such as 2x4 or [2,4], found `{raw}`");
    if raw.starts_with('[') {
        let v: Value = serde_json::from_str(raw).map_err(|_| bad())?;
        return match &v {
            Value::Array(a) if a.len() == 2 => Ok(v),
            _ => Err(bad()),
        };
    }
    let parts: Vec<&str> = raw.split(['x', 'X', ',', '\u{d7}']).collect();
    match parts.as_slice() {
        [a, b] => Ok(Value::Array(vec![
            number_or_quantity(a.trim()).map_err(|_| bad())?,
            number_or_quantity(b.trim()).map_err(|_| bad())?,
        ])),
        _ => Err(bad()),
    }
}

/// A plain number becomes a JSON number; a number with a unit (`0.6mm`,
/// `2stud`) stays text, which the evaluator converts to the parameter's unit.
fn number_or_quantity(raw: &str) -> Result<Value, String> {
    if let Ok(n) = raw.parse::<f64>() {
        if !n.is_finite() {
            return Err(format!("expected a finite number, found `{raw}`"));
        }
        return Ok(json_number(n));
    }
    let starts_like_a_number = raw
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.' || c == '+');
    if starts_like_a_number && raw.chars().any(|c| c.is_ascii_alphabetic()) {
        return Ok(Value::String(raw.to_string()));
    }
    Err(format!("expected a number, found `{raw}`"))
}

/// Whole numbers as JSON integers, others as floats.
fn json_number(n: f64) -> Value {
    // 2^53: every integer up to here is exact in f64.
    const EXACT: f64 = 9_007_199_254_740_992.0;
    if n.fract() == 0.0 && n.abs() < EXACT {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn boolean(raw: &str) -> Result<Value, String> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
        "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
        _ => Err(format!("expected true or false, found `{raw}`")),
    }
}

/// The option of an enum parameter that `raw` names; text that names none is
/// passed on as text, and the evaluator lists the options.
fn option(param: &Param, raw: &str) -> Value {
    for o in param.options.iter().flatten() {
        let v = o.get("value").unwrap_or(o);
        match v {
            Value::String(s) if s == raw => return v.clone(),
            Value::Number(n) if raw.parse::<f64>().ok() == n.as_f64() => return v.clone(),
            Value::Bool(b) if boolean(raw).ok() == Some(Value::Bool(*b)) => return v.clone(),
            _ => {}
        }
    }
    Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> PartDoc {
        PartDoc::parse(include_str!("../../../parts/ndn.logo-tile.obpart.json"))
            .expect("the logo tile parses")
    }

    fn one(pair: &str) -> Result<Value, String> {
        let p = parse(&doc(), &[pair.to_string()])?;
        Ok(p.values
            .values
            .values()
            .next()
            .cloned()
            .unwrap_or(Value::Null))
    }

    #[test]
    fn footprints() {
        for text in [
            "size=2x4",
            "size=2X4",
            "size=2,4",
            "size=[2,4]",
            "size= 2 x 4 ",
        ] {
            assert_eq!(one(text), Ok(json!([2, 4])), "{text}");
        }
        assert!(one("size=2x").is_err());
        assert!(one("size=2x4x6").is_err());
        assert!(one("size=[2]").is_err());
    }

    #[test]
    fn numbers_enums_and_assets() {
        assert_eq!(one("reliefHeight=0.8"), Ok(json!(0.8)));
        assert_eq!(one("reliefHeight=1"), Ok(json!(1)));
        assert_eq!(one("reliefHeight=0.8mm"), Ok(json!("0.8mm")));
        assert!(one("reliefHeight=tall").is_err());
        assert!(one("reliefHeight=NaN").is_err());
        assert!(one("reliefHeight=inf").is_err());
        assert_eq!(one("relief=inlay"), Ok(json!("inlay")));
        assert_eq!(one("height=brick"), Ok(json!("brick")));
        // Not an option: passed on, and the evaluator names the options.
        assert_eq!(one("relief=sparkle"), Ok(json!("sparkle")));
        let p = parse(&doc(), &["logo=./my logo.svg".into()]).expect("parses");
        assert_eq!(
            p.assets,
            vec![("logo".to_string(), "./my logo.svg".to_string())]
        );
        assert_eq!(p.values.values["logo"], json!("./my logo.svg"));
    }

    #[test]
    fn mistakes_name_the_fix() {
        let e = one("colour=red").expect_err("unknown");
        assert!(
            e.contains("no parameter `colour`") && e.contains("size"),
            "{e}"
        );
        let e = one("size").expect_err("no value");
        assert!(e.contains("NAME=VALUE"), "{e}");
        let e = parse(&doc(), &["size=2x4".into(), "size=2x2".into()]).expect_err("twice");
        assert!(e.contains("twice"), "{e}");
        assert!(one("logo=").is_err());
    }

    #[test]
    fn booleans() {
        for t in ["true", "Yes", "on", "1"] {
            assert_eq!(boolean(t), Ok(Value::Bool(true)));
        }
        for t in ["false", "NO", "off", "0"] {
            assert_eq!(boolean(t), Ok(Value::Bool(false)));
        }
        assert!(boolean("maybe").is_err());
    }
}
