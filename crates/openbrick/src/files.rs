//! Reading input files with size caps, the asset resolver of the CLI, and
//! writing output files so that a failed run never leaves half a file.
//!
//! Asset names (SVG artwork, images, fonts) come from two places:
//!
//! - **The command line** (`--param logo=./my-logo.svg`): a path relative to
//!   the current directory, read before the evaluation starts. The evaluator
//!   gets the file under its file name (`my-logo.svg`), not the path as typed,
//!   because the name is part of the content hash: the same file then gives
//!   the same hash, the same 3MF UUIDs and the same bytes whether it was typed
//!   as `my-logo.svg`, `./my-logo.svg` or an absolute path, from any folder.
//! - **The part document** (the default `assets/mark.svg` of the logo tile): a
//!   path relative to the part file's folder. A downloaded part document is
//!   untrusted, so it may only name files inside its own folder: absolute paths
//!   and `..` are refused, and so is a symbolic link that leads out of the
//!   folder (an archive can carry one).
//!
//! Either way the file must be a regular file of at most
//! [`MAX_ASSET_BYTES`], the evaluator's own SVG size limit.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use openbrick_part::eval::{sha256_hex, MAX_SVG_BYTES};
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

    /// Read an asset the user named on the command line, a path relative to
    /// the current directory, and return the name the evaluator gets for it:
    /// its file name, so the content hash does not depend on how the path was
    /// typed or where the command ran. When two different files share a file
    /// name, the later one is named by its content too
    /// (`<first 16 hex digits of its SHA-256>-<file name>`).
    pub fn add_given(&mut self, path: &str) -> Result<String, String> {
        let bytes = read_regular_file(Path::new(path), MAX_ASSET_BYTES, "asset")?;
        let file_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("asset");
        let name = match self.given.get(file_name) {
            Some(known) if *known != bytes => {
                format!("{}-{file_name}", &sha256_hex(&bytes)[..16])
            }
            _ => file_name.to_string(),
        };
        self.given.insert(name.clone(), bytes);
        Ok(name)
    }

    /// Read an asset the part document names, from the part's folder. The
    /// name must stay inside the folder ([`stays_inside`]), and so must the
    /// file it leads to once symbolic links are followed.
    fn read_document_asset(&self, name: &str) -> Result<Vec<u8>, String> {
        let outside = |what: String| {
            format!(
                "the part document names `{name}`, which {what}; a part document may only name files inside its own folder. To use a file of your own, pass it on the command line, for example --param logo=./my-logo.svg"
            )
        };
        if !stays_inside(name) {
            return Err(outside("is outside the part's folder".into()));
        }
        let path = self.part_dir.join(name);
        let folder = std::fs::canonicalize(&self.part_dir).map_err(|e| {
            format!(
                "cannot read the part's folder {}: {e}",
                self.part_dir.display()
            )
        })?;
        let real = std::fs::canonicalize(&path)
            .map_err(|e| format!("cannot read asset {}: {e}", path.display()))?;
        if !real.starts_with(&folder) {
            return Err(outside(format!(
                "leads through a symbolic link to {}, outside the part's folder",
                real.display()
            )));
        }
        read_regular_file(&real, MAX_ASSET_BYTES, "asset")
    }
}

impl AssetResolver for FileAssets {
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        if let Some(bytes) = self.given.get(name) {
            return Ok(bytes.clone());
        }
        self.read_document_asset(name)
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

    /// A fresh folder under the system temp directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("openbrick-files-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create the scratch folder");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_cannot_lead_out_of_the_part_folder() {
        let s = Scratch::new("symlink");
        let part = s.0.join("part");
        let outside = s.0.join("outside");
        std::fs::create_dir_all(part.join("assets")).expect("folder");
        std::fs::create_dir_all(&outside).expect("folder");
        std::fs::write(outside.join("secret.svg"), b"<svg/>").expect("write");
        std::fs::write(part.join("assets/own.svg"), b"<svg>own</svg>").expect("write");
        // A link out of the folder, as an extracted archive can carry it.
        std::os::unix::fs::symlink(outside.join("secret.svg"), part.join("assets/mark.svg"))
            .expect("symlink");
        // A link that stays inside is fine.
        std::os::unix::fs::symlink("own.svg", part.join("assets/alias.svg")).expect("symlink");
        // So is a part folder that is itself reached through a link.
        std::os::unix::fs::symlink(&part, s.0.join("linked-part")).expect("symlink");

        let assets = FileAssets::new(part.clone());
        let e = assets.read("assets/mark.svg").expect_err("leads outside");
        assert!(
            e.contains("symbolic link") && e.contains("outside the part's folder"),
            "{e}"
        );
        assert_eq!(
            assets.read("assets/alias.svg").as_deref(),
            Ok(&b"<svg>own</svg>"[..])
        );
        let linked = FileAssets::new(s.0.join("linked-part"));
        assert_eq!(
            linked.read("assets/own.svg").as_deref(),
            Ok(&b"<svg>own</svg>"[..])
        );
        assert!(linked.read("assets/mark.svg").is_err());
    }

    #[test]
    fn command_line_assets_are_named_by_file_name() {
        let s = Scratch::new("names");
        std::fs::create_dir_all(s.0.join("a")).expect("folder");
        std::fs::create_dir_all(s.0.join("b")).expect("folder");
        std::fs::write(s.0.join("a/logo.svg"), b"<svg>a</svg>").expect("write");
        std::fs::write(s.0.join("b/logo.svg"), b"<svg>b</svg>").expect("write");
        let a = s.0.join("a/logo.svg").to_string_lossy().into_owned();
        let b = s.0.join("b/logo.svg").to_string_lossy().into_owned();
        let mut assets = FileAssets::new(s.0.clone());
        // However the path is written, the evaluator sees the file name.
        assert_eq!(assets.add_given(&a).as_deref(), Ok("logo.svg"));
        assert_eq!(assets.add_given(&a).as_deref(), Ok("logo.svg"));
        // Another file with the same name is told apart by its content.
        let other = assets.add_given(&b).expect("reads");
        assert_eq!(
            other,
            format!("{}-logo.svg", &sha256_hex(b"<svg>b</svg>")[..16])
        );
        assert_eq!(assets.read("logo.svg").as_deref(), Ok(&b"<svg>a</svg>"[..]));
        assert_eq!(assets.read(&other).as_deref(), Ok(&b"<svg>b</svg>"[..]));
    }
}
