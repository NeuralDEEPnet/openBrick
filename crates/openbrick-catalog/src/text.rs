//! Text that came from a file we do not control (a pack, an LDraw library) and is about to be shown
//! or put in a message: control characters, bidirectional overrides and other invisible format
//! characters are replaced and the length is capped, so a hostile description cannot rewrite a
//! terminal line (ANSI escapes), hide itself, or flip the text around it.

/// The replacement shown in place of an unsafe character.
pub const REPLACEMENT: char = '\u{FFFD}';

/// True for a character that must not appear in a displayed name: a control character (C0, DEL,
/// C1), a line or paragraph separator, or a Unicode format character (category Cf: bidirectional
/// embeddings, overrides and isolates, zero-width marks, the byte order mark, tag characters).
pub fn is_unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}')
}

/// True when `s` holds an unsafe character ([`is_unsafe_char`]).
pub fn has_unsafe(s: &str) -> bool {
    s.chars().any(is_unsafe_char)
}

/// `s` with every unsafe character replaced by U+FFFD and at most `max_chars` characters (`max_chars`
/// at least 1), the last one an ellipsis when it was cut.
pub fn safe(s: &str, max_chars: usize) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut cut = false;
    for c in s.chars() {
        if out.len() == max_chars.max(1) {
            cut = true;
            break;
        }
        out.push(if is_unsafe_char(c) { REPLACEMENT } else { c });
    }
    if cut {
        out.pop();
        out.push('…');
    }
    out.into_iter().collect()
}

/// [`safe`] with the length every message uses.
pub fn safe_msg(s: &str) -> String {
    safe(s, MAX_MESSAGE_CHARS)
}

/// The longest piece of untrusted text a message repeats.
pub const MAX_MESSAGE_CHARS: usize = 200;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_bidi_and_control_characters_and_cuts_long_text() {
        assert_eq!(safe("Brick 2 x 4", 50), "Brick 2 x 4");
        assert_eq!(safe("a\u{1b}[31mred\u{7}", 50), "a\u{FFFD}[31mred\u{FFFD}");
        assert_eq!(safe("ab\u{202E}cd\u{2066}e\u{200B}f\u{FEFF}", 50), "ab\u{FFFD}cd\u{FFFD}e\u{FFFD}f\u{FFFD}");
        assert_eq!(safe("line\nbreak\r\u{85}\u{2028}", 50), "line\u{FFFD}break\u{FFFD}\u{FFFD}\u{FFFD}");
        assert_eq!(safe(&"x".repeat(1000), 5), "xxxx…");
        assert_eq!(safe("12345", 5), "12345", "exactly the limit is not cut");
        assert_eq!(safe("é日本語", 50), "é日本語");
        assert!(has_unsafe("a\u{202E}") && !has_unsafe("plain é"));
    }
}
