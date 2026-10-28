//! The project's non-affiliation disclaimer (docs/plan/12 section 4), kept in one
//! data file, `crates/openbrick-units/disclaimer.txt`. The CLI's `about`, the
//! WebAssembly core's `disclaimer()` and the web app's footer (which imports the
//! file at build time) all read that file, so the text has one source and stays
//! verbatim.

/// The file without its final newline.
const fn without_final_newline(s: &'static str) -> &'static str {
    let bytes = s.as_bytes();
    if !bytes.is_empty() && bytes[bytes.len() - 1] == b'\n' {
        s.split_at(s.len() - 1).0
    } else {
        s
    }
}

/// The disclaimer for the README, About screens and website footers, verbatim
/// from docs/plan/12 section 4: one paragraph, no surrounding white space.
pub const DISCLAIMER: &str = without_final_newline(include_str!("../disclaimer.txt"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disclaimer_is_the_plan_text_verbatim() {
        assert_eq!(DISCLAIMER, DISCLAIMER.trim(), "no surrounding white space");
        assert!(
            !DISCLAIMER.is_empty() && !DISCLAIMER.contains('\n'),
            "one paragraph"
        );
        // docs/plan/12 section 4 quotes it as a Markdown block quote.
        let plan = include_str!("../../../docs/plan/12-legal-and-naming.md");
        assert!(
            plan.lines()
                .any(|l| l.strip_prefix("> ") == Some(DISCLAIMER)),
            "crates/openbrick-units/disclaimer.txt must equal the quote in docs/plan/12 section 4"
        );
    }

    #[test]
    fn only_the_final_newline_is_dropped() {
        assert_eq!(without_final_newline("a\n"), "a");
        assert_eq!(without_final_newline("a"), "a");
        assert_eq!(without_final_newline(""), "");
        assert_eq!(without_final_newline("a\n\n"), "a\n");
    }
}
