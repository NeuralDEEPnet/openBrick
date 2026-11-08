//! `openbrick catalog import | info | fetch` (docs/plan/07 section 3, docs/plan/08
//! sections 1 and 3): turn the user's own copy of the LDraw Parts Library into an
//! `.obpack` the assembler loads, look inside one, and download the official
//! library.
//!
//! The library is data the user fetches at runtime (CC BY 2.0 / CC BY 4.0); a
//! pack made from it is user-local data and is never committed or bundled
//! (docs/plan/12). `fetch` uses the system `curl` (HTTPS only, through the
//! proxy settings of the environment), refuses to overwrite a file, caps the
//! size and checks the SHA-256 before it keeps the file.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, Subcommand};
use openbrick_catalog::ldraw::{import, ImportError, ImportOptions, Resolution};
use openbrick_catalog::{Connectivity, Pack, PackError};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::files::write_file;
use crate::ui::{Fail, Outcome, Ui, GENERATOR};

/// The official complete library, always the latest release.
pub const LDRAW_COMPLETE_URL: &str = "https://library.ldraw.org/library/updates/complete.zip";
/// The release this build was checked against: update 2026-10, 146,853,687 bytes.
/// Downloaded twice on 2026-11-08 with identical hashes, and equal to the hash
/// recorded in docs/spikes/q04-ldraw-connectivity.md. The URL serves whatever
/// release is newest, so a later release has a different hash: `fetch` then
/// keeps the file aside and asks for `--sha256`.
pub const LDRAW_COMPLETE_SHA256: &str =
    "023083918da2d3697c794fb4267975509584f3611b993095874206a803fa610f";
/// The largest download accepted: the 2026-10 library is 140 MiB.
pub const MAX_DOWNLOAD_BYTES: u64 = 400 * 1024 * 1024;

#[derive(Subcommand, Debug)]
pub enum CatalogCmd {
    /// Turn an LDraw library (folder or complete.zip) into an .obpack.
    ///
    /// Reads the parts of the chosen categories, derives their meshes, collision
    /// boxes and connectors from the primitives they reference, and writes a pack
    /// of the parts that have connectors. The library is the user's own copy
    /// (CC BY 2.0 / CC BY 4.0); the pack carries its attribution and is never
    /// to be committed or redistributed.
    #[command(after_help = IMPORT_EXAMPLES)]
    Import(ImportArgs),
    /// Print what an .obpack holds: counts, categories, connector coverage.
    Info(InfoArgs),
    /// Download the official LDraw library (complete.zip) into a folder.
    ///
    /// Refuses to overwrite, caps the size at 400 MiB and checks the SHA-256.
    Fetch(FetchArgs),
}

const IMPORT_EXAMPLES: &str = "Examples:
  openbrick catalog fetch --dest ~/ldraw
  openbrick catalog import ~/ldraw/complete.zip --out ldraw.obpack
  openbrick catalog import ~/ldraw/ldraw --out bricks.obpack --category-filter Brick,Plate,Tile --limit 150
  openbrick catalog info ldraw.obpack

Guide: docs/guides/ldraw-parts.md";

impl CatalogCmd {
    pub fn json(&self) -> bool {
        match self {
            CatalogCmd::Import(a) => a.json,
            CatalogCmd::Info(a) => a.json,
            CatalogCmd::Fetch(a) => a.json,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            CatalogCmd::Import(_) => "catalog import",
            CatalogCmd::Info(_) => "catalog info",
            CatalogCmd::Fetch(_) => "catalog fetch",
        }
    }

    pub fn run(&self, ui: Ui) -> Result<Outcome, Fail> {
        match self {
            CatalogCmd::Import(a) => import_cmd(a, ui),
            CatalogCmd::Info(a) => info_cmd(a, ui),
            CatalogCmd::Fetch(a) => fetch_cmd(a, ui),
        }
    }
}

#[derive(Args, Debug)]
pub struct ImportArgs {
    /// The LDraw library: the folder that holds `parts` and `p` (or its `ldraw` folder), or complete.zip.
    pub source: PathBuf,
    /// The pack file to write (.obpack).
    #[arg(long, value_name = "FILE")]
    pub out: PathBuf,
    /// LDraw categories to import, comma separated (case-insensitive). Default:
    /// Brick, Plate, Tile, Slope, Baseplate, Wedge, Arch, Window, Door,
    /// Windscreen, Panel, Cylinder, Cone, Fence, Bracket.
    #[arg(long, value_delimiter = ',', value_name = "A,B")]
    pub category_filter: Option<Vec<String>>,
    /// The most parts to write (default 600, taken from every category in turn; 0 for all that qualify).
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
    /// Which primitives round parts use: low (p/8), standard (p/) or high (p/48).
    #[arg(long, default_value = "standard", value_parser = parse_resolution, value_name = "LEVEL")]
    pub resolution: Resolution,
    /// Also write parts without a derived connector (placeable on the ground or at the pointer only).
    #[arg(long)]
    pub include_unconnected: bool,
    /// Replace the output file if it exists.
    #[arg(long)]
    pub force: bool,
    /// Print one JSON object on stdout instead of the report.
    #[arg(long)]
    pub json: bool,
}

fn parse_resolution(s: &str) -> Result<Resolution, String> {
    Resolution::from_name(s)
        .ok_or_else(|| format!("`{s}` is not a level; use low, standard or high"))
}

#[derive(Args, Debug)]
pub struct InfoArgs {
    /// The pack (.obpack).
    pub pack: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct FetchArgs {
    /// The folder to put complete.zip in (created if missing).
    #[arg(long, value_name = "DIR")]
    pub dest: PathBuf,
    /// The SHA-256 (hex) the file must have. Default: the release this build
    /// was checked against; the library changes every few months, and then you
    /// pass the new hash after checking the release at ldraw.org.
    #[arg(long, value_name = "HEX")]
    pub sha256: Option<String>,
    /// Download from another https address (a mirror); the hash is still checked.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long)]
    pub json: bool,
}

fn read_pack(path: &Path) -> Result<Pack, Fail> {
    let bytes = std::fs::read(path).map_err(|e| {
        Fail::Input(format!(
            "cannot read {}: {e}; give the path of an .obpack made by `openbrick catalog import`",
            path.display()
        ))
    })?;
    Pack::from_bytes(&bytes).map_err(|e: PackError| Fail::Input(format!("{}: {e}", path.display())))
}

fn import_cmd(a: &ImportArgs, ui: Ui) -> Result<Outcome, Fail> {
    if a.out.exists() && !a.force {
        return Err(Fail::Input(format!(
            "{} exists; choose another --out or pass --force to replace it",
            a.out.display()
        )));
    }
    let opts = ImportOptions {
        resolution: a.resolution,
        categories: a.category_filter.clone(),
        limit: match a.limit {
            Some(0) => None,
            Some(n) => Some(n),
            None => Some(600),
        },
        include_unconnected: a.include_unconnected,
        ..ImportOptions::default()
    };
    let mut last = String::new();
    let (pack, report) = import(&a.source, &opts, &mut |what, done, total| {
        let line = format!("{what}: {done} of {total}");
        if line != last {
            ui.err(&line);
            last = line;
        }
    })
    .map_err(|e| match e {
        ImportError::Library(e) => Fail::Input(e.to_string()),
        ImportError::Empty => Fail::Input(e.to_string()),
        ImportError::Pack(e) => Fail::Internal(e.to_string()),
    })?;
    let bytes = pack.to_bytes().map_err(|e| Fail::Internal(e.to_string()))?;
    write_file(&a.out, &bytes).map_err(|e| Fail::Input(e.to_string()))?;

    let c = &report.counts;
    ui.say(format!(
        "LDraw library {} ({} primitives): {} part files scanned",
        report.library, report.resolution, c.scanned
    ));
    ui.say(format!(
        "  left out before reading geometry: {} not parts (subparts, aliases, obsolete), {} minifigure, {} other systems, {} prints and stickers, {} with printed-variant names, {} in other categories",
        report.excluded.not_a_part,
        report.excluded.minifig,
        report.excluded.other_systems,
        report.excluded.prints_and_stickers,
        report.excluded.odd_names,
        report.excluded.other_categories
    ));
    ui.say(format!(
        "  eligible {}, of the chosen categories {}; with derived connectors {} ({:.1} percent); studs {}, receptacles {}, pin or axle connectors {}",
        c.eligible,
        c.in_categories,
        c.with_connectors,
        percent(c.with_connectors, c.in_categories),
        report.with_studs,
        report.with_receptacles,
        report.with_technic
    ));
    ui.say(format!(
        "  too complex {}, no geometry {}, failed {}",
        report.excluded.too_complex, report.excluded.no_geometry, report.excluded.failed
    ));
    for (p, m) in &report.failures {
        ui.err(format!("  failed {p}: {m}"));
    }
    ui.say(format!(
        "wrote {} parts to {} ({} bytes) in {:.1} s",
        c.imported,
        a.out.display(),
        bytes.len(),
        report.seconds
    ));
    if c.with_joints > 0 {
        ui.say(format!(
            "  {} of them have clip, hinge, ball or socket joints, which are not modelled yet: their studs snap, the joints do not",
            c.with_joints
        ));
    }
    if c.free_placement > 0 {
        ui.say(format!(
            "  {} have no connector: free placement only (on the ground or at the pointer)",
            c.free_placement
        ));
    }
    ui.say("Built from the LDraw Parts Library (CC BY 2.0 / CC BY 4.0), https://www.ldraw.org. The pack is for your own use: do not commit or redistribute it.");
    Ok(Outcome {
        code: 0,
        json: json!({
            "command": "catalog import",
            "generator": GENERATOR,
            "ok": true,
            "exitCode": 0,
            "out": a.out.display().to_string(),
            "bytes": bytes.len(),
            "fingerprint": pack.fingerprint(),
            "report": report,
        }),
    })
}

fn percent(n: u32, of: u32) -> f64 {
    if of == 0 {
        0.0
    } else {
        100.0 * f64::from(n) / f64::from(of)
    }
}

fn info_cmd(a: &InfoArgs, ui: Ui) -> Result<Outcome, Fail> {
    let pack = read_pack(&a.pack)?;
    let size = std::fs::metadata(&a.pack).map(|m| m.len()).unwrap_or(0);
    let h = &pack.header;
    let mut by_cat: BTreeMap<&str, u32> = BTreeMap::new();
    let (mut derived, mut none, mut joints, mut tris, mut conns) = (0u32, 0u32, 0u32, 0u64, 0u64);
    for p in &h.parts {
        *by_cat.entry(p.category.as_str()).or_default() += 1;
        match p.connectivity {
            Connectivity::Derived => derived += 1,
            Connectivity::None => none += 1,
        }
        if !p.joints.is_empty() {
            joints += 1;
        }
        tris += u64::from(p.triangles);
        conns += p.connectors.len() as u64;
    }
    ui.say(format!(
        "{}: {} pack `{}`, LDraw library {} ({} primitives), format {}, {} bytes",
        a.pack.display(),
        h.generator,
        h.id,
        h.library,
        h.resolution,
        h.format,
        size
    ));
    ui.say(format!("fingerprint {}", pack.fingerprint()));
    ui.say(format!(
        "{} parts: connectivity derived {derived}, none (free placement only) {none}; {joints} with unmodelled joints; {conns} connectors, {tris} triangles",
        h.parts.len()
    ));
    let cats: Vec<String> = by_cat.iter().map(|(k, v)| format!("{k} {v}")).collect();
    ui.say(format!("categories: {}", cats.join(", ")));
    let c = &h.counts;
    ui.say(format!(
        "import: scanned {}, eligible {}, in the chosen categories {} ({} with connectors), imported {}",
        c.scanned, c.eligible, c.in_categories, c.with_connectors, c.imported
    ));
    ui.say(&h.attribution);
    Ok(Outcome {
        code: 0,
        json: json!({
            "command": "catalog info",
            "generator": GENERATOR,
            "ok": true,
            "exitCode": 0,
            "pack": a.pack.display().to_string(),
            "bytes": size,
            "fingerprint": pack.fingerprint(),
            "id": h.id,
            "library": h.library,
            "resolution": h.resolution,
            "format": h.format,
            "parts": h.parts.len(),
            "connectivity": { "derived": derived, "none": none },
            "withJoints": joints,
            "connectors": conns,
            "triangles": tris,
            "categories": by_cat,
            "counts": h.counts,
            "attribution": h.attribution,
        }),
    })
}

/// SHA-256 of a file, streamed, in hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn fetch_cmd(a: &FetchArgs, ui: Ui) -> Result<Outcome, Fail> {
    let url = a.url.as_deref().unwrap_or(LDRAW_COMPLETE_URL);
    if !url.starts_with("https://") {
        return Err(Fail::Input(format!(
            "{url} is not an https address; the library is only downloaded over TLS"
        )));
    }
    let want = a
        .sha256
        .clone()
        .unwrap_or_else(|| LDRAW_COMPLETE_SHA256.to_string())
        .to_ascii_lowercase();
    if !valid_sha(&want) {
        return Err(Fail::Input(
            "--sha256 needs 64 hexadecimal digits, the SHA-256 of the complete.zip you trust"
                .into(),
        ));
    }
    let target = a.dest.join("complete.zip");
    let aside = a.dest.join("complete.zip.unverified");
    if target.exists() {
        return Err(Fail::Input(format!(
            "{} exists; not overwriting it. Delete it first if you want a fresh download",
            target.display()
        )));
    }
    std::fs::create_dir_all(&a.dest)
        .map_err(|e| Fail::Input(format!("cannot create {}: {e}", a.dest.display())))?;

    // A file kept aside by an earlier run is accepted when it matches the hash the person now trusts.
    if aside.exists() {
        let got = sha256_file(&aside).map_err(|e| Fail::Input(e.to_string()))?;
        if got == want {
            std::fs::rename(&aside, &target).map_err(|e| Fail::Input(e.to_string()))?;
            ui.say(format!(
                "accepted the earlier download: {}",
                target.display()
            ));
            return Ok(done(
                &target,
                &got,
                std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0),
            ));
        }
    }

    let part = a.dest.join("complete.zip.part");
    if part.exists() {
        return Err(Fail::Input(format!(
            "{} is left from an interrupted download; delete it and run again",
            part.display()
        )));
    }
    ui.say(format!(
        "downloading {url} (at most {} MiB)",
        MAX_DOWNLOAD_BYTES / (1024 * 1024)
    ));
    let status = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--max-filesize", &MAX_DOWNLOAD_BYTES.to_string()])
        .args(["--max-time", "3600"])
        .arg("--output")
        .arg(&part)
        .arg(url)
        .status()
        .map_err(|e| {
            Fail::Input(format!(
                "cannot run curl ({e}); install curl, or download complete.zip from https://library.ldraw.org/updates?latest by hand and give it to `catalog import` directly"
            ))
        })?;
    if !status.success() {
        let _ = std::fs::remove_file(&part);
        return Err(Fail::Input(format!(
            "the download failed ({status}); check the network and the proxy settings (HTTPS_PROXY) and try again"
        )));
    }
    let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if size > MAX_DOWNLOAD_BYTES {
        let _ = std::fs::remove_file(&part);
        return Err(Fail::Input(format!(
            "the download is {size} bytes, over the {MAX_DOWNLOAD_BYTES}-byte cap; it was deleted"
        )));
    }
    let got = sha256_file(&part).map_err(|e| Fail::Input(e.to_string()))?;
    if got != want {
        std::fs::rename(&part, &aside).map_err(|e| Fail::Input(e.to_string()))?;
        return Err(Fail::Input(format!(
            "the file's SHA-256 is {got}, not the expected {want}. The library is probably newer than this build; if you trust that release (check ldraw.org), run again with --sha256 {got}: the file is kept as {} and is accepted without downloading again",
            aside.display()
        )));
    }
    std::fs::rename(&part, &target).map_err(|e| Fail::Input(e.to_string()))?;
    ui.say(format!(
        "saved {} ({size} bytes), SHA-256 {got}",
        target.display()
    ));
    Ok(done(&target, &got, size))
}

fn done(target: &Path, sha: &str, size: u64) -> Outcome {
    Outcome {
        code: 0,
        json: json!({
            "command": "catalog fetch",
            "generator": GENERATOR,
            "ok": true,
            "exitCode": 0,
            "file": target.display().to_string(),
            "bytes": size,
            "sha256": sha,
            "licence": "LDraw Parts Library, CC BY 2.0 / CC BY 4.0 per file; see CAreadme.txt inside",
        }),
    }
}
