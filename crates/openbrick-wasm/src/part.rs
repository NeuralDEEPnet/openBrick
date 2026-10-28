//! The part designer's exports (docs/plan/06 sections 5, 11 and 13): evaluate a
//! part document with the browser's choices, check it with the validator, and
//! write it as a 3MF or STL.
//!
//! [`evaluate_part`] and [`export_part`] take the same inputs: the part document
//! (JSON text), the parameter values (a JSON object of overrides, empty text for
//! none), the id of a bundled process profile, a fit profile (JSON text, empty
//! for none) and the assets the document names, in an [`AssetSet`]. Exporting
//! evaluates again rather than keeping a part between calls: the logo tile takes
//! tens of milliseconds, and nothing can then go stale between what the page
//! shows and what it saves.

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::{CrossSection, Kernel, Solid};
use openbrick_io::{write_3mf, write_stl_binary, ExportMeta, ExportObject};
use openbrick_part::coupon::PLA_DENSITY_G_CM3;
use openbrick_part::eval::resolve_params;
use openbrick_part::{evaluate, validate, Artwork, Built, EvalInput, MemoryAssets, ParamValues};
use openbrick_part::{PartDoc, RoleMesh};
use openbrick_print::{default_process_profiles, FitProfile, ProcessProfile, Report};
use openbrick_units::STANDARD;
use serde_json::{json, Value};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::layout::{color_of, on_bed, role_colors};
use crate::{CoreError, MeshOut};

/// Thickness of the red overlay that shows artwork which does not print as
/// drawn, laid on the face it belongs to. A display aid, never exported or
/// measured: thin enough to read as paint, thick enough not to flicker against
/// the face in a depth buffer at the viewer's camera distances.
pub const REMOVED_OVERLAY_MM: f64 = 0.02;

/// What the generator field of an export says.
const GENERATOR: &str = concat!(
    env!("CARGO_PKG_NAME"),
    " ",
    env!("CARGO_PKG_VERSION"),
    " (web playground)"
);

/// The assets a part document names (SVG artwork), by the name the document
/// uses: the logo tile's sample is `assets/mark.svg`. In JavaScript this is a
/// class: `new AssetSet()`, `add(name, bytes)` for each file, pass it to
/// [`evaluate_part`] or [`export_part`], then `free()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Clone, Debug, Default)]
pub struct AssetSet {
    assets: MemoryAssets,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl AssetSet {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> AssetSet {
        AssetSet::default()
    }

    /// Add (or replace) one asset. Refuses an empty name and files larger than
    /// [`crate::max_asset_bytes`], with a message that names the limit.
    pub fn add(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError("an asset needs a name".into()));
        }
        let max = crate::max_asset_bytes() as usize;
        if bytes.len() > max {
            return Err(CoreError(format!(
                "`{name}` is {} bytes; the limit for artwork is {max} bytes ({} MiB). Simplify the drawing or remove embedded images and fonts, then try again",
                bytes.len(),
                max / (1024 * 1024)
            )));
        }
        self.assets.files.insert(name.to_string(), bytes);
        Ok(())
    }

    /// How many assets the set holds.
    pub fn count(&self) -> u32 {
        u32::try_from(self.assets.files.len()).unwrap_or(u32::MAX)
    }
}

/// Everything one evaluation reads, parsed from the page's text.
struct PartInputs {
    doc: PartDoc,
    params: ParamValues,
    process: ProcessProfile,
    fit: Option<FitProfile>,
}

/// The bundled process profile `id`, or an error listing the ids.
pub(crate) fn process_profile(id: &str) -> Result<ProcessProfile, CoreError> {
    let all = default_process_profiles();
    let ids: Vec<String> = all.iter().map(|p| p.id.clone()).collect();
    all.into_iter().find(|p| p.id == id).ok_or_else(|| {
        CoreError(format!(
            "there is no bundled process profile `{id}`; choose one of {}",
            ids.join(", ")
        ))
    })
}

/// A fit profile from JSON text (empty for none), with messages that say what
/// kind of file was given instead.
fn fit_profile(text: &str) -> Result<Option<FitProfile>, CoreError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let hint = "load the JSON file that `openbrick calibrate record` wrote, or choose the example fit profile";
    let v: Value = serde_json::from_str(text)
        .map_err(|e| CoreError(format!("the fit profile is not valid JSON ({e}); {hint}")))?;
    match v.get("kind").and_then(Value::as_str) {
        Some("fit") => {}
        Some(other) => {
            return Err(CoreError(format!(
                "this file is a `{other}` profile, not a fit profile; {hint}"
            )))
        }
        None => {
            return Err(CoreError(format!(
                "this file has no `\"kind\": \"fit\"`, so it is not a fit profile; {hint}"
            )))
        }
    }
    serde_json::from_value(v)
        .map(Some)
        .map_err(|e| CoreError(format!("the fit profile cannot be read: {e}; {hint}")))
}

fn part_inputs(
    doc_json: &str,
    params_json: &str,
    process_id: &str,
    fit_json: &str,
) -> Result<PartInputs, CoreError> {
    let doc = PartDoc::parse(doc_json)
        .map_err(|e| CoreError(format!("the part document is not valid: {e}")))?;
    let params = if params_json.trim().is_empty() {
        ParamValues::new()
    } else {
        serde_json::from_str(params_json).map_err(|e| {
            CoreError(format!(
                "the parameter values must be a JSON object of name to value: {e}"
            ))
        })?
    };
    Ok(PartInputs {
        doc,
        params,
        process: process_profile(process_id)?,
        fit: fit_profile(fit_json)?,
    })
}

/// Evaluate with the default kernel; every op must be built (no skipping).
fn build(inputs: &PartInputs, assets: &AssetSet) -> Result<Built, CoreError> {
    let input = EvalInput {
        doc: &inputs.doc,
        params: &inputs.params,
        process: &inputs.process,
        fit: inputs.fit.as_ref(),
        assets: &assets.assets,
        skip_unsupported: false,
    };
    evaluate(&ManifoldRust, &input).map_err(|e| CoreError(e.to_string()))
}

/// The red display overlay of [`Artwork::removed`]: the regions extruded
/// [`REMOVED_OVERLAY_MM`] up from the face they lie on. `None` when nothing was
/// removed; an error only if the kernel could not build it.
fn removed_overlay(k: &ManifoldRust, art: &Artwork) -> Result<Option<MeshOut>, String> {
    if art.removed.is_empty() {
        return Ok(None);
    }
    let section = CrossSection::from(art.removed.clone());
    let slab = k
        .extrude(&section, REMOVED_OVERLAY_MM, 1.0)
        .map_err(|e| e.to_string())?;
    let slab = k.translate(&slab, [0.0, 0.0, art.face_z_mm]);
    MeshOut::from_mesh(k.id(), slab.to_mesh(), slab.volume_mm3(), "removed")
        .map(Some)
        .map_err(|e| e.0)
}

/// An evaluated part for the page. In JavaScript this is a class: take each mesh
/// once ([`PartOut::take_mesh`], [`PartOut::take_removed`]), read the JSON
/// fields, then call `free()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct PartOut {
    meshes: Vec<Option<MeshOut>>,
    removed: Option<MeshOut>,
    report: Report,
    measurements_json: String,
    warnings: Vec<String>,
    content_hash: String,
    kernel: String,
    artwork_json: String,
    params_json: String,
    footprint: [u32; 2],
    bounds: [f64; 6],
    volume_mm3: f64,
    triangles: u32,
    face_on_bed: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PartOut {
    /// How many objects (colour roles) the part has; `body` is mesh 0.
    pub fn mesh_count(&self) -> u32 {
        u32::try_from(self.meshes.len()).unwrap_or(u32::MAX)
    }

    /// Move object `index` out (one [`MeshOut`] per colour role, `body` first);
    /// `None` when the index is out of range or the mesh was taken already.
    pub fn take_mesh(&mut self, index: u32) -> Option<MeshOut> {
        self.meshes.get_mut(index as usize)?.take()
    }

    /// Move the red overlay of artwork that does not print as drawn out (role
    /// `removed`, on the face it belongs to); `None` when nothing was removed.
    pub fn take_removed(&mut self) -> Option<MeshOut> {
        self.removed.take()
    }

    /// The validation report (`openbrick_part::validate`) as JSON, the contract
    /// of `schemas/report.schema.json`: findings sorted blockers first.
    pub fn report_json(&self) -> String {
        serde_json::to_string(&self.report).expect("the report serialises")
    }

    /// True when the report has no blocker; [`export_part`] refuses otherwise.
    pub fn export_allowed(&self) -> bool {
        self.report.export_allowed()
    }

    /// What the evaluator measured (`openbrick_print::Measurements`) as JSON.
    pub fn measurements_json(&self) -> String {
        self.measurements_json.clone()
    }

    /// The evaluator's warnings (for an inlay, where the filament change goes)
    /// as a JSON array of strings, plus a note when the overlay of removed
    /// artwork could not be drawn.
    pub fn warnings_json(&self) -> String {
        serde_json::to_string(&self.warnings).expect("strings serialise")
    }

    /// SHA-256 of the normalised inputs (`Built::content_hash`); the same on
    /// every target for the same inputs.
    pub fn content_hash(&self) -> String {
        self.content_hash.clone()
    }

    pub fn kernel(&self) -> String {
        self.kernel.clone()
    }

    /// What the relief did with its artwork (`Built::artwork`) as JSON, or
    /// `null`: the mode, the face on the bed, the depth built and asked for,
    /// the areas before and after the opening, and `removedPolygons`, the
    /// number of regions in the overlay (the polygons themselves are in the
    /// overlay mesh, not repeated here).
    pub fn artwork_json(&self) -> String {
        self.artwork_json.clone()
    }

    /// The parameters after defaults and checks (`ResolvedParams`) as JSON:
    /// `values` and `visible` (each parameter's `visibleWhen`, as the core
    /// evaluates it).
    pub fn params_json(&self) -> String {
        self.params_json.clone()
    }

    /// Footprint in studs along x and y, from the body's size: n studs are n
    /// pitches less the undersize on both sides (docs/plan/04).
    pub fn footprint_studs(&self) -> Vec<u32> {
        self.footprint.to_vec()
    }

    /// Axis-aligned bounds of the whole part, mm: min x, y, z, then max x, y, z.
    pub fn bounds(&self) -> Vec<f64> {
        self.bounds.to_vec()
    }

    /// Volume that prints, mm³: the fused union, or the one object.
    pub fn volume_mm3(&self) -> f64 {
        self.volume_mm3
    }

    /// Estimated mass in grams printed solid in PLA (1.24 g/cm³, the density the
    /// coupon estimate uses); infill makes a print lighter.
    pub fn mass_g_pla(&self) -> f64 {
        self.volume_mm3 / 1000.0 * PLA_DENSITY_G_CM3
    }

    /// Triangles of all objects together (the overlay not counted).
    pub fn triangle_count(&self) -> u32 {
        self.triangles
    }

    /// The face the export puts on the bed: `bottom` (as built, face up) or
    /// `top` (an inlay, turned face down).
    pub fn face_on_bed(&self) -> String {
        self.face_on_bed.clone()
    }
}

/// Footprint in studs of a body whose box starts at the origin.
fn footprint_studs(bounds_max_mm: [f64; 3]) -> [u32; 2] {
    let s = STANDARD.get();
    let pitch = s.mm("module");
    let undersize = s.mm("undersize_per_side");
    let studs = |len: f64| ((len + 2.0 * undersize) / pitch).round().max(0.0) as u32;
    [studs(bounds_max_mm[0]), studs(bounds_max_mm[1])]
}

/// The volume that prints: the kernel's fused union when there is one, else the
/// single object.
fn printed_volume(built: &Built) -> f64 {
    built
        .fused
        .as_ref()
        .map(|f| f.volume_mm3)
        .unwrap_or_else(|| built.objects.iter().map(|o| o.volume_mm3).sum())
}

fn part_out(inputs: &PartInputs, built: Built) -> Result<PartOut, CoreError> {
    let k = ManifoldRust;
    let report = validate(&built, &inputs.process, inputs.fit.as_ref());
    let resolved = resolve_params(&inputs.doc, &inputs.params, &inputs.process)
        .map_err(|e| CoreError(e.to_string()))?;
    let mut warnings = built.warnings.clone();
    let removed = match built.artwork.as_ref().map(|a| removed_overlay(&k, a)) {
        Some(Ok(overlay)) => overlay,
        Some(Err(e)) => {
            warnings.push(format!(
                "the regions removed from the artwork could not be drawn in red ({e}); the removed area is still reported"
            ));
            None
        }
        None => None,
    };
    let artwork_json = match &built.artwork {
        Some(a) => {
            let mut v = serde_json::to_value(a).expect("the artwork report serialises");
            if let Some(o) = v.as_object_mut() {
                o.remove("removed");
                o.insert("removedPolygons".into(), json!(a.removed.len()));
            }
            v.to_string()
        }
        None => "null".to_string(),
    };
    let m = &built.measurements;
    let bounds = [
        m.bounds_min_mm[0],
        m.bounds_min_mm[1],
        m.bounds_min_mm[2],
        m.bounds_max_mm[0],
        m.bounds_max_mm[1],
        m.bounds_max_mm[2],
    ];
    let face_on_bed = built
        .artwork
        .as_ref()
        .map_or("bottom".to_string(), |a| a.face_on_bed.clone());
    let volume_mm3 = printed_volume(&built);
    let triangles = built
        .objects
        .iter()
        .map(|o| o.mesh.triangle_count())
        .sum::<usize>();
    let measurements_json =
        serde_json::to_string(&built.measurements).expect("measurements serialise");
    let footprint = footprint_studs(m.bounds_max_mm);
    let kernel = built.kernel_id.clone();
    let content_hash = built.content_hash.clone();
    let meshes = built
        .objects
        .into_iter()
        .map(|o| MeshOut::from_mesh(kernel.clone(), o.mesh, o.volume_mm3, &o.role).map(Some))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PartOut {
        meshes,
        removed,
        report,
        measurements_json,
        warnings,
        content_hash,
        kernel,
        artwork_json,
        params_json: serde_json::to_string(&resolved).expect("parameters serialise"),
        footprint,
        bounds,
        volume_mm3,
        triangles: u32::try_from(triangles).unwrap_or(u32::MAX),
        face_on_bed,
    })
}

/// Evaluate a part document and validate it (see the module documentation for
/// the inputs). Errors are what the page shows: an invalid document or value,
/// an asset the evaluator refused (with the reason), an unknown process.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn evaluate_part(
    doc_json: &str,
    params_json: &str,
    process_id: &str,
    fit_json: &str,
    assets: &AssetSet,
) -> Result<PartOut, CoreError> {
    let inputs = part_inputs(doc_json, params_json, process_id, fit_json)?;
    let built = build(&inputs, assets)?;
    part_out(&inputs, built)
}

/// The parameters after defaults and checks, without building anything:
/// `{ values, visible }` (`ResolvedParams`), so the page can check its own
/// reading of `visibleWhen` against the core's.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn resolve_params_json(
    doc_json: &str,
    params_json: &str,
    process_id: &str,
) -> Result<String, CoreError> {
    let inputs = part_inputs(doc_json, params_json, process_id, "")?;
    let resolved = resolve_params(&inputs.doc, &inputs.params, &inputs.process)
        .map_err(|e| CoreError(e.to_string()))?;
    Ok(serde_json::to_string(&resolved).expect("parameters serialise"))
}

/// The error an export returns while the report has blockers.
fn blocked(report: &Report) -> CoreError {
    let list: Vec<String> = report
        .blockers()
        .map(|f| format!("{}: {}", f.rule, f.message))
        .collect();
    CoreError(format!(
        "export is disabled while the validation report has {} blocker{}: {}",
        list.len(),
        if list.len() == 1 { "" } else { "s" },
        list.join("; ")
    ))
}

/// Evaluate, validate and write a printable file: `format` is `3mf` (one object
/// per colour role, in the colours of `colors_json`, a JSON object of role to
/// `#rrggbb`, empty for the role colours) or `stl` (one solid: the kernel's
/// fused union of every object, for a single colour). An inlay is turned face
/// down, as it prints; everything else is written as built. Refuses while the
/// validation report has a blocker, naming each one.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn export_part(
    format: &str,
    doc_json: &str,
    params_json: &str,
    process_id: &str,
    fit_json: &str,
    assets: &AssetSet,
    colors_json: &str,
) -> Result<Vec<u8>, CoreError> {
    if format != "3mf" && format != "stl" {
        return Err(CoreError(format!(
            "unknown export format {format:?}; use \"3mf\" or \"stl\""
        )));
    }
    let colors = role_colors(colors_json)?;
    let inputs = part_inputs(doc_json, params_json, process_id, fit_json)?;
    let built = build(&inputs, assets)?;
    let report = validate(&built, &inputs.process, inputs.fit.as_ref());
    if !report.export_allowed() {
        return Err(blocked(&report));
    }
    let face_down = built
        .artwork
        .as_ref()
        .is_some_and(|a| a.face_on_bed == "top");
    let doc = &inputs.doc;
    let meta = ExportMeta {
        part_id: doc.id.clone(),
        version: doc.version.clone(),
        content_hash: built.content_hash.clone(),
        fit_profile_id: inputs
            .fit
            .as_ref()
            .map_or("none".to_string(), |f| f.id.clone()),
        process_id: inputs.process.id.clone(),
        generator: GENERATOR.to_string(),
        kernel_id: built.kernel_id.clone(),
        title: if face_down {
            format!("{} (face down, as it prints)", doc.name)
        } else {
            doc.name.clone()
        },
        serial: None,
    };
    let write = |objects: &[&RoleMesh]| -> Result<Vec<u8>, CoreError> {
        let meshes: Vec<_> = objects.iter().map(|o| &o.mesh).collect();
        let laid = on_bed(&meshes, face_down);
        let out: Vec<ExportObject> = objects
            .iter()
            .zip(&laid)
            .map(|(o, mesh)| {
                ExportObject::new(
                    format!("{} {}", doc.id, o.role),
                    o.role.clone(),
                    Some(color_of(&colors, &o.role)),
                    mesh,
                )
            })
            .collect();
        let bytes = if format == "3mf" {
            write_3mf(&out, &meta)
        } else {
            write_stl_binary(&out)
        };
        bytes.map_err(|e| CoreError(format!("the {format} writer refused the part: {e}")))
    };
    if format == "3mf" {
        write(&built.objects.iter().collect::<Vec<_>>())
    } else {
        // One solid for a single colour: merging the role meshes instead would
        // leave two shells touching where a relief meets the body.
        let one = built.fused.as_ref().or(built.objects.first());
        let one = one.ok_or_else(|| CoreError("the part has no objects".into()))?;
        write(&[one])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{example_fit_profile_json, logo_tile_part_json, sample_logo_svg};
    use openbrick_part::MemoryAssets;

    fn sample() -> AssetSet {
        let mut a = AssetSet::new();
        a.add("assets/mark.svg", sample_logo_svg().into_bytes())
            .unwrap();
        a
    }

    fn eval_with(params: &str, fit: &str) -> PartOut {
        evaluate_part(&logo_tile_part_json(), params, "fdm-0.4", fit, &sample()).expect("builds")
    }

    /// The same part straight from openbrick-part, for comparison.
    fn native(params: &str, fit: Option<FitProfile>) -> Built {
        let doc = PartDoc::parse(&logo_tile_part_json()).unwrap();
        let params: ParamValues = if params.is_empty() {
            ParamValues::new()
        } else {
            serde_json::from_str(params).unwrap()
        };
        let assets = MemoryAssets::new().with("assets/mark.svg", sample_logo_svg().into_bytes());
        let process = process_profile("fdm-0.4").unwrap();
        evaluate(
            &ManifoldRust,
            &EvalInput {
                doc: &doc,
                params: &params,
                process: &process,
                fit: fit.as_ref(),
                assets: &assets,
                skip_unsupported: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn the_logo_tile_evaluates_like_the_evaluator() {
        let fit_json = example_fit_profile_json();
        let mut out = eval_with("", &fit_json);
        let b = native("", Some(openbrick_print::example_fit_profile()));
        assert_eq!(out.content_hash(), b.content_hash);
        assert_eq!(out.kernel(), crate::kernel_id());
        assert_eq!(out.mesh_count(), 2);
        let mut triangles = 0;
        for (i, (role, color)) in [("body", "#2f6db5"), ("accent", "#f4f1e8")]
            .into_iter()
            .enumerate()
        {
            let mut m = out.take_mesh(i as u32).expect("mesh");
            assert_eq!((m.role().as_str(), m.color().as_str()), (role, color));
            let o = &b.objects[i];
            assert_eq!(o.role, role);
            assert_eq!(m.triangle_count() as usize, o.mesh.triangle_count());
            assert_eq!(m.fingerprint(), crate::hex(o.mesh.fingerprint()));
            assert_eq!(m.take_positions(), o.mesh.positions.as_flattened());
            assert_eq!(m.volume_mm3(), o.volume_mm3);
            triangles += m.triangle_count();
        }
        assert!(out.take_mesh(0).is_none(), "taken once");
        assert!(out.take_mesh(2).is_none());
        assert_eq!(out.triangle_count(), triangles);

        // The example fit profile records every cell the tube field needs.
        assert!(out.export_allowed(), "{}", out.report_json());
        let report: Report = serde_json::from_str(&out.report_json()).unwrap();
        assert_eq!(
            report,
            validate(
                &b,
                &process_profile("fdm-0.4").unwrap(),
                Some(&openbrick_print::example_fit_profile())
            )
        );
        let m: openbrick_print::Measurements =
            serde_json::from_str(&out.measurements_json()).unwrap();
        assert_eq!(m, b.measurements);
        let warnings: Vec<String> = serde_json::from_str(&out.warnings_json()).unwrap();
        assert_eq!(warnings, b.warnings);

        // Size, footprint, mass: the 2x4 default.
        let s = STANDARD.get();
        assert_eq!(out.footprint_studs(), [2, 4]);
        let bounds = out.bounds();
        let body_x = 2.0 * s.mm("module") - 2.0 * s.mm("undersize_per_side");
        assert!((bounds[3] - bounds[0] - body_x).abs() < 1e-6, "{bounds:?}");
        let fused = b.fused.as_ref().expect("fused").volume_mm3;
        assert_eq!(out.volume_mm3(), fused);
        assert!((out.mass_g_pla() - fused * PLA_DENSITY_G_CM3 / 1000.0).abs() < 1e-12);
        assert_eq!(out.face_on_bed(), "bottom");

        // The artwork summary leaves the polygons to the overlay mesh.
        let art: Value = serde_json::from_str(&out.artwork_json()).unwrap();
        let a = b.artwork.as_ref().unwrap();
        assert_eq!(art["mode"], "emboss");
        assert_eq!(art["removedPolygons"], json!(a.removed.len()));
        assert!(art.get("removed").is_none());
        // The sample's hairline is removed (rule FEATURE.MIN reports 1.03 %).
        let mut overlay = out.take_removed().expect("the hairline is drawn in red");
        assert_eq!(overlay.role(), "removed");
        let ob = overlay.bounds();
        assert!((f64::from(ob[2]) - a.face_z_mm).abs() < 1e-5);
        assert!((f64::from(ob[5]) - a.face_z_mm - REMOVED_OVERLAY_MM).abs() < 1e-5);
        let area: f64 = a.removed.iter().map(|p| p.signed_area()).sum();
        assert!((overlay.volume_mm3() - area * REMOVED_OVERLAY_MM).abs() < 1e-6 * area.max(1.0));
        assert!(!overlay.take_positions().is_empty());

        let params: Value = serde_json::from_str(&out.params_json()).unwrap();
        assert_eq!(params["values"]["size"], json!([2, 4]));
        assert_eq!(params["visible"]["reliefHeight"], json!(true));
    }

    #[test]
    fn without_a_fit_profile_the_tube_field_blocks_export() {
        let out = eval_with("", "");
        assert!(!out.export_allowed());
        let report: Report = serde_json::from_str(&out.report_json()).unwrap();
        let rules: Vec<&str> = report.blockers().map(|f| f.rule.as_str()).collect();
        assert!(
            rules.iter().all(|r| *r == "FIT.MISSING") && !rules.is_empty(),
            "{rules:?}"
        );
        for format in ["3mf", "stl"] {
            let err = export_part(
                format,
                &logo_tile_part_json(),
                "",
                "fdm-0.4",
                "",
                &sample(),
                "",
            )
            .unwrap_err();
            assert!(err.0.starts_with("export is disabled while"), "{err}");
            assert!(err.0.contains("FIT.MISSING"), "{err}");
        }
    }

    #[test]
    fn hidden_parameters_follow_the_core() {
        let r: Value = serde_json::from_str(
            &resolve_params_json(&logo_tile_part_json(), r#"{"relief":"inlay"}"#, "fdm-0.4")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(r["visible"]["reliefHeight"], json!(false));
        assert_eq!(r["visible"]["margin"], json!(true));
        assert!(resolve_params_json(&logo_tile_part_json(), r#"{"nope":1}"#, "fdm-0.4").is_err());
    }

    #[test]
    fn exports_write_the_evaluated_part() {
        let fit = example_fit_profile_json();
        let doc = logo_tile_part_json();
        let b = native("", Some(openbrick_print::example_fit_profile()));
        let threemf = export_part("3mf", &doc, "", "fdm-0.4", &fit, &sample(), "").unwrap();
        assert_eq!(&threemf[..4], b"PK\x03\x04", "a zip container");
        let stl = export_part("stl", &doc, "", "fdm-0.4", &fit, &sample(), "").unwrap();
        let fused = b.fused.as_ref().unwrap();
        assert_eq!(stl.len(), 84 + 50 * fused.mesh.triangle_count());
        // Colours are recorded in the 3MF (and change it), never in the STL.
        let orange = export_part(
            "3mf",
            &doc,
            "",
            "fdm-0.4",
            &fit,
            &sample(),
            r##"{"accent":"#ff8000"}"##,
        )
        .unwrap();
        assert_ne!(orange, threemf);
        let again = export_part("3mf", &doc, "", "fdm-0.4", &fit, &sample(), "").unwrap();
        assert_eq!(again, threemf, "exports are deterministic");
        for bad in ["obj", "", "3MF"] {
            assert!(export_part(bad, &doc, "", "fdm-0.4", &fit, &sample(), "").is_err());
        }
    }

    /// Triangle corners of a binary STL.
    fn stl_corners(bytes: &[u8]) -> Vec<[f32; 3]> {
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(3 * n);
        for t in 0..n {
            let rec = &bytes[84 + 50 * t..84 + 50 * (t + 1)];
            for c in 0..3 {
                let f = |k: usize| {
                    let at = 12 + 12 * c + 4 * k;
                    f32::from_le_bytes(rec[at..at + 4].try_into().unwrap())
                };
                out.push([f(0), f(1), f(2)]);
            }
        }
        out
    }

    #[test]
    fn an_inlay_is_exported_face_down() {
        let fit = example_fit_profile_json();
        let doc = logo_tile_part_json();
        let params = r#"{"relief":"inlay"}"#;
        let out = eval_with(params, &fit);
        assert_eq!(out.face_on_bed(), "top");
        let b = native(params, Some(openbrick_print::example_fit_profile()));
        let art = b.artwork.as_ref().unwrap();
        let top = art.face_z_mm;
        let stl = export_part("stl", &doc, params, "fdm-0.4", &fit, &sample(), "").unwrap();
        let corners = stl_corners(&stl);
        let fused = &b.fused.as_ref().unwrap().mesh;
        // Same box, turned: the top face (z = top) is now on the bed (z = 0),
        // and the open cavity faces up.
        let (lo, hi) = fused.bounds();
        let zmin = corners.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min);
        let zmax = corners
            .iter()
            .map(|p| p[2])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((zmin, zmax), (lo[2], hi[2]));
        assert!((f64::from(hi[2]) - top).abs() < 1e-5, "the face is the top");
        let on_bed_area = corners
            .chunks(3)
            .filter(|t| t.iter().all(|p| p[2] == 0.0))
            .map(|t| {
                let u = [t[1][0] - t[0][0], t[1][1] - t[0][1]];
                let v = [t[2][0] - t[0][0], t[2][1] - t[0][1]];
                f64::from(u[0] * v[1] - u[1] * v[0]).abs() / 2.0
            })
            .sum::<f64>();
        // Face down, the top face is on the bed: the flat face inside its 45
        // degree chamfer, less the artwork, which an inlay leaves as a recess in
        // the body colour (docs/plan/05 section 6). The example fit profile has
        // xyScale 1, so the artwork area is as fitted.
        let [w, d] = [hi[0] - lo[0], hi[1] - lo[1]].map(f64::from);
        let chamfer = art.face_chamfer_mm;
        let face = (w - 2.0 * chamfer) * (d - 2.0 * chamfer) - art.area_after_mm2;
        assert!(
            (on_bed_area - face).abs() < 1e-3 * face,
            "{on_bed_area} vs {face}"
        );
        // The emboss default stays face up: its bed face is the bottom ring.
        let up = export_part("stl", &doc, "", "fdm-0.4", &fit, &sample(), "").unwrap();
        let up_corners = stl_corners(&up);
        let fused_up = &native("", Some(openbrick_print::example_fit_profile()))
            .fused
            .unwrap()
            .mesh;
        assert_eq!(up_corners.len(), 3 * fused_up.triangle_count());
        assert_eq!(
            up_corners[0],
            fused_up.positions[fused_up.indices[0] as usize]
        );
    }

    /// Golden values of the geometry this crate adds on top of the evaluator: the
    /// red overlay of the default logo tile and the face-down inlay as exported.
    /// The evaluator's own goldens are in crates/openbrick-part/tests.
    #[test]
    #[cfg_attr(
        not(all(target_arch = "x86_64", target_os = "linux")),
        ignore = "bit-exact output is verified on x86-64 Linux only (docs/spikes/q01-kernel-conformance.md)"
    )]
    fn overlay_and_face_down_export_goldens() {
        let fit = example_fit_profile_json();
        let mut emboss = eval_with("", &fit);
        let overlay = emboss.take_removed().expect("overlay");
        let b = native("", Some(openbrick_print::example_fit_profile()));
        let a = b.artwork.as_ref().unwrap();
        // The regions the evaluator reports (the hairline and the slivers the
        // opening takes off sharp corners); the overlay draws them all.
        assert_eq!(
            a.removed.len(),
            REMOVED_REGIONS,
            "{} regions",
            a.removed.len()
        );
        eprintln!(
            "overlay {} triangles {} {}",
            overlay.triangle_count(),
            overlay.fingerprint(),
            overlay.canonical_fingerprint()
        );
        assert_eq!(overlay.triangle_count(), OVERLAY_TRIANGLES);
        assert_eq!(overlay.fingerprint(), OVERLAY_FINGERPRINT);

        let inlay = native(
            r#"{"relief":"inlay"}"#,
            Some(openbrick_print::example_fit_profile()),
        );
        let fused = &inlay.fused.as_ref().unwrap().mesh;
        let laid = on_bed(&[fused], true).remove(0);
        // Same triangles and box, turned: topology and size are unchanged.
        assert_eq!(laid.indices, fused.indices);
        assert_eq!(laid.bounds(), fused.bounds());
        eprintln!(
            "face down {} triangles {:#018x}",
            laid.triangle_count(),
            laid.fingerprint()
        );
        assert_eq!(crate::hex(laid.fingerprint()), FACE_DOWN_INLAY_FINGERPRINT);
        assert_ne!(laid.canonical_fingerprint(), fused.canonical_fingerprint());
    }

    /// Recorded with manifold-rust 0.19.1 on x86-64 Linux.
    const REMOVED_REGIONS: usize = 18;
    const OVERLAY_TRIANGLES: u32 = 296;
    const OVERLAY_FINGERPRINT: &str = "0xce7da5d8c07c1ddc";
    const FACE_DOWN_INLAY_FINGERPRINT: &str = "0x5cd58ae233d1d801";

    #[test]
    fn bad_inputs_get_messages_that_say_what_to_do() {
        let doc = logo_tile_part_json();
        let err = |r: Result<PartOut, CoreError>| r.err().expect("an error").0;
        let e = err(evaluate_part(&doc, "", "fdm-9", "", &sample()));
        assert!(e.contains("fdm-0.4, fdm-0.2, resin-msla"), "{e}");
        let e = err(evaluate_part(&doc, "", "fdm-0.4", "{", &sample()));
        assert!(
            e.contains("not valid JSON") && e.contains("calibrate record"),
            "{e}"
        );
        let process = crate::process_profiles_json();
        let first: Value = serde_json::from_str::<Value>(&process).unwrap()[0].clone();
        let e = err(evaluate_part(
            &doc,
            "",
            "fdm-0.4",
            &first.to_string(),
            &sample(),
        ));
        assert!(e.contains("`process` profile, not a fit profile"), "{e}");
        let e = err(evaluate_part(&doc, "[1]", "fdm-0.4", "", &sample()));
        assert!(e.contains("JSON object"), "{e}");
        let e = err(evaluate_part(
            &doc,
            r#"{"size":[0,4]}"#,
            "fdm-0.4",
            "",
            &sample(),
        ));
        assert!(e.contains("size"), "{e}");
        let e = err(evaluate_part("{}", "", "fdm-0.4", "", &sample()));
        assert!(e.starts_with("the part document is not valid"), "{e}");
        // No artwork given: the asset error names the file the document wants.
        let e = err(evaluate_part(&doc, "", "fdm-0.4", "", &AssetSet::new()));
        assert!(e.contains("assets/mark.svg"), "{e}");
        // Artwork that is not SVG.
        let mut junk = AssetSet::new();
        junk.add("assets/mark.svg", b"not an svg".to_vec()).unwrap();
        let e = err(evaluate_part(&doc, "", "fdm-0.4", "", &junk));
        assert!(e.starts_with("asset `assets/mark.svg`"), "{e}");
        // Too large: refused before the evaluator sees it.
        let mut big = AssetSet::new();
        let max = crate::max_asset_bytes() as usize;
        assert!(big.add("a.svg", vec![b' '; max]).is_ok());
        let e = big.add("b.svg", vec![b' '; max + 1]).unwrap_err().0;
        assert!(
            e.contains(&format!("limit for artwork is {max} bytes (4 MiB)")),
            "{e}"
        );
        assert_eq!(big.count(), 1);
        assert!(AssetSet::new().add(" ", vec![]).is_err());
    }
}
