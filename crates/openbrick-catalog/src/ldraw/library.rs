//! A bounded view of an LDraw parts library: a directory (`<dir>/parts`,
//! `<dir>/p`, or `<dir>/ldraw/...`) or a `complete.zip`, with the sub-file
//! search of the LDraw spec and a parse cache.
//!
//! Search order for a sub-file name (names are lower-cased, `\` becomes `/`,
//! `..`, absolute paths and drive letters are refused):
//!
//! 1. an explicit `48/x.dat` or `8/x.dat` names `p/48/x.dat` or `p/8/x.dat`;
//! 2. `parts/<name>` (so `s/3003s01.dat` is `parts/s/3003s01.dat`);
//! 3. for a plain name, the resolution variant first when one is asked for
//!    ([`Resolution::High`]: `p/48/<name>`, [`Resolution::Low`]: `p/8/<name>`),
//!    then `p/<name>`, then `parts/s/<name>`.
//!
//! The subset on purpose: `parts`, `parts/s`, `p`, `p/48`, `p/8`. Textures
//! (`parts/textures`), models, the `ldraw/unofficial` tree and colour
//! configuration are not read.
//!
//! Everything read is untrusted: a file over [`MAX_FILE_BYTES`] is refused, a
//! zip entry is inflated through a size-limited reader, entries with unsafe
//! paths are not indexed, symbolic links in a directory are not followed, and
//! the bytes read in one session are capped.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::parse::{parse, ParsedFile};

/// The largest single file read, in bytes. The biggest official file is a few
/// megabytes.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// The most bytes one [`Library`] reads in total (a guard against a zip whose
/// entries inflate to far more than they claim).
pub const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The most entries a zip index holds.
pub const MAX_ZIP_ENTRIES: usize = 200_000;

/// Which variant of the library's primitives to prefer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    /// `p/8/` first: round parts with few segments (cheap).
    Low,
    /// `p/` only.
    Standard,
    /// `p/48/` first: round parts with many segments.
    High,
}

impl Resolution {
    pub fn name(self) -> &'static str {
        match self {
            Resolution::Low => "low",
            Resolution::Standard => "standard",
            Resolution::High => "high",
        }
    }

    pub fn from_name(s: &str) -> Option<Resolution> {
        match s {
            "low" => Some(Resolution::Low),
            "standard" => Some(Resolution::Standard),
            "high" => Some(Resolution::High),
            _ => None,
        }
    }
}

/// Why a library could not be opened or read.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("cannot open {path}: {message}")]
    Open { path: String, message: String },
    #[error("{path} does not look like an LDraw library: it has no `parts` and `p` folders (looked in the folder itself and in its `ldraw` folder); give the folder that holds them, or the official complete.zip")]
    NotALibrary { path: String },
    #[error("the zip is not usable: {0}")]
    Zip(String),
    #[error("{name} is {size} bytes, over the {limit}-byte limit for one file; the library may be damaged")]
    TooBig {
        name: String,
        size: u64,
        limit: usize,
    },
    #[error("the library inflated to more than {0} bytes in all; refusing to read more")]
    TotalTooBig(u64),
    #[error("cannot read {name}: {message}")]
    Read { name: String, message: String },
}

/// Normalise a sub-file name to a safe, lower-case, `/`-separated relative
/// path, or `None` when it could leave the library.
pub fn normalise_name(name: &str) -> Option<String> {
    let n = name.trim().replace('\\', "/").to_ascii_lowercase();
    let n = n.trim_start_matches("./");
    if n.is_empty() || n.starts_with('/') || n.contains(':') || n.contains('\0') {
        return None;
    }
    if n.split('/').any(|c| c.is_empty() || c == ".." || c == ".") {
        return None;
    }
    Some(n.to_string())
}

/// Where the files are.
trait Source {
    /// Lower-case library-relative paths (`parts/3001.dat`) of every indexed file.
    fn names(&self) -> Vec<String>;
    fn contains(&self, rel: &str) -> bool;
    fn read(&self, rel: &str) -> Result<Vec<u8>, LibraryError>;
    /// A file at the library root (`CAreadme.txt`), when there is one.
    fn read_root(&self, name: &str) -> Option<Vec<u8>>;
}

const FOLDERS: [&str; 5] = ["parts", "parts/s", "p", "p/48", "p/8"];

struct DirSource {
    /// Lower-case relative path to the real path.
    index: HashMap<String, PathBuf>,
    root: PathBuf,
}

impl DirSource {
    fn open(path: &Path) -> Result<DirSource, LibraryError> {
        let candidates = [path.to_path_buf(), path.join("ldraw")];
        let root = candidates
            .into_iter()
            .find(|c| c.join("parts").is_dir() && c.join("p").is_dir())
            .ok_or_else(|| LibraryError::NotALibrary {
                path: path.display().to_string(),
            })?;
        let mut index = HashMap::new();
        for folder in FOLDERS {
            let Ok(rd) = std::fs::read_dir(root.join(folder)) else {
                continue;
            };
            for entry in rd.flatten() {
                // Regular files only: a symbolic link could point anywhere.
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_file() {
                    continue;
                }
                let fname = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if fname.ends_with(".dat") {
                    index.insert(format!("{folder}/{fname}"), entry.path());
                }
            }
        }
        Ok(DirSource { index, root })
    }
}

impl Source for DirSource {
    fn names(&self) -> Vec<String> {
        self.index.keys().cloned().collect()
    }
    fn contains(&self, rel: &str) -> bool {
        self.index.contains_key(rel)
    }
    fn read(&self, rel: &str) -> Result<Vec<u8>, LibraryError> {
        let path = self.index.get(rel).ok_or_else(|| LibraryError::Read {
            name: rel.into(),
            message: "not in the library".into(),
        })?;
        let f = File::open(path).map_err(|e| LibraryError::Read {
            name: rel.into(),
            message: e.to_string(),
        })?;
        read_limited(f, rel, MAX_FILE_BYTES)
    }
    fn read_root(&self, name: &str) -> Option<Vec<u8>> {
        let f = File::open(self.root.join(name)).ok()?;
        read_limited(f, name, 1024 * 1024).ok()
    }
}

fn read_limited(r: impl Read, name: &str, limit: usize) -> Result<Vec<u8>, LibraryError> {
    let mut buf = Vec::new();
    r.take(limit as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| LibraryError::Read {
            name: name.into(),
            message: e.to_string(),
        })?;
    if buf.len() > limit {
        return Err(LibraryError::TooBig {
            name: name.into(),
            size: buf.len() as u64,
            limit,
        });
    }
    Ok(buf)
}

// ---------------------------------------------------------------- zip

/// One entry of a zip's central directory.
#[derive(Clone, Debug)]
struct ZipEntry {
    method: u16,
    comp_size: u64,
    size: u64,
    local_offset: u64,
}

struct ZipSource {
    file: RefCell<File>,
    /// Lower-case path inside the library (without the leading `ldraw/`).
    entries: HashMap<String, ZipEntry>,
}

fn le16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// The library-relative path of a zip entry name, or `None` when the entry is
/// not under `parts/`, `p/` or the root files we read, or its path is unsafe
/// (the zip-slip rule: no `..`, no absolute path, no drive letter).
pub fn zip_entry_path(name: &str) -> Option<String> {
    let n = normalise_name(name)?;
    let n = n.strip_prefix("ldraw/").unwrap_or(&n).to_string();
    Some(n)
}

impl ZipSource {
    fn open(path: &Path) -> Result<ZipSource, LibraryError> {
        let zip_err = |m: &str| LibraryError::Zip(m.to_string());
        let mut file = File::open(path).map_err(|e| LibraryError::Open {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let len = file
            .metadata()
            .map_err(|e| LibraryError::Zip(e.to_string()))?
            .len();
        if len < 22 {
            return Err(zip_err("the file is too short to be a zip"));
        }
        // End of central directory: scan the tail for its signature.
        let tail_len = len.min(22 + 65_535);
        file.seek(SeekFrom::Start(len - tail_len))
            .map_err(|e| LibraryError::Zip(e.to_string()))?;
        let mut tail = vec![0u8; tail_len as usize];
        file.read_exact(&mut tail)
            .map_err(|e| LibraryError::Zip(e.to_string()))?;
        let eocd = (0..=tail.len().saturating_sub(22))
            .rev()
            .find(|&i| tail[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
            .ok_or_else(|| zip_err("no end-of-directory record; is this a zip file?"))?;
        let count = le16(&tail, eocd + 10).ok_or_else(|| zip_err("short record"))? as usize;
        let cd_size = le32(&tail, eocd + 12).ok_or_else(|| zip_err("short record"))?;
        let cd_off = le32(&tail, eocd + 16).ok_or_else(|| zip_err("short record"))?;
        if count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF {
            return Err(zip_err(
                "zip64 archives are not supported; use the official complete.zip",
            ));
        }
        if u64::from(cd_off) + u64::from(cd_size) > len || cd_size as usize > 64 * 1024 * 1024 {
            return Err(zip_err("the central directory is out of range"));
        }
        if count > MAX_ZIP_ENTRIES {
            return Err(zip_err("the zip has too many entries"));
        }
        file.seek(SeekFrom::Start(u64::from(cd_off)))
            .map_err(|e| LibraryError::Zip(e.to_string()))?;
        let mut cd = vec![0u8; cd_size as usize];
        file.read_exact(&mut cd)
            .map_err(|e| LibraryError::Zip(e.to_string()))?;
        let mut entries = HashMap::new();
        let mut i = 0usize;
        for _ in 0..count {
            if cd.get(i..i + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
                return Err(zip_err("a central directory entry is damaged"));
            }
            let flags = le16(&cd, i + 8).ok_or_else(|| zip_err("short entry"))?;
            let method = le16(&cd, i + 10).ok_or_else(|| zip_err("short entry"))?;
            let comp_size = le32(&cd, i + 20).ok_or_else(|| zip_err("short entry"))?;
            let size = le32(&cd, i + 24).ok_or_else(|| zip_err("short entry"))?;
            let n = le16(&cd, i + 28).ok_or_else(|| zip_err("short entry"))? as usize;
            let m = le16(&cd, i + 30).ok_or_else(|| zip_err("short entry"))? as usize;
            let k = le16(&cd, i + 32).ok_or_else(|| zip_err("short entry"))? as usize;
            let off = le32(&cd, i + 42).ok_or_else(|| zip_err("short entry"))?;
            let name = cd
                .get(i + 46..i + 46 + n)
                .ok_or_else(|| zip_err("a name is cut off"))?;
            i += 46 + n + m + k;
            if flags & 1 != 0 || !(method == 0 || method == 8) {
                continue; // encrypted or an unusual method: not a library file
            }
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            if name.ends_with('/') {
                continue;
            }
            let Some(rel) = zip_entry_path(name) else {
                continue; // unsafe path: never indexed, so never read or written
            };
            let wanted = rel.ends_with(".dat")
                && FOLDERS.iter().any(|f| {
                    rel.strip_prefix(f)
                        .is_some_and(|r| r.starts_with('/') && !r[1..].contains('/'))
                })
                || matches!(
                    rel.as_str(),
                    "caread" | "careadme.txt" | "calicense.txt" | "calicense4.txt"
                );
            if wanted {
                entries.insert(
                    rel,
                    ZipEntry {
                        method,
                        comp_size: u64::from(comp_size),
                        size: u64::from(size),
                        local_offset: u64::from(off),
                    },
                );
            }
        }
        Ok(ZipSource {
            file: RefCell::new(file),
            entries,
        })
    }

    fn read_entry(&self, rel: &str, limit: usize) -> Result<Vec<u8>, LibraryError> {
        let e = self.entries.get(rel).ok_or_else(|| LibraryError::Read {
            name: rel.into(),
            message: "not in the zip".into(),
        })?;
        if e.size > limit as u64 {
            return Err(LibraryError::TooBig {
                name: rel.into(),
                size: e.size,
                limit,
            });
        }
        let rd = |m: String| LibraryError::Read {
            name: rel.into(),
            message: m,
        };
        let mut f = self.file.borrow_mut();
        f.seek(SeekFrom::Start(e.local_offset))
            .map_err(|x| rd(x.to_string()))?;
        let mut head = [0u8; 30];
        f.read_exact(&mut head).map_err(|x| rd(x.to_string()))?;
        if head[..4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(rd("the local header is damaged".into()));
        }
        let skip =
            u64::from(le16(&head, 26).unwrap_or(0)) + u64::from(le16(&head, 28).unwrap_or(0));
        f.seek(SeekFrom::Current(skip as i64))
            .map_err(|x| rd(x.to_string()))?;
        let raw = read_limited(Read::take(&mut *f, e.comp_size), rel, limit + 64 * 1024)?;
        drop(f);
        match e.method {
            0 => {
                if raw.len() as u64 != e.size {
                    return Err(rd("stored size does not match".into()));
                }
                Ok(raw)
            }
            _ => {
                // The inflate limit is the file limit, never the size the entry claims.
                let out = miniz_oxide::inflate::decompress_to_vec_with_limit(&raw, limit)
                    .map_err(|x| rd(format!("cannot inflate: {x:?}")))?;
                Ok(out)
            }
        }
    }
}

impl Source for ZipSource {
    fn names(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }
    fn contains(&self, rel: &str) -> bool {
        self.entries.contains_key(rel)
    }
    fn read(&self, rel: &str) -> Result<Vec<u8>, LibraryError> {
        self.read_entry(rel, MAX_FILE_BYTES)
    }
    fn read_root(&self, name: &str) -> Option<Vec<u8>> {
        self.read_entry(&name.to_ascii_lowercase(), 1024 * 1024)
            .ok()
    }
}

// ---------------------------------------------------------------- library

/// A library with a parse cache.
pub struct Library {
    source: Box<dyn Source>,
    resolution: Resolution,
    cache: RefCell<HashMap<String, Option<Rc<ParsedFile>>>>,
    bytes_read: Cell<u64>,
}

/// A resolved file: its library path and parsed content.
pub struct Resolved {
    pub path: String,
    pub file: Rc<ParsedFile>,
}

impl Library {
    /// Open a library directory, or a zip when `path` is a file.
    pub fn open(path: &Path, resolution: Resolution) -> Result<Library, LibraryError> {
        let source: Box<dyn Source> = if path.is_file() {
            Box::new(ZipSource::open(path)?)
        } else if path.is_dir() {
            Box::new(DirSource::open(path)?)
        } else {
            return Err(LibraryError::Open {
                path: path.display().to_string(),
                message: "no such file or folder".into(),
            });
        };
        let lib = Library {
            source,
            resolution,
            cache: RefCell::new(HashMap::new()),
            bytes_read: Cell::new(0),
        };
        if !lib.source.names().iter().any(|n| n.starts_with("parts/")) {
            return Err(LibraryError::NotALibrary {
                path: path.display().to_string(),
            });
        }
        Ok(lib)
    }

    pub fn resolution(&self) -> Resolution {
        self.resolution
    }

    /// Paths (`parts/3001.dat`) of the top-level part files, sorted.
    pub fn part_paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .source
            .names()
            .into_iter()
            .filter(|n| {
                n.strip_prefix("parts/")
                    .is_some_and(|r| !r.contains('/') && r.ends_with(".dat"))
            })
            .collect();
        v.sort();
        v
    }

    /// A library root file as text (`CAreadme.txt`), if present (capped at 1 MiB).
    pub fn root_text(&self, name: &str) -> Option<String> {
        let b = self.source.read_root(name)?;
        Some(String::from_utf8_lossy(&b).into_owned())
    }

    /// The candidate library paths of a sub-file name, in search order.
    pub fn candidates(&self, name: &str) -> Vec<String> {
        let Some(n) = normalise_name(name) else {
            return Vec::new();
        };
        if n.starts_with("48/") || n.starts_with("8/") {
            return vec![format!("p/{n}")];
        }
        let mut c = vec![format!("parts/{n}")];
        if !n.contains('/') {
            match self.resolution {
                Resolution::High => c.push(format!("p/48/{n}")),
                Resolution::Low => c.push(format!("p/8/{n}")),
                Resolution::Standard => {}
            }
            c.push(format!("p/{n}"));
            c.push(format!("parts/s/{n}"));
        }
        c
    }

    /// Resolve and parse a sub-file name; `Ok(None)` when no candidate exists.
    pub fn resolve(&self, name: &str) -> Result<Option<Resolved>, LibraryError> {
        for path in self.candidates(name) {
            if let Some(cached) = self.cache.borrow().get(&path) {
                if let Some(file) = cached {
                    return Ok(Some(Resolved {
                        path,
                        file: file.clone(),
                    }));
                }
                continue;
            }
            if !self.source.contains(&path) {
                continue;
            }
            return self.load(&path).map(Some);
        }
        Ok(None)
    }

    /// Resolve a library path that is known to exist (`parts/3001.dat`).
    pub fn load(&self, path: &str) -> Result<Resolved, LibraryError> {
        if let Some(Some(file)) = self.cache.borrow().get(path) {
            return Ok(Resolved {
                path: path.into(),
                file: file.clone(),
            });
        }
        let bytes = self.source.read(path)?;
        let total = self.bytes_read.get() + bytes.len() as u64;
        if total > MAX_TOTAL_BYTES {
            return Err(LibraryError::TotalTooBig(MAX_TOTAL_BYTES));
        }
        self.bytes_read.set(total);
        let file = Rc::new(parse(&String::from_utf8_lossy(&bytes)));
        self.cache
            .borrow_mut()
            .insert(path.into(), Some(file.clone()));
        Ok(Resolved {
            path: path.into(),
            file,
        })
    }

    /// Forget parsed parts to bound memory: keeps primitives (`p/...`), which
    /// the next part reuses, and drops everything under `parts/`.
    pub fn trim_cache(&self) {
        self.cache.borrow_mut().retain(|k, _| k.starts_with("p/"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_normalised_and_unsafe_ones_refused() {
        assert_eq!(
            normalise_name("S\\3003S01.DAT").as_deref(),
            Some("s/3003s01.dat")
        );
        assert_eq!(normalise_name("./stud.dat").as_deref(), Some("stud.dat"));
        for bad in [
            "../x.dat",
            "a/../../x.dat",
            "/etc/passwd",
            "C:\\x.dat",
            "",
            "a//b.dat",
            "a\0b",
        ] {
            assert_eq!(normalise_name(bad), None, "{bad:?}");
        }
        assert_eq!(
            zip_entry_path("ldraw/parts/3001.dat").as_deref(),
            Some("parts/3001.dat")
        );
        assert_eq!(zip_entry_path("ldraw/../evil.dat"), None);
        assert_eq!(zip_entry_path("../../evil.dat"), None);
    }
}
