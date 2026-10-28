//! The two XML helpers the 3MF writer needs: a character check and escaping.

use crate::IoError;

/// Fail when `text` holds a character outside the XML 1.0 `Char` production
/// (W3C XML 1.0, section 2.2): control characters other than tab, line feed and
/// carriage return, and U+FFFE and U+FFFF. Rust strings cannot hold surrogates,
/// so nothing else needs checking.
pub(crate) fn check_text(field: &str, text: &str) -> Result<(), IoError> {
    match text.chars().find(|&c| {
        (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{FFFE}' | '\u{FFFF}')
    }) {
        Some(c) => Err(IoError::InvalidText {
            field: field.to_string(),
            code: c as u32,
        }),
        None => Ok(()),
    }
}

/// Append `text` escaped for both attribute values and element content. Tab, line
/// feed and carriage return become character references, which survive attribute
/// value normalisation (W3C XML 1.0, section 3.3.3) and line-end handling.
pub(crate) fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_and_whitespace_controls() {
        let mut s = String::new();
        escape_into(&mut s, "a<b>&\"c\" 'd'\te\nf\rg ü");
        assert_eq!(
            s,
            "a&lt;b&gt;&amp;&quot;c&quot; &apos;d&apos;&#9;e&#10;f&#13;g ü"
        );
    }

    #[test]
    fn rejects_characters_xml_cannot_carry() {
        assert_eq!(check_text("title", "Logo tile\tv2\n"), Ok(()));
        assert_eq!(
            check_text("title", "bad\u{1}"),
            Err(IoError::InvalidText {
                field: "title".into(),
                code: 1
            })
        );
        assert_eq!(
            check_text("name", "x\u{FFFF}"),
            Err(IoError::InvalidText {
                field: "name".into(),
                code: 0xFFFF
            })
        );
        assert!(check_text("serial", "\0").is_err());
    }
}
