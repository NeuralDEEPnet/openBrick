//! Our own parser for the LDraw file format, written from the public format
//! specification (https://www.ldraw.org/article/218.html): line types 0 to 5,
//! the colour and matrix fields of type 1, the BFC meta commands, and the
//! header lines the importer reads (description, `Name:`, `Author:`,
//! `!LDRAW_ORG`, `!LICENSE`, `!CATEGORY`, `!KEYWORDS`).
//!
//! No other parser's code or data is used (docs/plan/08 section 3; CLAUDE.md:
//! no GPL, AGPL or LGPL code, and no LDCad shadow data). The input is
//! untrusted: it is bounded in size and in line length, a line that does not
//! parse is skipped and counted (the format says to ignore such lines), numbers
//! that are not finite or absurdly large reject the line, and nothing panics.

/// The longest line read, in bytes; a longer line is skipped. The spec asks
/// for lines to stay far below this.
pub const MAX_LINE_LEN: usize = 4096;
/// The most lines kept from one file.
pub const MAX_LINES: usize = 400_000;
/// Coordinates beyond this magnitude reject the line (LDU; the biggest real
/// parts are a few thousand).
pub const MAX_COORD: f64 = 1.0e7;

/// Which way the vertices of a polygon run seen from its front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winding {
    Ccw,
    Cw,
}

/// A `0 BFC ...` meta command (LDraw spec, "Back Face Culling").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bfc {
    /// `CERTIFY` with its winding (CCW when none is given).
    Certify(Winding),
    /// `NOCERTIFY`: the file's winding is unknown.
    NoCertify,
    Ccw,
    Cw,
    /// Inverts the winding of the next sub-file reference.
    InvertNext,
    Clip,
    NoClip,
}

/// A type-1 line: a placed sub-file.
#[derive(Clone, Debug, PartialEq)]
pub struct SubFile {
    pub colour: u32,
    /// `x y z` then the 3 x 3 matrix `a b c d e f g h i`, row-major: a point
    /// `p` of the sub-file lands at `M p + t`.
    pub t: [f64; 3],
    pub m: [[f64; 3]; 3],
    /// The file name as written (the resolver normalises it).
    pub file: String,
}

/// One line of a file that the importer uses.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Bfc(Bfc),
    SubFile(SubFile),
    /// Type 2.
    Segment {
        colour: u32,
        p: [[f64; 3]; 2],
    },
    /// Type 3.
    Tri {
        colour: u32,
        p: [[f64; 3]; 3],
    },
    /// Type 4: four vertices in order around the quad.
    Quad {
        colour: u32,
        p: [[f64; 3]; 4],
    },
    /// Type 5: the optional line, with its two control points.
    Cond {
        colour: u32,
        p: [[f64; 3]; 2],
        c: [[f64; 3]; 2],
    },
}

/// What the header says about a file (LDraw spec, "File Header" and the
/// official library's header lines).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// The first meta line: the part's description.
    pub description: String,
    pub name: String,
    pub author: String,
    /// `Part`, `Subpart`, `Primitive`, `48_Primitive`, `8_Primitive`, `Shortcut`, ...
    pub org_type: String,
    /// Words after the type before `UPDATE` or `ORIGINAL`: `ALIAS`, `PHYSICAL_COLOUR`, ...
    pub org_qualifiers: Vec<String>,
    /// `2026-10` from `UPDATE 2026-10`.
    pub update: Option<String>,
    pub license: String,
    pub category: Option<String>,
    pub keywords: Vec<String>,
}

/// A parsed file.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedFile {
    pub header: Header,
    pub lines: Vec<Line>,
    /// `0 BFC CERTIFY` was seen (and not undone by `NOCERTIFY`).
    pub certified: bool,
    /// The winding in force at the top of the file, from its first
    /// `BFC CERTIFY` or `CCW`/`CW` before any geometry; CCW by default.
    pub winding: Winding,
    /// Lines skipped because they did not parse.
    pub skipped: usize,
}

fn num(tok: &str) -> Option<f64> {
    let v: f64 = tok.parse().ok()?;
    (v.is_finite() && v.abs() <= MAX_COORD).then_some(v)
}

fn colour(tok: &str) -> Option<u32> {
    if let Some(hex) = tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        tok.parse().ok()
    }
}

fn points<const N: usize>(toks: &[&str]) -> Option<[[f64; 3]; N]> {
    if toks.len() < N * 3 {
        return None;
    }
    let mut out = [[0.0; 3]; N];
    for (i, p) in out.iter_mut().enumerate() {
        for (k, v) in p.iter_mut().enumerate() {
            *v = num(toks[i * 3 + k])?;
        }
    }
    Some(out)
}

/// Parse one line; `None` when it is a comment we do not use or it is malformed
/// (the second value tells which).
fn parse_line(line: &str) -> (Option<Line>, bool) {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let Some(&kind) = toks.first() else {
        return (None, false);
    };
    let rest = &toks[1..];
    let bad = (None, true);
    match kind {
        "0" => (parse_bfc(rest).map(Line::Bfc), false),
        "1" => {
            if rest.len() < 14 {
                return bad;
            }
            let Some(c) = colour(rest[0]) else {
                return bad;
            };
            let mut v = [0.0; 12];
            for (i, slot) in v.iter_mut().enumerate() {
                let Some(x) = num(rest[1 + i]) else {
                    return bad;
                };
                *slot = x;
            }
            // The file name is the rest of the line; it may hold spaces.
            let after = line.trim_start().strip_prefix('1').unwrap_or("");
            let mut s = after;
            for _ in 0..13 {
                s = s.trim_start();
                let end = s.find(char::is_whitespace).unwrap_or(s.len());
                s = &s[end..];
            }
            let file = s.trim().to_string();
            if file.is_empty() {
                return bad;
            }
            (
                Some(Line::SubFile(SubFile {
                    colour: c,
                    t: [v[0], v[1], v[2]],
                    m: [[v[3], v[4], v[5]], [v[6], v[7], v[8]], [v[9], v[10], v[11]]],
                    file,
                })),
                false,
            )
        }
        "2" => match (
            rest.first().and_then(|t| colour(t)),
            points::<2>(&rest[1.min(rest.len())..]),
        ) {
            (Some(c), Some(p)) => (Some(Line::Segment { colour: c, p }), false),
            _ => bad,
        },
        "3" => match (
            rest.first().and_then(|t| colour(t)),
            points::<3>(&rest[1.min(rest.len())..]),
        ) {
            (Some(c), Some(p)) => (Some(Line::Tri { colour: c, p }), false),
            _ => bad,
        },
        "4" => match (
            rest.first().and_then(|t| colour(t)),
            points::<4>(&rest[1.min(rest.len())..]),
        ) {
            (Some(c), Some(p)) => (Some(Line::Quad { colour: c, p }), false),
            _ => bad,
        },
        "5" => match (
            rest.first().and_then(|t| colour(t)),
            points::<4>(&rest[1.min(rest.len())..]),
        ) {
            (Some(c), Some(q)) => (
                Some(Line::Cond {
                    colour: c,
                    p: [q[0], q[1]],
                    c: [q[2], q[3]],
                }),
                false,
            ),
            _ => bad,
        },
        // Not a line type: ignored, as the format says.
        _ => bad,
    }
}

fn parse_bfc(rest: &[&str]) -> Option<Bfc> {
    if rest.first().map(|t| t.to_ascii_uppercase()) != Some("BFC".into()) {
        return None;
    }
    let words: Vec<String> = rest[1..].iter().map(|t| t.to_ascii_uppercase()).collect();
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    match w.as_slice() {
        ["CERTIFY"] | ["CERTIFY", "CCW"] => Some(Bfc::Certify(Winding::Ccw)),
        ["CERTIFY", "CW"] => Some(Bfc::Certify(Winding::Cw)),
        ["CCW", "CLIP"] | ["CLIP", "CCW"] | ["CCW"] => Some(Bfc::Ccw),
        ["CW", "CLIP"] | ["CLIP", "CW"] | ["CW"] => Some(Bfc::Cw),
        ["NOCERTIFY"] => Some(Bfc::NoCertify),
        ["INVERTNEXT"] => Some(Bfc::InvertNext),
        ["CLIP"] => Some(Bfc::Clip),
        ["NOCLIP"] => Some(Bfc::NoClip),
        _ => None,
    }
}

fn header_field(text: &str, key: &str) -> Option<String> {
    let t = text.trim();
    let (k, v) = t.split_once(char::is_whitespace)?;
    k.eq_ignore_ascii_case(key).then(|| v.trim().to_string())
}

fn read_header(text: &str, h: &mut Header, first_meta_seen: &mut bool) {
    let t = text.trim();
    if t.is_empty() {
        return;
    }
    if !*first_meta_seen {
        *first_meta_seen = true;
        // The first meta line is the description, unless it is already a keyword line.
        if !t.starts_with('!')
            && !t.to_ascii_lowercase().starts_with("name:")
            && !t.to_ascii_lowercase().starts_with("author:")
        {
            h.description = t.to_string();
            return;
        }
    }
    let lower = t.to_ascii_lowercase();
    if let Some(v) = lower.strip_prefix("name:") {
        let off = t.len() - v.len();
        h.name = t[off..].trim().to_string();
    } else if lower.starts_with("author:") {
        h.author = t["author:".len()..].trim().to_string();
    } else if lower.starts_with("!ldraw_org") {
        let words: Vec<&str> = t.split_whitespace().skip(1).collect();
        if let Some((&ty, rest)) = words.split_first() {
            h.org_type = ty.to_string();
            let mut i = 0;
            while i < rest.len() {
                match rest[i].to_ascii_uppercase().as_str() {
                    "UPDATE" => {
                        h.update = rest.get(i + 1).map(|s| s.to_string());
                        i += 1;
                    }
                    "ORIGINAL" => {}
                    q => h.org_qualifiers.push(q.to_string()),
                }
                i += 1;
            }
        }
    } else if let Some(v) = header_field(t, "!LICENSE") {
        h.license = v;
    } else if let Some(v) = header_field(t, "!CATEGORY") {
        h.category = Some(v);
    } else if let Some(v) = header_field(t, "!KEYWORDS") {
        h.keywords.extend(
            v.split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty()),
        );
    }
}

/// Parse the text of one file. Never fails: malformed lines are skipped and
/// counted in [`ParsedFile::skipped`].
pub fn parse(text: &str) -> ParsedFile {
    let mut file = ParsedFile {
        header: Header::default(),
        lines: Vec::new(),
        certified: false,
        winding: Winding::Ccw,
        skipped: 0,
    };
    let mut first_meta_seen = false;
    let mut geometry_seen = false;
    let mut winding_set = false;
    for raw in text.lines() {
        if file.lines.len() >= MAX_LINES {
            file.skipped += 1;
            continue;
        }
        if raw.len() > MAX_LINE_LEN {
            file.skipped += 1;
            continue;
        }
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('0') {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                let (l, _) = parse_line(line);
                match l {
                    Some(Line::Bfc(b)) => {
                        match b {
                            Bfc::Certify(w) => {
                                file.certified = true;
                                if !winding_set && !geometry_seen {
                                    file.winding = w;
                                    winding_set = true;
                                }
                            }
                            Bfc::NoCertify => file.certified = false,
                            Bfc::Ccw if !winding_set && !geometry_seen => {
                                file.winding = Winding::Ccw;
                                winding_set = true;
                            }
                            Bfc::Cw if !winding_set && !geometry_seen => {
                                file.winding = Winding::Cw;
                                winding_set = true;
                            }
                            _ => {}
                        }
                        file.lines.push(Line::Bfc(b));
                    }
                    _ => read_header(rest, &mut file.header, &mut first_meta_seen),
                }
                continue;
            }
        }
        let (l, bad) = parse_line(line);
        match l {
            Some(l) => {
                geometry_seen = true;
                file.lines.push(l);
            }
            None if bad => file.skipped += 1,
            None => {}
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_five_line_types() {
        let text = "0 Test part\n0 Name: t.dat\n0 Author: Me\n0 !LDRAW_ORG Part UPDATE 2026-10\n0 !LICENSE Licensed under CC BY 4.0\n0 !CATEGORY Brick\n0 !KEYWORDS a, b\n0 BFC CERTIFY CCW\n\
            1 16 1 2 3 1 0 0 0 1 0 0 0 1 sub part.dat\n2 24 0 0 0 1 1 1\n3 16 0 0 0 1 0 0 0 1 0\n4 16 0 0 0 1 0 0 1 1 0 0 1 0\n5 24 0 0 0 1 0 0 0 1 0 1 1 0\n";
        let f = parse(text);
        assert_eq!(f.header.description, "Test part");
        assert_eq!(f.header.name, "t.dat");
        assert_eq!(f.header.author, "Me");
        assert_eq!(f.header.org_type, "Part");
        assert_eq!(f.header.update.as_deref(), Some("2026-10"));
        assert_eq!(f.header.category.as_deref(), Some("Brick"));
        assert_eq!(f.header.keywords, vec!["a", "b"]);
        assert!(f.certified);
        assert_eq!(f.winding, Winding::Ccw);
        assert_eq!(f.lines.len(), 6);
        match &f.lines[1] {
            Line::SubFile(s) => {
                assert_eq!(s.file, "sub part.dat");
                assert_eq!(s.t, [1.0, 2.0, 3.0]);
                assert_eq!(s.colour, 16);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(f.lines[2], Line::Segment { colour: 24, .. }));
        assert!(matches!(f.lines[3], Line::Tri { .. }));
        assert!(matches!(f.lines[4], Line::Quad { .. }));
        assert!(matches!(f.lines[5], Line::Cond { .. }));
        assert_eq!(f.skipped, 0);
    }

    #[test]
    fn bfc_commands_and_winding() {
        let f =
            parse("0 x\n0 BFC CERTIFY CW\n0 BFC INVERTNEXT\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 a.dat\n");
        assert!(f.certified);
        assert_eq!(f.winding, Winding::Cw);
        assert_eq!(f.lines[1], Line::Bfc(Bfc::InvertNext));
        let g = parse("0 x\n0 BFC NOCERTIFY\n3 16 0 0 0 1 0 0 0 1 0\n");
        assert!(!g.certified);
        let h = parse("0 x\n0 BFC CERTIFY\n0 BFC CLIP CCW\n");
        assert_eq!(h.winding, Winding::Ccw);
    }

    #[test]
    fn hex_direct_colour_and_bad_lines() {
        let f = parse("3 0x2FF8000 0 0 0 1 0 0 0 1 0\n3 16 0 0 0 1 0 0 0 1\n1 16 0 0 0 1 0 0 0 1 0 0 0\n4 16 nan 0 0 0 0 0 0 0 0 0 0 0\n9 what\n");
        assert_eq!(f.lines.len(), 1);
        assert!(matches!(
            f.lines[0],
            Line::Tri {
                colour: 0x2FF8000,
                ..
            }
        ));
        assert_eq!(f.skipped, 4);
    }

    #[test]
    fn absurd_numbers_and_long_lines_are_skipped() {
        let long = format!("0 {}\n", "x".repeat(MAX_LINE_LEN + 1));
        let f = parse(&format!(
            "{long}3 16 1e300 0 0 1 0 0 0 1 0\n3 16 inf 0 0 1 0 0 0 1 0\n"
        ));
        assert!(f.lines.is_empty());
        assert_eq!(f.skipped, 3);
    }

    #[test]
    fn random_bytes_never_panic() {
        // A small deterministic generator: no test dependency needed.
        let mut s = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let alphabet: Vec<&str> = vec![
            "0",
            "1",
            "2",
            "3",
            "4",
            "5",
            "16",
            "24",
            "BFC",
            "CERTIFY",
            "CCW",
            "CW",
            "INVERTNEXT",
            "-1",
            "1e9",
            "1e400",
            "nan",
            "0x2123456",
            "a.dat",
            "!LDRAW_ORG",
            "Part",
            "UPDATE",
            "\n",
            "\n",
            " ",
            "\t",
            "\u{feff}",
            "é",
        ];
        for _ in 0..2000 {
            let mut text = String::new();
            for _ in 0..(next() % 120) {
                text.push_str(alphabet[(next() % alphabet.len() as u64) as usize]);
                text.push(if next() % 5 == 0 { '\n' } else { ' ' });
            }
            let f = parse(&text);
            // Whatever came in, the lines kept are finite.
            for l in &f.lines {
                if let Line::Tri { p, .. } = l {
                    assert!(p.iter().flatten().all(|v| v.is_finite()));
                }
            }
        }
    }
}
