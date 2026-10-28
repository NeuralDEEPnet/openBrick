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
//!    Outlines with more than [`MAX_ARTWORK_POINTS`] points, or whose scanline
//!    sweep would need more than [`MAX_ARTWORK_SWEEP`] work, are refused before
//!    any polygon operation runs (untrusted input: a few thousand points drawn
//!    as long thin spikes would otherwise block the evaluator for minutes).
//! 4. Fill rules: each SVG path is resolved under its own `fill-rule` (nonzero
//!    or evenodd, through `openbrick_geom::region`, so two nested circles drawn
//!    the same way make a ring under evenodd and a disc under nonzero), then all
//!    paths are unioned.
//! 5. Morphological opening with `minFeature` (default: the process's
//!    `minFeatureMm`): inset by half of it, then outset by half of it, with
//!    `Kernel::offset2d_round` (round joins drawn within [`FLATTEN_TOLERANCE_MM`]
//!    of the true arc: 8 chords per turn at 0.4 mm), clipped to the artwork.
//!    Strokes thinner than `minFeature` disappear; the removed region is kept
//!    for the preview and its share of the area is `removed_area_pct`. Each
//!    offset is estimated first (`region::offset_sweep_cost`) and refused past
//!    [`MAX_ARTWORK_SWEEP`]: insetting dense outlines past their width folds
//!    them over each other. The thinnest stroke is then estimated within a work
//!    budget (see [`min_stroke`]).
//! 6. The mode: `emboss` extrudes the artwork by `height` on top of the face, as
//!    a separate object of `colorRole` that touches the body without overlapping
//!    it; `deboss` cuts it `height` deep into the face; `inlay` cuts it the same
//!    way and splits off the face slab as the `colorRole` object (docs/plan/05
//!    section 6), printed face down. Inlay builds the single-extruder
//!    filament-swap variant only: the artwork prints as a recess in the body
//!    colour. The multi-material variant, whose artwork fill is its own object,
//!    is not built yet.
//! 7. Thin face material (deboss and inlay): what prints is the face around the
//!    cuts, so the body footprint minus the opened artwork is opened at the same
//!    radius too (see [`thin_face`]). Walls between recesses and rims between
//!    the artwork and the body edge thinner than `minFeature` are not removed,
//!    but they count in `removed` (red in the preview) and in
//!    `removed_area_pct`, so rule FEATURE.MIN reports them. Emboss prints the
//!    artwork itself and is measured in step 5 only.
//!
//! Inlay prints face down, so the top edges are its first layer: the top
//! chamfer is raised to the first-layer chamfer (the larger of the process
//! `firstLayerChamferMm` and the fit profile `firstLayerChamfer`, capped at the
//! roof as any top chamfer is), which replaces elephant-foot compensation
//! (docs/plan/05 section 6). [`Artwork::face_chamfer_mm`] reports it. The
//! `Measurements` fields `first_layer_chamfer_mm`, `min_body_wall_at_bed_mm` and
//! `min_tube_wall_at_bed_mm` keep describing the bottom edges at z = 0, also
//! for an inlay: that is where the first-layer chamfer of the walls and tubes
//! is built, and the face an inlay puts on the bed is a solid slab with no
//! side wall or tube in that plane.
//!
//! Deboss and inlay thicken the roof by the depth so the roof under the artwork
//! keeps its thickness (rule `ROOF.MIN`), and clamp the depth so the cavity keeps
//! `minCavityDepthMm` (rule `CAVITY.DEPTH`; 0.4 mm on plate-height parts). The
//! whole cavity ceiling is lowered rather than only the part under the artwork:
//! a pad in the shape of the artwork hanging from the ceiling would be an
//! unsupported island when the part prints face up, while a lowered ceiling is
//! still one flat bridge between the walls and the tubes.

use openbrick_geom::region::{self, FillRule};
use openbrick_geom::{round_segments, CrossSection, GeomError, Kernel, Polygon};
use openbrick_print::ProcessProfile;

use super::geometry::{Geo, OVERSHOOT_MM};
use super::params::FVal;
use super::svg::{self, Seg, Shape};
use super::{BodyPlan, EvalError, Feat, RecordingAssets, MIN_LENGTH_MM};

/// Curves are flattened so no point of a curve is farther than this from its
/// polyline, measured on the part after fitting (docs/plan/06 section 5).
pub const FLATTEN_TOLERANCE_MM: f64 = 0.05;

/// Most points of the flattened artwork: a resource guard (the kernel
/// triangulates every point twice, and the opening and the stroke estimate
/// offset the outline about 40 times), not a brick dimension.
pub const MAX_ARTWORK_POINTS: usize = 50_000;

/// Most sweep work, by `openbrick_geom::region` (`SweepCost::work`: edge-band
/// pairs plus 20 per crossing), that the flattened artwork, and each offset of
/// the opening, may need: a resource guard. Polygon booleans and offsets sweep
/// a scanline over the outlines, and their time grows with the edges every
/// scanline meets and with the crossings, not with the point count, so a few
/// thousand points drawn as long thin spikes, as lines that all cross each
/// other, or as dense outlines inset past their width would block the
/// evaluator for minutes. Measured in a release build on x86-64 (clipper2-rust
/// 1.2.0), one unit of work costs about 30 to 70 ns, so an operation at the
/// limit takes well under a second. For scale: the sample mark needs 384 for
/// its outline (12 crossings where its shapes overlap) and 908 for its inset at
/// 0.4 mm; a star of 1,000 thin spikes (2,000 points) needs 291,048 and 6.2
/// million (213 ms); 1,000 lines that all cross each other need 39.9 million
/// and are refused.
pub const MAX_ARTWORK_SWEEP: u64 = 10_000_000;

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
    /// The 45 degree chamfer on the edges of that face (the body's top chamfer,
    /// `round`). For an inlay the face is on the bed, so this is its first-layer
    /// chamfer: at least the process and fit profile first-layer chamfer (the
    /// evaluator raises it with a warning), unless capped at the roof. The
    /// `Measurements` first-layer fields describe the bottom edges at z = 0.
    #[serde(default)]
    pub face_chamfer_mm: f64,
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
    /// Regions that do not print as drawn (draw them in red in the preview),
    /// at `face_z_mm`: artwork the opening removed, and, for deboss and inlay,
    /// face material thinner than `minFeature` (see `thin_face_area_mm2`).
    pub removed: Vec<Polygon>,
    /// Area of the face material, among `removed`, that a deboss or inlay
    /// leaves thinner than `minFeature`: walls between recesses, and rims
    /// between the artwork and the body edge (measured on the body footprint,
    /// so a rim counts with the top chamfer under it). It is still built; it
    /// counts in `removed_area_pct` so rule FEATURE.MIN reports it. Always 0
    /// for emboss.
    #[serde(default)]
    pub thin_face_area_mm2: f64,
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
    /// The regions of [`Artwork::removed`] as a share of the artwork area
    /// before the opening, in percent (`Measurements::removed_area_pct`).
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

/// The result of [`offset_within`].
enum Offset {
    /// The offset region, and the sweep work it was estimated to need.
    Done(CrossSection, u64),
    /// Not run: Clipper2's sweep of the raw offset path would need about this
    /// much work, more than the limit.
    TooComplex(u64),
}

/// `s` offset by `delta` (negative: inset) with `Kernel::offset2d_round`, its
/// round joins drawn within [`FLATTEN_TOLERANCE_MM`] of the arc, unless the
/// sweep of the raw offset path would need more than `limit` work
/// (`region::offset_sweep_cost`). Before an inset, regions too small to hold a
/// disc of radius `-delta` are dropped: their inset is empty anyway, and a small
/// region inset far past its size folds every edge over the others.
fn offset_within<K: Kernel>(
    k: &K,
    s: &CrossSection,
    delta: f64,
    limit: u64,
) -> Result<Offset, GeomError> {
    if delta == 0.0 || s.polygons.is_empty() {
        return Ok(Offset::Done(s.clone(), 0));
    }
    let kept;
    let s = if delta < 0.0 {
        kept = without_islands_under(s, -2.0 * delta);
        &kept
    } else {
        s
    };
    if s.polygons.iter().all(|p| p.signed_area() <= 0.0) {
        return Ok(Offset::Done(CrossSection::default(), 0));
    }
    let n = round_segments(delta, FLATTEN_TOLERANCE_MM);
    let work = region::offset_sweep_cost(s, delta, n, limit).work();
    if work > limit {
        return Ok(Offset::TooComplex(work));
    }
    Ok(Offset::Done(k.offset2d_round(s, delta, n)?, work))
}

/// `s` without its outer (counter-clockwise) contours whose bounding box is
/// narrower than `width` in x or y. Holes stay: a hole whose outer contour went
/// is dropped by the kernel, whose offsets read regions under the Positive fill
/// rule, and a hole inside a kept contour still matters.
fn without_islands_under(s: &CrossSection, width: f64) -> CrossSection {
    CrossSection::from(
        s.polygons
            .iter()
            .filter(|p| {
                p.signed_area() <= 0.0
                    || bounds_of(std::slice::from_ref(p))
                        .is_some_and(|(lo, hi)| hi[0] - lo[0] >= width && hi[1] - lo[1] >= width)
            })
            .cloned()
            .collect(),
    )
}

fn bounds_of(polygons: &[Polygon]) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in polygons.iter().flat_map(|p| &p.points) {
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

/// Sweep work [`min_stroke`] may spend in all (its offsets, by
/// `region::offset_sweep_cost`, and two sweeps of the artwork per step): twice
/// what one operation on the artwork may need. Measured in a release build, the
/// estimate then takes at most about 1.5 s (a star of 1,000 thin spikes, which
/// runs out of budget) and 7 ms for the sample mark. Past the budget the
/// estimate is not made, and `min_stroke_mm` stays empty.
pub(crate) const STROKE_WORK_BUDGET: u64 = 2 * MAX_ARTWORK_SWEEP;

/// An estimate of the thinnest stroke of `art`, which has already been opened
/// at radius `r0`, given `eroded`, the inset of the original artwork by `r0`;
/// `None` when `art` is empty or when the estimate would need more than
/// [`STROKE_WORK_BUDGET`] (then it is not measured).
///
/// Every point of an opened shape lies in a disc of diameter `2 r0` inside it,
/// so its plain minimum width is `2 r0` by construction (at the corners); this
/// measures the strokes instead. Opening `art` again at a radius r > r0 removes
/// two kinds of region: small pieces at convex corners, which the joins cut back
/// further (for a corner of opening angle a, area (r^2 - r0^2) times
/// 1 / tan(a / 2) - (pi - a) / 2 with true arcs, a little more with the chords
/// of the joins, and at most sin(a) / (2 tan^2(a / 2)) with single-chord joins,
/// so under r^2 for corners of about 70 degrees or more, 50 with true arcs);
/// and whole strokes or necks narrower than 2 r, whose area is about their width
/// times their length, at least r^2 just above their half width for a stroke at
/// least half as long as it is wide. So the estimate is twice the smallest r at
/// which one removed piece reaches r^2 of area: radii from r0 up grow by
/// [`STROKE_SCAN_STEP`] until that happens (from the thin end, so a thin stroke
/// is found before larger radii remove everything), then bisection refines it
/// to [`STROKE_RESOLUTION_MM`]. A piece is an outer contour of the removed
/// region with its holes not subtracted, which can only make it look larger, so
/// the estimate errs thin, except that a stub shorter than half its width is
/// not counted as a stroke. Sharp corners count as thin: near their tip they are
/// narrower than 2 r. A solid disc of diameter D measures D, a square of side s
/// measures s, a ring its width, the blocks of the hairline fixture their width
/// (tests/eval_relief.rs). When no piece ever counts as a stroke the result is
/// `2 r0`, what the opening guarantees.
///
/// Each step insets `eroded` further, by r - r0, rather than the artwork by r:
/// insetting by discs adds up (a disc of radius r is one of r0 swept by one of
/// r - r0), and the opening's inset equals the inset of what it opened, so this
/// is the same region, while the thin parts that make a deep inset costly are
/// already gone. Cost: about 20 steps for a 13 mm wide face.
pub(crate) fn min_stroke<K: Kernel>(
    k: &K,
    art: &CrossSection,
    eroded: &CrossSection,
    r0: f64,
) -> Result<Option<f64>, GeomError> {
    min_stroke_within(k, art, eroded, r0, STROKE_WORK_BUDGET)
}

/// [`min_stroke`] with a work budget of `budget`.
fn min_stroke_within<K: Kernel>(
    k: &K,
    art: &CrossSection,
    eroded: &CrossSection,
    r0: f64,
    mut budget: u64,
) -> Result<Option<f64>, GeomError> {
    let Some((lo, hi)) = bounds_of(&art.polygons) else {
        return Ok(None);
    };
    let r0 = r0.max(0.0);
    // Each step clips against `art` and takes the difference: two sweeps.
    let art_work = region::sweep_cost(&art.polygons, budget)
        .work()
        .saturating_mul(2);
    // Whether opening at `r` removes a stroke; `None` when over budget.
    let mut removes_a_stroke = |r: f64| -> Result<Option<bool>, GeomError> {
        let e = match offset_within(k, eroded, -(r - r0), budget)? {
            Offset::Done(e, work) => {
                budget -= work;
                e
            }
            Offset::TooComplex(_) => return Ok(None),
        };
        let mut opened = CrossSection::default();
        if !e.polygons.is_empty() {
            match offset_within(k, &e, r, budget)? {
                Offset::Done(d, work) => {
                    budget -= work;
                    opened = d;
                }
                Offset::TooComplex(_) => return Ok(None),
            }
        }
        if art_work > budget {
            return Ok(None);
        }
        budget -= art_work;
        // Clipped to `art`, so the polygonal joins never add area.
        let removed = region::difference(art, &region::intersection(&opened, art));
        Ok(Some(
            removed
                .polygons
                .iter()
                .map(Polygon::signed_area)
                .any(|a| a >= r * r),
        ))
    };
    // No disc wider than the bounding box fits inside the artwork, and the
    // chords of the two insets' round joins keep at most the tolerance each
    // that a true inset would remove, so opening at `top` removes all of it.
    let top = (hi[0] - lo[0]).min(hi[1] - lo[1]) / 2.0
        + 2.0 * FLATTEN_TOLERANCE_MM
        + 2.0 * STROKE_RESOLUTION_MM;
    // Scan up from the thin end: `below` removes no stroke, `above` does.
    let mut below = r0;
    let mut above = loop {
        let r = (below.max(STROKE_RESOLUTION_MM) * STROKE_SCAN_STEP).min(top);
        match removes_a_stroke(r)? {
            None => return Ok(None),
            Some(true) => break r,
            Some(false) => {}
        }
        if r >= top {
            // No removed piece ever counted as a stroke: nothing measured, so
            // report what the opening guarantees.
            return Ok(Some(2.0 * r0));
        }
        below = r;
    };
    while above - below > STROKE_RESOLUTION_MM {
        let mid = (above + below) / 2.0;
        match removes_a_stroke(mid)? {
            None => return Ok(None),
            Some(true) => above = mid,
            Some(false) => below = mid,
        }
    }
    Ok(Some(2.0 * below))
}

/// The result of [`opening`].
enum Opened {
    /// The inset by the radius, and the opening (the inset outset again,
    /// clipped to the region).
    Done {
        eroded: CrossSection,
        opened: CrossSection,
    },
    /// Not run: one of the offsets would need about this much sweep work, more
    /// than [`MAX_ARTWORK_SWEEP`].
    TooComplex(u64),
}

/// The morphological opening of `s` at radius `r`: inset by `r`, then outset by
/// `r`, clipped to `s` so the polygonal joins never add area. Nothing is
/// removed when `r` is not positive.
fn opening<K: Kernel>(k: &K, s: &CrossSection, r: f64) -> Result<Opened, GeomError> {
    if r <= 0.0 {
        return Ok(Opened::Done {
            eroded: s.clone(),
            opened: s.clone(),
        });
    }
    let eroded = match offset_within(k, s, -r, MAX_ARTWORK_SWEEP)? {
        Offset::Done(e, _) => e,
        Offset::TooComplex(work) => return Ok(Opened::TooComplex(work)),
    };
    let opened = if eroded.polygons.is_empty() {
        CrossSection::default()
    } else {
        match offset_within(k, &eroded, r, MAX_ARTWORK_SWEEP)? {
            Offset::Done(d, _) => region::intersection(&d, s),
            Offset::TooComplex(work) => return Ok(Opened::TooComplex(work)),
        }
    };
    Ok(Opened::Done { eroded, opened })
}

/// Face material thinner than `2 r` that cutting `art` (already opened) into the
/// top of a body with footprint `size` leaves (deboss and inlay): the footprint
/// minus `art`, less its opening at radius `r`. That finds walls between
/// recesses and rims between the artwork and the body edge. The footprint's own
/// corners, which any opening rounds, are not counted: only what lies inside the
/// opening of the bare footprint is.
///
/// The footprint is the body outline, not the flat face inside the top chamfer,
/// because the material around a recess reaches the outline below the chamfer:
/// a rim is as wide as the margin plus the chamfer under it, and artwork that
/// reaches the face box with no margin leaves the chamfer as a knife edge, which
/// this finds. `None` when an offset would need more than [`MAX_ARTWORK_SWEEP`].
fn thin_face<K: Kernel>(
    k: &K,
    size: [f64; 2],
    art: &CrossSection,
    r: f64,
) -> Result<Option<CrossSection>, GeomError> {
    let footprint = CrossSection::from(vec![Polygon::rect([0.0, 0.0], size)]);
    let material = region::difference(&footprint, art);
    let Opened::Done { opened: kept, .. } = opening(k, &material, r)? else {
        return Ok(None);
    };
    let Opened::Done { opened: whole, .. } = opening(k, &footprint, r)? else {
        return Ok(None);
    };
    Ok(Some(region::intersection(
        &region::difference(&material, &kept),
        &whole,
    )))
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
/// artwork, open it, measure it and clamp the depth. An inlay raises the body's
/// top chamfer to the first-layer chamfer (see the module comment).
pub(crate) fn plan_relief<K: Kernel>(
    kernel: &K,
    feat: &Feat,
    body: &mut BodyPlan,
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

    // An inlay prints face down: the top edges are its first layer, so they get
    // the first-layer chamfer that replaces elephant-foot compensation
    // (docs/plan/05 section 6), capped at the roof as plan_body caps any top
    // chamfer. Before the face box, which lies inside the top chamfer.
    if mode == Mode::Inlay && body.geom.top_chamfer < body.geom.first_layer_chamfer {
        let g = &mut body.geom;
        let want = g.first_layer_chamfer;
        let raised = want.min(g.roof);
        let cap = if raised < want {
            format!(" (capped at the {} mm roof)", g.roof)
        } else {
            String::new()
        };
        warnings.push(format!(
            "feature `{}`: inlay prints face down, so the top edges are its first layer: the top chamfer is raised from {} mm to {raised} mm{cap}, the first-layer chamfer (the larger of firstLayerChamferMm of {} and the fit profile's firstLayerChamfer), which stands in for elephant-foot compensation (docs/plan/05 section 6); give the body a `round` chamfer of at least {want} mm to choose it yourself",
            feat.id(),
            g.top_chamfer,
            process.id
        ));
        g.top_chamfer = raised;
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
    if parsed.fill_paints > 1 {
        warnings.push(format!(
            "feature `{}`: `{name}` fills its shapes with {} different paints (colours, gradients or patterns); relief artwork is one colour: every filled shape is built whatever its colour, and paint order is ignored, so a white shape painted over a dark one does not cut a hole; draw holes as holes (Inkscape: Path > Difference)",
            feat.id(),
            parsed.fill_paints
        ));
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
    // Refuse outlines whose sweeps would take minutes, before any sweep runs.
    let contours: Vec<Polygon> = flat.iter().flat_map(|(_, c)| c.iter().cloned()).collect();
    let sweep = region::sweep_cost(&contours, MAX_ARTWORK_SWEEP);
    if sweep.work() > MAX_ARTWORK_SWEEP {
        return Err(asset_err(format!(
            "the artwork is too complex to build: a scanline sweep of its outlines needs about {} steps (limit {MAX_ARTWORK_SWEEP}), from long thin spikes that span the same heights or lines that cross each other ({} crossings); simplify it in your editor (Inkscape: Path > Simplify) or remove fine texture",
            sweep.work(),
            sweep.crossings
        )));
    }
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
    let too_detailed = |work: u64| {
        asset_err(format!(
            "the artwork is too detailed for the {min_feature} mm opening (minFeature): offsetting its outlines by {r0} mm needs a scanline sweep of about {work} steps (limit {MAX_ARTWORK_SWEEP}), from dense outlines thinner than minFeature; simplify it in your editor (Inkscape: Path > Simplify) or remove texture finer than {min_feature} mm"
        ))
    };
    // The inset (erosion) by r0, then the outset of that, clipped to the
    // artwork so the polygonal joins never add area.
    let (eroded, opened) = match opening(kernel, &art, r0).map_err(super::kernel_err(&what))? {
        Opened::Done { eroded, opened } => (eroded, opened),
        Opened::TooComplex(work) => return Err(too_detailed(work)),
    };
    let area_after = opened.area();
    if opened.polygons.is_empty() || area_after <= MIN_LENGTH_MM * MIN_LENGTH_MM {
        return Err(feat.fail(format!(
            "every stroke of `{name}` is thinner than minFeature {min_feature} mm once fitted into the {:.1} x {:.1} mm face box, so the opening removed all of it; use bolder artwork, a smaller margin or a larger part",
            box_size[0], box_size[1]
        )));
    }
    let mut removed = region::difference(&art, &opened);
    // A deboss or inlay prints the face around the cuts: face material thinner
    // than minFeature is still built, but shown and counted with the removed
    // strokes (see `thin_face`).
    let mut thin_area = 0.0;
    if mode.cuts() && r0 > 0.0 {
        match thin_face(kernel, g.size, &opened, r0).map_err(super::kernel_err(&what))? {
            Some(thin) => {
                // A removed stroke is not cut, so it is face material too:
                // count each region once.
                let thin = region::difference(&thin, &removed);
                thin_area = thin.area().max(0.0);
                removed = region::union_all(&[removed, thin]);
            }
            None => warnings.push(format!(
                "feature `{}`: the face material the {} leaves around `{name}` was not checked for walls thinner than minFeature {min_feature} mm: the artwork is too detailed for the work limit",
                feat.id(),
                mode.name()
            )),
        }
    }
    let stroke_pct = 100.0 * (area_before - area_after) / area_before;
    let removed_pct = (stroke_pct + 100.0 * thin_area / area_before).max(0.0);
    if let Some(max) = process.max_removed_area_pct {
        // Rule FEATURE.MIN's own comparison, so this warning and the rule agree.
        if removed_pct > max + 1e-9 {
            let rule = format!(
                "more than the {max} % maxRemovedAreaPct of {} (rule FEATURE.MIN, a blocker when the artwork is fit-critical)",
                process.id
            );
            warnings.push(if thin_area > 0.0 {
                format!(
                    "feature `{}`: {removed_pct:.2} % of the artwork area does not print as drawn, {rule}: the {min_feature} mm opening (minFeature) removed {:.2} % (strokes thinner than {min_feature} mm), and the {} leaves another {:.2} % as face material thinner than {min_feature} mm between the recesses or at the body edge, which is still built; the regions are red in the preview: make lines, the gaps between them and the margin to the body edge at least {min_feature} mm wide, or the artwork larger",
                    feat.id(),
                    stroke_pct.max(0.0),
                    mode.name(),
                    100.0 * thin_area / area_before
                )
            } else {
                format!(
                    "feature `{}`: the {min_feature} mm opening (minFeature) removed {removed_pct:.2} % of the artwork area, {rule}: strokes thinner than {min_feature} mm cannot print; the removed regions are red in the preview: make those lines at least {min_feature} mm wide, or the artwork larger",
                    feat.id()
                )
            });
        }
    }
    let min_stroke = min_stroke(kernel, &opened, &eroded, r0).map_err(super::kernel_err(&what))?;
    if min_stroke.is_none() {
        warnings.push(format!(
            "feature `{}`: the thinnest stroke of `{name}` was not measured (min_stroke_mm is empty): the artwork is too detailed for the estimate's work limit; the opening still removed every stroke under {min_feature} mm",
            feat.id()
        ));
    }

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
        // The slicer assigns each layer to an object by where the layer lies,
        // and a first layer plus whole layers need not add up to the depth (a
        // 0.2 mm first layer and 0.16 mm layers end at 0.36 and 0.52, never at
        // 0.4), so the swap goes where the preview starts the body.
        warnings.push(format!(
            "feature `{}`: inlay is built as the single-extruder filament-swap variant: print it face down with the top face on the bed; the `{}` object is the {height:.3} mm face slab minus the artwork, so the artwork prints as a recess in the body colour; add the filament change before the first layer the slicer's preview shows as the body (the slab ends on a layer boundary only if your first layer and layer height add up to {height:.3} mm). A multi-material variant, with the artwork filled as its own object for a multi-material unit or tool changer, is coming; until then a second slot for `{}` gives the same recessed artwork",
            feat.id(),
            feat.role(),
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
        face_chamfer_mm: g.top_chamfer,
        height_mm: height,
        requested_height_mm: requested,
        box_min_mm: box_min.map(|v| v * xy),
        box_max_mm: box_max.map(|v| v * xy),
        min_feature_mm: min_feature,
        area_before_mm2: area_before * xy * xy,
        area_after_mm2: area_after * xy * xy,
        removed: scaled(&removed, xy).polygons,
        thin_face_area_mm2: thin_area * xy * xy,
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
    fn the_stroke_estimate_keeps_to_its_budget() {
        use openbrick_geom::manifold_rust::ManifoldRust;
        // A unit-free 10 x 4 rectangle opened at radius 0.5 (no brick
        // dimension is involved): its eroded core is 9 x 3.
        let art = CrossSection::from(vec![Polygon::rect([0.0, 0.0], [10.0, 4.0])]);
        let eroded = CrossSection::from(vec![Polygon::rect([0.5, 0.5], [9.0, 3.0])]);
        let k = ManifoldRust;
        let w = min_stroke_within(&k, &art, &eroded, 0.5, STROKE_WORK_BUDGET)
            .expect("kernel")
            .expect("measured");
        assert!((w - 4.0).abs() < 0.02, "a 4 wide bar measures {w}");
        // Without budget it is not measured, rather than run long.
        assert_eq!(
            min_stroke_within(&k, &art, &eroded, 0.5, 0).expect("kernel"),
            None
        );
        assert_eq!(
            min_stroke(&k, &CrossSection::default(), &eroded, 0.5).expect("kernel"),
            None
        );
    }

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
