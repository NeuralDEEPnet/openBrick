//! The calibration coupon in the browser (docs/plan/05 section 4): the cell
//! layout, the coupon mesh and its 3MF or STL. The layout argument is
//! `openbrick_part::coupon::CouponOptions` as JSON (every field optional, empty
//! text for the defaults); there is no fit profile, because the coupon measures
//! the printer and is never compensated.

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_io::{write_3mf, write_stl_binary, ExportMeta, ExportObject};
use openbrick_part::coupon::{self, Coupon, CouponOptions};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::layout::{color_of, role_colors};
use crate::part::process_profile;
use crate::{CoreError, MeshOut};

/// The coupon's part id in exports, as the coupon example writes it.
pub const COUPON_PART_ID: &str = "openbrick.calibration-coupon";

fn options(layout_json: &str) -> Result<CouponOptions, CoreError> {
    if layout_json.trim().is_empty() {
        return Ok(CouponOptions::default());
    }
    serde_json::from_str(layout_json).map_err(|e| {
        CoreError(format!(
            "the coupon layout is not valid ({e}); it takes pockets (cross, crossAndDiagonal or full), copies, bedMm and bedMarginMm"
        ))
    })
}

/// `crossAndDiagonal` as `cross-and-diagonal`: the file-name form the coupon
/// example uses.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn build(process_id: &str, layout_json: &str) -> Result<Coupon, CoreError> {
    let process = process_profile(process_id)?;
    let opts = options(layout_json)?;
    coupon::build_coupon(&ManifoldRust, &process, &opts).map_err(|e| CoreError(e.to_string()))
}

/// A built coupon for the page. In JavaScript this is a class: take the mesh
/// once, read the JSON fields, then call `free()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct CouponOut {
    mesh: Option<MeshOut>,
    cells_json: String,
    summary_json: String,
    estimate_json: String,
    warnings_json: String,
    content_hash: String,
    kernel: String,
    file_stem: String,
    triangles: u32,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl CouponOut {
    /// Move the coupon mesh out (one `body` object); `None` once taken.
    pub fn take_mesh(&mut self) -> Option<MeshOut> {
        self.mesh.take()
    }

    /// Every cell (`CouponCellLayout`: row, copy, cell id, value, the pocket's
    /// rib depth in `cross`, the label printed on it, centre in mm and LDU).
    pub fn cells_json(&self) -> String {
        self.cells_json.clone()
    }

    /// `CouponSummary`: process, pocket set, copies, footprint, height, cells
    /// not built yet.
    pub fn summary_json(&self) -> String {
        self.summary_json.clone()
    }

    /// `PrintEstimate`: volume, PLA mass, assumed flow and time.
    pub fn estimate_json(&self) -> String {
        self.estimate_json.clone()
    }

    pub fn warnings_json(&self) -> String {
        self.warnings_json.clone()
    }

    pub fn content_hash(&self) -> String {
        self.content_hash.clone()
    }

    pub fn kernel(&self) -> String {
        self.kernel.clone()
    }

    /// `coupon-<process>-<pockets>`, the name the coupon example gives its files.
    pub fn file_stem(&self) -> String {
        self.file_stem.clone()
    }

    pub fn triangle_count(&self) -> u32 {
        self.triangles
    }
}

fn file_stem(c: &Coupon) -> String {
    format!(
        "coupon-{}-{}",
        c.summary.process,
        kebab(c.summary.pockets.name())
    )
}

/// Build the coupon for a bundled process profile and a layout (see the module
/// documentation). The default fdm-0.4 coupon is 118.4 x 160.8 x 3.2 mm.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn build_coupon(process_id: &str, layout_json: &str) -> Result<CouponOut, CoreError> {
    let c = build(process_id, layout_json)?;
    let stem = file_stem(&c);
    let mut built = c.built;
    let object = built.objects.swap_remove(0);
    let triangles = u32::try_from(object.mesh.triangle_count()).unwrap_or(u32::MAX);
    Ok(CouponOut {
        mesh: Some(MeshOut::from_mesh(
            built.kernel_id.clone(),
            object.mesh,
            object.volume_mm3,
            &object.role,
        )?),
        cells_json: json(&c.cells),
        summary_json: json(&c.summary),
        estimate_json: json(&c.estimate),
        warnings_json: json(&built.warnings),
        content_hash: built.content_hash,
        kernel: built.kernel_id,
        file_stem: stem,
        triangles,
    })
}

/// JSON text of coupon data; every type here serialises.
fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("coupon data serialises")
}

/// The cells and summary of a coupon without building it (fast):
/// `{ cells, summary }`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn coupon_layout_json(process_id: &str, layout_json: &str) -> Result<String, CoreError> {
    let process = process_profile(process_id)?;
    let opts = options(layout_json)?;
    let layout = coupon::coupon_layout(&process, &opts).map_err(|e| CoreError(e.to_string()))?;
    Ok(serde_json::to_string(&layout).expect("the layout serialises"))
}

/// The coupon as a printable file: `format` is `3mf` or `stl`, `colors_json` as
/// for [`crate::export_part`] (only `body` applies). It prints as built, face
/// up, and its metadata names no fit profile (`none`).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn export_coupon(
    format: &str,
    process_id: &str,
    layout_json: &str,
    colors_json: &str,
) -> Result<Vec<u8>, CoreError> {
    if format != "3mf" && format != "stl" {
        return Err(CoreError(format!(
            "unknown export format {format:?}; use \"3mf\" or \"stl\""
        )));
    }
    let colors = role_colors(colors_json)?;
    let c = build(process_id, layout_json)?;
    let s = &c.summary;
    let object = &c.built.objects[0];
    let pockets = kebab(s.pockets.name());
    let objects = [ExportObject::new(
        "calibration coupon",
        object.role.clone(),
        Some(color_of(&colors, &object.role)),
        &object.mesh,
    )];
    let bytes = if format == "3mf" {
        let meta = ExportMeta {
            part_id: COUPON_PART_ID.into(),
            version: format!("{}.{}.0", s.coupon_version, s.coupon_revision),
            content_hash: c.built.content_hash.clone(),
            // The coupon measures the printer and is never compensated.
            fit_profile_id: "none".into(),
            process_id: s.process.clone(),
            generator: concat!(
                env!("CARGO_PKG_NAME"),
                " ",
                env!("CARGO_PKG_VERSION"),
                " (web playground)"
            )
            .into(),
            kernel_id: c.built.kernel_id.clone(),
            title: format!(
                "Calibration coupon v{} ({}, {} pockets)",
                s.coupon_version, s.process, pockets
            ),
            serial: None,
        };
        write_3mf(&objects, &meta)
    } else {
        write_stl_binary(&objects)
    };
    bytes.map_err(|e| CoreError(format!("the {format} writer refused the coupon: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_part::coupon::{CouponCellLayout, CouponSummary, PocketSet, PrintEstimate};

    #[test]
    fn the_default_coupon_matches_the_generator() {
        let mut out = build_coupon("fdm-0.4", "").expect("builds");
        let native = coupon::build_coupon(
            &ManifoldRust,
            &process_profile("fdm-0.4").unwrap(),
            &CouponOptions::default(),
        )
        .unwrap();
        assert_eq!(out.content_hash(), native.built.content_hash);
        assert_eq!(out.file_stem(), "coupon-fdm-0.4-cross");
        let cells: Vec<CouponCellLayout> = serde_json::from_str(&out.cells_json()).unwrap();
        assert_eq!(cells, native.cells);
        let summary: CouponSummary = serde_json::from_str(&out.summary_json()).unwrap();
        assert_eq!(summary, native.summary);
        let estimate: PrintEstimate = serde_json::from_str(&out.estimate_json()).unwrap();
        assert_eq!(estimate, native.estimate);
        let mut mesh = out.take_mesh().expect("mesh");
        assert!(out.take_mesh().is_none());
        let n = &native.built.objects[0].mesh;
        assert_eq!(mesh.triangle_count() as usize, n.triangle_count());
        assert_eq!(out.triangle_count(), mesh.triangle_count());
        assert_eq!(mesh.fingerprint(), crate::hex(n.fingerprint()));
        assert_eq!(mesh.role(), "body");
        assert_eq!(mesh.take_indices(), n.indices);
        // The footprint the plan quotes, from the standard's module, not typed in.
        let b = mesh.bounds();
        let size = [b[3] - b[0], b[4] - b[1]].map(f64::from);
        for (got, want) in size.iter().zip(summary.footprint_mm) {
            assert!(
                (got - want).abs() < 1e-4,
                "{size:?} vs {:?}",
                summary.footprint_mm
            );
        }
        let layout: serde_json::Value =
            serde_json::from_str(&coupon_layout_json("fdm-0.4", "").unwrap()).unwrap();
        assert_eq!(
            layout["cells"],
            serde_json::to_value(&native.cells).unwrap()
        );
    }

    #[test]
    fn layouts_and_formats() {
        let full = r#"{"pockets":"full","bedMm":[256,256]}"#;
        let layout: serde_json::Value =
            serde_json::from_str(&coupon_layout_json("fdm-0.4", full).unwrap()).unwrap();
        assert_eq!(layout["summary"]["pockets"], "full");
        assert_eq!(
            kebab(PocketSet::CrossAndDiagonal.name()),
            "cross-and-diagonal"
        );
        // The full cross does not fit the default bed: the generator says so.
        let e = coupon_layout_json("fdm-0.4", r#"{"pockets":"full"}"#).unwrap_err();
        assert!(!e.0.is_empty());
        // A fit profile is refused: the coupon is never compensated.
        let e = coupon_layout_json("fdm-0.4", r#"{"fit":"example"}"#).unwrap_err();
        assert!(e.0.contains("coupon layout is not valid"), "{e}");
        assert!(coupon_layout_json("nope", "").is_err());
        assert!(export_coupon("obj", "fdm-0.4", "", "").is_err());
    }

    #[test]
    fn the_coupon_exports() {
        let threemf = export_coupon("3mf", "fdm-0.4", "", "").unwrap();
        assert_eq!(&threemf[..4], b"PK\x03\x04");
        let stl = export_coupon("stl", "fdm-0.4", "", "").unwrap();
        let out = build_coupon("fdm-0.4", "").unwrap();
        assert_eq!(stl.len(), 84 + 50 * out.triangle_count() as usize);
    }
}
