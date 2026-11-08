//! Turning an LDraw library into a [`Pack`] (docs/plan/08 section 1, `openbrick
//! catalog import`).
//!
//! Selection, in order:
//!
//! 1. Top-level `parts/*.dat` files whose header says `Part` (not `Subpart`, a
//!    shortcut, an alias or a physical-colour variant) and whose description
//!    does not start with `~`, `=` or `_` (obsolete, moved or sticker files).
//! 2. A file name of digits and at most one trailing letter (`3001`, `3040b`):
//!    the common moulds. Printed and patterned variants (`3001p01`, `...pr0001`)
//!    and assemblies (`...c01`) have longer names.
//! 3. Never minifigure parts (an LDraw category beginning `Minifig`, or a
//!    description containing "minifig"), other construction systems that are
//!    not brick-compatible (Duplo, Scala, Belville, Fabuland, Primo, Quatro,
//!    Znap, Modulex), patterns and stickers, or a description containing the
//!    manufacturer's word mark ([`WORD_MARK`]; CLAUDE.md: the word mark appears
//!    only in the disclaimer and in docs/plan/12, so the program never prints
//!    it and a part named with it is left out).
//! 4. The chosen categories ([`DEFAULT_CATEGORIES`] unless the caller names
//!    others). A part's category is its `!CATEGORY` line, else the first word
//!    of its description (the LDraw rule).
//! 5. Parts with at least one derived connector (unless the caller asks for
//!    the free-placement ones too), ordered by category, then plain numbers
//!    before suffixed ones, then number; the first `limit` are imported.
//!
//! Every part that passes 1 to 4 is derived (mesh, boxes, connectors), so the
//! report states the coverage over all of them, not only the imported ones.

use std::path::Path;
use std::time::Instant;

use super::connect::{derive, Derived};
use super::flatten::{Limits, WalkError};
use super::library::{Library, LibraryError, Resolution};
use super::parse::Header;
use crate::builtin::PartMesh;
use crate::pack::{
    footprint_studs, Connectivity, LicenceText, Pack, PackCounts, PackError, PackHeader, PackPart,
    LDRAW_ID_PREFIX, MAX_BLOB_BYTES, MAX_CATEGORY_CHARS, MAX_CREDIT_CHARS, MAX_NAME_CHARS,
    MAX_PARTS, PACK_FORMAT,
};
use crate::text::{is_unsafe_char, safe, safe_msg, REPLACEMENT};
use crate::LDRAW_ATTRIBUTION;

/// The categories imported unless the caller names others: the system parts
/// that snap by studs (LDraw's own category names), in palette order.
pub const DEFAULT_CATEGORIES: &[&str] = &[
    "Brick",
    "Plate",
    "Tile",
    "Slope",
    "Baseplate",
    "Wedge",
    "Arch",
    "Window",
    "Door",
    "Windscreen",
    "Panel",
    "Cylinder",
    "Cone",
    "Fence",
    "Bracket",
];

/// The manufacturer's word mark, lower case, written in two pieces so that this file does not spell
/// it out: a part whose description holds it is left out of a pack.
pub const WORD_MARK: [&str; 2] = ["le", "go"];

/// Other construction systems and patterned parts that are left out.
const OTHER_SYSTEMS: [&str; 8] = [
    "duplo", "scala", "belville", "fabuland", "primo", "quatro", "znap", "modulex",
];

/// What to import.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub resolution: Resolution,
    /// Category names (case-insensitive); `None` for [`DEFAULT_CATEGORIES`].
    pub categories: Option<Vec<String>>,
    /// The most parts written; `None` for all that qualify.
    pub limit: Option<usize>,
    /// Also write parts without a derived connector (free placement only).
    pub include_unconnected: bool,
    pub limits: Limits,
    /// A part with more triangles than this is left out.
    pub max_part_triangles: usize,
    /// The most work the whole import may do, in units of one sub-file reference expanded or one
    /// triangle produced, over every part (`limits` bound one part). A counter, so the same
    /// library and options always end the same way. Default [`DEFAULT_MAX_WORK`].
    pub max_work: u64,
}

/// The largest licence file the importer copies into a pack, in bytes (the real ones are under 20 KB).
pub const MAX_LICENCE_FILE_BYTES: usize = 256 * 1024;

/// The default work budget of an import (see [`ImportOptions::max_work`]).
pub const DEFAULT_MAX_WORK: u64 = 50_000_000;

impl Default for ImportOptions {
    fn default() -> ImportOptions {
        ImportOptions {
            resolution: Resolution::Standard,
            categories: None,
            limit: Some(600),
            include_unconnected: false,
            limits: Limits::default(),
            max_part_triangles: 30_000,
            max_work: DEFAULT_MAX_WORK,
        }
    }
}

/// Parts left out before derivation, by reason.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Excluded {
    pub not_a_part: u32,
    pub minifig: u32,
    pub other_systems: u32,
    pub prints_and_stickers: u32,
    pub odd_names: u32,
    pub other_categories: u32,
    pub too_complex: u32,
    pub no_geometry: u32,
    pub failed: u32,
}

/// What an import found.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub counts: PackCounts,
    pub excluded: Excluded,
    pub library: String,
    pub resolution: String,
    /// Parts of the chosen categories with at least one derived stud.
    pub with_studs: u32,
    /// Parts of the chosen categories with at least one derived receptacle.
    pub with_receptacles: u32,
    /// Parts of the chosen categories with a Technic pin or axle connector.
    pub with_technic: u32,
    /// Parts of the chosen categories in which some connector was off the lattice and dropped.
    pub with_off_lattice: u32,
    /// Parts written to the pack that were not BFC certified somewhere.
    pub uncertified: u32,
    /// The first few failures: path and message.
    pub failures: Vec<(String, String)>,
    /// Work units the import did (references expanded and triangles produced, both walks of every part).
    pub work: u64,
    pub seconds: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("the library has no part to write: {0}")]
    Empty(String),
}

/// The category of a part: its `!CATEGORY` line, else the first word of its description.
pub fn category_of(h: &Header) -> String {
    if let Some(c) = &h.category {
        return c.clone();
    }
    h.description
        .trim_start_matches(['~', '=', '_', '|', ' '])
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(',')
        .to_string()
}

/// Why a part file is left out before any geometry is read, or `None` when it stays.
pub fn exclusion(file_name: &str, h: &Header) -> Option<&'static str> {
    if h.org_type != "Part" || !h.org_qualifiers.is_empty() {
        return Some("not_a_part");
    }
    let d = h.description.trim();
    if d.is_empty() || d.starts_with(['~', '=', '_']) {
        return Some("not_a_part");
    }
    let lower = d.to_ascii_lowercase();
    let cat = category_of(h).to_ascii_lowercase();
    if cat.starts_with("minifig") || lower.contains("minifig") {
        return Some("minifig");
    }
    if OTHER_SYSTEMS
        .iter()
        .any(|s| cat.starts_with(s) || lower.contains(s))
    {
        return Some("other_systems");
    }
    if lower.contains("pattern")
        || lower.contains("sticker")
        || cat.starts_with("sticker")
        || lower.contains(&WORD_MARK.concat())
    {
        return Some("prints_and_stickers");
    }
    let stem = file_name.strip_suffix(".dat").unwrap_or(file_name);
    let digits = stem.chars().take_while(char::is_ascii_digit).count();
    let rest = &stem[digits..];
    if digits == 0 || rest.len() > 1 || !rest.chars().all(|c| c.is_ascii_lowercase()) {
        return Some("odd_names");
    }
    None
}

/// The id of a part file: `ld.3001`.
pub fn part_id(file_name: &str) -> String {
    format!(
        "{LDRAW_ID_PREFIX}{}",
        file_name.strip_suffix(".dat").unwrap_or(file_name)
    )
}

fn sort_key(category_rank: usize, id: &str) -> (usize, bool, u64, String) {
    let stem = id.strip_prefix(LDRAW_ID_PREFIX).unwrap_or(id);
    let digits: String = stem.chars().take_while(char::is_ascii_digit).collect();
    let suffixed = stem.len() > digits.len();
    (
        category_rank,
        suffixed,
        digits.parse().unwrap_or(u64::MAX),
        stem.to_string(),
    )
}

struct Candidate {
    path: String,
    id: String,
    header: Header,
    category: String,
    rank: usize,
}

struct Done {
    cand: Candidate,
    d: Derived,
}

/// Import the library at `path` (a folder or `complete.zip`).
pub fn import(
    path: &Path,
    opts: &ImportOptions,
    progress: &mut dyn FnMut(&str, usize, usize),
) -> Result<(Pack, ImportReport), ImportError> {
    let t0 = Instant::now();
    let lib = Library::open(path, opts.resolution)?;
    lib.set_work_budget(opts.max_work);
    let wanted: Vec<String> = opts.categories.clone().unwrap_or_else(|| {
        DEFAULT_CATEGORIES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    });
    let wanted_lower: Vec<String> = wanted.iter().map(|s| s.to_ascii_lowercase()).collect();

    let mut report = ImportReport {
        resolution: opts.resolution.name().into(),
        ..ImportReport::default()
    };
    // Pass 1: headers only.
    let paths = lib.part_paths();
    report.counts.scanned = paths.len() as u32;
    let mut cands: Vec<Candidate> = Vec::new();
    let mut newest = String::new();
    for (i, p) in paths.iter().enumerate() {
        if i % 2000 == 0 {
            progress("reading headers", i, paths.len());
        }
        let res = match lib.load(p) {
            Ok(r) => r,
            Err(_) => {
                report.excluded.failed += 1;
                continue;
            }
        };
        let name = p.rsplit('/').next().unwrap_or(p);
        let h = &res.file.header;
        if h.org_type == "Part" {
            if let Some(u) = &h.update {
                if *u > newest {
                    newest.clone_from(u);
                }
            }
        }
        match exclusion(name, h) {
            Some("not_a_part") => report.excluded.not_a_part += 1,
            Some("minifig") => report.excluded.minifig += 1,
            Some("other_systems") => report.excluded.other_systems += 1,
            Some("prints_and_stickers") => report.excluded.prints_and_stickers += 1,
            Some(_) => report.excluded.odd_names += 1,
            None => {
                report.counts.eligible += 1;
                let category = category_of(h);
                match wanted_lower
                    .iter()
                    .position(|w| *w == category.to_ascii_lowercase())
                {
                    Some(rank) => cands.push(Candidate {
                        path: p.clone(),
                        id: part_id(name),
                        header: h.clone(),
                        category,
                        rank,
                    }),
                    None => report.excluded.other_categories += 1,
                }
            }
        }
        drop(res);
        lib.trim_cache();
    }
    report.counts.in_categories = cands.len() as u32;
    cands.sort_by_key(|a| sort_key(a.rank, &a.id));

    // Pass 2: derive every candidate (so the coverage covers all of them).
    let mut done: Vec<Done> = Vec::new();
    let total = cands.len();
    for (i, cand) in cands.into_iter().enumerate() {
        if i % 50 == 0 {
            progress("deriving connectors", i, total);
        }
        let derived = derive(&lib, &cand.path, opts.limits);
        lib.trim_cache();
        match derived {
            Ok(Some(d)) => {
                if d.mesh.triangle_count() > opts.max_part_triangles {
                    report.excluded.too_complex += 1;
                    continue;
                }
                if d.studs > 0 {
                    report.with_studs += 1;
                }
                if d.studf > 0 {
                    report.with_receptacles += 1;
                }
                if d.conns.iter().any(|c| c.kind.is_axial()) {
                    report.with_technic += 1;
                }
                if d.off_lattice > 0 {
                    report.with_off_lattice += 1;
                }
                if !d.conns.is_empty() {
                    report.counts.with_connectors += 1;
                }
                done.push(Done { cand, d });
            }
            Ok(None) => report.excluded.no_geometry += 1,
            // The budget is for the whole import: when it is spent, nothing more is read.
            Err(WalkError::Library(e @ LibraryError::WorkBudget(_))) => {
                return Err(ImportError::Library(e));
            }
            Err(e) => {
                report.excluded.failed += 1;
                if report.failures.len() < 20 {
                    report.failures.push((cand.path.clone(), failure_text(&e)));
                }
            }
        }
    }

    // Select.
    let cap = opts.limit.unwrap_or(MAX_PARTS).min(MAX_PARTS);
    let usable: Vec<Done> = done
        .into_iter()
        .filter(|d| opts.include_unconnected || !d.d.conns.is_empty())
        .collect();
    let mut chosen = take_balanced(usable, cap);
    if chosen.is_empty() {
        return Err(ImportError::Empty(empty_reason(&report, opts)));
    }
    // Keep the blob inside its bound.
    let mut bytes = 0usize;
    chosen.retain(|d| {
        bytes += d.d.mesh.positions.len() * 12 + d.d.mesh.indices.len() * 4;
        bytes <= MAX_BLOB_BYTES
    });

    let library = newest;
    let mut parts = Vec::new();
    let mut meshes = Vec::new();
    for Done { cand, d } in chosen {
        let connectivity = if d.conns.is_empty() {
            report.counts.free_placement += 1;
            Connectivity::None
        } else {
            Connectivity::Derived
        };
        if !d.joints.is_empty() {
            report.counts.with_joints += 1;
        }
        if d.uncertified_triangles > 0 {
            report.uncertified += 1;
        }
        let studs = footprint_studs(&d.body);
        parts.push(PackPart {
            id: cand.id,
            // Text from the library is cut and made printable here, so the pack reads back.
            name: safe(
                &cand
                    .header
                    .description
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                MAX_NAME_CHARS,
            ),
            category: safe(&cand.category, MAX_CATEGORY_CHARS),
            author: safe(&cand.header.author, MAX_CREDIT_CHARS),
            license: safe(&cand.header.license, MAX_CREDIT_CHARS),
            connectivity,
            joints: d.joints.iter().map(|s| (*s).to_string()).collect(),
            studs,
            height_ldu: d.body.max[1] - d.body.min[1],
            body: d.body,
            pick: d.pick,
            connectors: d.conns,
            vertices: 0,
            triangles: 0,
            offset: 0,
        });
        meshes.push(PartMesh {
            positions: d.mesh.positions,
            indices: d.mesh.indices,
        });
    }
    report.counts.imported = parts.len() as u32;
    report.work = lib.work();
    report.library = if library.is_empty() {
        "unknown".into()
    } else {
        library
    };

    let mut licences = Vec::new();
    for (file, name) in [
        (
            "CAreadme.txt",
            "LDraw.org Parts Library agreement (CAreadme.txt)",
        ),
        (
            "CAlicense.txt",
            "Creative Commons Attribution 2.0 legal code (CAlicense.txt)",
        ),
        (
            "CAlicense4.txt",
            "Creative Commons Attribution 4.0 legal code (CAlicense4.txt)",
        ),
    ] {
        if let Some(text) = lib.root_text(file) {
            if text.len() > MAX_LICENCE_FILE_BYTES {
                return Err(ImportError::Library(LibraryError::TooBig {
                    name: file.into(),
                    size: text.len() as u64,
                    limit: MAX_LICENCE_FILE_BYTES,
                }));
            }
            // Line breaks and tabs stay; any other control or invisible character is replaced.
            let text = text
                .chars()
                .map(|c| {
                    if !matches!(c, '\n' | '\r' | '\t') && is_unsafe_char(c) {
                        REPLACEMENT
                    } else {
                        c
                    }
                })
                .collect();
            licences.push(LicenceText {
                name: name.into(),
                text,
            });
        }
    }
    let header = PackHeader {
        openbrick: "1.0".into(),
        kind: "pack".into(),
        format: PACK_FORMAT,
        id: "ldraw".into(),
        library: report.library.clone(),
        resolution: opts.resolution.name().into(),
        generator: format!("openbrick-catalog {}", env!("CARGO_PKG_VERSION")),
        attribution: LDRAW_ATTRIBUTION.into(),
        licences,
        counts: report.counts.clone(),
        parts,
    };
    let pack = Pack::build(header, &meshes)?;
    report.seconds = t0.elapsed().as_secs_f64();
    Ok((pack, report))
}

/// The first `cap` parts, taken one category at a time in turn (each category in its own order), so
/// a small limit still holds bricks, plates, tiles, slopes and the rest instead of only the first
/// category; the result is in palette order again (category, then number).
fn take_balanced(parts: Vec<Done>, cap: usize) -> Vec<Done> {
    if parts.len() <= cap {
        return parts;
    }
    let mut queues: std::collections::BTreeMap<usize, std::collections::VecDeque<Done>> =
        std::collections::BTreeMap::new();
    for d in parts {
        queues.entry(d.cand.rank).or_default().push_back(d);
    }
    let mut out = Vec::with_capacity(cap);
    while out.len() < cap {
        let mut took = false;
        for q in queues.values_mut() {
            if out.len() >= cap {
                break;
            }
            if let Some(d) = q.pop_front() {
                out.push(d);
                took = true;
            }
        }
        if !took {
            break;
        }
    }
    out.sort_by_key(|d| sort_key(d.cand.rank, &d.cand.id));
    out
}

fn failure_text(e: &WalkError) -> String {
    safe_msg(&e.to_string())
}

/// Why nothing qualified, with the numbers: what was scanned, what was left out and why, and the
/// first failures, so the person is not sent to the category filter when the files were the problem.
fn empty_reason(r: &ImportReport, opts: &ImportOptions) -> String {
    let x = &r.excluded;
    let mut m = format!(
        "{} part files scanned, {} eligible, {} in the chosen categories; of those {} failed to read, {} were too complex, {} had no geometry, and {} had no connector{}",
        r.counts.scanned,
        r.counts.eligible,
        r.counts.in_categories,
        x.failed,
        x.too_complex,
        x.no_geometry,
        r.counts.in_categories.saturating_sub(r.counts.with_connectors + x.failed + x.too_complex + x.no_geometry),
        if opts.include_unconnected { "" } else { " (--include-unconnected writes those too)" },
    );
    if x.failed > 0 {
        let first: Vec<String> = r
            .failures
            .iter()
            .take(3)
            .map(|(p, why)| format!("{}: {}", safe_msg(p), why))
            .collect();
        m.push_str(&format!("; first failures: {}", first.join("; ")));
    } else if r.counts.in_categories == 0 {
        m.push_str(&format!(
            "; check the category filter (default: {})",
            DEFAULT_CATEGORIES.join(", ")
        ));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(desc: &str) -> Header {
        Header {
            description: desc.into(),
            org_type: "Part".into(),
            ..Header::default()
        }
    }

    #[test]
    fn exclusions_follow_the_rules_in_the_module_documentation() {
        assert_eq!(exclusion("3001.dat", &header("Brick 2 x 4")), None);
        assert_eq!(
            exclusion("3040b.dat", &header("Slope Brick 45 2 x 1")),
            None
        );
        assert_eq!(
            exclusion("3001p01.dat", &header("Brick 2 x 4 with Print")),
            Some("odd_names")
        );
        assert_eq!(
            exclusion("3001c01.dat", &header("Brick Assembly")),
            Some("odd_names")
        );
        assert_eq!(
            exclusion("973.dat", &header("Minifig Torso")),
            Some("minifig")
        );
        assert_eq!(
            exclusion("1.dat", &header("Torso with Minifigure Parts")),
            Some("minifig")
        );
        assert_eq!(
            exclusion(
                "2.dat",
                &Header {
                    category: Some("Minifig Headwear".into()),
                    ..header("Hat")
                }
            ),
            Some("minifig")
        );
        assert_eq!(
            exclusion("3.dat", &header("Duplo Brick 2 x 2")),
            Some("other_systems")
        );
        assert_eq!(
            exclusion("4.dat", &header("Brick 1 x 1 with Pattern")),
            Some("prints_and_stickers")
        );
        assert_eq!(
            exclusion("5.dat", &header("Sticker 1 x 1")),
            Some("prints_and_stickers")
        );
        assert_eq!(
            exclusion("6.dat", &header("~Moved to 3001")),
            Some("not_a_part")
        );
        assert_eq!(
            exclusion(
                "7.dat",
                &Header {
                    org_type: "Subpart".into(),
                    ..header("Sub")
                }
            ),
            Some("not_a_part")
        );
        assert_eq!(
            exclusion(
                "8.dat",
                &Header {
                    org_qualifiers: vec!["ALIAS".into()],
                    ..header("Brick")
                }
            ),
            Some("not_a_part")
        );
        // A description with the manufacturer's word mark is left out (the word is assembled here, not typed).
        let marked = format!(
            "Brick 1 x 1 with {} Logo",
            WORD_MARK.concat().to_uppercase()
        );
        assert_eq!(
            exclusion("9.dat", &header(&marked)),
            Some("prints_and_stickers")
        );
    }

    #[test]
    fn categories_come_from_the_header_or_the_first_word() {
        assert_eq!(category_of(&header("Brick  2 x  4")), "Brick");
        assert_eq!(category_of(&header("Plate, Round 1 x 1")), "Plate");
        assert_eq!(
            category_of(&Header {
                category: Some("Slope".into()),
                ..header("Brick 45")
            }),
            "Slope"
        );
        assert_eq!(part_id("3001.dat"), "ld.3001");
    }

    #[test]
    fn plain_numbers_sort_before_suffixed_ones() {
        assert!(sort_key(0, "ld.3001") < sort_key(0, "ld.3001a"));
        assert!(
            sort_key(0, "ld.3003") < sort_key(0, "ld.3001a"),
            "every plain number comes before every suffixed one"
        );
        assert!(sort_key(0, "ld.999") < sort_key(0, "ld.3001"));
        assert!(sort_key(0, "ld.9") < sort_key(1, "ld.1"));
    }
}
