//! A tiny pixel font for the coupon labels, drawn for this project (no font file
//! and no font licence is involved): the digits 0 to 9 on a grid three pixels
//! wide and five tall, and the decimal point, one pixel wide.
//!
//! The coupon makes one pixel one label stroke wide, and the stroke is at least
//! the process's `minFeatureMm`, so every stroke and every gap (inside a glyph,
//! between glyphs and between lines) is at least one minimum feature. No two
//! pixels of a glyph touch only at a corner: such a pinch would make the
//! extruded label non-manifold. The tests check both properties for every glyph.
//!
//! ```text
//!  ###  .#.  ###  ###  #.#  ###  ###  ###  ###  ###  .
//!  #.#  ##.  ..#  ..#  #.#  #..  #..  ..#  #.#  #.#  .
//!  #.#  .#.  ###  .##  ###  ###  ###  ..#  ###  ###  .
//!  #.#  .#.  #..  ..#  ..#  ..#  #.#  ..#  #.#  ..#  .
//!  ###  ###  ###  ###  ..#  ###  ###  ..#  ###  ###  #
//! ```

/// Pixel rows per glyph and per line of text.
pub(crate) const ROWS: usize = 5;

/// Empty pixels between two glyphs of a line and between two lines.
pub(crate) const SPACING: usize = 1;

/// Empty pixels at least between two texts side by side, so that neighbouring
/// labels read as separate numbers: one digit width.
pub(crate) const WORD_SPACE: usize = 3;

/// Every glyph, rows from the top; `#` is a filled pixel.
const GLYPHS: &[(char, [&str; ROWS])] = &[
    ('0', ["###", "#.#", "#.#", "#.#", "###"]),
    ('1', [".#.", "##.", ".#.", ".#.", "###"]),
    ('2', ["###", "..#", "###", "#..", "###"]),
    ('3', ["###", "..#", ".##", "..#", "###"]),
    ('4', ["#.#", "#.#", "###", "..#", "..#"]),
    ('5', ["###", "#..", "###", "..#", "###"]),
    ('6', ["###", "#..", "###", "#.#", "###"]),
    ('7', ["###", "..#", "..#", "..#", "..#"]),
    ('8', ["###", "#.#", "###", "#.#", "###"]),
    ('9', ["###", "#.#", "###", "..#", "###"]),
    ('.', [".", ".", ".", ".", "#"]),
];

/// The rows of the glyph for `c`, if the font has one.
pub(crate) fn glyph(c: char) -> Option<&'static [&'static str; ROWS]> {
    GLYPHS.iter().find(|(g, _)| *g == c).map(|(_, rows)| rows)
}

/// The characters the font draws.
#[cfg(test)]
fn charset() -> impl Iterator<Item = char> {
    GLYPHS.iter().map(|(c, _)| *c)
}

/// A horizontal run of filled pixels in a text block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    /// Pixel row, 0 at the top of the block.
    pub row: usize,
    /// First pixel column, 0 at the left of the block.
    pub col: usize,
    /// Number of pixels.
    pub len: usize,
}

/// A laid-out text block: its size in pixels and its filled runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub width: usize,
    pub height: usize,
    pub runs: Vec<Run>,
}

/// Width in pixels of one line of text, or the first character without a glyph.
fn line_width(line: &str) -> Result<usize, char> {
    let mut width = 0;
    for (i, c) in line.chars().enumerate() {
        let g = glyph(c).ok_or(c)?;
        width += g[0].len() + if i > 0 { SPACING } else { 0 };
    }
    Ok(width)
}

/// Lay out `text`: lines separated by `\n`, each centred in the block (rounded
/// to the left), [`SPACING`] pixels between glyphs and between lines. Fails with
/// the first character the font has no glyph for, or on an empty line.
pub(crate) fn layout(text: &str) -> Result<Block, String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut widths = Vec::with_capacity(lines.len());
    for line in &lines {
        if line.is_empty() {
            return Err(format!("label {text:?} has an empty line"));
        }
        let w = line_width(line)
            .map_err(|c| format!("label {text:?}: the coupon font has no glyph for {c:?}"))?;
        widths.push(w);
    }
    let width = widths.iter().copied().max().unwrap_or(0);
    let height = lines.len() * ROWS + (lines.len() - 1) * SPACING;
    let mut runs = Vec::new();
    for (l, (line, w)) in lines.iter().zip(&widths).enumerate() {
        let top = l * (ROWS + SPACING);
        let mut left = (width - w) / 2;
        for c in line.chars() {
            let Some(g) = glyph(c) else { continue };
            for (r, row) in g.iter().enumerate() {
                let mut start = None;
                for (k, px) in row.bytes().chain(std::iter::once(b'.')).enumerate() {
                    match (px == b'#', start) {
                        (true, None) => start = Some(k),
                        (false, Some(s)) => {
                            runs.push(Run {
                                row: top + r,
                                col: left + s,
                                len: k - s,
                            });
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
            left += g[0].len() + SPACING;
        }
    }
    Ok(Block {
        width,
        height,
        runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(rows: &[&str; ROWS], x: isize, y: isize) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        rows.get(y as usize)
            .and_then(|r| r.as_bytes().get(x as usize))
            .is_some_and(|b| *b == b'#')
    }

    #[test]
    fn glyphs_are_rectangular_and_have_no_corner_pinches() {
        for (c, rows) in GLYPHS {
            let w = rows[0].len();
            assert!(w > 0, "{c:?}");
            assert!(rows.iter().all(|r| r.len() == w), "{c:?}: ragged rows");
            assert!(rows
                .iter()
                .all(|r| r.bytes().all(|b| b == b'#' || b == b'.')));
            assert!(rows.iter().any(|r| r.contains('#')), "{c:?} is blank");
            // Every 2 x 2 window (including the border) with exactly the two
            // diagonal pixels filled is a pinch.
            for y in -1..ROWS as isize {
                for x in -1..w as isize {
                    let a = filled(rows, x, y);
                    let b = filled(rows, x + 1, y);
                    let d = filled(rows, x, y + 1);
                    let e = filled(rows, x + 1, y + 1);
                    assert!(
                        !((a && e && !b && !d) || (b && d && !a && !e)),
                        "{c:?}: pixels touch only at a corner near ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn every_digit_and_the_point_are_drawn() {
        let have: String = charset().collect();
        assert_eq!(have, "0123456789.");
    }

    #[test]
    fn layout_centres_lines_and_merges_runs() {
        let b = layout("4.80").expect("drawable");
        // 3 + 1 + 1 + 1 + 3 + 1 + 3 pixels: three digits, the point and the gaps.
        assert_eq!((b.width, b.height), (13, ROWS));
        // The top row of "4" is two separate pixels; of "8" and "0" one run each.
        let top: Vec<_> = b.runs.iter().filter(|r| r.row == 0).collect();
        assert_eq!(top.len(), 4);
        assert_eq!(top[0].len + top[1].len, 2);
        let two = layout("6.50\n0.30").expect("drawable");
        assert_eq!((two.width, two.height), (13, 2 * ROWS + SPACING));
        assert!(
            two.runs.iter().all(|r| r.row != ROWS),
            "the line gap is empty"
        );
        let narrow = layout("12.5\n7").expect("drawable");
        // "7" is centred: (13 - 3) / 2 = 5 pixels from the left.
        let seven: Vec<_> = narrow.runs.iter().filter(|r| r.row > ROWS).collect();
        assert!(seven.iter().all(|r| r.col >= 5 && r.col + r.len <= 8));
    }

    #[test]
    fn unknown_characters_and_empty_lines_are_refused() {
        assert!(layout("4.8a").unwrap_err().contains("no glyph for 'a'"));
        assert!(layout("4.80\n").unwrap_err().contains("empty line"));
    }
}
