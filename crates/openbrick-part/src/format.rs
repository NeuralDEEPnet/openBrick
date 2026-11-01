//! The text of a saved part document (docs/plan/06 sections 2, 3 and 11): one
//! deterministic layout, so the playground, the desktop app and the CLI write
//! byte-identical files for the same edit, and a document saved twice does
//! not change.
//!
//! The layout keeps the order of the text it starts from (members are never
//! sorted) and indents by two spaces. An array or object is written on one
//! line, `[2, 4]` or `{ "value": "plate", "label": "Plate (1/3)" }`, when that
//! line, with its indentation, key and trailing comma, is at most
//! [`LINE_WIDTH`] characters; otherwise it opens a line per member. The
//! document itself always opens a line per member. Strings and numbers are
//! kept exactly as written (`1.0` stays `1.0`, an escape stays an escape);
//! values the designer changed are written as serde_json writes them. The
//! file ends with a newline.

use serde::Serialize;

/// Longest line, in characters, that a one-line array or object may take.
pub const LINE_WIDTH: usize = 100;

/// A JSON value in the order it was written. Strings (quotes and escapes
/// included) and numbers keep their spelling.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Scalar(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    /// The member `key` (a bare name; the stored key is quoted) of an object.
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        let quoted = quote(key);
        match self {
            Node::Object(members) => members
                .iter_mut()
                .find(|(k, _)| *k == quoted)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Set member `key` of an object: in place when it exists, else right
    /// after the member `after` (the first of them that exists), else last.
    pub(crate) fn set(&mut self, key: &str, value: Node, after: &[&str]) {
        let quoted = quote(key);
        let Node::Object(members) = self else {
            return;
        };
        if let Some(slot) = members.iter_mut().find(|(k, _)| *k == quoted) {
            slot.1 = value;
            return;
        }
        let at = after
            .iter()
            .find_map(|a| members.iter().position(|(k, _)| *k == quote(a)))
            .map_or(members.len(), |i| i + 1);
        members.insert(at, (quoted, value));
    }

    /// Remove member `key` of an object, if it has one.
    pub(crate) fn remove(&mut self, key: &str) {
        let quoted = quote(key);
        if let Node::Object(members) = self {
            members.retain(|(k, _)| *k != quoted);
        }
    }

    /// The node of any serialisable value, as serde_json writes it.
    pub(crate) fn of<T: Serialize + ?Sized>(value: &T) -> Result<Node, serde_json::Error> {
        let text = serde_json::to_string(value)?;
        Ok(parse(&text).expect("serde_json writes JSON"))
    }
}

/// A key as stored: a JSON string with its quotes.
fn quote(key: &str) -> String {
    serde_json::Value::String(key.to_string()).to_string()
}

/// Reads JSON text that is known to be valid (serde_json read it first).
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn skip_space(&mut self) {
        let bytes = self.text.as_bytes();
        while self.at < bytes.len() && matches!(bytes[self.at], b' ' | b'\t' | b'\n' | b'\r') {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Result<u8, String> {
        self.skip_space();
        self.text
            .as_bytes()
            .get(self.at)
            .copied()
            .ok_or_else(|| "the JSON text ends early".to_string())
    }

    fn take(&mut self, b: u8) -> Result<(), String> {
        if self.peek()? != b {
            return Err(format!(
                "expected `{}` at byte {} of the JSON text",
                b as char, self.at
            ));
        }
        self.at += 1;
        Ok(())
    }

    /// A string, quotes and escapes included.
    fn string(&mut self) -> Result<String, String> {
        let start = self.at;
        self.take(b'"')?;
        let bytes = self.text.as_bytes();
        loop {
            match bytes.get(self.at) {
                None => return Err("a string in the JSON text does not end".to_string()),
                Some(b'"') => break,
                Some(b'\\') => self.at += 2,
                Some(_) => self.at += 1,
            }
        }
        self.at += 1;
        Ok(self.text[start..self.at].to_string())
    }

    fn value(&mut self) -> Result<Node, String> {
        match self.peek()? {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                if self.peek()? == b'}' {
                    self.at += 1;
                    return Ok(Node::Object(members));
                }
                loop {
                    self.skip_space();
                    let key = self.string()?;
                    self.take(b':')?;
                    members.push((key, self.value()?));
                    match self.peek()? {
                        b',' => self.at += 1,
                        b'}' => {
                            self.at += 1;
                            return Ok(Node::Object(members));
                        }
                        _ => return Err(format!("expected `,` or `}}` at byte {}", self.at)),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                if self.peek()? == b']' {
                    self.at += 1;
                    return Ok(Node::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    match self.peek()? {
                        b',' => self.at += 1,
                        b']' => {
                            self.at += 1;
                            return Ok(Node::Array(items));
                        }
                        _ => return Err(format!("expected `,` or `]` at byte {}", self.at)),
                    }
                }
            }
            b'"' => Ok(Node::Scalar(self.string()?)),
            _ => {
                let start = self.at;
                let bytes = self.text.as_bytes();
                while self.at < bytes.len()
                    && !matches!(
                        bytes[self.at],
                        b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r'
                    )
                {
                    self.at += 1;
                }
                if start == self.at {
                    return Err(format!("expected a value at byte {start}"));
                }
                Ok(Node::Scalar(self.text[start..self.at].to_string()))
            }
        }
    }
}

/// The node tree of JSON text, in written order. Meant for text serde_json has
/// already accepted; anything else may be refused with a short reason.
pub(crate) fn parse(text: &str) -> Result<Node, String> {
    let mut r = Reader { text, at: 0 };
    let node = r.value()?;
    r.skip_space();
    if r.at != text.len() {
        return Err(format!(
            "unexpected text after the JSON value at byte {}",
            r.at
        ));
    }
    Ok(node)
}

/// The value on one line: `[2, 4]`, `{ "a": 1 }`, `[]`, `{}`.
fn inline(node: &Node, out: &mut String) {
    match node {
        Node::Scalar(s) => out.push_str(s),
        Node::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                inline(item, out);
            }
            out.push(']');
        }
        Node::Object(members) if members.is_empty() => out.push_str("{}"),
        Node::Object(members) => {
            out.push_str("{ ");
            for (i, (key, value)) in members.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(key);
                out.push_str(": ");
                inline(value, out);
            }
            out.push_str(" }");
        }
    }
}

/// Write `node` at `depth`; `used` is what its line already holds (the
/// indentation and the key) and `tail` the characters that follow it (a
/// comma, or nothing). The top level always opens.
fn block(node: &Node, depth: usize, used: usize, tail: usize, out: &mut String) {
    let mut one = String::new();
    inline(node, &mut one);
    let fits = depth > 0 && used + one.chars().count() + tail <= LINE_WIDTH;
    let count = match node {
        Node::Array(items) if !items.is_empty() && !fits => items.len(),
        Node::Object(members) if !members.is_empty() && !fits => members.len(),
        _ => {
            out.push_str(&one);
            return;
        }
    };
    let pad = "  ".repeat(depth + 1);
    let (open, close) = match node {
        Node::Array(_) => ('[', ']'),
        _ => ('{', '}'),
    };
    out.push(open);
    out.push('\n');
    for i in 0..count {
        let comma = usize::from(i + 1 < count);
        out.push_str(&pad);
        match node {
            Node::Array(items) => block(&items[i], depth + 1, pad.len(), comma, out),
            Node::Object(members) => {
                let (key, value) = &members[i];
                out.push_str(key);
                out.push_str(": ");
                let used = pad.len() + key.chars().count() + 2;
                block(value, depth + 1, used, comma, out);
            }
            Node::Scalar(_) => unreachable!("scalars are written above"),
        }
        if comma == 1 {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&"  ".repeat(depth));
    out.push(close);
}

/// A node tree in the saved layout, ending with a newline.
pub(crate) fn layout(node: &Node) -> String {
    let mut out = String::new();
    block(node, 0, 0, 0, &mut out);
    out.push('\n');
    out
}

/// The saved text of any serialisable value: the layout of this module over
/// what serde_json writes (struct fields in declaration order).
pub fn to_saved_text<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    Ok(layout(&Node::of(value)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn short_containers_stay_on_one_line_and_long_ones_open() {
        let text = r#"{"size":[2,4],"empty":[ ],"none":{},
            "text":"a \"quoted\" \\ string, with: [brackets] {and} braces \u00e9",
            "long":["aaaaaaaaaaaaaaaaaaaa","bbbbbbbbbbbbbbbbbbbb","cccccccccccccccccccc","dddddddddddddddddddd"],
            "nested":{"b":{"a":[1.50,-2e-7,true,null]}}}"#;
        let saved = layout(&parse(text).unwrap());
        // Members keep their order; numbers and escapes their spelling.
        assert_eq!(
            saved,
            concat!(
                "{\n",
                "  \"size\": [2, 4],\n",
                "  \"empty\": [],\n",
                "  \"none\": {},\n",
                "  \"text\": \"a \\\"quoted\\\" \\\\ string, with: [brackets] {and} braces \\u00e9\",\n",
                "  \"long\": [\n",
                "    \"aaaaaaaaaaaaaaaaaaaa\",\n",
                "    \"bbbbbbbbbbbbbbbbbbbb\",\n",
                "    \"cccccccccccccccccccc\",\n",
                "    \"dddddddddddddddddddd\"\n",
                "  ],\n",
                "  \"nested\": { \"b\": { \"a\": [1.50, -2e-7, true, null] } }\n",
                "}\n"
            )
        );
        let a: Value = serde_json::from_str(text).unwrap();
        let b: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(a, b, "the same JSON value");
        assert_eq!(layout(&parse(&saved).unwrap()), saved, "idempotent");
    }

    #[test]
    fn the_line_width_is_a_hard_limit_for_one_line_containers() {
        // `  "k": ["x…x"]` is the string's length plus 11 characters: a
        // 100-character line stays; one more character opens it.
        for (n, opens) in [(LINE_WIDTH - 11, false), (LINE_WIDTH - 10, true)] {
            let text = to_saved_text(&json!({ "k": [ "x".repeat(n) ] })).unwrap();
            let line = text.lines().nth(1).unwrap();
            assert_eq!(line.ends_with('['), opens, "{n}: {line}");
            if !opens {
                assert_eq!(line.chars().count(), LINE_WIDTH);
            }
        }
    }

    #[test]
    fn members_are_set_in_place_or_after_a_neighbour_and_removed() {
        let mut node = parse(r#"{"a":1,"b":2}"#).unwrap();
        node.set("b", Node::of(&json!([3])).unwrap(), &[]);
        node.set("c", Node::of(&json!("x")).unwrap(), &["zz", "a"]);
        node.set("d", Node::of(&json!(null)).unwrap(), &["zz"]);
        assert_eq!(
            layout(&node),
            "{\n  \"a\": 1,\n  \"c\": \"x\",\n  \"b\": [3],\n  \"d\": null\n}\n"
        );
        node.remove("c");
        node.remove("nope");
        assert!(node.get_mut("c").is_none());
        assert_eq!(node.get_mut("a"), Some(&mut Node::Scalar("1".into())));
    }

    #[test]
    fn scalars_top_level_values_and_bad_text() {
        assert_eq!(to_saved_text(&json!(1.0)).unwrap(), "1.0\n");
        assert_eq!(to_saved_text(&json!([])).unwrap(), "[]\n");
        assert_eq!(to_saved_text(&json!([1])).unwrap(), "[\n  1\n]\n");
        assert_eq!(to_saved_text(&json!("é")).unwrap(), "\"é\"\n");
        for bad in ["", "{", "[1,", "{\"a\" 1}", "\"open", "1 2", "{\"a\":}"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
