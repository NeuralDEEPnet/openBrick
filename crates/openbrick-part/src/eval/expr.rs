//! The small expression language of `if`, `visibleWhen` and computed feature fields
//! (docs/plan/06 sections 2 and 3), for example `$studsOnTop`, `!$studsOnTop`,
//! `$loop != 'none'`, `relief != 'inlay'` or `$length - 1`.
//!
//! Grammar, loosest binding first:
//!
//! ```text
//! or      := and ('||' and)*
//! and     := compare ('&&' compare)*
//! compare := sum (('==' | '!=' | '<' | '<=' | '>' | '>=') sum)?
//! sum     := product (('+' | '-') product)*
//! product := unary (('*' | '/') unary)*
//! unary   := ('!' | '-') unary | primary
//! primary := number | 'string' | "string" | true | false | name | '(' or ')'
//! name    := '$'? identifier ('.' identifier)*
//! ```
//!
//! A name is a parameter (`$size` or `size`) or a process profile field
//! (`profile.minFeature`); the caller resolves it. There are no functions, no
//! assignment and no loops, so evaluation always terminates. Values are numbers,
//! booleans and strings; operators never convert between them, so a type mismatch
//! is an error with a message rather than a silent `false`.

use std::fmt;

/// A value an expression produces or a name resolves to.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Num(f64),
    Bool(bool),
    Str(String),
}

impl Scalar {
    fn type_name(&self) -> &'static str {
        match self {
            Scalar::Num(_) => "number",
            Scalar::Bool(_) => "boolean",
            Scalar::Str(_) => "string",
        }
    }
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scalar::Num(n) => write!(f, "{n}"),
            Scalar::Bool(b) => write!(f, "{b}"),
            Scalar::Str(s) => write!(f, "'{s}'"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Op(&'static str),
    Open,
    Close,
}

const OPS: &[&str] = &[
    "==", "!=", "<=", ">=", "&&", "||", "<", ">", "!", "+", "-", "*", "/",
];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let n = text
                .parse::<f64>()
                .map_err(|_| format!("`{text}` is not a number"))?;
            out.push(Tok::Num(n));
        } else if c == '\'' || c == '"' {
            let close = chars[i + 1..]
                .iter()
                .position(|&d| d == c)
                .ok_or_else(|| format!("string starting at character {} is not closed", i + 1))?;
            out.push(Tok::Str(chars[i + 1..i + 1 + close].iter().collect()));
            i += close + 2;
        } else if c == '$' || is_ident_start(c) {
            let start = i;
            if c == '$' {
                i += 1;
                if !chars.get(i).is_some_and(|&d| is_ident_start(d)) {
                    return Err("`$` must be followed by a parameter name".into());
                }
            }
            loop {
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                // A dot continues the name only when an identifier follows it.
                if chars.get(i) == Some(&'.')
                    && chars.get(i + 1).is_some_and(|&d| is_ident_start(d))
                {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else if c == '(' {
            out.push(Tok::Open);
            i += 1;
        } else if c == ')' {
            out.push(Tok::Close);
            i += 1;
        } else {
            let rest: String = chars[i..].iter().take(2).collect();
            let op = OPS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| format!("unexpected character `{c}`"))?;
            out.push(Tok::Op(op));
            i += op.len();
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Ast {
    Lit(Scalar),
    Name(String),
    Not(Box<Ast>),
    Neg(Box<Ast>),
    Bin(&'static str, Box<Ast>, Box<Ast>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek_op(&self, ops: &[&'static str]) -> Option<&'static str> {
        match self.toks.get(self.pos) {
            Some(Tok::Op(op)) if ops.contains(op) => Some(op),
            _ => None,
        }
    }

    fn binary(
        &mut self,
        ops: &[&'static str],
        next: fn(&mut Parser) -> Result<Ast, String>,
        chain: bool,
    ) -> Result<Ast, String> {
        let mut left = next(self)?;
        while let Some(op) = self.peek_op(ops) {
            self.pos += 1;
            let right = next(self)?;
            left = Ast::Bin(op, Box::new(left), Box::new(right));
            if !chain {
                if self.peek_op(ops).is_some() {
                    return Err("comparisons cannot be chained; use && between them".into());
                }
                break;
            }
        }
        Ok(left)
    }

    fn or(&mut self) -> Result<Ast, String> {
        self.binary(&["||"], Parser::and, true)
    }

    fn and(&mut self) -> Result<Ast, String> {
        self.binary(&["&&"], Parser::compare, true)
    }

    fn compare(&mut self) -> Result<Ast, String> {
        self.binary(&["==", "!=", "<", "<=", ">", ">="], Parser::sum, false)
    }

    fn sum(&mut self) -> Result<Ast, String> {
        self.binary(&["+", "-"], Parser::product, true)
    }

    fn product(&mut self) -> Result<Ast, String> {
        self.binary(&["*", "/"], Parser::unary, true)
    }

    fn unary(&mut self) -> Result<Ast, String> {
        match self.peek_op(&["!", "-"]) {
            Some("!") => {
                self.pos += 1;
                Ok(Ast::Not(Box::new(self.unary()?)))
            }
            Some(_) => {
                self.pos += 1;
                Ok(Ast::Neg(Box::new(self.unary()?)))
            }
            None => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Ast, String> {
        let tok = self
            .toks
            .get(self.pos)
            .cloned()
            .ok_or("the expression ends where a value was expected")?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => Ok(Ast::Lit(Scalar::Num(n))),
            Tok::Str(s) => Ok(Ast::Lit(Scalar::Str(s))),
            Tok::Name(n) if n == "true" => Ok(Ast::Lit(Scalar::Bool(true))),
            Tok::Name(n) if n == "false" => Ok(Ast::Lit(Scalar::Bool(false))),
            Tok::Name(n) => Ok(Ast::Name(n)),
            Tok::Open => {
                let inner = self.or()?;
                match self.toks.get(self.pos) {
                    Some(Tok::Close) => {
                        self.pos += 1;
                        Ok(inner)
                    }
                    _ => Err("a `(` is not closed".into()),
                }
            }
            Tok::Close => Err("unexpected `)`".into()),
            Tok::Op(op) => Err(format!("unexpected operator `{op}`")),
        }
    }
}

fn parse(src: &str) -> Result<Ast, String> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err("the expression is empty".into());
    }
    let mut p = Parser { toks, pos: 0 };
    let ast = p.or()?;
    if p.pos != p.toks.len() {
        return Err(format!(
            "unexpected {} after a complete expression",
            match &p.toks[p.pos] {
                Tok::Num(n) => format!("number {n}"),
                Tok::Str(s) => format!("string '{s}'"),
                Tok::Name(n) => format!("name `{n}`"),
                Tok::Op(op) => format!("operator `{op}`"),
                Tok::Open => "`(`".into(),
                Tok::Close => "`)`".into(),
            }
        ));
    }
    Ok(ast)
}

type Resolve<'a> = dyn Fn(&str) -> Result<Scalar, String> + 'a;

fn eval_ast(ast: &Ast, resolve: &Resolve) -> Result<Scalar, String> {
    match ast {
        Ast::Lit(v) => Ok(v.clone()),
        Ast::Name(n) => resolve(n),
        Ast::Not(a) => match eval_ast(a, resolve)? {
            Scalar::Bool(b) => Ok(Scalar::Bool(!b)),
            v => Err(format!("`!` needs a boolean, found {} {v}", v.type_name())),
        },
        Ast::Neg(a) => match eval_ast(a, resolve)? {
            Scalar::Num(n) => Ok(Scalar::Num(-n)),
            v => Err(format!("`-` needs a number, found {} {v}", v.type_name())),
        },
        Ast::Bin(op @ ("&&" | "||"), a, b) => {
            let left = match eval_ast(a, resolve)? {
                Scalar::Bool(x) => x,
                v => {
                    return Err(format!(
                        "`{op}` needs booleans, found {} {v}",
                        v.type_name()
                    ))
                }
            };
            // Short-circuit, so `$a && $b` does not evaluate $b when $a is false.
            if (*op == "&&" && !left) || (*op == "||" && left) {
                return Ok(Scalar::Bool(left));
            }
            match eval_ast(b, resolve)? {
                Scalar::Bool(y) => Ok(Scalar::Bool(y)),
                v => Err(format!(
                    "`{op}` needs booleans, found {} {v}",
                    v.type_name()
                )),
            }
        }
        Ast::Bin(op, a, b) => {
            let (x, y) = (eval_ast(a, resolve)?, eval_ast(b, resolve)?);
            binary(op, x, y)
        }
    }
}

fn binary(op: &str, x: Scalar, y: Scalar) -> Result<Scalar, String> {
    use Scalar::{Bool, Num, Str};
    match (op, &x, &y) {
        ("==", _, _) | ("!=", _, _) => {
            if std::mem::discriminant(&x) != std::mem::discriminant(&y) {
                return Err(format!(
                    "cannot compare {} {x} with {} {y}",
                    x.type_name(),
                    y.type_name()
                ));
            }
            Ok(Bool((x == y) == (op == "==")))
        }
        ("<", Num(a), Num(b)) => Ok(Bool(a < b)),
        ("<=", Num(a), Num(b)) => Ok(Bool(a <= b)),
        (">", Num(a), Num(b)) => Ok(Bool(a > b)),
        (">=", Num(a), Num(b)) => Ok(Bool(a >= b)),
        ("+", Num(a), Num(b)) => Ok(Num(a + b)),
        ("-", Num(a), Num(b)) => Ok(Num(a - b)),
        ("*", Num(a), Num(b)) => Ok(Num(a * b)),
        ("/", Num(_), Num(b)) if *b == 0.0 => Err("division by zero".into()),
        ("/", Num(a), Num(b)) => Ok(Num(a / b)),
        (_, Str(_) | Bool(_), _) | (_, _, Str(_) | Bool(_)) => Err(format!(
            "`{op}` needs numbers, found {} {x} and {} {y}",
            x.type_name(),
            y.type_name()
        )),
        _ => Err(format!("unknown operator `{op}`")),
    }
}

/// Evaluate an expression; `resolve` looks up names (with their `$` if written).
pub fn eval(src: &str, resolve: &Resolve) -> Result<Scalar, String> {
    eval_ast(&parse(src)?, resolve)
}

/// Evaluate an expression that must produce a boolean (`if`, `visibleWhen`).
pub fn eval_bool(src: &str, resolve: &Resolve) -> Result<bool, String> {
    match eval(src, resolve)? {
        Scalar::Bool(b) => Ok(b),
        v => Err(format!(
            "must be true or false, but it is the {} {v}",
            v.type_name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &str) -> Result<Scalar, String> {
        match n.trim_start_matches('$') {
            "studsOnTop" => Ok(Scalar::Bool(false)),
            "loop" => Ok(Scalar::Str("left".into())),
            "relief" => Ok(Scalar::Str("inlay".into())),
            "length" => Ok(Scalar::Num(8.0)),
            "profile.minFeature" => Ok(Scalar::Num(0.8)),
            other => Err(format!("unknown name `{other}`")),
        }
    }

    #[test]
    fn example_part_expressions() {
        assert!(!eval_bool("$studsOnTop", &names).unwrap());
        assert!(eval_bool("!$studsOnTop", &names).unwrap());
        assert!(eval_bool("$loop != 'none'", &names).unwrap());
        assert!(!eval_bool("relief != 'inlay'", &names).unwrap());
        assert!(eval_bool("relief == \"inlay\"", &names).unwrap());
    }

    #[test]
    fn arithmetic_precedence_and_logic() {
        assert_eq!(eval("$length - 1", &names).unwrap(), Scalar::Num(7.0));
        assert_eq!(eval("2 + 3 * 4", &names).unwrap(), Scalar::Num(14.0));
        assert_eq!(eval("(2 + 3) * 4", &names).unwrap(), Scalar::Num(20.0));
        assert_eq!(eval("-$length / 2", &names).unwrap(), Scalar::Num(-4.0));
        assert!(eval_bool("$length >= 4 && !$studsOnTop", &names).unwrap());
        assert!(eval_bool("$studsOnTop || $length > 7.5", &names).unwrap());
        assert!(eval_bool("profile.minFeature <= 0.8", &names).unwrap());
        // Short-circuit: the unknown name on the right is never looked up.
        assert!(!eval_bool("$studsOnTop && $missing", &names).unwrap());
    }

    #[test]
    fn errors_are_explained() {
        let err = |src: &str| eval(src, &names).unwrap_err();
        assert!(err("$loop == 3").contains("cannot compare string"));
        assert!(err("$missing").contains("unknown name"));
        assert!(err("1 +").contains("ends where a value was expected"));
        assert!(err("(1 + 2").contains("not closed"));
        assert!(err("'open").contains("not closed"));
        assert!(err("1 < 2 < 3").contains("cannot be chained"));
        assert!(err("1 / 0").contains("division by zero"));
        assert!(err("!$length").contains("needs a boolean"));
        assert!(err("1 2").contains("after a complete expression"));
        assert!(err("$").contains("parameter name"));
        assert!(err("a # b").contains("unexpected character"));
        assert!(eval_bool("$length", &names)
            .unwrap_err()
            .contains("must be true or false"));
    }
}
