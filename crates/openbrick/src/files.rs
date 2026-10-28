//! Reading input files with size caps, the asset resolver of the CLI, and
//! writing output files so that a failed run never leaves half a file.
//!
//! Asset names (SVG artwork, images, fonts) come from two places:
//!
//! - **The command line** (`--param logo=./my-logo.svg`): a path relative to
//!   the current directory, read before the evaluation starts.
//! - **The part document** (the default `assets/mark.svg` of the logo tile): a
//!   path relative to the part file's folder. A downloaded part document is
//!   untrusted, so it may only name files inside its own folder: absolute paths
//!   and `..` are refused.
//!
//! Either way the file must be a regular file of at most
//! [`MAX_ASSET_BYTES`], the evaluator's own SVG size limit.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use openbrick_part::eval::MAX_SVG_BYTES;
use openbrick_part::AssetResolver;

/// Largest asset file the CLI reads: the evaluator's SVG limit (4 MiB).
pub const MAX_ASSET_BYTES: usize = MAX_SVG_BYTES;

/// Largest part document the CLI reads. Part documents are a few kilobytes;
/// the cap keeps a wrong path (a disk image, a device) from being read whole.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

/// Largest fit profile the CLI reads (they are about 1 KB).
pub const MAX_PROFILE_BYTES: usize = 1024 * 1024;

/// Read a regular file of at most `limit` bytes. `what` names the file in
/// messages ("part document", "SVG asset").
pub fn read_regular_file(path: &Path, limit: usize, what: &str) -> Result<Vec<u8>, String> {
    let shown = path.display();
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot read {what} {shown}: {e}"))?;
    if !meta.is_file() {
        return Err(format!(
            "{what} {shown} is not a regular file (a folder, device or pipe); give the path of a file"
        ));
    }
    let too_big = |size: u64| {
        format!(
            "{what} {shown} is {size} bytes; the limit is {limit} bytes ({} MiB)",
            limit / (1024 * 1024)
        )
    };
    if meta.len() > limit as u64 {
        return Err(too_big(meta.len()));
    }
    let file = File::open(path).map_err(|e| format!("cannot read {what} {shown}: {e}"))?;
    let mut bytes = Vec::new();
    // The file may grow between the size check and the read.
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {what} {shown}: {e}"))?;
    if bytes.len() > limit {
        return Err(too_big(bytes.len() as u64));
    }
    Ok(bytes)
}

/// [`read_regular_file`] as UTF-8 text.
pub fn read_text_file(path: &Path, limit: usize, what: &str) -> Result<String, String> {
    let bytes = read_regular_file(path, limit, what)?;
    String::from_utf8(bytes)
        .map_err(|e| format!("{what} {} is not UTF-8 text: {e}", path.display()))
}

/// Whether an asset name from a part document stays inside the document's
/// folder: relative, and without `..` or a drive prefix.
pub fn stays_inside(name: &str) -> bool {
    let p = Path::new(name);
    !name.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Assets named on the command line (read up front) and in the part document
/// (read from the part's folder on demand).
pub struct FileAssets {
    given: BTreeMap<String, Vec<u8>>,
    part_dir: PathBuf,
}

impl FileAssets {
    pub fn new(part_dir: PathBuf) -> FileAssets {
        FileAssets {
            given: BTreeMap::new(),
            part_dir,
        }
    }

    /// Read an asset the user named on the command line, relative to the
    /// current directory. The evaluator then finds it under the same name.
    pub fn add_given(&mut self, name: &str) -> Result<(), String> {
        let bytes = read_regular_file(Path::new(name), MAX_ASSET_BYTES, "asset")?;
        self.given.insert(name.to_string(), bytes);
        Ok(())
    }
}

impl AssetResolver for FileAssets {
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        if let Some(bytes) = self.given.get(name) {
            return Ok(bytes.clone());
        }
        if !stays_inside(name) {
            return Err(format!(
                "the part document names `{name}`, which is outside the part's folder; a part document may only name files inside its own folder. To use a file of your own, pass it on the command line, for example --param logo=./my-logo.svg"
            ));
        }
        read_regular_file(&self.part_dir.join(name), MAX_ASSET_BYTES, "asset")
    }
}

/// Write `bytes` to `path` through a temporary file in the same folder, so a
/// failure never leaves a truncated file at `path`. Creates missing parent
/// folders.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let shown = path.display();
    if path.is_dir() {
        return Err(format!(
            "{shown} is a folder; give a file name, for example {}",
            path.join("out.3mf").display()
        ));
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&parent)
        .map_err(|e| format!("cannot create the folder {}: {e}", parent.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{shown} has no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.partial", std::process::id()));
    let tmp = parent.join(tmp_name);
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot write {shown}: {e}"));
    }
    Ok(())
}

/// `path` relative to `base` when it lies inside it, else absolute; used for
/// paths a manifest records next to itself.
pub fn relative_to(path: &Path, base: &Path) -> String {
    let abs = absolute(path);
    let base = absolute(base);
    match abs.strip_prefix(&base) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().replace('\\', "/"),
        _ => abs.to_string_lossy().into_owned(),
    }
}

/// An absolute form of `path` (canonical when it exists).
pub fn absolute(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    std::env::current_dir()
        .map(|d| d.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_assets_stay_inside_the_part_folder() {
        for ok in ["assets/mark.svg", "./mark.svg", "a/b/c.svg"] {
            assert!(stays_inside(ok), "{ok}");
        }
        for bad in ["", "../secret.svg", "assets/../../x.svg", "/etc/passwd"] {
            assert!(!stays_inside(bad), "{bad}");
        }
    }

    #[test]
    fn folders_and_missing_files_are_refused() {
        let dir = std::env::temp_dir();
        let e = read_regular_file(&dir, 10, "asset").expect_err("a folder");
        assert!(e.contains("not a regular file"), "{e}");
        let e = read_regular_file(&dir.join("openbrick-no-such-file.svg"), 10, "asset")
            .expect_err("missing");
        assert!(e.contains("cannot read asset"), "{e}");
    }

    #[test]
    fn document_assets_cannot_escape() {
        let assets = FileAssets::new(std::env::temp_dir());
        let e = assets.read("../x.svg").expect_err("outside");
        assert!(e.contains("outside the part's folder"), "{e}");
    }
}
