//! The `relief.svg` op (docs/plan/06 sections 5 and 7; docs/plan/05 sections 5
//! to 7): SVG artwork embossed on, debossed into or inlaid in the top face of
//! the body.
//!
//! Frame: millimetres in the kernel frame, like every other op (+Z up, body
//! minimum corner at the origin). The SVG's y axis points down, so it is flipped:
//! seen from above the top face, the artwork reads as it does in the SVG.
//!
//! Steps, in order:
//!
//! 1. The asset (a parameter of type `svg` names it) is read through the
//!    [`AssetResolver`](super::AssetResolver), so its bytes enter the content
//!    hash, and parsed as untrusted input by `svg.rs` (usvg, every transform
//!    applied, fills only).
//! 2. Fit: the artwork's exact bounding box (curve extrema included) is scaled
//!    into the face box and centred. The face box is the flat top of the body:
//!    its footprint after the standard undersize, inside the top chamfer
//!    (`round`), minus the `fit.margin`; `fit.keepAspect` (default true) keeps
//!    the proportions, else each axis fills the box.
//! 3. Flatten: curves become polylines at [`FLATTEN_TOLERANCE_MM`] on the part
//!    (after fitting), from the second-derivative bound of each Bezier segment.
//! 4. Fill rules: each SVG path is resolved under its own `fill-rule` (nonzero
//!    or evenodd, through `openbrick_geom::region`, so two nested circles drawn
//!    the same way make a ring under evenodd and a disc under nonzero), then all
//!    paths are unioned.
//! 5. Morphological opening with `minFeature` (default: the process's
//!    `minFeatureMm`): inset by half of it, then outset by half of it, with
//!    `Kernel::offset2d` (round joins), clipped to the artwork. Strokes thinner
//!    than `minFeature` disappear; the removed region is kept for the preview and
//!    its share of the area is `removed_area_pct`. The round joins follow the
//!    kernel's default circle quality, which is coarse for small radii: on
//!    manifold-rust 0.19.1 a 0.4 mm quarter circle is a single chord, so the
//!    opening bevels convex corners at 45 degrees rather than rounding them
//!    (0.4 mm legs; invisible at a 0.4 mm nozzle).
//! 6. The mode: `emboss` extrudes the artwork by `height` on top of the face, as
//!    a separate object of `colorRole` that touches the body without overlapping
//!    it; `deboss` cuts it `height` deep into the face; `inlay` cuts it the same
//!    way and splits off the face slab as the `colorRole` object (docs/plan/05
//!    section 6), printed face down.
//!
//! Deboss and inlay thicken the roof by the depth so the roof under the artwork
//! keeps its thickness (rule `ROOF.MIN`), and clamp the depth so the cavity keeps
//! `minCavityDepthMm` (rule `CAVITY.DEPTH`; 0.4 mm on plate-height parts). The
//! whole cavity ceiling is lowered rather than only the part under the artwork:
//! a pad in the shape of the artwork hanging from the ceiling would be an
//! unsupported island when the part prints face up, while a lowered ceiling is
//! still one flat bridge between the walls and the tubes.

use openbrick_geom::region::{self, FillRule};
use openbrick_geom::{CrossSection, GeomError, Kernel, Polygon};
use openbrick_print::ProcessProfile;

use super::geometry::{Geo, OVERSHOOT_MM};
use super::params::FVal;
use super::svg::{self, Seg, Shape};
use super::{BodyPlan, EvalError, Feat, RecordingAssets, MIN_LENGTH_MM};

/// Curves are flattened so no point of a curve is farther than this from its
/// polyline, measured on the part after fitting (docs/plan/06 section 5).
pub const FLATTEN_TOLERANCE_MM: f64 = 0.05;

/// Most points of the flattened artwork: a resource guard (the kernel
/// triangulates every point twice), not a brick dimension.
pub const MAX_ARTWORK_POINTS: usize = 200_000;

/// Share of the artwork area the opening may remove before the evaluator warns,
/// in percent. Opening a 13.2 mm square at 0.8 mm only bevels its corners and
/// removes 0.18 %, which is not worth a warning; a hairline usually removes more
/// (the sample mark's 1.1 %).
pub const REMOVED_WARN_PCT: f64 = 1.0;

/// A deboss or inlay depth that has to be clamped is rounded down to this grid
/// (one micrometre, the evaluator's zero length) and checked again, so
/// floating-point rounding never leaves the cavity a hair under
/// `minCavityDepthMm`.
const DEPTH_GRID_MM: f64 = MIN_LENGTH_MM;

/// How the artwork meets the face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Emboss,
    Deboss,
    Inlay,
}

impl Mode {
    fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "emboss" => Mode::Emboss,
            "deboss" => Mode::Deboss,
            "inlay" => Mode::Inlay,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Mode::Emboss => "emboss",
            Mode::Deboss => "deboss",
            Mode::Inlay => "inlay",
        }
    }

    /// Deboss and inlay cut into the face (and thicken the roof).
    pub(crate) fn cuts(self) -> bool {
        self != Mode::Emboss
    }
}

/// What a `relief.svg` feature did with its artwork, for the preview, the
/// exporter and the validator. Lengths in millimetres; polygons in the kernel
/// frame (x, y, with the fit profile's `xyScale` applied), outer contours
/// counter-clockwise and holes clockwise.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    /// The feature id.
    pub feature: String,
    /// `emboss`, `deboss` or `inlay`.
    pub mode: String,
    /// The face the part prints on: `bottom` (face up) for emboss and deboss,
    /// `top` (face down) for inlay, so the exporter and the UI can lay it out.
    pub face_on_bed: String,
    /// Height of the face the artwork sits on (the body top).
    pub face_z_mm: f64,
    /// The height (emboss) or depth (deboss, inlay) built.
    pub height_mm: f64,
    /// The height the document asked for; larger than `height_mm` when the
    /// depth was clamped to keep the cavity.
    pub requested_height_mm: f64,
    /// The face box the artwork was fitted into (minimum and maximum corner).
    pub box_min_mm: [f64; 2],
    pub box_max_mm: [f64; 2],
    /// The opening width (`minFeature`).
    pub min_feature_mm: f64,
    /// Area of the artwork after fitting, before the opening, and after it.
    pub area_before_mm2: f64,
    pub area_after_mm2: f64,
    /// Regions the opening removed (draw them in red in the preview), at
    /// `face_z_mm`.
    pub removed: Vec<Polygon>,
    /// Paths whose stroke was ignored (v1 builds fills only).
    pub strokes_ignored: usize,
}

/// A planned relief.
pub(crate) struct ReliefPlan {
    pub id: String,
    pub mode: Mode,
    pub role: String,
    /// The height (emboss) or depth (deboss, inlay) to build.
    pub height: f64,
    /// The opened artwork in design millimetres (before `xyScale`).
    pub art: CrossSection,
    pub removed_pct: f64,
    /// Thinnest surviving stroke, design millimetres (see [`min_stroke`]).
    pub min_stroke: Option<f64>,
    pub info: Artwork,
}

/// A map from SVG canvas units to design millimetres: x' = sx x + tx,
/// y' = ty - sy y (the y flip).
#[derive(Clone, Copy, Debug)]
struct Fit {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}

impl Fit {
    fn map(&self, p: [f64; 2]) -> [f64; 2] {
        [self.sx * p[0] + self.tx, self.ty - self.sy * p[1]]
    }
}

/// Parameters of a Bezier coordinate where its derivative is zero, in (0, 1).
fn extrema_params(c: &[f64]) -> Vec<f64> {
    let inside = |t: f64| t > 0.0 && t < 1.0;
    match *c {
        [p0, p1, p2] => {
            let den = p0 - 2.0 * p1 + p2;
            if den == 0.0 {
                return Vec::new();
            }
            let t = (p0 - p1) / den;
            if inside(t) {
                vec![t]
            } else {
                Vec::new()
            }
        }
        [p0, p1, p2, p3] => {
            // B'(t) / 3 = a t^2 + b t + c.
            let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
            let b = 2.0 * (p0 - 2.0 * p1 + p2);
            let c = p1 - p0;
            let mut out = Vec::new();
            if a.abs() < 1e-12 * (b.abs() + c.abs()).max(f64::MIN_POSITIVE) {
                if b != 0.0 {
                    out.push(-c / b);
                }
            } else {
                let disc = b * b - 4.0 * a * c;
                if disc >= 0.0 {
                    let s = disc.sqrt();
                    out.push((-b + s) / (2.0 * a));
                    out.push((-b - s) / (2.0 * a));
                }
            }
            out.retain(|&t| inside(t));
            out
        }
        _ => Vec::new(),
    }
}

fn quad_at(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    [0, 1].map(|k| u * u * p0[k] + 2.0 * u * t * p1[k] + t * t * p2[k])
}

fn cubic_at(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    [0, 1].map(|k| {
        u * u * u * p0[k] + 3.0 * u * u * t * p1[k] + 3.0 * u * t * t * p2[k] + t * t * t * p3[k]
    })
}

/// The exact bounding box of the shapes (curve extrema included).
fn tight_bounds(shapes: &[Shape]) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut add = |p: [f64; 2]| {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    for sub in shapes.iter().flat_map(|s| &s.subpaths) {
        let mut from = sub.start;
        add(from);
        for seg in &sub.segs {
            match *seg {
                Seg::Line(p) => {
                    add(p);
                    from = p;
                }
                Seg::Quad(a, p) => {
                    for k in 0..2 {
                        for t in extrema_params(&[from[k], a[k], p[k]]) {
                            add(quad_at(from, a, p, t));
                        }
                    }
                    add(p);
                    from = p;
                }
                Seg::Cubic(a, b, p) => {
                    for k in 0..2 {
                        for t in extrema_params(&[from[k], a[k], b[k], p[k]]) {
                            add(cubic_at(from, a, b, p, t));
                        }
                    }
                    add(p);
                    from = p;
                }
            }
        }
    }
    (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some((lo, hi))
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Segments for a curve whose second derivative is at most `max_d2` in length:
/// a chord over a parameter step h deviates at most h^2 max_d2 / 8 from the curve.
fn pieces(max_d2: f64, tolerance: f64) -> usize {
    let n = (max_d2 / (8.0 * tolerance)).sqrt().ceil();
    if n.is_finite() && n >= 1.0 {
        n.min(MAX_ARTWORK_POINTS as f64) as usize
    } else {
        1
    }
}

/// Map every shape to millimetres with `fit` and flatten it into polygons, one
/// list per shape. Mapping first measures the tolerance on the part.
fn flatten(
    shapes: &[Shape],
    fit: Fit,
    tolerance: f64,
) -> Result<Vec<(FillRule, Vec<Polygon>)>, String> {
    let mut total = 0usize;
    let mut out = Vec::new();
    for shape in shapes {
        let mut contours = Vec::new();
        for sub in &shape.subpaths {
            let mut pts = vec![fit.map(sub.start)];
            let mut from = pts[0];
            for seg in &sub.segs {
                match *seg {
                    Seg::Line(p) => {
                        from = fit.map(p);
                        pts.push(from);
                    }
                    Seg::Quad(a, p) => {
                        let (a, p) = (fit.map(a), fit.map(p));
                        // B'' = 2 (p0 - 2 p1 + p2).
                        let d = [0, 1].map(|k| from[k] - 2.0 * a[k] + p[k]);
                        let n = pieces(2.0 * d[0].hypot(d[1]), tolerance);
                        total += n;
                        if total > MAX_ARTWORK_POINTS {
                            break;
                        }
                        pts.extend((1..=n).map(|i| quad_at(from, a, p, i as f64 / n as f64)));
                        from = p;
                    }
                    Seg::Cubic(a, b, p) => {
                        let (a, b, p) = (fit.map(a), fit.map(b), fit.map(p));
                        // |B''| <= 6 max(|p0 - 2 p1 + p2|, |p1 - 2 p2 + p3|).
                        let d1 = [0, 1].map(|k| from[k] - 2.0 * a[k] + b[k]);
                        let d2 = [0, 1].map(|k| a[k] - 2.0 * b[k] + p[k]);
                        let m = d1[0].hypot(d1[1]).max(d2[0].hypot(d2[1]));
                        let n = pieces(6.0 * m, tolerance);
                        total += n;
                        if total > MAX_ARTWORK_POINTS {
                            break;
                        }
                        pts.extend((1..=n).map(|i| cubic_at(from, a, b, p, i as f64 / n as f64)));
                        from = p;
                    }
                }
            }
            total += pts.len();
            if total > MAX_ARTWORK_POINTS {
                return Err(format!(
                    "the artwork flattens to more than {MAX_ARTWORK_POINTS} points; simplify it"
                ));
            }
            // A closed contour: drop repeated points and the closing repeat.
            pts.dedup_by(|a, b| dist(*a, *b) < 1e-9);
            while pts.len() > 1 && dist(pts[0], pts[pts.len() - 1]) < 1e-9 {
                pts.pop();
            }
            if pts.len() >= 3 {
                contours.push(Polygon { points: pts });
            }
        }
        out.push((shape.rule, contours));
    }
    Ok(out)
}

/// Morphological opening by a disc of radius `r`: inset by `r`, outset by `r`,
/// clipped to the input so the polygonal round joins never add area.
fn opening<K: Kernel>(k: &K, s: &CrossSection, r: f64) -> Result<CrossSection, GeomError> {
    if r <= 0.0 || s.polygons.is_empty() {
        return Ok(s.clone());
    }
    let inset = k.offset2d(s, -r)?;
    if inset.polygons.is_empty() {
        return Ok(CrossSection::default());
    }
    Ok(region::intersection(&k.offset2d(&inset, r)?, s))
}

fn bounds_of(s: &CrossSection) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in s.polygons.iter().flat_map(|p| &p.points) {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some((lo, hi))
}

/// Radii [`min_stroke`] tries grow by this factor until a stroke disappears.
const STROKE_SCAN_STEP: f64 = 1.25;

/// [`min_stroke`] refines the radius to this, in millimetres (0.01 mm on the
/// stroke width).
const STROKE_RESOLUTION_MM: f64 = 0.005;

/// An estimate of the thinnest stroke of `art`, which has already been opened
/// at radius `r0`; `None` when `art` is empty.
///
/// Every point of an opened shape lies in a disc of diameter `2 r0` inside it,
/// so its plain minimum width is `2 r0` by construction (at the corners); this
/// measures the strokes instead. Opening `art` again at a radius r > r0 removes
/// two kinds of region: small pieces at convex corners, which the joins cut back
/// further (for a corner of opening angle a, area (r^2 - r0^2) times
/// 1 / tan(a / 2) - (pi - a) / 2 with true arcs, or sin(a) / (2 tan^2(a / 2))
/// with the bevels the kernel makes at small radii, so under r^2 for corners of
/// about 70 degrees or more); and whole strokes or necks narrower than 2 r,
/// whose area is about their width times their length, at least r^2 just above
/// their half width for a stroke at least half as long as it is wide. So the
/// estimate is twice the smallest r at which one removed piece reaches r^2 of
/// area: radii from r0 up grow by [`STROKE_SCAN_STEP`] until that happens (from
/// the thin end, so a thin stroke is found before larger radii remove
/// everything), then bisection refines it to [`STROKE_RESOLUTION_MM`]. A piece
/// is an outer contour of the removed region with its holes not subtracted,
/// which can only make it look larger, so the estimate errs thin, except that a
/// stub shorter than half its width is not counted as a stroke. Sharp corners
/// count as thin: near their tip they are narrower than 2 r. A solid disc of
/// diameter D measures D, a square of side s measures s, a ring its width, the
/// blocks of the hairline fixture their width (tests/eval_relief.rs). When no
/// piece ever counts as a stroke the result is `2 r0`, what the opening
/// guarantees. Cost: one opening per step, about 20 for a 13 mm wide face.
pub(crate) fn min_stroke<K: Kernel>(
    k: &K,
    art: &CrossSection,
    r0: f64,
) -> Result<Option<f64>, GeomError> {
    let Some((lo, hi)) = bounds_of(art) else {
        return Ok(None);
    };
    let removes_a_stroke = |r: f64| -> Result<bool, GeomError> {
        let removed = region::difference(art, &opening(k, art, r)?);
        Ok(removed
            .polygons
            .iter()
            .map(Polygon::signed_area)
            .any(|a| a >= r * r))
    };
    // No disc wider than the bounding box fits inside the artwork, so opening
    // at `top` removes all of it.
    let top = (hi[0] - lo[0]).min(hi[1] - lo[1]) / 2.0 + 2.0 * STROKE_RESOLUTION_MM;
    // Scan up from the thin end: `below` removes no stroke, `above` does.
    let mut below = r0.max(STROKE_RESOLUTION_MM);
    let mut above = loop {
        let r = (below * STROKE_SCAN_STEP).min(top);
        if removes_a_stroke(r)? {
            break r;
        }
        if r >= top {
            // No removed piece ever counted as a stroke: nothing measured, so
            // report what the opening guarantees.
            return Ok(Some(2.0 * r0.max(0.0)));
        }
        below = r;
    };
    while above - below > STROKE_RESOLUTION_MM {
        let mid = (above + below) / 2.0;
        if removes_a_stroke(mid)? {
            above = mid;
        } else {
            below = mid;
        }
    }
    Ok(Some(2.0 * below))
}

fn length_field(feat: &Feat, name: &str, v: &FVal) -> Result<f64, EvalError> {
    let x = v.mm().map_err(|m| feat.err(name, m))?;
    if !x.is_finite() {
        return Err(feat.err(name, "must be a finite length"));
    }
    Ok(x)
}

fn scaled(s: &CrossSection, xy: f64) -> CrossSection {
    CrossSection::from(
        s.polygons
            .iter()
            .map(|p| Polygon {
                points: p.points.iter().map(|q| [q[0] * xy, q[1] * xy]).collect(),
            })
            .collect(),
    )
}

/// Plan a `relief.svg` feature on the top face of `body`: read and fit the
/// artwork, open it, measure it and clamp the depth.
pub(crate) fn plan_relief<K: Kernel>(
    kernel: &K,
    feat: &Feat,
    body: &BodyPlan,
    process: &ProcessProfile,
    xy: f64,
    assets: &RecordingAssets,
    warnings: &mut Vec<String>,
) -> Result<ReliefPlan, EvalError> {
    feat.warn_unused(&["src", "mode", "height", "fit", "minFeature"], warnings);
    match feat.face_of(&body.id)? {
        None | Some("top") => {}
        Some(face) => {
            return Err(feat.unsupported(format!(" on face `{face}` (only `top` is built so far)")))
        }
    }
    let mode = match feat.field("mode")? {
        None => Mode::Emboss,
        Some(v) => {
            let s = v.str().map_err(|m| feat.err("mode", m))?;
            Mode::parse(s).ok_or_else(|| {
                feat.err(
                    "mode",
                    format!("`{s}` is not one of emboss, deboss or inlay"),
                )
            })?
        }
    };
    let requested = feat
        .field("height")?
        .ok_or_else(|| feat.err("height", "is required, for example \"0.6mm\""))?;
    let requested = length_field(feat, "height", &requested)?;
    if requested < MIN_LENGTH_MM {
        return Err(feat.err(
            "height",
            format!("{requested} mm is too small; a relief needs at least {MIN_LENGTH_MM} mm"),
        ));
    }
    let min_feature = match feat.field("minFeature")? {
        None => process.min_feature_mm,
        Some(v) => length_field(feat, "minFeature", &v)?,
    };
    if min_feature < 0.0 {
        return Err(feat.err("minFeature", "must not be negative"));
    }

    // fit: margin, align, keepAspect, box.
    let mut margin = 0.0;
    let mut keep_aspect = true;
    if let Some(fit) = feat.field("fit")? {
        let fit = fit.obj().map_err(|m| feat.err("fit", m))?;
        for (key, v) in fit {
            match key.as_str() {
                "margin" => {
                    margin = length_field(feat, "fit.margin", v)?;
                    if margin < 0.0 {
                        return Err(feat.err("fit.margin", "must not be negative"));
                    }
                }
                "keepAspect" => match v {
                    FVal::Bool(b) => keep_aspect = *b,
                    _ => return Err(feat.err("fit.keepAspect", "expected true or false")),
                },
                "align" => {
                    let a = v.str().map_err(|m| feat.err("fit.align", m))?;
                    if a != "center" {
                        return Err(feat.unsupported(format!(
                            " with fit.align `{a}` (only \"center\" is built so far)"
                        )));
                    }
                }
                "box" => {
                    let b = v.str().map_err(|m| feat.err("fit.box", m))?;
                    if b != "face" {
                        return Err(feat.unsupported(format!(
                            " with fit.box `{b}` (only \"face\" is built so far)"
                        )));
                    }
                }
                other => warnings.push(format!(
                    "feature `{}`: field `fit.{other}` is not used by relief.svg",
                    feat.id()
                )),
            }
        }
    }

    // The face box: flat top inside the top chamfer, minus the margin.
    let g = &body.geom;
    let inset = g.top_chamfer + margin;
    let box_min = [inset, inset];
    let box_max = [g.size[0] - inset, g.size[1] - inset];
    let box_size = [box_max[0] - box_min[0], box_max[1] - box_min[1]];
    if box_size[0] < MIN_LENGTH_MM || box_size[1] < MIN_LENGTH_MM {
        return Err(feat.err(
            "fit.margin",
            format!(
                "a {margin} mm margin inside the {} mm top chamfer leaves no room on the {:.3} x {:.3} mm top face; lower the margin",
                g.top_chamfer, g.size[0], g.size[1]
            ),
        ));
    }

    // The artwork.
    let name = feat.field("src")?.ok_or_else(|| {
        feat.err(
            "src",
            "is required: the name of an SVG asset, such as \"$logo\"",
        )
    })?;
    let name = name.str().map_err(|m| feat.err("src", m))?.to_string();
    let bytes = assets.read(&name)?;
    let asset_err = |message: String| EvalError::Asset {
        name: name.clone(),
        message,
    };
    let parsed = svg::parse(&bytes).map_err(asset_err)?;
    let ignored = [
        (parsed.strokes, "stroke", "strokes become outlines in v1.1 (docs/plan/06 section 5); convert them to paths in your editor (Inkscape: Path > Stroke to Path)"),
        (parsed.texts, "<text> element", "convert text to paths in your editor (Inkscape: Path > Object to Path)"),
        (parsed.images, "<image> element", "relief artwork is vector only; trace images first"),
        (parsed.clip_paths, "clip path", "the clipped shapes are built whole; apply the clip in your editor (Inkscape: Path > Intersection)"),
        (parsed.masks, "mask", "the masked shapes are built whole"),
        (parsed.filters, "filter", "filters have no outline"),
    ];
    for (count, what, hint) in ignored {
        if count > 0 {
            let s = if count == 1 { "" } else { "s" };
            warnings.push(format!(
                "feature `{}`: {count} {what}{s} in `{name}` ignored (v1 builds filled shapes only); {hint}",
                feat.id()
            ));
        }
    }
    let no_fill = || {
        asset_err(
            "the SVG has no filled shapes to build (v1 builds fills; strokes, text and images are ignored)"
                .into(),
        )
    };
    let (lo, hi) = tight_bounds(&parsed.shapes).ok_or_else(no_fill)?;
    let size = [hi[0] - lo[0], hi[1] - lo[1]];
    if !(size[0] > 0.0 && size[1] > 0.0 && size[0].is_finite() && size[1].is_finite()) {
        return Err(asset_err(format!(
            "the filled shapes span {} x {} units, which has no area",
            size[0], size[1]
        )));
    }
    let (mut sx, mut sy) = (box_size[0] / size[0], box_size[1] / size[1]);
    if keep_aspect {
        let s = sx.min(sy);
        (sx, sy) = (s, s);
    }
    let centre = [
        (box_min[0] + box_max[0]) / 2.0,
        (box_min[1] + box_max[1]) / 2.0,
    ];
    let fit = Fit {
        sx,
        sy,
        tx: centre[0] - sx * (lo[0] + hi[0]) / 2.0,
        ty: centre[1] + sy * (lo[1] + hi[1]) / 2.0,
    };
    let flat = flatten(&parsed.shapes, fit, FLATTEN_TOLERANCE_MM).map_err(asset_err)?;
    let filled: Vec<CrossSection> = flat
        .iter()
        .map(|(rule, contours)| region::fill(contours, *rule))
        .collect();
    let art = region::union_all(&filled);
    let area_before = art.area();
    // Also catches a NaN area.
    if area_before.is_nan() || area_before <= MIN_LENGTH_MM * MIN_LENGTH_MM {
        return Err(no_fill());
    }

    // The opening.
    let r0 = min_feature / 2.0;
    let what = format!("the artwork opening of `{}`", feat.id());
    let opened = opening(kernel, &art, r0).map_err(super::kernel_err(&what))?;
    let area_after = opened.area();
    if opened.polygons.is_empty() || area_after <= MIN_LENGTH_MM * MIN_LENGTH_MM {
        return Err(feat.fail(format!(
            "every stroke of `{name}` is thinner than minFeature {min_feature} mm once fitted into the {:.1} x {:.1} mm face box, so the opening removed all of it; use bolder artwork, a smaller margin or a larger part",
            box_size[0], box_size[1]
        )));
    }
    let removed_pct = (100.0 * (area_before - area_after) / area_before).max(0.0);
    if removed_pct >= REMOVED_WARN_PCT {
        warnings.push(format!(
            "feature `{}`: the {min_feature} mm opening (minFeature) removed {removed_pct:.1} % of the artwork area: strokes thinner than {min_feature} mm cannot print on {}; the removed regions are in the preview",
            feat.id(),
            process.id
        ));
    }
    let removed = region::difference(&art, &opened);
    let min_stroke = min_stroke(kernel, &opened, r0).map_err(super::kernel_err(&what))?;

    // The depth of a cut keeps the cavity (docs/plan/05 section 6).
    let mut height = requested;
    if mode.cuts() {
        let cavity = g.cavity_depth();
        let room = cavity - process.min_cavity_depth_mm;
        let mut depth = if requested <= room {
            requested
        } else {
            // The slack keeps a room of 0.39999999999999997 at 0.4.
            (room / DEPTH_GRID_MM + 1e-9).floor() * DEPTH_GRID_MM
        };
        while depth > 0.0 && g.height - (g.roof + depth) < process.min_cavity_depth_mm {
            depth -= DEPTH_GRID_MM;
        }
        if depth < MIN_LENGTH_MM {
            return Err(feat.fail(format!(
                "no room to {}: the cavity under the {} mm roof is {cavity:.3} mm deep and must keep {} mm (minCavityDepthMm of {}); use emboss, or a taller body",
                mode.name(),
                g.roof,
                process.min_cavity_depth_mm,
                process.id
            )));
        }
        if depth < requested - 1e-9 {
            warnings.push(format!(
                "feature `{}`: {} depth {requested} mm is clamped to {depth:.3} mm so the cavity keeps {} mm (minCavityDepthMm of {}); the cavity under the thickened roof measures {:.3} mm (rule CAVITY.DEPTH)",
                feat.id(),
                mode.name(),
                process.min_cavity_depth_mm,
                process.id,
                g.height - (g.roof + depth)
            ));
        }
        height = depth;
    }
    if mode == Mode::Inlay {
        warnings.push(format!(
            "feature `{}`: inlay prints face down: put the top face on the bed and swap filament after the {height:.3} mm face slab (the `{}` object), so the artwork shows the body colour",
            feat.id(),
            feat.role()
        ));
        if feat.role() == body.role {
            warnings.push(format!(
                "feature `{}`: inlay with colorRole `{}`, the body's own role, is a plain deboss printed face down; set colorRole to `accent`",
                feat.id(),
                body.role
            ));
        }
    }

    let info = Artwork {
        feature: feat.id().to_string(),
        mode: mode.name().to_string(),
        face_on_bed: if mode == Mode::Inlay { "top" } else { "bottom" }.to_string(),
        face_z_mm: g.height,
        height_mm: height,
        requested_height_mm: requested,
        box_min_mm: box_min.map(|v| v * xy),
        box_max_mm: box_max.map(|v| v * xy),
        min_feature_mm: min_feature,
        area_before_mm2: area_before * xy * xy,
        area_after_mm2: area_after * xy * xy,
        removed: scaled(&removed, xy).polygons,
        strokes_ignored: parsed.strokes,
    };
    Ok(ReliefPlan {
        id: feat.id().to_string(),
        mode,
        role: feat.role(),
        height,
        art: opened,
        removed_pct,
        min_stroke,
        info,
    })
}

/// The objects a relief adds to the part.
pub(crate) struct ReliefSolids<S> {
    /// The relief's own object (the embossed artwork or the inlay's face slab),
    /// touching the body without overlapping it; `None` for a deboss.
    pub object: Option<S>,
    /// The same object reaching `sink` into the body, for unions with it (the
    /// fused single-colour part, or a relief in the body's own colour). The
    /// overlap lies inside material both already have, so the union is the
    /// same solid, but it merges through a shared volume instead of relying on
    /// two faces meeting exactly in one plane.
    pub bond: Option<S>,
}

impl ReliefPlan {
    /// The artwork extruded from `z0` to `z1`, `xyScale` applied.
    fn prism<K: Kernel>(&self, g: &Geo<K>, z0: f64, z1: f64) -> Result<K::Solid, GeomError> {
        let s = g.k.extrude(&scaled(&self.art, g.xy), z1 - z0, 1.0)?;
        Ok(g.k.translate(&s, [0.0, 0.0, z0]))
    }

    /// Build the relief on the face at height `top` of `body`, a body whose
    /// roof under the face is at least `2 sink` thick (and `sink` is under
    /// `OVERSHOOT_MM`). Returns the body to use from now on (cut for deboss
    /// and inlay) and the relief's objects.
    pub(crate) fn build<K: Kernel>(
        &self,
        g: &Geo<K>,
        body: K::Solid,
        size: [f64; 2],
        top: f64,
        sink: f64,
    ) -> Result<(K::Solid, ReliefSolids<K::Solid>), GeomError> {
        let h = self.height;
        match self.mode {
            // The artwork standing `h` on the face.
            Mode::Emboss => Ok((
                body,
                ReliefSolids {
                    object: Some(self.prism(g, top, top + h)?),
                    bond: Some(self.prism(g, top - sink, top + h)?),
                },
            )),
            // The artwork cut `h` deep into the face; the cutter starts inside
            // the roof and ends above the body, so it shares no face with it.
            Mode::Deboss => {
                let cutter = self.prism(g, top - h, top + OVERSHOOT_MM)?;
                Ok((
                    g.k.difference(&body, &cutter)?,
                    ReliefSolids {
                        object: None,
                        bond: None,
                    },
                ))
            }
            // The debossed body split in the plane `top - h`: the part below,
            // and the face slab minus the artwork. They touch in that plane and
            // do not overlap.
            Mode::Inlay => {
                let split = top - h;
                // Reaches past the body on every side, so no face of the slab
                // box is coplanar with a face of the body.
                let slab_box = |z0: f64| {
                    g.prism(
                        [-OVERSHOOT_MM, -OVERSHOOT_MM],
                        [size[0] + OVERSHOOT_MM, size[1] + OVERSHOOT_MM],
                        z0,
                        top + OVERSHOOT_MM,
                    )
                };
                let upper = slab_box(split)?;
                let below = g.k.difference(&body, &upper)?;
                // The cutter starts below both slab boxes, so it shares no face
                // with either.
                let cutter = self.prism(g, split - OVERSHOOT_MM, top + OVERSHOOT_MM)?;
                let slab = g.k.difference(&g.k.intersection(&body, &upper)?, &cutter)?;
                let bond =
                    g.k.difference(&g.k.intersection(&body, &slab_box(split - sink)?)?, &cutter)?;
                Ok((
                    below,
                    ReliefSolids {
                        object: Some(slab),
                        bond: Some(bond),
                    },
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_extrema_are_found() {
        // x(t) of a symmetric arch: 0, 1, 1, 0 peaks at t = 0.5 with 0.75.
        let t = extrema_params(&[0.0, 1.0, 1.0, 0.0]);
        assert_eq!(t.len(), 1);
        assert!((t[0] - 0.5).abs() < 1e-12);
        assert!(
            (cubic_at([0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 0.0], 0.5)[0] - 0.75).abs() < 1e-12
        );
        // A quadratic 0, 2, 0 peaks at t = 0.5 with 1.
        assert_eq!(extrema_params(&[0.0, 2.0, 0.0]), vec![0.5]);
        assert!(extrema_params(&[0.0, 1.0, 2.0, 3.0]).is_empty(), "monotone");
    }

    #[test]
    fn flattening_meets_the_tolerance() {
        // A quarter circle as one cubic (control distance 0.5523 r) of radius
        // 10: every flattened point lies on the curve, and the chords stay within
        // the tolerance of it, so within tolerance plus the cubic's own 2.7e-4 r
        // error of the true circle.
        let r = 10.0;
        let c = 0.552_284_749_8 * r;
        let shape = Shape {
            rule: FillRule::NonZero,
            subpaths: vec![svg::Subpath {
                start: [r, 0.0],
                segs: vec![Seg::Cubic([r, c], [c, r], [0.0, r]), Seg::Line([0.0, 0.0])],
            }],
        };
        let fit = Fit {
            sx: 1.0,
            sy: -1.0,
            tx: 0.0,
            ty: 0.0,
        };
        let tol = FLATTEN_TOLERANCE_MM;
        let flat = flatten(&[shape], fit, tol).expect("flattens");
        let pts = &flat[0].1[0].points;
        for w in pts.windows(2) {
            let mid = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
            if w[0][0] > 0.0 && w[0][1] > 0.0 && w[1][0] > 0.0 && w[1][1] > 0.0 {
                let gap = r - mid[0].hypot(mid[1]);
                assert!(
                    gap < tol + 3e-4 * r,
                    "chord midpoint {gap} mm inside the arc"
                );
            }
        }
        // Not wildly more points than needed: sqrt(6 |d| / (8 tol)) segments.
        assert!(pts.len() < 40, "{} points", pts.len());
    }
}
