//! The file rules of the desktop app: which files the native dialogs offer,
//! the name a saved file gets, and reading and writing the one file the user
//! chose.
//!
//! The webview never names a path. It asks the app's commands (src/app.rs) to
//! show a native dialog; the user chooses the file; the command reads or writes
//! that file and nothing else. These rules hold no Tauri types, so
//! `cargo test --workspace` checks them on every OS without the webview
//! libraries.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Largest file the open command reads, whatever the page asks for. The page
/// asks for the core's own limits (the SVG limit from `max_asset_bytes()`, the
/// fit profile limit in apps/web/src/main.js), which are smaller; this bound
/// only keeps a wrong choice (a disk image, a video) from being read whole.
pub const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;

/// Header that carries the suggested file name of a save, percent-encoded
/// (`encodeURIComponent`) because header values are ASCII.
pub const NAME_HEADER: &str = "x-openbrick-name";
/// Header that carries the format of a save: `3mf`, `stl` or `json`.
pub const FORMAT_HEADER: &str = "x-openbrick-format";

/// Fallback stem when the suggested name has nothing usable left.
const FALLBACK_STEM: &str = "openbrick";
/// Longest file name suggested, in characters, extension included.
const MAX_NAME_CHARS: usize = 120;

/// What the page saves: the part and coupon exports and a fit profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    /// A 3MF model (`openbrick_io::write_3mf`).
    ThreeMf,
    /// A binary STL mesh (`openbrick_io::write_stl_binary`).
    Stl,
    /// A JSON document: a fit profile written by the calibration wizard, or a
    /// part document saved by the designer (`.obpart.json`).
    Json,
}

impl SaveFormat {
    /// The format named by the page (`3mf`, `stl`, `json`).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "3mf" => Ok(Self::ThreeMf),
            "stl" => Ok(Self::Stl),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "cannot save a file of format {other:?}; the desktop app saves 3mf, stl or json"
            )),
        }
    }

    /// The file extension, lower case, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::ThreeMf => "3mf",
            Self::Stl => "stl",
            Self::Json => "json",
        }
    }

    /// The name of the dialog's file type filter.
    pub fn filter_name(self) -> &'static str {
        match self {
            Self::ThreeMf => "3MF model",
            Self::Stl => "STL mesh",
            Self::Json => "JSON document",
        }
    }

    /// The save dialog's title.
    pub fn dialog_title(self) -> &'static str {
        match self {
            Self::ThreeMf => "Save the 3MF file",
            Self::Stl => "Save the STL file",
            Self::Json => "Save the JSON file",
        }
    }
}

/// What the page opens: an SVG drawing for the artwork, a fit profile or a
/// part document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    /// The artwork of a part (relief.svg).
    Svg,
    /// A fit profile, the JSON file `openbrick calibrate record` writes.
    FitProfile,
    /// A part document (`openbrick/part@1`, `.obpart.json`), opened in the
    /// designer.
    Part,
}

impl OpenKind {
    /// The kind named by the page (`svg`, `fit`).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "svg" => Ok(Self::Svg),
            "fit" => Ok(Self::FitProfile),
            "part" => Ok(Self::Part),
            other => Err(format!(
                "cannot open a file of kind {other:?}; the desktop app opens svg, fit or part"
            )),
        }
    }

    /// The extensions the dialog offers, lower case, without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Svg => &["svg"],
            // A part is `name.obpart.json`; dialogs filter on the last extension.
            Self::FitProfile | Self::Part => &["json"],
        }
    }

    /// The file the scripted dialogs of the debug build pick for this kind
    /// (`TEST_DIALOGS_ENV` in src/app.rs), in the folder they are given.
    pub fn test_file_name(self) -> &'static str {
        match self {
            Self::Svg => "open.svg",
            Self::FitProfile => "open.json",
            Self::Part => "open.obpart.json",
        }
    }

    /// The name of the dialog's file type filter.
    pub fn filter_name(self) -> &'static str {
        match self {
            Self::Svg => "SVG drawing",
            Self::FitProfile => "Fit profile (JSON)",
            Self::Part => "Part document (.obpart.json)",
        }
    }

    /// The open dialog's title.
    pub fn dialog_title(self) -> &'static str {
        match self {
            Self::Svg => "Choose an SVG drawing",
            Self::FitProfile => "Choose a fit profile",
            Self::Part => "Open a part document",
        }
    }
}

/// The file name the save dialog suggests for `name` (the page's download
/// name, such as `ndn.logo-tile-2x4-emboss-neuraldeep.3mf`).
///
/// Only the last path component is kept; characters that no file system of
/// the three desktop platforms accepts, control characters, leading dots (a
/// hidden file) and trailing dots and spaces (Windows drops them) are removed;
/// a Windows device name (`CON`, `NUL`, `COM1` ...) gets a prefix; the name is
/// cut to [`MAX_NAME_CHARS`]; and it ends in the format's extension.
pub fn suggested_file_name(name: &str, format: SaveFormat) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let ext = format.extension();
    let mut stem = cleaned
        .trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' '])
        .to_string();
    if has_extension(&stem, ext) {
        stem.truncate(stem.len() - ext.len() - 1);
        stem = stem.trim_end_matches(['.', ' ']).to_string();
    }
    if stem.is_empty() {
        stem = FALLBACK_STEM.to_string();
    }
    if is_windows_device_name(&stem) {
        stem = format!("{FALLBACK_STEM}-{stem}");
    }
    let room = MAX_NAME_CHARS - ext.len() - 1;
    if stem.chars().count() > room {
        stem = stem.chars().take(room).collect();
        stem = stem.trim_end_matches(['.', ' ']).to_string();
    }
    format!("{stem}.{ext}")
}

/// Whether `name` ends in `.{ext}`, ignoring case.
fn has_extension(name: &str, ext: &str) -> bool {
    let n = name.len();
    n > ext.len() + 1
        && name.is_char_boundary(n - ext.len() - 1)
        && name[n - ext.len() - 1..].eq_ignore_ascii_case(&format!(".{ext}"))
}

/// Windows reserves these names (with any extension) for devices.
fn is_windows_device_name(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit())
}

/// The path to write for the path the save dialog returned.
///
/// Some dialogs (GTK, Windows with "All files") let the user type a name
/// without the extension; then the extension is added, so a slicer recognises
/// the file. The second value is true when it was added: the dialog asked
/// about replacing the name the user typed, not this one, so the caller must
/// not replace an existing file under the new name ([`check_save_target`]).
pub fn with_format_extension(path: &Path, format: SaveFormat) -> (PathBuf, bool) {
    let ext = format.extension();
    let matches = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if matches {
        return (path.to_path_buf(), false);
    }
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".{ext}"));
    (path.with_file_name(name), true)
}

/// Refuse to write `path` when the extension was added after the dialog
/// ([`with_format_extension`]) and a file of that name exists: the user never
/// confirmed replacing it.
pub fn check_save_target(path: &Path, extension_added: bool) -> Result<(), String> {
    if extension_added && path.exists() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(format!(
            "{name} already exists and was not replaced. Save again and choose {name} in the \
             dialog to replace it, or type another name"
        ));
    }
    Ok(())
}

/// Decode the percent-encoded file name of [`NAME_HEADER`].
pub fn decode_name(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("the file name {value:?} has a broken % escape"))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("the file name {value:?} is not UTF-8"))
}

/// Read the regular file at `path` if it has at most `limit` bytes.
pub fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let shown = path.display();
    let meta = fs::metadata(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
    if !meta.is_file() {
        return Err(format!(
            "{shown} is not a regular file (a folder, device or pipe); choose a file"
        ));
    }
    let too_big = |size: u64| format!("{shown} is {size} bytes; the limit is {limit} bytes");
    if meta.len() > limit {
        return Err(too_big(meta.len()));
    }
    let file = File::open(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    // The file may grow between the size check and the read.
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {shown}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(too_big(bytes.len() as u64));
    }
    Ok(bytes)
}

/// Write `bytes` to `path` so that a failed save never leaves half a file: the
/// bytes go to a new temporary file in the same folder, which then replaces
/// `path` in one rename.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let shown = path.display();
    let name = path
        .file_name()
        .ok_or_else(|| format!("cannot save to {shown}: it names no file"))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(".{}.openbrick-partial", std::process::id()));
    let temp = path.with_file_name(temp_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("cannot save {shown}: {e}")
    })
}

/// The file name of `path` as text, for the page's status line.
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder under the system temporary folder.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openbrick-desktop-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn formats_and_kinds_parse_only_what_the_page_sends() {
        assert_eq!(SaveFormat::parse("3mf"), Ok(SaveFormat::ThreeMf));
        assert_eq!(SaveFormat::parse("stl"), Ok(SaveFormat::Stl));
        assert_eq!(SaveFormat::parse("json"), Ok(SaveFormat::Json));
        for bad in ["", "3MF", "exe", "../3mf", "svg"] {
            assert!(SaveFormat::parse(bad).is_err(), "{bad:?} parsed");
        }
        assert_eq!(OpenKind::parse("svg"), Ok(OpenKind::Svg));
        assert_eq!(OpenKind::parse("fit"), Ok(OpenKind::FitProfile));
        assert_eq!(OpenKind::parse("part"), Ok(OpenKind::Part));
        for bad in ["", "json", "SVG", "any", "obpart"] {
            assert!(OpenKind::parse(bad).is_err(), "{bad:?} parsed");
        }
        assert_eq!(OpenKind::Svg.extensions(), ["svg"]);
        assert_eq!(OpenKind::FitProfile.extensions(), ["json"]);
        assert_eq!(OpenKind::Part.extensions(), ["json"]);
        // Each kind has its own scripted file, so a test folder can hold all three.
        let names =
            [OpenKind::Svg, OpenKind::FitProfile, OpenKind::Part].map(OpenKind::test_file_name);
        assert_eq!(names, ["open.svg", "open.json", "open.obpart.json"]);
    }

    #[test]
    fn page_download_names_pass_unchanged() {
        for (name, format) in [
            (
                "ndn.logo-tile-2x4-emboss-neuraldeep.3mf",
                SaveFormat::ThreeMf,
            ),
            (
                "ndn.logo-tile-2x4-inlay-parts-neuraldeep.3mf",
                SaveFormat::ThreeMf,
            ),
            (
                "ndn.logo-tile-4x4-emboss-neuraldeep-badge.stl",
                SaveFormat::Stl,
            ),
            ("coupon-fdm-0.4-cross.3mf", SaveFormat::ThreeMf),
            ("fit-prusa-mk4s-petg.json", SaveFormat::Json),
        ] {
            assert_eq!(suggested_file_name(name, format), name);
        }
    }

    #[test]
    fn suggested_names_lose_paths_and_reserved_characters() {
        let f = SaveFormat::ThreeMf;
        assert_eq!(suggested_file_name("../../etc/passwd", f), "passwd.3mf");
        assert_eq!(suggested_file_name("C:\\Users\\x\\tile.3mf", f), "tile.3mf");
        assert_eq!(
            suggested_file_name("a<b>c:d\"e|f?g*h.3mf", f),
            "abcdefgh.3mf"
        );
        assert_eq!(suggested_file_name("tab\there\n.3mf", f), "tabhere.3mf");
        assert_eq!(suggested_file_name("..hidden", f), "hidden.3mf");
        assert_eq!(suggested_file_name("trailing. . ", f), "trailing.3mf");
        assert_eq!(suggested_file_name("", f), "openbrick.3mf");
        assert_eq!(suggested_file_name("...", f), "openbrick.3mf");
        assert_eq!(suggested_file_name("dir/", f), "openbrick.3mf");
        assert_eq!(suggested_file_name("tile.3MF", f), "tile.3mf");
        assert_eq!(suggested_file_name("tile.stl", f), "tile.stl.3mf");
        assert_eq!(suggested_file_name("Ünïcödé tile", f), "Ünïcödé tile.3mf");
    }

    #[test]
    fn suggested_names_avoid_windows_device_names() {
        let f = SaveFormat::Stl;
        assert_eq!(suggested_file_name("CON", f), "openbrick-CON.stl");
        assert_eq!(suggested_file_name("nul.stl", f), "openbrick-nul.stl");
        assert_eq!(suggested_file_name("com7", f), "openbrick-com7.stl");
        assert_eq!(suggested_file_name("LPT1.txt", f), "openbrick-LPT1.txt.stl");
        assert_eq!(suggested_file_name("console", f), "console.stl");
        assert_eq!(suggested_file_name("COM10", f), "COM10.stl");
    }

    #[test]
    fn suggested_names_are_cut_to_the_limit_on_a_character_boundary() {
        let long = "é".repeat(300);
        let name = suggested_file_name(&long, SaveFormat::Json);
        assert_eq!(name.chars().count(), MAX_NAME_CHARS);
        assert!(name.ends_with(".json"));
    }

    #[test]
    fn the_extension_is_added_only_when_missing() {
        let f = SaveFormat::ThreeMf;
        assert_eq!(
            with_format_extension(Path::new("/x/tile.3mf"), f),
            (PathBuf::from("/x/tile.3mf"), false)
        );
        assert_eq!(
            with_format_extension(Path::new("/x/tile.3MF"), f),
            (PathBuf::from("/x/tile.3MF"), false)
        );
        assert_eq!(
            with_format_extension(Path::new("/x/tile"), f),
            (PathBuf::from("/x/tile.3mf"), true)
        );
        assert_eq!(
            with_format_extension(Path::new("/x/tile.stl"), f),
            (PathBuf::from("/x/tile.stl.3mf"), true)
        );
        assert_eq!(
            with_format_extension(Path::new("/x/v1.2"), SaveFormat::Json),
            (PathBuf::from("/x/v1.2.json"), true)
        );
    }

    #[test]
    fn an_added_extension_never_replaces_an_existing_file() {
        let dir = temp_dir("target");
        let existing = dir.join("tile.3mf");
        fs::write(&existing, b"keep").unwrap();
        // The user typed "tile" and confirmed nothing about tile.3mf.
        let (path, added) = with_format_extension(&dir.join("tile"), SaveFormat::ThreeMf);
        assert!(added);
        let err = check_save_target(&path, added).unwrap_err();
        assert!(err.contains("tile.3mf already exists"), "{err}");
        // The user chose tile.3mf in the dialog, which asked about replacing it.
        let (path, added) = with_format_extension(&existing, SaveFormat::ThreeMf);
        assert!(check_save_target(&path, added).is_ok());
        // A new name is fine either way.
        let (path, added) = with_format_extension(&dir.join("new"), SaveFormat::ThreeMf);
        assert!(check_save_target(&path, added).is_ok());
        assert_eq!(fs::read(&existing).unwrap(), b"keep");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_decode_from_the_header() {
        assert_eq!(decode_name("tile.3mf").unwrap(), "tile.3mf");
        assert_eq!(
            decode_name("ndn.logo-tile-2x4%20copy.3mf").unwrap(),
            "ndn.logo-tile-2x4 copy.3mf"
        );
        assert_eq!(decode_name("%C3%9Cber.stl").unwrap(), "Über.stl");
        assert!(decode_name("bad%2").is_err());
        assert!(decode_name("bad%zz").is_err());
        assert!(decode_name("%FF.stl").is_err());
    }

    #[test]
    fn reads_are_bounded_and_only_of_regular_files() {
        let dir = temp_dir("read");
        let file = dir.join("mark.svg");
        fs::write(&file, b"<svg/>").unwrap();
        assert_eq!(read_limited(&file, 6).unwrap(), b"<svg/>");
        let err = read_limited(&file, 5).unwrap_err();
        assert!(err.contains("is 6 bytes; the limit is 5 bytes"), "{err}");
        let err = read_limited(&dir, 1024).unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
        assert!(read_limited(&dir.join("missing.svg"), 1024).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writes_replace_the_file_whole_and_leave_no_partial_file() {
        let dir = temp_dir("write");
        let path = dir.join("tile.3mf");
        write_atomically(&path, b"first").unwrap();
        write_atomically(&path, b"second, longer").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second, longer");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["tile.3mf"]);
        // A folder that does not exist fails without leaving anything behind.
        let err = write_atomically(&dir.join("missing").join("tile.3mf"), b"x").unwrap_err();
        assert!(err.starts_with("cannot save"), "{err}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn display_names_are_the_file_name() {
        assert_eq!(display_name(Path::new("/home/u/tile.3mf")), "tile.3mf");
    }
}
