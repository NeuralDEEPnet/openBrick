//! WebAssembly facade over the OpenBrick3D core (docs/plan/02 sections 2 and 3).
//!
//! The browser runs this module in a Web Worker (`apps/web/src/worker.js`) and the
//! page only displays what comes back: JavaScript never computes geometry or
//! connectivity (CLAUDE.md).
//!
//! Every export is an ordinary Rust item. On wasm32 the same items carry
//! `#[wasm_bindgen]` through `cfg_attr`, so native `cargo test` and `clippy` cover
//! exactly the functions the browser calls, and native builds do not link
//! wasm-bindgen at all. JavaScript sees the Rust names (snake_case); `packages/
//! core-wasm` may wrap them in camelCase later. A [`CoreError`] becomes a
//! JavaScript `Error` with the same message.
//!
//! Build for the browser with `cargo xtask wasm`, which runs the pinned
//! wasm-bindgen CLI and writes the module and its glue into `apps/web/src/wasm/`.
//!
//! # Adding an export
//!
//! Small inputs and outputs cross as JSON text and numbers; meshes cross as
//! [`MeshOut`]. Its arrays are moved out of Rust ([`MeshOut::take_positions`],
//! [`MeshOut::take_indices`]), so each is copied exactly once, from WebAssembly
//! memory into a JavaScript `Float32Array` or `Uint32Array` (that memory cannot be
//! handed over itself), and the worker then transfers those to the page without
//! another copy (docs/plan/02 section 3).
//! `evaluate_part(doc, params, process, fit, assets)` arrives with the part
//! evaluator (docs/plan/06): it takes the part document, params, process profile
//! and fit profile as JSON text and the assets as bytes, and returns one
//! [`MeshOut`] per colour role (built with [`MeshOut::from_solid`], as
//! [`build_nominal_brick`] does) plus the validation report as JSON.

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::nominal::{nominal_brick, Detail, NominalDims};
use openbrick_geom::{Kernel, Solid};
use openbrick_units::STANDARD;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

/// Largest footprint [`build_nominal_brick`] accepts in either direction. The
/// pure-Rust kernel's batch union grows faster than linearly with part size (Q01
/// finding 2 in docs/spikes/q01-kernel-conformance.md), and a 16x16 brick already
/// takes tens of seconds, so larger requests are refused rather than left to hang
/// the worker.
pub const MAX_STUDS: u32 = 16;

/// An error reported to JavaScript as an `Error` with this message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreError(pub String);

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CoreError {}

#[cfg(target_arch = "wasm32")]
impl From<CoreError> for wasm_bindgen::JsValue {
    fn from(e: CoreError) -> Self {
        wasm_bindgen::JsError::new(&e.0).into()
    }
}

/// Identifier and version of the geometry kernel, as used in content hashes. A mesh
/// fingerprint is only meaningful together with it.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn kernel_id() -> String {
    ManifoldRust.id()
}

/// The Dimensional Standard as JSON for the UI.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn standard_json() -> String {
    serde_json::to_string(openbrick_units::STANDARD.get()).expect("standard serialises")
}

/// Parse a part document and return its structural errors as JSON (empty array when valid).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn check_part_json(text: &str) -> String {
    match openbrick_part::PartDoc::parse(text) {
        Ok(_) => "[]".to_string(),
        Err(e) => {
            serde_json::to_string(&vec![e.to_string()]).unwrap_or_else(|_| "[\"error\"]".into())
        }
    }
}

/// [`MAX_STUDS`], for the UI's input limits.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn max_studs() -> u32 {
    MAX_STUDS
}

/// A triangle mesh for display, in the kernel frame: millimetres, right-handed, +Z
/// up. Triangles are counter-clockwise seen from outside. In JavaScript this is a
/// class; take its arrays once, then call `free()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Clone, Debug, PartialEq)]
pub struct MeshOut {
    positions: Vec<f32>,
    indices: Vec<u32>,
    /// Counts at construction; they stay valid after the arrays are taken.
    vertex_count: u32,
    triangle_count: u32,
    volume_mm3: f64,
    fingerprint: u64,
    canonical_fingerprint: u64,
    bounds: [f32; 6],
    kernel: String,
}

/// `0x` and 16 lower-case hex digits, as `kernel_timing` and the harness manifest
/// print fingerprints. JavaScript has no exact 64-bit number, so they cross as text.
fn hex(x: u64) -> String {
    format!("0x{x:016x}")
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl MeshOut {
    /// Move the vertex positions out: x, y, z in millimetres (a `Float32Array` in
    /// JavaScript). The array is not cloned; a second call returns an empty one.
    pub fn take_positions(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.positions)
    }

    /// Move the triangle vertex indices out, three per triangle (a `Uint32Array` in
    /// JavaScript). The array is not cloned; a second call returns an empty one.
    pub fn take_indices(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.indices)
    }

    pub fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.triangle_count
    }

    /// Volume of the solid in mm³, from the kernel (not from the f32 mesh).
    pub fn volume_mm3(&self) -> f64 {
        self.volume_mm3
    }

    /// `Mesh::fingerprint` (FNV-1a over positions and indices) as `0x` + 16 hex
    /// digits; equals the native value for the same kernel and inputs.
    pub fn fingerprint(&self) -> String {
        hex(self.fingerprint)
    }

    /// `Mesh::canonical_fingerprint` (independent of vertex numbering and triangle
    /// order) as `0x` + 16 hex digits.
    pub fn canonical_fingerprint(&self) -> String {
        hex(self.canonical_fingerprint)
    }

    /// Axis-aligned bounds of the positions: min x, y, z, then max x, y, z.
    pub fn bounds(&self) -> Vec<f32> {
        self.bounds.to_vec()
    }

    /// The kernel that built the mesh ([`kernel_id`]).
    pub fn kernel(&self) -> String {
        self.kernel.clone()
    }
}

impl MeshOut {
    /// Convert a kernel solid. Refuses a solid the kernel does not report as
    /// manifold, so nothing broken reaches the display or an export.
    pub fn from_solid<K: Kernel>(k: &K, solid: &K::Solid) -> Result<MeshOut, CoreError> {
        if !solid.is_manifold() {
            return Err(CoreError(format!(
                "{} returned a solid that is not manifold",
                k.id()
            )));
        }
        let mesh = solid.to_mesh();
        let (lo, hi) = mesh.bounds();
        let count = |n: usize, what: &str| {
            u32::try_from(n).map_err(|_| CoreError(format!("the mesh has too many {what} ({n})")))
        };
        Ok(MeshOut {
            vertex_count: count(mesh.positions.len(), "vertices")?,
            triangle_count: count(mesh.triangle_count(), "triangles")?,
            volume_mm3: solid.volume_mm3(),
            fingerprint: mesh.fingerprint(),
            canonical_fingerprint: mesh.canonical_fingerprint(),
            bounds: [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]],
            kernel: k.id(),
            // Both arrays are moved, not copied: `into_flattened` reuses the
            // allocation of the `[f32; 3]` vector.
            positions: mesh.positions.into_flattened(),
            indices: mesh.indices,
        })
    }
}

/// Build a nominal brick `width_studs` (x) by `length_studs` (y) from the
/// Dimensional Standard: shell, roof, studs, and tubes or pins underneath, with no
/// fit offsets. Same construction as LOD0 of the render harness
/// (`openbrick_geom::nominal`), so 2x4 has the fingerprint of the native
/// `kernel_timing` 2x4 row. The body's minimum corner is at the origin; the brick's
/// lattice cell corner is one undersize (`undersize_per_side`) further out.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn build_nominal_brick(width_studs: u32, length_studs: u32) -> Result<MeshOut, CoreError> {
    for (what, n) in [("width", width_studs), ("length", length_studs)] {
        if !(1..=MAX_STUDS).contains(&n) {
            return Err(CoreError(format!(
                "brick {what} must be 1 to {MAX_STUDS} studs, got {n}"
            )));
        }
    }
    let k = ManifoldRust;
    let d = NominalDims::from_standard(STANDARD.get());
    let solid = nominal_brick(&k, &d, [width_studs, length_studs], &Detail::full())
        .map_err(|e| CoreError(e.to_string()))?;
    MeshOut::from_solid(&k, &solid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_geom::nominal::nominal_volume;

    #[test]
    fn kernel_and_standard_are_reported() {
        assert!(kernel_id().starts_with("manifold-rust "), "{}", kernel_id());
        let standard: serde_json::Value = serde_json::from_str(&standard_json()).unwrap();
        let module = standard["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == "module")
            .expect("module row");
        assert_eq!(
            module["nominal_mm"].as_f64(),
            Some(STANDARD.get().mm("module"))
        );
    }

    #[test]
    fn check_part_json_reports_structural_errors() {
        let logo = include_str!("../../../parts/ndn.logo-tile.obpart.json");
        assert_eq!(check_part_json(logo), "[]");
        let errors: Vec<String> = serde_json::from_str(&check_part_json("{}")).unwrap();
        assert_eq!(errors.len(), 1, "{errors:?}");
    }

    #[test]
    fn nominal_2x4_is_the_shared_construction() {
        let mut out = build_nominal_brick(2, 4).expect("2x4");
        let d = NominalDims::from_standard(STANDARD.get());
        let solid = nominal_brick(&ManifoldRust, &d, [2, 4], &Detail::full()).unwrap();
        let mesh = solid.to_mesh();
        assert_eq!(out.triangle_count() as usize, mesh.triangle_count());
        assert_eq!(out.vertex_count() as usize, mesh.positions.len());
        let positions = out.take_positions();
        let indices = out.take_indices();
        assert_eq!(positions, mesh.positions.as_flattened());
        assert_eq!(indices, mesh.indices);
        assert!(indices.iter().all(|&i| i < out.vertex_count()));
        // Taking moves the arrays out; the counts and numbers stay.
        assert!(out.take_positions().is_empty() && out.take_indices().is_empty());
        assert_eq!(out.triangle_count() as usize, mesh.triangle_count());
        assert_eq!(out.vertex_count() as usize, mesh.positions.len());
        assert_eq!(out.fingerprint(), hex(mesh.fingerprint()));
        assert_eq!(
            out.canonical_fingerprint(),
            hex(mesh.canonical_fingerprint())
        );
        assert_eq!(out.kernel(), kernel_id());
        let expected = nominal_volume(&d, [2, 4], &Detail::full());
        assert!(((out.volume_mm3() - expected) / expected).abs() < 1e-6);
        let [w, depth] = d.body([2, 4]);
        let want = [0.0, 0.0, 0.0, w, depth, d.height + d.stud_height];
        for (got, want) in out.bounds().iter().zip(want) {
            assert!((*got as f64 - want).abs() < 1e-5, "{got} vs {want}");
        }
    }

    /// The value the browser smoke test (apps/web/smoke.mjs) compares with the
    /// native `kernel_timing` 2x4 row and the harness LOD0 manifest entry.
    #[test]
    #[cfg_attr(
        not(all(target_arch = "x86_64", target_os = "linux")),
        ignore = "bit-exact output is verified on x86-64 Linux only (docs/spikes/q01-kernel-conformance.md)"
    )]
    fn nominal_2x4_has_the_harness_fingerprint() {
        let out = build_nominal_brick(2, 4).expect("2x4");
        assert_eq!(out.triangle_count(), 3620);
        assert_eq!(out.fingerprint(), "0xfa969a6c0bf657e7");
        assert_eq!(out.canonical_fingerprint(), "0xf3ee26e7e06f4b52");
    }

    #[test]
    fn footprints_outside_the_limits_are_refused() {
        assert_eq!(max_studs(), MAX_STUDS);
        for (w, l) in [(0, 4), (4, 0), (MAX_STUDS + 1, 1), (1, u32::MAX)] {
            let err = build_nominal_brick(w, l).unwrap_err();
            assert!(
                err.0.contains(&format!("1 to {MAX_STUDS} studs")),
                "{w}x{l}: {err}"
            );
        }
        // The smallest footprint builds (one stud, no underside).
        assert!(build_nominal_brick(1, 1).unwrap().triangle_count() > 0);
    }
}
