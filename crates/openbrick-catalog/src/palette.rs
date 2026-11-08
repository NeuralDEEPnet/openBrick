//! The assembler's colour palette, `profiles/palette/default.json` (schema
//! `schemas/palette.schema.json`): neutral names and display colours; codes
//! below 10000 are LDraw colour numbers for interchange (docs/plan/08 section 3).

use std::sync::OnceLock;

use openbrick_units::Credit;

/// One colour.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Colour {
    pub code: u16,
    pub name: String,
    /// Display colour, `#rrggbb` (sRGB).
    pub rgb: String,
}

/// The palette document.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    pub openbrick: String,
    pub kind: String,
    pub id: String,
    pub name: String,
    pub credit: Credit,
    pub source: String,
    /// The colour a new part gets.
    pub default_part: u16,
    /// The colour a new baseplate gets.
    pub default_base: u16,
    pub colours: Vec<Colour>,
}

const SOURCE: &str = include_str!("../../../profiles/palette/default.json");

impl Palette {
    /// The bundled palette, parsed once.
    pub fn bundled() -> &'static Palette {
        static P: OnceLock<Palette> = OnceLock::new();
        P.get_or_init(|| {
            serde_json::from_str(SOURCE).expect("profiles/palette/default.json is valid")
        })
    }

    /// The colour with this code.
    pub fn get(&self, code: u16) -> Option<&Colour> {
        self.colours.iter().find(|c| c.code == code)
    }

    /// Problems the schema cannot state: unique codes, and both defaults in
    /// the palette. Empty when the palette is sound.
    pub fn check(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (i, c) in self.colours.iter().enumerate() {
            if self.colours[..i].iter().any(|d| d.code == c.code) {
                problems.push(format!(
                    "colour code {} is used twice; give `{}` its own code",
                    c.code, c.name
                ));
            }
        }
        for (what, code) in [
            ("defaultPart", self.default_part),
            ("defaultBase", self.default_base),
        ] {
            if self.get(code).is_none() {
                problems.push(format!(
                    "{what} is {code}, which is not a colour of the palette; use one of its codes"
                ));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_palette_is_sound() {
        let p = Palette::bundled();
        assert_eq!(p.kind, "palette");
        assert!(p.check().is_empty(), "{:?}", p.check());
        assert!(p
            .colours
            .iter()
            .all(|c| c.rgb.len() == 7 && c.rgb.starts_with('#')));
    }
}
