//! File formats (docs/plan/08): `.obrick`, `.obpart.json`, `.obprofile.json`, LDraw,
//! 3MF, STL, glTF and mesh import sanitising. The 3MF and binary STL writers landed
//! in M1 (backlog items 12 and 13 in docs/plan/14); LDraw round-trip and `.obrick`
//! follow in M3.
//!
//! The writers take meshes in millimetres, Z up, as the kernel produces them, and
//! return the file as bytes: the core API never touches a filesystem, so the same
//! code runs in the CLI, the desktop app and WebAssembly.
//!
//! - [`write_3mf`]: a 3MF package (Core 1.4.0, colour groups from Materials and
//!   Properties 1.2.1, UUIDs from Production 1.1.2), one object per colour role,
//!   byte-identical for identical input (docs/plan/05 section 7, decision D18).
//!   [`write_3mf_with_layout`] chooses an [`ExportLayout`]: one build item per
//!   object, or one object whose parts are the colour roles
//!   ([`ExportLayout::PART_DESIGNER`], for multi-material printers; see
//!   docs/spikes/q18-slicer-multimaterial-import.md).
//! - [`write_stl_binary`] and [`write_stl_binary_per_role`]: legacy binary STL,
//!   without colour (docs/plan/08 section 5).

mod export;
pub mod stl;
pub mod threemf;
mod xml;
mod zip_writer;

pub use export::{EdgeProblem, ExportMeta, ExportObject, IoError, GENERATOR, ROLES};
pub use stl::{stl_binary_len, write_stl_binary, write_stl_binary_per_role};
pub use threemf::{
    threemf_model_xml, threemf_model_xml_with_layout, write_3mf, write_3mf_with_layout,
    ExportLayout,
};

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
