//! Nominal brick-compatible bricks built straight from the Dimensional Standard.
//!
//! "Nominal" means the numbers of `standards/dimensions-v1.json` with no fit
//! offsets: offsets belong to fit profiles and are applied by the part evaluator
//! (docs/plan/06), never here. Until that evaluator builds bricks from part
//! documents, this module is the one brick construction shared by the device
//! harness meshes (`examples/export_lods.rs`), the kernel WebAssembly probe
//! (`examples/wasm_probe.rs`) and the browser pipeline (`openbrick-wasm`, `apps/web`).
//!
//! The brick is a shell (walls), a roof, a stud on every lattice cell and, under
//! the roof, the underside of docs/plan/04 section 4 (`tube.field`): hollow tubes at
//! the centres between four studs on parts two or more studs wide, solid pins at
//! the mid-points between two studs on 1-wide parts, nothing on a 1x1. Ribs and
//! chamfers are omitted; they arrive with the evaluator.
//!
//! Frame: the kernel frame, millimetres, right-handed, +Z up. The body's minimum
//! corner is at the origin, the body spans z = 0 to `brick_height`, and the studs
//! stand on top. Stud `i` along an axis is centred at `i * pitch + pitch / 2 -
//! undersize`, so the lattice cell corner of the brick is at `(-undersize,
//! -undersize, 0)`.

use openbrick_units::Standard;

use crate::{CrossSection, GeomError, Kernel, Polygon, Tessellation};

/// The nominal dimensions a brick needs, in millimetres, read from the standard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NominalDims {
    /// Stud pitch (`module`).
    pub pitch: f64,
    /// Horizontal undersize per side (`undersize_per_side`).
    pub undersize: f64,
    /// Side wall thickness (`side_wall`).
    pub wall: f64,
    /// Roof thickness (`roof`).
    pub roof: f64,
    /// Brick height without studs (`brick_height`).
    pub height: f64,
    pub stud_diameter: f64,
    pub stud_height: f64,
    pub tube_outer_diameter: f64,
    pub tube_inner_diameter: f64,
    /// Solid pin under 1-wide parts (`pin_1wide_diameter`).
    pub pin_diameter: f64,
}

impl NominalDims {
    /// Read every dimension from the standard (the sources of each number are cited
    /// there).
    pub fn from_standard(s: &Standard) -> NominalDims {
        NominalDims {
            pitch: s.mm("module"),
            undersize: s.mm("undersize_per_side"),
            wall: s.mm("side_wall"),
            roof: s.mm("roof"),
            height: s.mm("brick_height"),
            stud_diameter: s.mm("stud_diameter"),
            stud_height: s.mm("stud_height"),
            tube_outer_diameter: s.mm("tube_outer_diameter"),
            tube_inner_diameter: s.mm("tube_inner_diameter"),
            pin_diameter: s.mm("pin_1wide_diameter"),
        }
    }

    /// Body size (x, y) of a brick `studs[0]` by `studs[1]` studs: n studs times
    /// the pitch minus the undersize on both sides.
    pub fn body(&self, studs: [u32; 2]) -> [f64; 2] {
        studs.map(|n| n as f64 * self.pitch - 2.0 * self.undersize)
    }

    /// Depth of the cavity under the roof.
    pub fn cavity(&self) -> f64 {
        self.height - self.roof
    }
}

/// How much of the brick to build; the levels of detail of the render harness are
/// three settings of this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    /// Segments of the stud, tube and pin cylinders, or `None` for the stud-less
    /// hull box of the body.
    pub segments: Option<u32>,
    /// Build the underside: tubes on parts two or more studs wide, pins on 1-wide
    /// parts.
    pub underside: bool,
}

impl Detail {
    /// Everything, with the default cylinder tessellation ([`Tessellation`]).
    pub fn full() -> Detail {
        Detail {
            segments: Some(Tessellation::default().stud_segments),
            underside: true,
        }
    }
}

/// What sits under the roof of a brick of the given footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Underside {
    /// 1x1: nothing.
    None,
    /// One stud wide and longer: solid pins between neighbouring studs.
    Pins,
    /// Two or more studs in both directions: hollow tubes between four studs.
    Tubes,
}

impl Underside {
    pub fn of(studs: [u32; 2]) -> Underside {
        match (studs[0] >= 2, studs[1] >= 2) {
            (true, true) => Underside::Tubes,
            (false, false) => Underside::None,
            _ => Underside::Pins,
        }
    }
}

/// Centres (x, y) of the tubes or pins under a brick, in the body frame, in the
/// order they are added to the union (x outer, y inner). Along an axis with two or
/// more studs they sit between neighbouring studs, at multiples of the pitch from
/// the cell corner; along a 1-stud axis, on the stud row's centre line.
pub fn underside_centres(d: &NominalDims, studs: [u32; 2]) -> Vec<[f64; 2]> {
    if Underside::of(studs) == Underside::None {
        return Vec::new();
    }
    let along = |n: u32| -> Vec<f64> {
        if n == 1 {
            vec![d.pitch / 2.0 - d.undersize]
        } else {
            (1..n).map(|i| i as f64 * d.pitch - d.undersize).collect()
        }
    };
    let ys = along(studs[1]);
    along(studs[0])
        .into_iter()
        .flat_map(|x| ys.iter().map(move |&y| [x, y]))
        .collect()
}

/// Build a nominal brick `studs[0]` by `studs[1]` studs (x by y) with kernel `k`.
///
/// The 2xN construction is the one of `examples/kernel_timing.rs` step for step
/// (stud, tube, shell, roof, then the translated copies), so a 2xN brick at
/// [`Detail::full`] has the same mesh fingerprint as the 2xN row printed there.
/// The vertex order of a batch union follows the order in which the kernel created
/// its inputs, so another order gives the same solid with another fingerprint
/// (`Mesh::canonical_fingerprint` does not depend on it).
///
/// Returns [`GeomError::InvalidInput`] for a footprint with zero studs.
pub fn nominal_brick<K: Kernel>(
    k: &K,
    d: &NominalDims,
    studs: [u32; 2],
    detail: &Detail,
) -> Result<K::Solid, GeomError> {
    if studs.contains(&0) {
        return Err(GeomError::InvalidInput(format!(
            "a brick needs at least one stud in each direction, got {} x {}",
            studs[0], studs[1]
        )));
    }
    let [w, depth] = d.body(studs);
    let outer = Polygon::rect([0.0, 0.0], [w, depth]);
    let Some(segments) = detail.segments else {
        return k.extrude(&CrossSection::from(vec![outer]), d.height, 1.0);
    };
    let cavity = d.cavity();
    let inner =
        Polygon::rect([d.wall, d.wall], [w - 2.0 * d.wall, depth - 2.0 * d.wall]).reversed();
    let disc = |diameter: f64| Polygon::circle([0.0, 0.0], diameter / 2.0, segments);
    // Keep this creation order (stud, underside, shell, roof); see the doc comment.
    let stud = k.extrude(
        &CrossSection::from(vec![disc(d.stud_diameter)]),
        d.stud_height,
        1.0,
    )?;
    let under = match (detail.underside, Underside::of(studs)) {
        (true, Underside::Tubes) => Some(k.extrude(
            &CrossSection::from(vec![
                disc(d.tube_outer_diameter),
                disc(d.tube_inner_diameter).reversed(),
            ]),
            cavity,
            1.0,
        )?),
        (true, Underside::Pins) => {
            Some(k.extrude(&CrossSection::from(vec![disc(d.pin_diameter)]), cavity, 1.0)?)
        }
        _ => None,
    };
    let mut parts = vec![
        k.extrude(&CrossSection::from(vec![outer.clone(), inner]), cavity, 1.0)?,
        k.translate(
            &k.extrude(&CrossSection::from(vec![outer]), d.roof, 1.0)?,
            [0.0, 0.0, cavity],
        ),
    ];
    for ix in 0..studs[0] {
        for iy in 0..studs[1] {
            parts.push(k.translate(
                &stud,
                [
                    ix as f64 * d.pitch + d.pitch / 2.0 - d.undersize,
                    iy as f64 * d.pitch + d.pitch / 2.0 - d.undersize,
                    d.height,
                ],
            ));
        }
    }
    if let Some(under) = under {
        for [x, y] in underside_centres(d, studs) {
            parts.push(k.translate(&under, [x, y, 0.0]));
        }
    }
    k.union_all(&parts)
}

/// Area of the regular polygon that [`Polygon::circle`] makes: `n / 2 * r^2 *
/// sin(2 pi / n)`.
fn polygon_disc_area(diameter: f64, segments: u32) -> f64 {
    let n = segments.max(3) as f64;
    let r = diameter / 2.0;
    n / 2.0 * r * r * (std::f64::consts::TAU / n).sin()
}

/// Analytic volume of [`nominal_brick`] in mm³, with the cylinders as the regular
/// polygons the kernel extrudes (so faceting is not an error). The parts only touch
/// (the roof sits on the walls, the studs on the roof, the tubes and pins under it),
/// so the volume of the union is the sum of the parts.
pub fn nominal_volume(d: &NominalDims, studs: [u32; 2], detail: &Detail) -> f64 {
    let [w, depth] = d.body(studs);
    let Some(segments) = detail.segments else {
        return w * depth * d.height;
    };
    let cavity = d.cavity();
    let walls = (w * depth - (w - 2.0 * d.wall) * (depth - 2.0 * d.wall)) * cavity;
    let roof = w * depth * d.roof;
    let disc = |diameter| polygon_disc_area(diameter, segments);
    let studs_volume = (studs[0] * studs[1]) as f64 * disc(d.stud_diameter) * d.stud_height;
    let under_one = match (detail.underside, Underside::of(studs)) {
        (true, Underside::Tubes) => {
            (disc(d.tube_outer_diameter) - disc(d.tube_inner_diameter)) * cavity
        }
        (true, Underside::Pins) => disc(d.pin_diameter) * cavity,
        _ => 0.0,
    };
    walls + roof + studs_volume + under_one * underside_centres(d, studs).len() as f64
}
