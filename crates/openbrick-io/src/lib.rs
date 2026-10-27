//! File formats (docs/plan/08): `.obrick`, `.obpart.json`, `.obprofile.json`, LDraw,
//! 3MF, STL, glTF and mesh import sanitising. Writers land in M1 (3MF, STL) and
//! M3 (LDraw round-trip, `.obrick`).

/// Document kinds and their extensions.
pub const EXTENSIONS: &[(&str, &str)] = &[
    ("model", ".obrick"),
    ("part", ".obpart.json"),
    ("process", ".obprofile.json"),
    ("fit", ".obprofile.json"),
    ("run", ".obrun.json"),
    ("pack", ".obpack"),
];

/// Limits applied by every zip reader (SECURITY.md).
pub const ZIP_MAX_INFLATE_BYTES: u64 = 200 * 1024 * 1024;
pub const ZIP_MAX_ENTRIES: usize = 10_000;
/// Limits for imported meshes.
pub const MESH_MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const MESH_MAX_TRIANGLES: usize = 2_000_000;

/// Reject zip entry names that could escape the extraction root.
pub fn zip_entry_name_is_safe(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.starts_with('\\')
        && !name.contains("..")
        && !name.contains(':')
        && !name.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names() {
        assert!(zip_entry_name_is_safe("model.json"));
        assert!(zip_entry_name_is_safe("parts/abc.obpart.json"));
        assert!(!zip_entry_name_is_safe("../etc/passwd"));
        assert!(!zip_entry_name_is_safe("/abs"));
        assert!(!zip_entry_name_is_safe("C:\\x"));
    }
}
