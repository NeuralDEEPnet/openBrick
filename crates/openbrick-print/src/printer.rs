//! Printers and their beds (rule `SIZE.BED`, docs/plan/05 section 5): the
//! build volume a part must fit in its print orientation.
//!
//! Printer profiles are data, `profiles/printer/*.json`
//! (`schemas/printer-profile.schema.json`), one per printer and tool setup,
//! each with the manufacturer's published build volume and the page it was
//! read from. They describe hardware, not a calibration: the fit of a printer
//! and material lives in a fit profile.

/// Where a build volume was published.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterSource {
    /// The manufacturer's page or specification sheet.
    pub url: String,
    /// When it was read, YYYY-MM-DD.
    pub accessed: String,
    /// The words on that page, as published.
    pub quote: String,
}

/// One printer and tool setup (`profiles/printer/*.json`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterProfile {
    pub openbrick: String,
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<crate::Credit>,
    pub maker: String,
    /// Build volume, x (width) by y (depth) by z (height), millimetres, as the
    /// manufacturer publishes it.
    pub build_volume_mm: [f64; 3],
    /// Toolheads (nozzles) the setup prints with.
    pub toolheads: u32,
    /// Filaments it prints in one job without a manual change: toolheads
    /// times filaments per toolhead (an AMS lite feeds four into one nozzle).
    pub filaments: u32,
    pub source: PrinterSource,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl PrinterProfile {
    /// The bed this printer offers a part.
    pub fn bed(&self) -> Bed {
        Bed {
            name: self.name.clone(),
            printer: Some(self.id.clone()),
            size_mm: self.build_volume_mm,
        }
    }
}

/// The build volume a part must fit (rule `SIZE.BED`): a printer's, or one
/// given by hand (`openbrick validate --bed 250x210x220`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bed {
    /// For messages: the printer's name, or the size for a hand-given bed.
    pub name: String,
    /// The printer profile id, when the bed is a printer's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<String>,
    /// x by y by z, millimetres.
    pub size_mm: [f64; 3],
}

impl Bed {
    /// A bed given by its size alone; `None` unless every side is a positive
    /// finite number.
    pub fn of_size(size_mm: [f64; 3]) -> Option<Bed> {
        if !size_mm.iter().all(|s| s.is_finite() && *s > 0.0) {
            return None;
        }
        Some(Bed {
            name: format!(
                "{} x {} x {} mm bed",
                crate::validate::mm(size_mm[0]),
                crate::validate::mm(size_mm[1]),
                crate::validate::mm(size_mm[2])
            ),
            printer: None,
            size_mm,
        })
    }

    /// Parse `WxDxH` in millimetres (`250x210x220`).
    pub fn parse(s: &str) -> Result<Bed, String> {
        let bad = || format!("expected the bed as WxDxH in mm, such as 250x210x220; found `{s}`");
        let parts: Vec<f64> = s
            .split(['x', 'X'])
            .map(|p| p.trim().trim_end_matches("mm").trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| bad())?;
        let [x, y, z] = parts[..] else {
            return Err(bad());
        };
        Bed::of_size([x, y, z]).ok_or_else(|| {
            format!("every side of the bed must be a positive number of millimetres; found `{s}`")
        })
    }
}

/// The bundled printer profiles, in the order of `profiles/printer/` by id.
pub fn default_printers() -> Vec<PrinterProfile> {
    let mut all: Vec<PrinterProfile> = [
        include_str!("../../../profiles/printer/bambu-a1.json"),
        include_str!("../../../profiles/printer/bambu-a1-ams-lite.json"),
        include_str!("../../../profiles/printer/prusa-mini-plus.json"),
        include_str!("../../../profiles/printer/prusa-xl.json"),
        include_str!("../../../profiles/printer/prusa-xl-5t.json"),
    ]
    .iter()
    .map(|s| serde_json::from_str(s).expect("bundled printer profile is valid"))
    .collect();
    all.sort_by(|a, b| a.id.cmp(&b.id));
    all
}

/// A bundled printer by id.
pub fn printer(id: &str) -> Option<PrinterProfile> {
    default_printers().into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_printers_carry_their_sources() {
        let all = default_printers();
        assert_eq!(all.len(), 5);
        for p in &all {
            assert_eq!(p.kind, "printer");
            assert!(p.source.url.starts_with("https://"), "{}", p.id);
            assert!(!p.source.quote.is_empty(), "{}", p.id);
            assert!(p.toolheads >= 1 && p.filaments >= p.toolheads, "{}", p.id);
            assert!(p.build_volume_mm.iter().all(|s| *s > 0.0), "{}", p.id);
        }
        // The owner's printers (the volumes their makers publish).
        let vol = |id: &str| printer(id).expect(id).build_volume_mm;
        assert_eq!(vol("prusa-xl"), [360.0, 360.0, 360.0]);
        assert_eq!(vol("prusa-xl-5t"), [360.0, 360.0, 360.0]);
        assert_eq!(vol("prusa-mini-plus"), [180.0, 180.0, 180.0]);
        assert_eq!(vol("bambu-a1"), [256.0, 256.0, 256.0]);
        assert_eq!(vol("bambu-a1-ams-lite"), [256.0, 256.0, 256.0]);
    }

    #[test]
    fn beds_parse_from_text() {
        let b = Bed::parse("250x210x220").expect("parses");
        assert_eq!(b.size_mm, [250.0, 210.0, 220.0]);
        assert_eq!(b.name, "250 x 210 x 220 mm bed");
        assert_eq!(
            Bed::parse("180X180x180mm").expect("parses").size_mm,
            [180.0; 3]
        );
        assert!(Bed::parse("250x210").is_err());
        assert!(Bed::parse("250x0x220").is_err());
        assert!(Bed::parse("wide").is_err());
        let p = printer("prusa-mini-plus").expect("bundled").bed();
        assert_eq!(p.printer.as_deref(), Some("prusa-mini-plus"));
    }
}
