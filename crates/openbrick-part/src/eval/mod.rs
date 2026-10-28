//! The part evaluator (docs/plan/06 sections 1 to 4, 6 and 10): an
//! `openbrick/part@1` document plus parameter values, a [`ProcessProfile`] and an
//! optional [`FitProfile`] become printable meshes, derived connectors and the
//! measurements the validator reads.
//!
//! Implemented ops: `body.tile`, `body.plate`, `body.brick`, `stud.field`,
//! `tube.field` and `relief.svg` (module `relief`: SVG artwork embossed, debossed
//! or inlaid on the top face). Every other op is reported as
//! [`EvalError::Unsupported`], or skipped with a warning when
//! [`EvalInput::skip_unsupported`] is set.
//!
//! Where the numbers come from:
//!
//! - Dimensions: `standards/dimensions-v1.json` through `openbrick-units`
//!   (module, undersize_per_side, plate_height, brick_height, side_wall, rib_depth,
//!   roof, stud_diameter, stud_height, tube_outer_diameter, tube_inner_diameter,
//!   pin_1wide_diameter); the sources of each value are cited there.
//! - Process limits: the [`ProcessProfile`] (wall quantisation, minimum roof,
//!   first-layer and stud-top chamfers, wall-to-roof chamfer, minimum tube wall,
//!   minimum feature for rib width, circle segments).
//! - Fit offsets: the [`FitProfile`] only, never the part document (CLAUDE.md).
//!
//! Frames: meshes are in the kernel frame (millimetres, +Z up, body minimum corner
//! at the origin, as in `openbrick-geom/examples/export_lods.rs`). Connectors are
//! in integer LDU in the LDraw frame of docs/plan/04 section 3: minus Y up, origin
//! centred on the top stud group with the stud bases at Y = 0, so a body spans
//! Y = 0 (top) to its height (bottom). Kernel (x, y, z) maps to LDraw
//! (x, -z, y) up to that origin shift, a proper rotation.

mod expr;
pub(crate) mod geometry;
mod hash;
mod params;
mod relief;
mod svg;

use std::cell::RefCell;
use std::collections::BTreeMap;

use openbrick_geom::{GeomError, Kernel, Mesh, Solid};
use openbrick_print::{FitProfile, Measurements, Offsets, ProcessProfile};
use openbrick_units::{BRICK_LDU, HALF_STUD_LDU, PLATE_LDU, STANDARD, STUD_PITCH_LDU};
use serde_json::{json, Value};

pub use hash::{canonical_json, sha256_hex};
pub use params::{resolve_params, ResolvedParams};
pub use relief::{
    Artwork, FLATTEN_TOLERANCE_MM, MAX_ARTWORK_POINTS, MAX_ARTWORK_SWEEP, REMOVED_WARN_PCT,
};
pub use svg::{
    MAX_CSS_SELECTOR_PARTS, MAX_SVG_ATTRIBUTES, MAX_SVG_BYTES, MAX_SVG_CSS_DECLARATIONS,
    MAX_SVG_CSS_WORK, MAX_SVG_DEPTH, MAX_SVG_ELEMENTS, MAX_SVG_NODES, MAX_SVG_PATHS, MAX_SVG_WORK,
};

use geometry::{BodyGeom, Geo, StudGeom, TubeGeom};
use params::{FVal, Scope};
use relief::ReliefPlan;

use crate::{Feature, PartDoc, PartError};

/// Bump when the evaluator builds different geometry or connectors for the same
/// inputs, so content hashes made by older builds no longer match.
pub const EVALUATOR_REVISION: u32 = 1;

/// The ops [`evaluate`] builds.
pub const SUPPORTED_OPS: &[&str] = &[
    "body.tile",
    "body.plate",
    "body.brick",
    "stud.field",
    "tube.field",
    "relief.svg",
];

/// Largest footprint side, in studs, the evaluator accepts: a resource guard
/// (a 64 x 64 plate already has 4,096 studs), not a brick dimension.
pub const MAX_STUDS_PER_SIDE: u32 = 64;

/// Tallest body, in plates, the evaluator accepts: 64 brick heights, a resource
/// and sanity guard like [`MAX_STUDS_PER_SIDE`], not a brick dimension. It also
/// keeps every LDU coordinate far inside `i32`.
pub const MAX_BODY_PLATES: u32 = 64 * (BRICK_LDU / PLATE_LDU) as u32;

/// Lengths below this many millimetres are treated as zero: one micrometre,
/// far below what any process prints, and above both the floating-point residue
/// of offset arithmetic (`1.2 + 0.3 - 1.2 - 0.3` is 5.6e-17 mm) and the `f32`
/// mesh resolution of the largest part the evaluator accepts (6.1e-5 mm for
/// coordinates from 512 to 1024 mm). Thinner solids would reach the kernel as
/// degenerate slivers, which it rejects (on manifold-rust 0.19.1 a 1e-9 mm rib
/// fails to extrude; 1e-7 mm still builds).
pub const MIN_LENGTH_MM: f64 = 1e-3;

/// Parameter overrides by name; parameters not named keep their defaults.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ParamValues {
    pub values: BTreeMap<String, Value>,
}

impl ParamValues {
    pub fn new() -> ParamValues {
        ParamValues::default()
    }

    /// Set one override (builder style).
    pub fn with(mut self, name: &str, value: Value) -> ParamValues {
        self.values.insert(name.to_string(), value);
        self
    }
}

/// Reads assets a document names (SVG artwork, images, fonts). Implementations
/// decide where names point; the evaluator never touches the file system.
pub trait AssetResolver {
    fn read(&self, name: &str) -> Result<Vec<u8>, String>;
}

/// Assets held in memory by name.
#[derive(Clone, Debug, Default)]
pub struct MemoryAssets {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl MemoryAssets {
    pub fn new() -> MemoryAssets {
        MemoryAssets::default()
    }

    /// Add one asset (builder style).
    pub fn with(mut self, name: &str, bytes: Vec<u8>) -> MemoryAssets {
        self.files.insert(name.to_string(), bytes);
        self
    }
}

impl AssetResolver for MemoryAssets {
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| format!("no asset named `{name}`"))
    }
}

/// Everything one evaluation reads.
pub struct EvalInput<'a> {
    pub doc: &'a PartDoc,
    pub params: &'a ParamValues,
    pub process: &'a ProcessProfile,
    pub fit: Option<&'a FitProfile>,
    pub assets: &'a dyn AssetResolver,
    /// Skip features whose op is not implemented yet (with a warning) instead of
    /// failing. Meant for previews; what was skipped is part of the content hash.
    pub skip_unsupported: bool,
}

/// One printable object: all geometry of one colour role, unioned.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleMesh {
    /// `body`, `accent` or `serial` (docs/plan/05 section 7).
    pub role: String,
    /// Millimetres, kernel frame (+Z up, body minimum corner at the origin).
    pub mesh: Mesh,
    pub volume_mm3: f64,
}

/// Section shape of a connector (LDCad SNAP_CYL `secs`): round, axle or square.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SectionShape {
    #[serde(rename = "R")]
    Round,
    #[serde(rename = "A")]
    Axle,
    #[serde(rename = "S")]
    Square,
}

/// One section of a connector cylinder, in LDU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorSection {
    pub shape: SectionShape,
    pub radius_ldu: i32,
    pub length_ldu: i32,
}

/// A connector derived from a feature (docs/plan/06 section 6).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    /// `studM`, `tubeF`, ... (the connector types of docs/plan/06 section 6).
    pub kind: String,
    /// Integer LDU in the LDraw frame (minus Y up).
    pub pos: [i32; 3],
    /// Unit vector out of the part along the connector axis, LDraw frame: studs
    /// point up (0, -1, 0), underside sockets down (0, 1, 0).
    pub dir: [i32; 3],
    pub sections: Vec<ConnectorSection>,
    /// The feature that emitted it.
    pub feature: String,
    pub fit_critical: bool,
}

/// The result of an evaluation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Built {
    /// One object per colour role, `body` first.
    pub objects: Vec<RoleMesh>,
    pub connectors: Vec<Connector>,
    pub measurements: Measurements,
    pub warnings: Vec<String>,
    pub kernel_id: String,
    /// SHA-256 (hex) of the canonical JSON of the normalised document, the
    /// resolved parameters, the process and fit profiles (ids and contents), the
    /// kernel id, the standard version, [`EVALUATOR_REVISION`], the features
    /// skipped as unsupported and the hashes of assets read. Equal inputs give
    /// equal hashes on every target (docs/plan/06 section 10).
    pub content_hash: String,
    /// Every object unioned by the kernel into one solid, in the body's role:
    /// the single-colour print (one watertight shell set for a single-material
    /// STL or a fused 3MF, docs/plan/05 section 7). Present only when the part
    /// has more than one object: merging the objects' meshes instead would leave
    /// two shells touching where an embossed relief meets the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fused: Option<RoleMesh>,
    /// What the `relief.svg` feature did with its artwork (the removed regions
    /// for the preview, the face on the bed, the depth after clamping), when the
    /// part has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<Artwork>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("the document is not structurally valid: {0}")]
    Part(#[from] PartError),
    #[error("unknown parameter `{0}`: the part defines no parameter of that name")]
    UnknownParam(String),
    #[error("parameter `{name}`: {message}")]
    Param { name: String, message: String },
    #[error("feature `{feature}`, field `{field}`: {message}")]
    Field {
        feature: String,
        field: String,
        message: String,
    },
    #[error("feature `{feature}`: {message}")]
    Feature { feature: String, message: String },
    #[error("feature `{feature}` uses `{op}`{detail}, which this evaluator does not build yet; remove the feature, turn it off with its `if`, or evaluate with skip_unsupported for a preview without it")]
    Unsupported {
        feature: String,
        op: String,
        /// Extra detail, such as ` with studType hollow`; may be empty.
        detail: String,
    },
    #[error("the part has no active body feature (body.tile, body.plate or body.brick); every part needs exactly one")]
    NoBody,
    #[error("asset `{name}`: {message}")]
    Asset { name: String, message: String },
    #[error("the geometry kernel failed while building {what}: {source}")]
    Kernel {
        what: String,
        #[source]
        source: GeomError,
    },
}

fn kernel_err(what: &str) -> impl FnOnce(GeomError) -> EvalError + '_ {
    move |source| EvalError::Kernel {
        what: what.to_string(),
        source,
    }
}

/// Records the assets an evaluation reads, so their bytes enter the content hash.
struct RecordingAssets<'a> {
    inner: &'a dyn AssetResolver,
    /// Name to SHA-256 of every asset read.
    seen: RefCell<BTreeMap<String, String>>,
}

impl RecordingAssets<'_> {
    /// Read an asset for a feature (`relief.svg` reads its artwork here).
    fn read(&self, name: &str) -> Result<Vec<u8>, EvalError> {
        let bytes = self.inner.read(name).map_err(|message| EvalError::Asset {
            name: name.to_string(),
            message,
        })?;
        self.seen
            .borrow_mut()
            .insert(name.to_string(), sha256_hex(&bytes));
        Ok(bytes)
    }
}

/// A feature with its fields resolved on demand.
struct Feat<'a> {
    f: &'a Feature,
    scope: &'a Scope<'a>,
}

impl Feat<'_> {
    fn id(&self) -> &str {
        &self.f.id
    }

    fn field(&self, name: &str) -> Result<Option<FVal>, EvalError> {
        match self.f.fields.get(name) {
            None => Ok(None),
            Some(v) => self.scope.field(v).map(Some).map_err(|m| self.err(name, m)),
        }
    }

    fn err(&self, field: &str, message: impl Into<String>) -> EvalError {
        EvalError::Field {
            feature: self.f.id.clone(),
            field: field.to_string(),
            message: message.into(),
        }
    }

    fn fail(&self, message: impl Into<String>) -> EvalError {
        EvalError::Feature {
            feature: self.f.id.clone(),
            message: message.into(),
        }
    }

    fn unsupported(&self, detail: impl Into<String>) -> EvalError {
        EvalError::Unsupported {
            feature: self.f.id.clone(),
            op: self.f.op.clone(),
            detail: detail.into(),
        }
    }

    /// Warn about op-specific fields this op does not read.
    fn warn_unused(&self, known: &[&str], warnings: &mut Vec<String>) {
        for name in self
            .f
            .fields
            .keys()
            .filter(|k| !known.contains(&k.as_str()))
        {
            warnings.push(format!(
                "feature `{}`: field `{name}` is not used by {}",
                self.f.id, self.f.op
            ));
        }
    }

    /// The face an anchored feature sits on: `None` for no anchor or a bare
    /// feature id (the op's default face), else the part after the dot, checked
    /// to belong to `body_id`.
    fn face_of(&self, body_id: &str) -> Result<Option<&str>, EvalError> {
        let Some(on) = &self.f.on else {
            return Ok(None);
        };
        let (base, face) = match on.split_once('.') {
            Some((base, face)) => (base, Some(face)),
            None => (on.as_str(), None),
        };
        if base != body_id {
            return Err(self.err(
                "on",
                format!("`{on}` must name a face of the body feature `{body_id}`"),
            ));
        }
        Ok(face)
    }

    fn role(&self) -> String {
        self.f
            .color_role
            .clone()
            .unwrap_or_else(|| "body".to_string())
    }
}

fn mm(id: &str) -> f64 {
    STANDARD.get().mm(id)
}

/// The LDraw-unit value of a standard row (stud radius 6, stud height 4, ...).
fn std_ldu(id: &str) -> i32 {
    let v = STANDARD
        .get()
        .feature(id)
        .and_then(|f| f.ldraw_ldu)
        .unwrap_or_else(|| panic!("standard feature {id} has no ldraw_ldu"));
    v.round() as i32
}

/// The width of a wall of `wall` mm with a 45 degree chamfer of `chamfer` mm on
/// both faces at its foot, where a slicer cuts the first layer: half the
/// process's thinnest layer height above the bed (the narrowest first layer it
/// could print).
fn first_layer_width(process: &ProcessProfile, wall: f64, chamfer: f64) -> f64 {
    let layer = process
        .layer_height_mm
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let z = if layer.is_finite() { layer / 2.0 } else { 0.0 };
    wall - 2.0 * (chamfer - z).max(0.0)
}

/// The roof (top) thickness of a body: the standard roof, at least the process
/// minimum (rule `ROOF.MIN`).
pub(crate) fn roof_mm(process: &ProcessProfile) -> f64 {
    mm("roof").max(process.min_roof_mm)
}

/// Wall rib depth before fit offsets, for side walls `wall` mm thick: wall plus
/// rib is the standard side_wall + rib_depth, so a wall that quantising made
/// thinner gets a deeper rib (docs/plan/04 section 6).
pub(crate) fn nominal_rib_depth(wall: f64) -> f64 {
    mm("side_wall") + mm("rib_depth") - wall
}

/// Height and top chamfer of a solid stud, as `stud.field` builds it.
pub(crate) struct StudShape {
    /// The standard stud height.
    pub height: f64,
    /// The process's stud-top chamfer, capped at a quarter of the diameter and
    /// half the height.
    pub top_chamfer: f64,
    /// The chamfer the process asked for, when the cap lowered it.
    pub capped_from: Option<f64>,
}

/// The shape of a solid stud of `diameter` mm on `process`.
pub(crate) fn stud_shape(process: &ProcessProfile, diameter: f64) -> StudShape {
    let height = mm("stud_height");
    let asked = process.stud_top_chamfer_mm.max(0.0);
    let cap = (diameter / 4.0).min(height / 2.0);
    StudShape {
        height,
        top_chamfer: asked.min(cap),
        capped_from: (asked > cap).then_some(asked),
    }
}

/// The planned body (sizes are known before any geometry is built).
struct BodyPlan {
    id: String,
    tile: bool,
    studs: [u32; 2],
    /// Body height in LDU; at most [`MAX_BODY_PLATES`] plates.
    height_ldu: i32,
    role: String,
    geom: BodyGeom,
}

impl BodyPlan {
    /// Kernel-frame centre of stud (i, j) in millimetres.
    fn stud_centre(&self, i: u32, j: u32) -> [f64; 2] {
        let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
        [
            f64::from(i) * pitch + pitch / 2.0 - gap,
            f64::from(j) * pitch + pitch / 2.0 - gap,
        ]
    }

    /// LDraw X and Z (LDU) of stud (i, j): centred on the stud group.
    fn stud_ldu(&self, i: u32, j: u32) -> (i32, i32) {
        let along =
            |k: u32, n: u32| STUD_PITCH_LDU * k as i32 + HALF_STUD_LDU - HALF_STUD_LDU * n as i32;
        (along(i, self.studs[0]), along(j, self.studs[1]))
    }

    fn stud_positions(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let [nx, ny] = self.studs;
        (0..nx).flat_map(move |i| (0..ny).map(move |j| (i, j)))
    }
}

/// What an evaluation left out because it is not built yet; only filled when
/// [`EvalInput::skip_unsupported`] is set, and part of the content hash.
struct Skips {
    allowed: bool,
    items: Vec<String>,
}

fn plan_body(
    feat: &Feat,
    process: &ProcessProfile,
    offsets: &Offsets,
    skips: &mut Skips,
    warnings: &mut Vec<String>,
) -> Result<BodyPlan, EvalError> {
    feat.warn_unused(&["size", "height", "round"], warnings);
    let studs = feat
        .field("size")?
        .ok_or_else(|| feat.err("size", "is required, for example [2, 4] or \"$size\""))?
        .grid2()
        .map_err(|m| feat.err("size", m))?;
    if studs.iter().any(|&n| n > MAX_STUDS_PER_SIDE) {
        return Err(feat.err(
            "size",
            format!(
                "{} x {} studs is larger than the evaluator's limit of {MAX_STUDS_PER_SIDE} studs per side",
                studs[0], studs[1]
            ),
        ));
    }
    let default_height = if feat.f.op == "body.brick" {
        "brick"
    } else {
        "plate"
    };
    let plates = feat
        .field("height")?
        .unwrap_or_else(|| params::literal(default_height))
        .plates()
        .map_err(|m| feat.err("height", m))?;
    let height_ldu = i32::try_from(plates)
        .ok()
        .and_then(|p| p.checked_mul(PLATE_LDU))
        .filter(|_| plates <= MAX_BODY_PLATES)
        .ok_or_else(|| {
            feat.err(
                "height",
                format!(
                    "{plates} plates is taller than the evaluator's limit of {MAX_BODY_PLATES} plates ({} bricks)",
                    MAX_BODY_PLATES * PLATE_LDU as u32 / BRICK_LDU as u32
                ),
            )
        })?;
    let brick_plates = (BRICK_LDU / PLATE_LDU) as u32;
    // Whole bricks use the brick height row, so a brick is exactly brick_height.
    let height = f64::from(plates / brick_plates) * mm("brick_height")
        + f64::from(plates % brick_plates) * mm("plate_height");

    let mut top_chamfer = 0.0;
    if let Some(round) = feat.field("round")? {
        let round = round.obj().map_err(|m| feat.err("round", m))?;
        let tier = match round.get("tier") {
            Some(t) => t.str().map_err(|m| feat.err("round.tier", m))?.to_string(),
            None => "chamfer".to_string(),
        };
        if tier == "chamfer" {
            top_chamfer = round
                .get("radius")
                .ok_or_else(|| feat.err("round.radius", "is required for a chamfer"))?
                .mm()
                .map_err(|m| feat.err("round.radius", m))?;
            if top_chamfer < 0.0 {
                return Err(feat.err("round.radius", "must not be negative"));
            }
        } else if skips.allowed {
            warnings.push(format!(
                "feature `{}`: round tier `{tier}` is not built yet; the top edges stay sharp",
                feat.id()
            ));
            skips
                .items
                .push(format!("{}: round tier {tier}", feat.id()));
        } else {
            return Err(feat.unsupported(format!(" with round tier `{tier}`")));
        }
    }

    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let size = [
        f64::from(studs[0]) * pitch - 2.0 * gap,
        f64::from(studs[1]) * pitch - 2.0 * gap,
    ];
    let wall = process.quantise_wall(mm("side_wall"));
    let roof = roof_mm(process);
    if roof >= height {
        return Err(feat.fail(format!(
            "the roof ({roof} mm) leaves no cavity in a {height} mm body"
        )));
    }
    let narrow = size[0].min(size[1]);
    if 2.0 * wall >= narrow {
        return Err(feat.fail(format!(
            "two {wall} mm walls leave no cavity in a {narrow} mm wide body"
        )));
    }
    let cavity = height - roof;

    let chamfer = process
        .first_layer_chamfer_mm
        .max(offsets.first_layer_chamfer)
        .max(0.0);
    if 2.0 * chamfer >= wall || chamfer >= cavity {
        return Err(feat.fail(format!(
            "a {chamfer} mm first-layer chamfer on both sides would cut through the {wall} mm wall"
        )));
    }
    // The chamfer stands in for elephant-foot compensation, so the squashed first
    // layer spreads back towards the full wall; but a first layer narrower than
    // one line may be thinned or dropped by the slicer.
    let first = first_layer_width(process, wall, chamfer);
    if first < process.line_width_mm - 1e-9 {
        warnings.push(format!(
            "feature `{}`: the {chamfer} mm first-layer chamfer on both faces of the {wall} mm wall leaves {first:.3} mm in the first layer, narrower than one {} mm line, so the slicer may thin or drop it there; lower firstLayerChamfer in the fit profile",
            feat.id(),
            process.line_width_mm
        ));
    }
    if top_chamfer > roof {
        warnings.push(format!(
            "feature `{}`: top chamfer {top_chamfer} mm is capped at the roof thickness {roof} mm",
            feat.id()
        ));
        top_chamfer = roof;
    }

    let fillet = cap_wall_roof_chamfer(
        feat.id(),
        process.wall_roof_fillet(plates >= brick_plates),
        cavity,
        narrow,
        wall,
        process,
        warnings,
    );

    Ok(BodyPlan {
        id: feat.id().to_string(),
        tile: feat.f.op == "body.tile",
        studs,
        height_ldu,
        role: feat.role(),
        geom: BodyGeom {
            size,
            height,
            wall,
            roof,
            first_layer_chamfer: chamfer,
            top_chamfer,
            wall_roof_chamfer: fillet,
        },
    })
}

/// The wall-to-roof chamfer `want` of a body, capped so the cavity of depth
/// `cavity` keeps the depth a stud needs clear and some flat ceiling between
/// walls `wall` thick across a body `narrow` wide; a chamfer under
/// [`MIN_LENGTH_MM`] is none.
fn cap_wall_roof_chamfer(
    feature: &str,
    want: f64,
    cavity: f64,
    narrow: f64,
    wall: f64,
    process: &ProcessProfile,
    warnings: &mut Vec<String>,
) -> f64 {
    let depth_cap = (cavity - process.min_cavity_depth_mm).max(0.0);
    let width_cap = ((narrow - 2.0 * wall) / 2.0 - process.line_width_mm).max(0.0);
    let cap = depth_cap.min(width_cap);
    let mut fillet = want;
    if fillet > cap {
        warnings.push(format!(
            "feature `{feature}`: wall-to-roof chamfer {fillet} mm is capped at {cap:.3} mm so the cavity keeps {} mm of depth and a flat ceiling",
            process.min_cavity_depth_mm
        ));
        fillet = cap;
    }
    if fillet < MIN_LENGTH_MM {
        0.0
    } else {
        fillet
    }
}

/// The `grid` field: only `auto` (every stud position) so far.
fn check_grid(feat: &Feat) -> Result<(), EvalError> {
    match feat.field("grid")? {
        None => Ok(()),
        Some(FVal::Str(s)) if s == "auto" => Ok(()),
        Some(_) => Err(feat.unsupported(" with an explicit grid (only \"auto\" is built so far)")),
    }
}

struct StudPlan {
    geom: StudGeom,
    role: String,
}

fn plan_studs(
    feat: &Feat,
    body: &BodyPlan,
    process: &ProcessProfile,
    offsets: &Offsets,
    warnings: &mut Vec<String>,
    connectors: &mut Vec<Connector>,
    fit_critical: bool,
) -> Result<StudPlan, EvalError> {
    feat.warn_unused(&["grid", "studType", "logo"], warnings);
    match feat.face_of(&body.id)? {
        None | Some("top") => {}
        Some(face) => {
            return Err(feat.unsupported(format!(" on face `{face}` (only `top` is built so far)")))
        }
    }
    if body.tile {
        return Err(feat.fail(
            "a tile has a smooth top and no studs (docs/plan/06 section 4); use body.plate for a plate with studs",
        ));
    }
    check_grid(feat)?;
    if let Some(t) = feat.field("studType")? {
        let t = t.str().map_err(|m| feat.err("studType", m))?;
        if t != "solid" {
            return Err(feat.unsupported(format!(
                " with studType `{t}` (only solid studs are built so far)"
            )));
        }
    }
    if feat.f.fields.contains_key("logo") {
        return Err(feat
            .unsupported(" with a stud logo (OpenBrick3D never puts text or marks on stud tops)"));
    }
    let diameter = mm("stud_diameter") + offsets.stud_diameter;
    if diameter <= 0.0 {
        return Err(feat.fail(format!("stud diameter after offsets is {diameter} mm")));
    }
    let StudShape {
        height,
        top_chamfer,
        capped_from,
    } = stud_shape(process, diameter);
    if let Some(asked) = capped_from {
        warnings.push(format!(
            "feature `{}`: stud-top chamfer {asked} mm is capped at {top_chamfer:.3} mm",
            feat.id()
        ));
    }
    let centres = body
        .stud_positions()
        .map(|(i, j)| body.stud_centre(i, j))
        .collect();
    if !feat.f.off_grid {
        for (i, j) in body.stud_positions() {
            let (x, z) = body.stud_ldu(i, j);
            connectors.push(Connector {
                kind: "studM".into(),
                pos: [x, 0, z],
                dir: [0, -1, 0],
                sections: vec![ConnectorSection {
                    shape: SectionShape::Round,
                    radius_ldu: std_ldu("stud_diameter") / 2,
                    length_ldu: std_ldu("stud_height"),
                }],
                feature: feat.id().to_string(),
                fit_critical,
            });
        }
    }
    Ok(StudPlan {
        geom: StudGeom {
            centres,
            base: body.geom.height,
            diameter,
            height,
            top_chamfer,
            segments: process.stud_segments(),
        },
        role: feat.role(),
    })
}

struct TubePlan {
    geom: TubeGeom,
    role: String,
    tube_wall: Option<f64>,
    rib_depth: Option<f64>,
}

fn plan_tubes(
    feat: &Feat,
    body: &BodyPlan,
    process: &ProcessProfile,
    offsets: &Offsets,
    warnings: &mut Vec<String>,
    connectors: &mut Vec<Connector>,
    fit_critical: bool,
) -> Result<TubePlan, EvalError> {
    feat.warn_unused(&["grid"], warnings);
    match feat.face_of(&body.id)? {
        None | Some("bottom") => {}
        Some(face) => {
            return Err(
                feat.unsupported(format!(" on face `{face}` (only `bottom` is built so far)"))
            )
        }
    }
    check_grid(feat)?;
    let b = &body.geom;
    let [nx, ny] = body.studs;
    let (pitch, gap) = (mm("module"), mm("undersize_per_side"));
    let c = b.first_layer_chamfer;

    // Tubes at the 2x2 intersections of a part at least 2 x 2; solid pins at the
    // mid-points between studs of a 1-wide part (docs/plan/04 section 4).
    let at = |k: u32| f64::from(k) * pitch - gap;
    let mut tubes = Vec::new();
    let mut pins = Vec::new();
    if nx >= 2 && ny >= 2 {
        for i in 1..nx {
            for j in 1..ny {
                tubes.push([at(i), at(j)]);
            }
        }
    } else if nx == 1 {
        pins.extend((1..ny).map(|j| [pitch / 2.0 - gap, at(j)]));
    } else {
        pins.extend((1..nx).map(|i| [at(i), pitch / 2.0 - gap]));
    }

    let tube = offsets.tube(
        mm("tube_outer_diameter"),
        mm("tube_inner_diameter"),
        process.min_tube_wall_mm,
    );
    if !tubes.is_empty() && tube.grown_mm >= MIN_LENGTH_MM {
        warnings.push(format!(
            "feature `{}`: tube outer diameter grown by {:.3} mm so the tube wall stays at {} mm after the bore offset",
            feat.id(),
            tube.grown_mm,
            process.min_tube_wall_mm
        ));
    }
    if tube.inner_mm <= 0.0 {
        return Err(feat.fail(format!("tube bore after offsets is {} mm", tube.inner_mm)));
    }
    // A bottom chamfer on both sides of a tube wall must leave most of it standing.
    let tube_chamfer = c.min(tube.wall_mm() / 3.0);
    let first = first_layer_width(process, tube.wall_mm(), tube_chamfer);
    if !tubes.is_empty() && first < process.line_width_mm - 1e-9 {
        warnings.push(format!(
            "feature `{}`: the {tube_chamfer:.3} mm first-layer chamfer inside and outside the {:.3} mm tube wall leaves {first:.3} mm in the first layer, narrower than one {} mm line, so the slicer may thin or drop it there; lower firstLayerChamfer or the tubeInnerDiameter offset",
            feat.id(),
            tube.wall_mm(),
            process.line_width_mm
        ));
    }
    let pin_diameter = mm("pin_1wide_diameter") + offsets.pin_diameter;
    if !pins.is_empty() && pin_diameter <= 0.0 {
        return Err(feat.fail(format!("pin diameter after offsets is {pin_diameter} mm")));
    }
    let pin_chamfer = c.min(pin_diameter / 4.0);

    // Ribs: wall plus rib is the standard side_wall + rib_depth, so a wall that
    // quantising made thinner gets a deeper rib (docs/plan/04 section 6), then the
    // fit profile's ribDepth offset. One rib on each wall at every stud centre
    // line, where it touches the side of a stud below; as wide as the process's
    // minimum printable feature.
    // Depths under MIN_LENGTH_MM count as none: 1.2 + 0.3 - 1.2 - 0.3 leaves
    // 5.6e-17 mm, which the kernel cannot extrude.
    let rib_depth = nominal_rib_depth(b.wall) + offsets.rib_depth;
    let mut ribs = Vec::new();
    if rib_depth >= MIN_LENGTH_MM {
        let xs: Vec<f64> = (0..nx).map(|i| body.stud_centre(i, 0)[0]).collect();
        let ys: Vec<f64> = (0..ny).map(|j| body.stud_centre(0, j)[1]).collect();
        ribs = geometry::wall_ribs(b.size, b.wall, rib_depth, process.min_feature_mm, &xs, &ys);
    } else {
        warnings.push(format!(
            "feature `{}`: rib depth after offsets is {rib_depth:.3} mm (under {MIN_LENGTH_MM} mm), so the wall ribs are left out",
            feat.id()
        ));
    }

    if !feat.f.off_grid {
        let y = body.height_ldu;
        for (i, j) in body.stud_positions() {
            let (x, z) = body.stud_ldu(i, j);
            connectors.push(Connector {
                kind: "tubeF".into(),
                pos: [x, y, z],
                dir: [0, 1, 0],
                sections: vec![ConnectorSection {
                    shape: SectionShape::Round,
                    radius_ldu: std_ldu("stud_diameter") / 2,
                    length_ldu: std_ldu("plate_height"),
                }],
                feature: feat.id().to_string(),
                fit_critical,
            });
        }
    }

    Ok(TubePlan {
        tube_wall: (!tubes.is_empty()).then(|| tube.wall_mm()),
        rib_depth: (!ribs.is_empty()).then_some(rib_depth),
        geom: TubeGeom {
            tubes,
            tube_outer: tube.outer_mm,
            tube_inner: tube.inner_mm,
            tube_chamfer,
            pins,
            pin_diameter,
            pin_chamfer,
            ribs,
            rib_bottom: c,
            top: b.cavity_depth(),
            segments: process.stud_segments(),
        },
        role: feat.role(),
    })
}

/// Order of objects in [`Built::objects`].
fn role_rank(role: &str) -> (usize, &str) {
    let rank = ["body", "accent", "serial"]
        .iter()
        .position(|r| *r == role)
        .unwrap_or(3);
    (rank, role)
}

/// Evaluate a part document into meshes, connectors and measurements.
pub fn evaluate<K: Kernel>(kernel: &K, input: &EvalInput) -> Result<Built, EvalError> {
    let doc = input.doc;
    let process = input.process;
    doc.check()?;
    let resolved = resolve_params(doc, input.params, process)?;
    let scope = Scope::new(doc, &resolved, process);
    let offsets = Offsets::of(input.fit);
    let assets = RecordingAssets {
        inner: input.assets,
        seen: RefCell::new(BTreeMap::new()),
    };
    let mut warnings = Vec::new();
    if let Some(fit) = input.fit {
        if fit.process != process.id {
            warnings.push(format!(
                "fit profile `{}` was calibrated for process `{}`, not `{}` (rule FIT.PROCESS)",
                fit.id, fit.process, process.id
            ));
        }
    }

    // Active features, in document order; unsupported ops fail or are skipped.
    let mut skips = Skips {
        allowed: input.skip_unsupported,
        items: Vec::new(),
    };
    let mut active = Vec::new();
    for f in &doc.features {
        if let Some(cond) = &f.condition {
            let on = scope.condition(cond).map_err(|m| EvalError::Field {
                feature: f.id.clone(),
                field: "if".into(),
                message: format!("`{cond}`: {m}"),
            })?;
            if !on {
                continue;
            }
        }
        let feat = Feat { f, scope: &scope };
        if !SUPPORTED_OPS.contains(&f.op.as_str()) {
            if skips.allowed {
                warnings.push(format!(
                    "feature `{}` ({}) is not built by this evaluator yet and was skipped",
                    f.id, f.op
                ));
                skips.items.push(format!("{}: {}", f.id, f.op));
                continue;
            }
            return Err(feat.unsupported(""));
        }
        active.push(feat);
    }

    let fit_critical_ids = doc.fit_critical_features();
    // Rule FEATURE.MIN blocks thin or removed strokes of fit-critical artwork
    // (a label someone reads to calibrate) and only warns about other artwork.
    let fit_critical_artwork = active
        .iter()
        .any(|f| f.f.op.starts_with("relief.") && fit_critical_ids.iter().any(|i| i == f.id()));
    let mut connectors = Vec::new();
    let mut body: Option<BodyPlan> = None;
    let mut studs: Option<(String, StudPlan)> = None;
    let mut tubes: Option<(String, TubePlan)> = None;
    let mut relief: Option<ReliefPlan> = None;
    for feat in &active {
        let fit_critical = fit_critical_ids.iter().any(|i| i == feat.id());
        match feat.f.op.as_str() {
            "body.tile" | "body.plate" | "body.brick" => {
                if let Some(b) = &body {
                    return Err(feat.unsupported(format!(
                        " as a second body (the part already has `{}`)",
                        b.id
                    )));
                }
                body = Some(plan_body(
                    feat,
                    process,
                    &offsets,
                    &mut skips,
                    &mut warnings,
                )?);
            }
            "stud.field" => {
                let b = body.as_ref().ok_or(EvalError::NoBody)?;
                if let Some((other, _)) = &studs {
                    return Err(feat.fail(format!("the body already has the stud field `{other}`")));
                }
                let plan = plan_studs(
                    feat,
                    b,
                    process,
                    &offsets,
                    &mut warnings,
                    &mut connectors,
                    fit_critical,
                )?;
                studs = Some((feat.id().to_string(), plan));
            }
            "relief.svg" => {
                let b = body.as_ref().ok_or(EvalError::NoBody)?;
                if let Some(other) = &relief {
                    return Err(feat.fail(format!(
                        "the body already has the relief `{}`; one relief.svg per part is built so far, so combine the artwork into one SVG",
                        other.id
                    )));
                }
                relief = Some(relief::plan_relief(
                    kernel,
                    feat,
                    b,
                    process,
                    offsets.xy_scale,
                    &assets,
                    &mut warnings,
                )?);
            }
            _ => {
                let b = body.as_ref().ok_or(EvalError::NoBody)?;
                if let Some((other, _)) = &tubes {
                    return Err(feat.fail(format!("the body already has the tube field `{other}`")));
                }
                let plan = plan_tubes(
                    feat,
                    b,
                    process,
                    &offsets,
                    &mut warnings,
                    &mut connectors,
                    fit_critical,
                )?;
                tubes = Some((feat.id().to_string(), plan));
            }
        }
    }
    let mut body = body.ok_or(EvalError::NoBody)?;

    // Artwork never shares a face with studs (docs/plan/06 section 5: text and
    // artwork on flat top or bottom faces; docs/plan/05 section 6: the inlay
    // face is flat and stud-free). The top is the only face either is built on
    // so far, so the two together are an error with a fix hint.
    if let (Some(r), Some((stud_id, _))) = (&relief, &studs) {
        return Err(EvalError::Feature {
            feature: r.id.clone(),
            message: format!(
                "relief.svg and the stud field `{stud_id}` are both on the top face of `{}`, and artwork cannot share a face with studs; use body.tile (a smooth top) for a logo tile, or turn the stud field off with its `if`",
                body.id
            ),
        });
    }

    // A deboss or inlay thickens the roof by its depth, so the roof under the
    // artwork keeps its thickness; the whole ceiling moves down (see the module
    // comment of `relief`).
    let mut roof_under_artwork = None;
    if let Some(r) = relief.as_ref().filter(|r| r.mode.cuts()) {
        let roof = body.geom.roof;
        roof_under_artwork = Some(roof);
        body.geom.roof += r.height;
        let narrow = body.geom.size[0].min(body.geom.size[1]);
        body.geom.wall_roof_chamfer = cap_wall_roof_chamfer(
            &body.id,
            body.geom.wall_roof_chamfer,
            body.geom.cavity_depth(),
            narrow,
            body.geom.wall,
            process,
            &mut warnings,
        );
        // Tubes, pins and ribs reach past the new ceiling into the thickened
        // roof (by the depth, at most half the roof, so never up to the cut)
        // rather than ending in the ceiling plane, so the union merges them
        // through an overlap. Solids that only touch in a plane merge or stay
        // apart depending on rounding: measured on manifold-rust 0.19.1, a plain
        // 2x4 tile with a 1.4 mm roof keeps its three tubes as loose solids.
        if let Some((_, plan)) = &mut tubes {
            plan.geom.top = body.geom.cavity_depth() + r.height.min(roof / 2.0);
        }
    }

    // Geometry: every solid, tagged with its colour role, in a fixed order (body,
    // studs, tubes, pins, ribs, relief) so the meshes are reproducible.
    let g = Geo {
        k: kernel,
        xy: offsets.xy_scale,
    };
    let mut pieces: Vec<(String, K::Solid)> = Vec::new();
    let mut body_solid = body.geom.build(&g).map_err(kernel_err("the body"))?;
    let mut relief_solids = None;
    if let Some(r) = &relief {
        let what = format!("the relief of `{}`", r.id);
        // How far a relief's bonding copy reaches into the body: inside the
        // roof under the face (half of it, as thick as before any deboss).
        let roof = roof_under_artwork.unwrap_or(body.geom.roof);
        let sink = (roof / 2.0).min(geometry::OVERSHOOT_MM / 2.0);
        let (cut, solids) = r
            .build(&g, body_solid, body.geom.size, body.geom.height, sink)
            .map_err(kernel_err(&what))?;
        body_solid = cut;
        relief_solids = Some((r.role.clone(), solids));
    }
    pieces.push((body.role.clone(), body_solid));
    if let Some((id, plan)) = &studs {
        let what = format!("the studs of `{id}`");
        for s in plan.geom.build(&g).map_err(kernel_err(&what))? {
            pieces.push((plan.role.clone(), s));
        }
    }
    if let Some((id, plan)) = &tubes {
        let what = format!("the tubes of `{id}`");
        for s in plan.geom.build(&g).map_err(kernel_err(&what))? {
            pieces.push((plan.role.clone(), s));
        }
    }
    let mut feature_volume_sum: f64 = pieces.iter().map(|(_, s)| s.volume_mm3()).sum();

    // Per role: the body (when it has this role) and the feature solids.
    type Group<S> = (Option<S>, Vec<S>);
    let mut by_role: BTreeMap<(usize, String), Group<K::Solid>> = BTreeMap::new();
    for (i, (role, solid)) in pieces.into_iter().enumerate() {
        let (rank, name) = role_rank(&role);
        let group = by_role.entry((rank, name.to_string())).or_default();
        if i == 0 {
            group.0 = Some(solid);
        } else {
            group.1.push(solid);
        }
    }
    // The relief: its own object in its colour role, or, in the body's own
    // role, joined to the body through its bonding copy. `bond_for` names the
    // role whose object is the relief alone, for the fused part below.
    let mut bond = None;
    let mut bond_for = None;
    if let Some((role, solids)) = relief_solids {
        if let Some(object) = solids.object {
            feature_volume_sum += object.volume_mm3();
            let (rank, name) = role_rank(&role);
            let group = by_role.entry((rank, name.to_string())).or_default();
            match solids.bond {
                Some(b) if role == body.role => group.1.push(b),
                b => {
                    if group.0.is_none() && group.1.is_empty() {
                        bond_for = Some(role.clone());
                        bond = b;
                    }
                    group.1.push(object);
                }
            }
        }
    }
    let mut objects = Vec::new();
    let mut solids = Vec::new();
    for ((_, role), (base, features)) in by_role {
        let what = format!("the union of the {role} object");
        let solid = geometry::union_onto(kernel, base, features).map_err(kernel_err(&what))?;
        objects.push(RoleMesh {
            role: role.clone(),
            mesh: solid.to_mesh(),
            volume_mm3: solid.volume_mm3(),
        });
        solids.push((role, solid));
    }
    // The single-colour print: the objects touch but never overlap, so their
    // union has their total volume. The relief joins through its bonding copy,
    // so the result is one solid, not two shells touching in a plane.
    let fused = if solids.len() > 1 {
        let what = "the fused single-colour object";
        let parts: Vec<&K::Solid> = solids
            .iter()
            .map(|(role, solid)| match &bond {
                Some(b) if bond_for.as_ref() == Some(role) => b,
                _ => solid,
            })
            .collect();
        let mut all = kernel.union(parts[0], parts[1]).map_err(kernel_err(what))?;
        for s in &parts[2..] {
            all = kernel.union(&all, s).map_err(kernel_err(what))?;
        }
        Some(RoleMesh {
            role: body.role.clone(),
            mesh: all.to_mesh(),
            volume_mm3: all.volume_mm3(),
        })
    } else {
        None
    };
    let solids: Vec<K::Solid> = solids.into_iter().map(|(_, s)| s).collect();

    let xy = offsets.xy_scale;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for o in &objects {
        let (a, b) = o.mesh.bounds();
        for k in 0..3 {
            lo[k] = lo[k].min(f64::from(a[k]));
            hi[k] = hi[k].max(f64::from(b[k]));
        }
    }
    let mut fit_critical_connectors: Vec<String> = connectors
        .iter()
        .filter(|c| c.fit_critical)
        .map(|c| c.kind.clone())
        .collect();
    fit_critical_connectors.sort();
    fit_critical_connectors.dedup();
    let allowed_processes = doc
        .print
        .get("profiles")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let tube_plan = tubes.as_ref().map(|(_, p)| p);
    let flc = body.geom.first_layer_chamfer;
    let measurements = Measurements {
        process: process.id.clone(),
        fit_profile: input.fit.map(|f| f.id.clone()),
        fit_offsets: input.fit.map(|f| f.offsets_mm.clone()),
        allowed_processes,
        fit_critical_artwork,
        triangle_count: objects.iter().map(|o| o.mesh.triangle_count()).sum(),
        volume_mm3: objects.iter().map(|o| o.volume_mm3).sum(),
        feature_volume_sum_mm3: feature_volume_sum,
        bounds_min_mm: lo,
        bounds_max_mm: hi,
        is_manifold: solids.iter().all(|s| s.is_manifold()),
        genus: solids.iter().map(|s| s.genus()).sum(),
        components: solids.iter().map(|s| s.component_count()).sum(),
        body_height_mm: Some(body.geom.height),
        min_body_wall_mm: Some(body.geom.wall * xy),
        min_body_wall_at_bed_mm: Some((body.geom.wall - 2.0 * flc) * xy),
        min_tube_wall_mm: tube_plan.and_then(|p| p.tube_wall).map(|w| w * xy),
        min_tube_wall_at_bed_mm: tube_plan
            .and_then(|p| p.tube_wall.map(|w| (w - 2.0 * p.geom.tube_chamfer) * xy)),
        first_layer_chamfer_mm: Some(flc),
        // Under a deboss or inlay: the thinnest roof (under the artwork, as thick
        // as before the cut) and the cavity under the thickened roof.
        roof_mm: Some(roof_under_artwork.unwrap_or(body.geom.roof)),
        cavity_depth_mm: Some(body.geom.cavity_depth()),
        stud_diameter_mm: studs.as_ref().map(|(_, p)| p.geom.diameter * xy),
        tube_outer_diameter_mm: tube_plan
            .filter(|p| !p.geom.tubes.is_empty())
            .map(|p| p.geom.tube_outer * xy),
        tube_inner_diameter_mm: tube_plan
            .filter(|p| !p.geom.tubes.is_empty())
            .map(|p| p.geom.tube_inner * xy),
        pin_diameter_mm: tube_plan
            .filter(|p| !p.geom.pins.is_empty())
            .map(|p| p.geom.pin_diameter * xy),
        rib_depth_mm: tube_plan.and_then(|p| p.rib_depth).map(|d| d * xy),
        fit_critical_connectors,
        min_stroke_mm: relief.as_ref().and_then(|r| r.min_stroke).map(|s| s * xy),
        removed_area_pct: relief.as_ref().map(|r| r.removed_pct),
        relief_height_mm: relief.as_ref().map(|r| r.height),
    };

    let content = json!({
        "evaluator": EVALUATOR_REVISION,
        "standard": STANDARD.get().version,
        "kernel": kernel.id(),
        "doc": serde_json::to_value(doc).unwrap_or(Value::Null),
        "params": resolved.values,
        "process": process.id,
        "processProfile": serde_json::to_value(process).unwrap_or(Value::Null),
        "fit": input.fit.map(|f| f.id.clone()),
        "fitProfile": input.fit.map(serde_json::to_value).transpose().unwrap_or(None),
        "skipped": skips.items,
        "assets": assets.seen.borrow().clone(),
    });
    let content_hash = sha256_hex(canonical_json(&content).as_bytes());

    Ok(Built {
        objects,
        connectors,
        measurements,
        warnings,
        kernel_id: kernel.id(),
        content_hash,
        fused,
        artwork: relief.map(|r| r.info),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_read_are_recorded_by_content() {
        let store = MemoryAssets::new().with("assets/mark.svg", b"<svg/>".to_vec());
        let rec = RecordingAssets {
            inner: &store,
            seen: RefCell::new(BTreeMap::new()),
        };
        assert_eq!(rec.read("assets/mark.svg").expect("present"), b"<svg/>");
        assert_eq!(
            rec.seen.borrow().get("assets/mark.svg"),
            Some(&sha256_hex(b"<svg/>"))
        );
        match rec.read("missing.svg") {
            Err(EvalError::Asset { name, message }) => {
                assert_eq!(name, "missing.svg");
                assert!(message.contains("no asset named"));
            }
            other => panic!("expected an asset error, got {:?}", other.map(|b| b.len())),
        }
        assert_eq!(rec.seen.borrow().len(), 1, "failed reads are not recorded");
    }
}
