//! Solids for the body, stud and tube features, built through the [`Kernel`] trait
//! only. Kernel frame: millimetres, +Z up, body minimum corner at the origin
//! (the frame of `openbrick-geom/examples/export_lods.rs`).
//!
//! The kernel extrudes along Z with one uniform `scale_top`, and has no rotation,
//! so 45 degree chamfers on rectangular edges are made by intersecting with a
//! pair of "taper" slabs: a rectangle centred on the body, long in one axis,
//! extruded with a `scale_top` that moves its short sides by one millimetre per
//! millimetre of height. Every taper reaches past the solid it trims, so no two
//! faces of a boolean are coplanar.
//!
//! Every length here is in design millimetres; [`Geo`] multiplies horizontal
//! coordinates by the fit profile's `xyScale` as it hands them to the kernel, so
//! the scale applies last, after all offsets (docs/plan/05 section 3).

use openbrick_geom::{CrossSection, GeomError, Kernel, Polygon};

/// How far a cutter reaches past the solid it cuts. Any positive length works;
/// it is not a brick dimension.
pub(crate) const OVERSHOOT_MM: f64 = 1.0;

/// Kernel calls with the `xyScale` applied to every horizontal coordinate.
pub(crate) struct Geo<'k, K: Kernel> {
    pub k: &'k K,
    pub xy: f64,
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl<K: Kernel> Geo<'_, K> {
    fn rect(&self, min: [f64; 2], max: [f64; 2]) -> Polygon {
        let s = self.xy;
        Polygon {
            points: vec![
                [min[0] * s, min[1] * s],
                [max[0] * s, min[1] * s],
                [max[0] * s, max[1] * s],
                [min[0] * s, max[1] * s],
            ],
        }
    }

    fn translate(&self, solid: &K::Solid, [x, y, z]: [f64; 3]) -> K::Solid {
        self.k.translate(solid, [x * self.xy, y * self.xy, z])
    }

    /// A box from `min` to `max` in plan, `z0` to `z1` in height.
    pub(crate) fn prism(
        &self,
        min: [f64; 2],
        max: [f64; 2],
        z0: f64,
        z1: f64,
    ) -> Result<K::Solid, GeomError> {
        let s = self
            .k
            .extrude(&CrossSection::from(vec![self.rect(min, max)]), z1 - z0, 1.0)?;
        Ok(self.k.translate(&s, [0.0, 0.0, z0]))
    }

    /// The slab `|a - centre_a| <= half0 + slope * (z - z0)` for `z` in `[z0, z1]`,
    /// where `a` is the coordinate along `axis`; along the other axis it reaches
    /// past `other_half` from the centre at every height.
    #[allow(clippy::too_many_arguments)]
    fn taper(
        &self,
        axis: Axis,
        centre: [f64; 2],
        half0: f64,
        z0: f64,
        z1: f64,
        slope: f64,
        other_half: f64,
    ) -> Result<K::Solid, GeomError> {
        let half1 = half0 + slope * (z1 - z0);
        if half0 <= 0.0 || half1 <= 0.0 {
            return Err(GeomError::NonManifold(format!(
                "taper with half widths {half0} and {half1} mm"
            )));
        }
        let scale = half1 / half0;
        // The long sides scale too; start them wide enough to stay outside.
        let long = (other_half + OVERSHOOT_MM) / scale.min(1.0);
        let (hx, hy) = match axis {
            Axis::X => (half0, long),
            Axis::Y => (long, half0),
        };
        let section = CrossSection::from(vec![self.rect([-hx, -hy], [hx, hy])]);
        let solid = self.k.extrude(&section, z1 - z0, scale)?;
        Ok(self.translate(&solid, [centre[0], centre[1], z0]))
    }

    /// Both tapers of a rectangle with half sizes `half0` at `z0`.
    #[allow(clippy::too_many_arguments)]
    fn tapers(
        &self,
        centre: [f64; 2],
        half0: [f64; 2],
        z0: f64,
        z1: f64,
        slope: f64,
        reach: [f64; 2],
    ) -> Result<[K::Solid; 2], GeomError> {
        Ok([
            self.taper(Axis::X, centre, half0[0], z0, z1, slope, reach[1])?,
            self.taper(Axis::Y, centre, half0[1], z0, z1, slope, reach[0])?,
        ])
    }

    /// Revolve a (radius, height) profile about the vertical axis through
    /// `centre`, base at `z0`. Profiles must be counter-clockwise.
    fn revolve_at(
        &self,
        profile: &[[f64; 2]],
        centre: [f64; 2],
        z0: f64,
        segments: u32,
    ) -> Result<K::Solid, GeomError> {
        let p = Polygon {
            points: profile.iter().map(|[r, z]| [r * self.xy, *z]).collect(),
        };
        let s = self.k.revolve(&p, segments)?;
        Ok(self.translate(&s, [centre[0], centre[1], z0]))
    }

    /// Copies of `solid` (built at the origin) moved to each centre at height `z`.
    fn copies(&self, solid: &K::Solid, centres: &[[f64; 2]], z: f64) -> Vec<K::Solid> {
        centres
            .iter()
            .map(|c| self.translate(solid, [c[0], c[1], z]))
            .collect()
    }
}

/// The hollow body: outer block with first-layer and top chamfers, minus the
/// cavity with its wall-to-roof chamfer and first-layer rim chamfer.
#[derive(Clone, Debug)]
pub(crate) struct BodyGeom {
    /// Plan size (x, y) after the undersize.
    pub size: [f64; 2],
    pub height: f64,
    pub wall: f64,
    pub roof: f64,
    /// 45 degree chamfer on the bottom edges, outside and around the cavity rim.
    pub first_layer_chamfer: f64,
    /// 45 degree chamfer on the top outer edges (`round` tier `chamfer`).
    pub top_chamfer: f64,
    /// 45 degree chamfer where the side walls meet the roof inside the cavity.
    pub wall_roof_chamfer: f64,
}

impl BodyGeom {
    pub(crate) fn cavity_depth(&self) -> f64 {
        self.height - self.roof
    }

    fn centre(&self) -> [f64; 2] {
        [self.size[0] / 2.0, self.size[1] / 2.0]
    }

    fn inner_half(&self) -> [f64; 2] {
        [
            self.size[0] / 2.0 - self.wall,
            self.size[1] / 2.0 - self.wall,
        ]
    }

    pub(crate) fn build<K: Kernel>(&self, g: &Geo<K>) -> Result<K::Solid, GeomError> {
        let k = g.k;
        let [w, d] = self.size;
        let h = self.height;
        let half = [w / 2.0, d / 2.0];
        let centre = self.centre();
        let c = self.first_layer_chamfer;

        let mut outer = g.prism([0.0, 0.0], [w, d], 0.0, h)?;
        if c > 0.0 {
            // Half width w/2 - c at z = 0, growing 1:1 upward; starts at z = -c.
            let z0 = -c;
            let [tx, ty] = g.tapers(
                centre,
                [half[0] - 2.0 * c, half[1] - 2.0 * c],
                z0,
                h + c,
                1.0,
                half,
            )?;
            outer = k.intersection(&k.intersection(&outer, &tx)?, &ty)?;
        }
        let rc = self.top_chamfer;
        if rc > 0.0 {
            // Half width w/2 - rc at z = h, shrinking 1:1 upward; starts at z = -rc.
            let z0 = -rc;
            let [tx, ty] = g.tapers(
                centre,
                [half[0] - rc + (h - z0), half[1] - rc + (h - z0)],
                z0,
                h + rc,
                -1.0,
                half,
            )?;
            outer = k.intersection(&k.intersection(&outer, &tx)?, &ty)?;
        }

        let inner = self.inner_half();
        let top = self.cavity_depth();
        let below = -OVERSHOOT_MM;
        let mut cavity = g.prism(
            [self.wall, self.wall],
            [w - self.wall, d - self.wall],
            below,
            top,
        )?;
        let f = self.wall_roof_chamfer;
        if f > 0.0 {
            // Half width inner at z = top - f, shrinking 1:1 upward to inner - f at
            // the roof; the taper ends a little above the roof.
            let past = OVERSHOOT_MM.min((inner[0].min(inner[1]) - f) / 2.0);
            let rise = top - f - below;
            let [tx, ty] = g.tapers(
                centre,
                [inner[0] + rise, inner[1] + rise],
                below,
                top + past,
                -1.0,
                [inner[0] + rise, inner[1] + rise],
            )?;
            cavity = k.intersection(&k.intersection(&cavity, &tx)?, &ty)?;
        }
        if c > 0.0 {
            // The cavity rim opens by c at z = 0 and closes 1:1 to the wall at z = c.
            let past = OVERSHOOT_MM.min(inner[0].min(inner[1]) / 2.0);
            let open = [inner[0] + c - below, inner[1] + c - below];
            let [tx, ty] = g.tapers(centre, open, below, c + past, -1.0, open)?;
            cavity = k.union(&cavity, &k.intersection(&tx, &ty)?)?;
        }
        k.difference(&outer, &cavity)
    }
}

/// Solid studs on the top face.
#[derive(Clone, Debug)]
pub(crate) struct StudGeom {
    pub centres: Vec<[f64; 2]>,
    /// Height of the stud bases (the body top).
    pub base: f64,
    pub diameter: f64,
    pub height: f64,
    pub top_chamfer: f64,
    pub segments: u32,
}

impl StudGeom {
    pub(crate) fn build<K: Kernel>(&self, g: &Geo<K>) -> Result<Vec<K::Solid>, GeomError> {
        if self.centres.is_empty() {
            return Ok(Vec::new());
        }
        let r = self.diameter / 2.0;
        let (h, ch) = (self.height, self.top_chamfer);
        let profile: Vec<[f64; 2]> = if ch > 0.0 {
            vec![[0.0, 0.0], [r, 0.0], [r, h - ch], [r - ch, h], [0.0, h]]
        } else {
            vec![[0.0, 0.0], [r, 0.0], [r, h], [0.0, h]]
        };
        let stud = g.revolve_at(&profile, [0.0, 0.0], 0.0, self.segments)?;
        Ok(g.copies(&stud, &self.centres, self.base))
    }
}

/// A wall rib: a box from `min` to `max` in plan.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rib {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// Underside tubes, 1-wide pins and wall ribs, all reaching from the bed (or the
/// first-layer chamfer, for ribs) to the roof.
#[derive(Clone, Debug)]
pub(crate) struct TubeGeom {
    pub tubes: Vec<[f64; 2]>,
    pub tube_outer: f64,
    pub tube_inner: f64,
    pub tube_chamfer: f64,
    pub pins: Vec<[f64; 2]>,
    pub pin_diameter: f64,
    pub pin_chamfer: f64,
    pub ribs: Vec<Rib>,
    pub rib_bottom: f64,
    /// The cavity ceiling: tubes, pins and ribs end here, against the roof.
    pub top: f64,
    pub segments: u32,
}

impl TubeGeom {
    pub(crate) fn build<K: Kernel>(&self, g: &Geo<K>) -> Result<Vec<K::Solid>, GeomError> {
        let mut out = Vec::new();
        let top = self.top;
        if !self.tubes.is_empty() {
            let (ro, ri, c) = (
                self.tube_outer / 2.0,
                self.tube_inner / 2.0,
                self.tube_chamfer,
            );
            let profile: Vec<[f64; 2]> = if c > 0.0 {
                vec![
                    [ri + c, 0.0],
                    [ro - c, 0.0],
                    [ro, c],
                    [ro, top],
                    [ri, top],
                    [ri, c],
                ]
            } else {
                vec![[ri, 0.0], [ro, 0.0], [ro, top], [ri, top]]
            };
            let tube = g.revolve_at(&profile, [0.0, 0.0], 0.0, self.segments)?;
            out.extend(g.copies(&tube, &self.tubes, 0.0));
        }
        if !self.pins.is_empty() {
            let (rp, c) = (self.pin_diameter / 2.0, self.pin_chamfer);
            let profile: Vec<[f64; 2]> = if c > 0.0 {
                vec![[0.0, 0.0], [rp - c, 0.0], [rp, c], [rp, top], [0.0, top]]
            } else {
                vec![[0.0, 0.0], [rp, 0.0], [rp, top], [0.0, top]]
            };
            let pin = g.revolve_at(&profile, [0.0, 0.0], 0.0, self.segments)?;
            out.extend(g.copies(&pin, &self.pins, 0.0));
        }
        for rib in &self.ribs {
            out.push(g.prism(rib.min, rib.max, self.rib_bottom, top)?);
        }
        Ok(out)
    }
}

/// Union in a balanced binary tree: each solid joins the result in about
/// log2(n) booleans of similar size, instead of n booleans against an ever
/// larger accumulated result.
fn union_tree<K: Kernel>(k: &K, mut solids: Vec<K::Solid>) -> Result<K::Solid, GeomError> {
    match solids.len() {
        0 => Err(GeomError::NonManifold("union of nothing".into())),
        1 | 2 => k.union_all(&solids),
        n => {
            let right = solids.split_off(n / 2);
            let a = union_tree(k, solids)?;
            let b = union_tree(k, right)?;
            k.union(&a, &b)
        }
    }
}

/// Union of a base solid (the body) with many feature solids that barely touch
/// one another (studs, tubes, pins, ribs): the features first, in a balanced
/// tree, then the base once.
///
/// `Kernel::union_all` on `manifold-rust` 0.19.1 unions its inputs one after
/// another (`Manifold::batch_boolean` folds `boolean`), so its cost grows with
/// the square of the part size. Measured in release, single-threaded, on the
/// x86-64 development container (`examples/eval_timing.rs`): 2x4 brick 82 ms
/// with one flat `union_all`, 36 ms as one balanced tree, 28 to 30 ms like this;
/// 6x16 plate 8.4 s, 1.5 s and 0.66 to 0.72 s.
pub(crate) fn union_onto<K: Kernel>(
    k: &K,
    base: Option<K::Solid>,
    features: Vec<K::Solid>,
) -> Result<K::Solid, GeomError> {
    match (base, features.is_empty()) {
        (Some(b), true) => Ok(b),
        (Some(b), false) => k.union(&b, &union_tree(k, features)?),
        (None, _) => union_tree(k, features),
    }
}
