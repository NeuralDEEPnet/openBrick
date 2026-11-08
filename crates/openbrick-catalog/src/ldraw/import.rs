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
//!    word mark "LEGO" (CLAUDE.md: the word mark appears only in the
//!    disclaimer and in docs/plan/12).
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
    Connectivity, LicenceText, Pack, PackCounts, PackError, PackHeader, PackPart, LDRAW_ID_PREFIX,
    MAX_BLOB_BYTES, MAX_PARTS, PACK_FORMAT,
};
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
}

impl Default for ImportOptions {
    fn default() -> ImportOptions {
        ImportOptions {
            resolution: Resolution::Standard,
            categories: None,
            limit: Some(400),
            include_unconnected: false,
            limits: Limits::default(),
            max_part_triangles: 30_000,
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
    pub seconds: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    Pack(#[from] PackError),
    #[error("the library has no part that qualifies; check the category filter (default: {})", DEFAULT_CATEGORIES.join(", "))]
    Empty,
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
        || lower.contains("lego")
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
    let mut chosen: Vec<Done> = done
        .into_iter()
        .filter(|d| opts.include_unconnected || !d.d.conns.is_empty())
        .collect();
    chosen.truncate(cap);
    if chosen.is_empty() {
        return Err(ImportError::Empty);
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
        let size = [d.body.max[0] - d.body.min[0], d.body.max[2] - d.body.min[2]];
        let studs = size.map(|v| ((v + 19) / 20).max(1) as u32);
        parts.push(PackPart {
            id: cand.id,
            name: cand
                .header
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            category: cand.category,
            author: cand.header.author.clone(),
            license: cand.header.license.clone(),
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

fn failure_text(e: &WalkError) -> String {
    e.to_string()
}
