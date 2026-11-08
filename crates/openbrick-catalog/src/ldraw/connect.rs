//! Connectors derived from the primitives a part references (docs/plan/07
//! section 3, the Q04 spike report `docs/spikes/q04-ldraw-connectivity.md`).
//!
//! Nothing here is typed in per part: a part's connectors are what its
//! sub-file references say (a `stud.dat` is a stud, a `peghole.dat` pair is a
//! pin hole), placed with the composed transform of the reference, and the
//! female studs under a body are synthesised from its triangles (the Q04
//! prototype: LDraw has no primitive for them).
//!
//! Mapping, by primitive file name, from the Q04 report's table. The part's
//! own `0` description supplies the length of a pin ("Technic Pin 1.0"); the
//! Dimensional Standard (`standards/dimensions-v1.json`, through
//! `openbrick-units`) supplies the module, the stud's radius and height.
//!
//! | primitive names (Q04 table) | engine connector |
//! |---|---|
//! | `stud`, `studa`, `stud2`, `stud2a`, `stud6`, `stud6a`, `stud9`, `stud10`, `stud13`, `stud15`, `stud17`, `stud17a`, `stud26`, `studel`, `studx`, `studxa`, `studh*`, `studp01`, `stud-logo*`, `stud2-logo*` | `studM`, outward local -Y |
//! | synthesised from triangles | `tubeF`, one per receptacle cell |
//! | `peghole*` pairs, `beamhole`, `connhole`, `connhol2` | `pinF`, one per hole |
//! | `axlehole`, `axl2hole`, `axl3hole`, `axl4hole`, `axlehol4`, `axlehol5`, `axl2hol8`, `axl3hol8`, `axl5hol8`; `bush`, `bush0`, `bush0a` | `axleF`, one per module |
//! | `connect*`, `confric*` | `pinM`, one per module of length |
//! | `axlehol8` ("Technic Axle Perimeter") | `axleM`, one per module of length |
//! | `clip<n>`, `clh*`, `arm1`-`arm3`, `joint8ball`, `joint8socket*` | not modelled: the part is flagged with its joint kinds |
//!
//! The unlisted tube primitives (`stud4` and its relatives) only count as
//! evidence of a system underside; the engine's `tubeF` is the receptacle cell.

use std::collections::BTreeSet;

use openbrick_units::STANDARD;

use super::flatten::{Bounds, Collect, IndexedMesh, Limits, Mat, Soup, Visitor, WalkError, Walker};
use super::library::Library;
use crate::shape::{Aabb, Conn, ConnKind};

/// A number of the Dimensional Standard in LDU (`ldraw_ldu`), by feature id.
fn std_ldu(id: &str) -> f64 {
    STANDARD
        .get()
        .feature(id)
        .and_then(|f| f.ldraw_ldu)
        .unwrap_or_else(|| panic!("standard feature {id} has no ldraw_ldu"))
}

/// What a primitive file is, from its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    Stud,
    /// Underside tube or pin primitive: evidence only.
    UnderTube,
    PegHole,
    BeamHole,
    AxleHole,
    Bush,
    Pin,
    AxleShaft,
    Joint(&'static str),
}

fn basename(path: &str) -> &str {
    let f = path.rsplit('/').next().unwrap_or(path);
    f.strip_suffix(".dat").unwrap_or(f)
}

fn digits_after<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let r = name.strip_prefix(prefix)?;
    r.chars().all(|c| c.is_ascii_digit()).then_some(r)
}

/// The primitive kind of a library path (`p/stud.dat`, `p/48/stud.dat`), if it is one the importer reads.
pub fn classify(path: &str) -> Option<Prim> {
    if !path.starts_with("p/") {
        return None;
    }
    let n = basename(path);
    Some(match n {
        "stud" | "studa" | "stud2" | "stud2a" | "stud6" | "stud6a" | "stud9" | "stud10"
        | "stud13" | "stud15" | "stud17" | "stud17a" | "stud26" | "studel" | "studx" | "studxa"
        | "studp01" => Prim::Stud,
        _ if n.starts_with("studh")
            || n.starts_with("stud-logo")
            || n.starts_with("stud2-logo") =>
        {
            Prim::Stud
        }
        "stud3" | "stud3a" | "stud4" | "stud4a" | "stud4h" | "stud4s" | "stud4s2" | "stud4o"
        | "stud4od" | "stud4oda" | "stud12" | "stud16" | "stud16a" | "stud16od" | "stud18a"
        | "stud21a" | "stud22a" | "stud23" | "stud23d" => Prim::UnderTube,
        _ if n.starts_with("stud4f") => Prim::UnderTube,
        "beamhole" | "connhole" | "connhol2" => Prim::BeamHole,
        _ if n == "peghole" || digits_after(n, "peghole").is_some() => Prim::PegHole,
        "axlehole" | "axl2hole" | "axl3hole" | "axl4hole" | "axlehol4" | "axlehol5"
        | "axl2hol8" | "axl3hol8" | "axl5hol8" => Prim::AxleHole,
        "bush" | "bush0" | "bush0a" => Prim::Bush,
        "axlehol8" => Prim::AxleShaft,
        _ if n == "connect"
            || digits_after(n, "connect").is_some()
            || n == "confric"
            || digits_after(n, "confric").is_some() =>
        {
            Prim::Pin
        }
        _ if digits_after(n, "clip").is_some() => Prim::Joint("clip"),
        _ if n.starts_with("clh") => Prim::Joint("hinge"),
        "arm1" | "arm2" | "arm3" => Prim::Joint("hinge"),
        "joint8ball" => Prim::Joint("ball"),
        _ if n.starts_with("joint8socket") => Prim::Joint("socket"),
        _ => return None,
    })
}

/// Whether a path is a stud primitive (the body box leaves it out).
pub fn is_stud(path: &str) -> bool {
    classify(path) == Some(Prim::Stud)
}

/// A direction snapped to a lattice axis (`[0, -1, 0]`), or `None` when it is not on one.
pub fn lattice_axis(d: [f64; 3]) -> Option<[i32; 3]> {
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-9 {
        return None;
    }
    let mut out = [0i32; 3];
    for k in 0..3 {
        let c = d[k] / len;
        out[k] = if c.abs() > 0.999 {
            c.signum() as i32
        } else if c.abs() < 0.001 {
            0
        } else {
            return None;
        };
    }
    (out.iter().map(|v| v.abs()).sum::<i32>() == 1).then_some(out)
}

/// A point snapped to integer LDU, or `None` when it is more than 0.06 LDU from one.
pub fn lattice_point(p: [f64; 3]) -> Option<[i32; 3]> {
    let mut out = [0; 3];
    for k in 0..3 {
        let r = p[k].round();
        if (p[k] - r).abs() > 0.06 || r.abs() > 1.0e7 {
            return None;
        }
        out[k] = r as i32;
    }
    Some(out)
}

/// Joint kinds this slice does not model.
pub type Joints = BTreeSet<&'static str>;

/// A connector before the engine's types: kind name, position, direction.
#[derive(Clone, Debug, PartialEq)]
struct Raw {
    kind: ConnKind,
    pos: [f64; 3],
    dir: [f64; 3],
}

/// The scan of a part's primitive references.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub conns: Vec<Conn>,
    pub joints: Joints,
    /// Underside tube or pin primitives seen (evidence of a system underside).
    pub tubes: usize,
    /// Connectors found but dropped for not being on the lattice.
    pub off_lattice: usize,
    pub studs: usize,
}

struct Scanner<'a> {
    lib: &'a Library,
    raw: Vec<Raw>,
    peg: Vec<([f64; 3], [f64; 3])>,
    joints: Joints,
    tubes: usize,
}

/// The first number in a primitive's description after a `Pin` word: its length in modules.
fn pin_modules(lib: &Library, path: &str) -> f64 {
    let desc = lib
        .load(path)
        .map(|r| r.file.header.description.clone())
        .unwrap_or_default();
    let mut words = desc.split_whitespace();
    while let Some(w) = words.next() {
        if w.eq_ignore_ascii_case("pin") {
            if let Some(v) = words.next().and_then(|t| t.parse::<f64>().ok()) {
                return v;
            }
        }
    }
    1.0
}

impl Visitor for Scanner<'_> {
    fn wants_triangles(&self) -> bool {
        false
    }

    fn enter(&mut self, path: &str, m: &Mat, _flip: bool) -> bool {
        let Some(prim) = classify(path) else {
            return true;
        };
        let module = std_ldu("module");
        match prim {
            Prim::Stud => self.raw.push(Raw {
                kind: ConnKind::StudM,
                pos: m.t,
                dir: m.apply_dir([0.0, -1.0, 0.0]),
            }),
            Prim::UnderTube => self.tubes += 1,
            Prim::PegHole => self.peg.push((m.t, m.apply_dir([0.0, -1.0, 0.0]))),
            Prim::BeamHole => self.raw.push(Raw {
                kind: ConnKind::PinF,
                pos: m.t,
                dir: m.apply_dir([0.0, 1.0, 0.0]),
            }),
            Prim::AxleHole => {
                let axis = m.apply_dir([0.0, 1.0, 0.0]);
                let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
                let n = (len / module + 0.01).floor().max(1.0) as usize;
                for i in 0..n {
                    let f = (i as f64 + 0.5) * module / len.max(1e-9);
                    self.raw.push(Raw {
                        kind: ConnKind::AxleF,
                        pos: [
                            m.t[0] + axis[0] * f,
                            m.t[1] + axis[1] * f,
                            m.t[2] + axis[2] * f,
                        ],
                        dir: axis,
                    });
                }
            }
            Prim::Bush => {
                let axis = m.apply_dir([0.0, 0.0, 1.0]);
                let c = m.apply([0.0, 0.0, module / 2.0]);
                self.raw.push(Raw {
                    kind: ConnKind::AxleF,
                    pos: c,
                    dir: axis,
                });
            }
            Prim::AxleShaft => {
                let axis = m.apply_dir([0.0, 1.0, 0.0]);
                let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
                let n = (len / module + 0.01).floor() as usize;
                for i in 0..n {
                    let f = (i as f64 + 0.5) * module / len.max(1e-9);
                    self.raw.push(Raw {
                        kind: ConnKind::AxleM,
                        pos: [
                            m.t[0] + axis[0] * f,
                            m.t[1] + axis[1] * f,
                            m.t[2] + axis[2] * f,
                        ],
                        dir: axis,
                    });
                }
            }
            Prim::Pin => {
                let modules = pin_modules(self.lib, path);
                let n = (modules + 0.01).floor() as usize;
                for i in 0..n {
                    // From the origin along local -Y (Q04 table): module i's centre.
                    let c = m.apply([0.0, -(i as f64 + 0.5) * module, 0.0]);
                    self.raw.push(Raw {
                        kind: ConnKind::PinM,
                        pos: c,
                        dir: m.apply_dir([0.0, -1.0, 0.0]),
                    });
                }
            }
            Prim::Joint(j) => {
                self.joints.insert(j);
            }
        }
        // Mapped primitives are leaves: their own sub-primitives are not connectors.
        false
    }

    fn triangle(&mut self, _tri: [[f64; 3]; 3]) {}
}

/// Pair the openings of through holes (Q04, post-processing): two `peghole`
/// openings that are coaxial, face away from each other and are at most two
/// modules apart make one `pinF` at their midpoint. An opening without a
/// partner is a blind hole and makes none.
fn pair_pegs(peg: &[([f64; 3], [f64; 3])], out: &mut Vec<Raw>) {
    let module = std_ldu("module");
    let mut used = vec![false; peg.len()];
    for i in 0..peg.len() {
        if used[i] {
            continue;
        }
        let (pa, da) = peg[i];
        for (j, &(pb, db)) in peg.iter().enumerate().skip(i + 1) {
            if used[j] {
                continue;
            }
            let dot = da[0] * db[0] + da[1] * db[1] + da[2] * db[2];
            let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if dot > -0.99 || dist < 1.0 || dist > 2.0 * module + 0.5 {
                continue;
            }
            // Coaxial: the offset is along the axis.
            let along = d[0] * da[0] + d[1] * da[1] + d[2] * da[2];
            if (along.abs() - dist).abs() > 0.05 {
                continue;
            }
            // Facing away from each other: B lies behind A's outward direction.
            if along > 0.0 {
                continue;
            }
            used[i] = true;
            used[j] = true;
            out.push(Raw {
                kind: ConnKind::PinF,
                pos: [
                    (pa[0] + pb[0]) / 2.0,
                    (pa[1] + pb[1]) / 2.0,
                    (pa[2] + pb[2]) / 2.0,
                ],
                dir: da,
            });
            break;
        }
    }
}

/// Scan a part (library path `parts/3001.dat`) for its primitive connectors.
pub fn scan(lib: &Library, path: &str, limits: Limits) -> Result<Scan, WalkError> {
    let mut s = Scanner {
        lib,
        raw: Vec::new(),
        peg: Vec::new(),
        joints: Joints::new(),
        tubes: 0,
    };
    Walker::new(lib, limits).walk(&mut s, path, &Mat::IDENTITY)?;
    let pegs = std::mem::take(&mut s.peg);
    pair_pegs(&pegs, &mut s.raw);
    let mut out = Scan {
        joints: s.joints,
        tubes: s.tubes,
        ..Scan::default()
    };
    for r in s.raw {
        let (Some(pos), Some(dir)) = (lattice_point(r.pos), lattice_axis(r.dir)) else {
            out.off_lattice += 1;
            continue;
        };
        out.conns.push(Conn {
            kind: r.kind,
            pos,
            dir,
        });
    }
    normalise(&mut out.conns);
    out.studs = out
        .conns
        .iter()
        .filter(|c| c.kind == ConnKind::StudM)
        .count();
    Ok(out)
}

/// Sort and drop duplicates; axial kinds (pins, axles, holes) are the same
/// connector in either direction, so their direction is made positive first.
pub fn normalise(conns: &mut Vec<Conn>) {
    for c in conns.iter_mut() {
        if c.kind.is_axial() {
            let flip = c.dir.iter().find(|v| **v != 0).is_some_and(|v| *v < 0);
            if flip {
                c.dir = c.dir.map(|v| -v);
            }
        }
    }
    conns.sort_by_key(|c| (c.kind, c.pos, c.dir));
    conns.dedup();
}

// ------------------------------------------------------------------ body

/// The collision box of a part's body and its pick box, in whole LDU: the
/// bounds of its triangles, rounded to the nearest LDU, the body box without
/// its studs (the engine's stud-in-cavity rule, docs/plan/07 section 6) and the
/// pick box with them. A part with no triangles has neither.
pub fn boxes(body: &Bounds, studs: &Bounds) -> Option<(Aabb, Aabb)> {
    let mut all = *body;
    all.union(studs);
    if !all.any {
        return None;
    }
    let r = |b: &Bounds| Aabb {
        min: b.min.map(|v| v.round() as i32),
        max: b.max.map(|v| v.round() as i32),
    };
    let pick = r(&all);
    let body_box = if body.any { r(body) } else { pick };
    Some((body_box, pick))
}

// ------------------------------------------------------------------ studF

/// A vertical-ray index over a triangle soup: triangles bucketed on the XZ plane.
struct Rays<'a> {
    soup: &'a Soup,
    bucket: f64,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

impl<'a> Rays<'a> {
    fn new(soup: &'a Soup) -> Rays<'a> {
        let bucket = std_ldu("half_stud");
        let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> =
            std::collections::HashMap::new();
        for (i, t) in soup.tris.iter().enumerate() {
            let (mut x0, mut x1, mut z0, mut z1) = (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            );
            for p in t {
                x0 = x0.min(p[0]);
                x1 = x1.max(p[0]);
                z0 = z0.min(p[2]);
                z1 = z1.max(p[2]);
            }
            let (cx0, cx1) = ((x0 / bucket).floor() as i32, (x1 / bucket).floor() as i32);
            let (cz0, cz1) = ((z0 / bucket).floor() as i32, (z1 / bucket).floor() as i32);
            if (cx1 - cx0 + 1) as i64 * (cz1 - cz0 + 1) as i64 > 40_000 {
                continue; // a vast flat face is never in a receptacle's range
            }
            for cx in cx0..=cx1 {
                for cz in cz0..=cz1 {
                    cells.entry((cx, cz)).or_default().push(i as u32);
                }
            }
        }
        Rays {
            soup,
            bucket,
            cells,
        }
    }

    /// The lowest face (largest Y) the vertical line through (x, z) crosses.
    fn deepest(&self, x: f64, z: f64) -> Option<f64> {
        let key = (
            (x / self.bucket).floor() as i32,
            (z / self.bucket).floor() as i32,
        );
        let list = self.cells.get(&key)?;
        let mut best: Option<f64> = None;
        for &i in list {
            if let Some(y) = face_y(&self.soup.tris[i as usize], x, z) {
                best = Some(best.map_or(y, |b| b.max(y)));
            }
        }
        best
    }

    /// The opening plane of the cell around (cx, cz): the lowest face over a 5 x 5
    /// sample of lines inside its square (a wall's bottom face has no vertex inside the
    /// cell it borders, so vertices would miss it).
    fn lowest(&self, cx: f64, cz: f64, half: f64) -> Option<f64> {
        let mut open: Option<f64> = None;
        for i in -2..=2 {
            for j in -2..=2 {
                let (x, z) = (
                    cx + f64::from(i) * half * 0.45,
                    cz + f64::from(j) * half * 0.45,
                );
                if let Some(y) = self.deepest(x, z) {
                    open = Some(open.map_or(y, |o| o.max(y)));
                }
            }
        }
        open
    }

    /// True when the vertical line through (x, z) crosses a face at a height `y`
    /// with `lo <= y <= hi` (Y down: smaller is higher).
    fn crosses(&self, x: f64, z: f64, lo: f64, hi: f64) -> bool {
        let key = (
            (x / self.bucket).floor() as i32,
            (z / self.bucket).floor() as i32,
        );
        let Some(list) = self.cells.get(&key) else {
            return false;
        };
        for &i in list {
            if let Some(y) = face_y(&self.soup.tris[i as usize], x, z) {
                if y >= lo - 1e-3 && y <= hi + 1e-3 {
                    return true;
                }
            }
        }
        false
    }
}

/// The height at which the vertical line through (x, z) crosses a triangle, if it does
/// (a triangle that is vertical or degenerate seen from above is never crossed).
fn face_y(t: &[[f64; 3]; 3], x: f64, z: f64) -> Option<f64> {
    let (ax, az) = (t[0][0], t[0][2]);
    let (bx, bz) = (t[1][0] - ax, t[1][2] - az);
    let (cx, cz) = (t[2][0] - ax, t[2][2] - az);
    let det = bx * cz - bz * cx;
    if det.abs() < 1e-9 {
        return None;
    }
    let (px, pz) = (x - ax, z - az);
    let u = (px * cz - pz * cx) / det;
    let v = (bx * pz - bz * px) / det;
    let eps = 1e-6;
    if u < -eps || v < -eps || u + v > 1.0 + eps {
        return None;
    }
    Some(t[0][1] + u * (t[1][1] - t[0][1]) + v * (t[2][1] - t[0][1]))
}

/// Most receptacle cells synthesised for one part (a 32 x 32 baseplate has none; a big brick has hundreds).
pub const MAX_STUDF: usize = 2048;

/// Female studs (`tubeF`) synthesised from a part's triangles, the Q04
/// prototype. Candidate cells lie on the stud grid (both phases are tried in X
/// and in Z and the one with the most cells wins). For a cell centred at
/// (cx, cz), the opening plane is the lowest face inside the cell's square (sampled).
/// The cell is a receptacle when, with `h` the stud height and `r` the stud
/// radius of the Standard:
///
/// - room: nine vertical lines (the centre and eight at `r - 1`) cross no face
///   strictly between `opening - h` and the opening, nor one at the opening
///   itself (a solid bottom is not an opening);
/// - cover: at least one of them meets a face at `opening - h` or higher (a
///   plate's roof is exactly one stud height above its opening);
/// - grip: at least two of sixteen lines at `r + 1.5` cross a face between
///   `opening - h` and the opening (a wall, tube or pin that touches the stud).
///
/// Only downward receptacles are searched.
pub fn synth_studf(soup: &Soup, phase: Option<[f64; 2]>) -> Vec<Conn> {
    let pitch = std_ldu("module");
    let h = std_ldu("stud_height");
    let r = std_ldu("stud_diameter") / 2.0;
    let mut b = Bounds::default();
    for t in &soup.tris {
        for p in t {
            b.add(*p);
        }
    }
    if !b.any {
        return Vec::new();
    }
    let rays = Rays::new(soup);
    let ring = |cx: f64, cz: f64, radius: f64, n: usize| -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                (cx + radius * a.cos(), cz + radius * a.sin())
            })
            .collect()
    };
    let cell_ok = |cx: f64, cz: f64| -> Option<f64> {
        // The opening plane: the lowest face within the square.
        let open = rays.lowest(cx, cz, pitch / 2.0)?;
        let mut room = vec![(cx, cz)];
        room.extend(ring(cx, cz, r - 1.0, 8));
        // A roof exactly one stud height above the opening (a plate's) is the cover, not an obstacle.
        let eps = 0.05;
        let blocked = room
            .iter()
            .any(|&(x, z)| rays.crosses(x, z, open - h + eps, open - eps));
        // A face right at the opening plane across the cell's interior is a solid bottom, not an opening.
        let floor = room
            .iter()
            .any(|&(x, z)| rays.crosses(x, z, open - eps, open + eps));
        if blocked || floor {
            return None;
        }
        let covered = room
            .iter()
            .any(|&(x, z)| rays.crosses(x, z, -1.0e9, open - h + eps));
        if !covered {
            return None;
        }
        let grip = ring(cx, cz, r + 1.5, 16)
            .into_iter()
            .filter(|&(x, z)| rays.crosses(x, z, open - h, open))
            .count();
        (grip >= 2).then_some(open)
    };
    let mut best: Vec<(f64, f64, f64)> = Vec::new();
    // Phases: centres at k * pitch or at (k + 1/2) * pitch, in X and in Z; when the
    // part has studs, its studs say which.
    let phases: Vec<(f64, f64)> = match phase {
        Some([x, z]) => vec![(x.rem_euclid(pitch), z.rem_euclid(pitch))],
        None => vec![
            (0.0, 0.0),
            (0.0, pitch / 2.0),
            (pitch / 2.0, 0.0),
            (pitch / 2.0, pitch / 2.0),
        ],
    };
    for (px, pz) in phases {
        {
            let mut cells = Vec::new();
            let kx0 = ((b.min[0] - px) / pitch).floor() as i64;
            let kx1 = ((b.max[0] - px) / pitch).ceil() as i64;
            let kz0 = ((b.min[2] - pz) / pitch).floor() as i64;
            let kz1 = ((b.max[2] - pz) / pitch).ceil() as i64;
            if (kx1 - kx0 + 1) * (kz1 - kz0 + 1) > 4 * MAX_STUDF as i64 {
                return Vec::new();
            }
            'outer: for kx in kx0..=kx1 {
                for kz in kz0..=kz1 {
                    let (cx, cz) = (px + kx as f64 * pitch, pz + kz as f64 * pitch);
                    // The whole cell lies within the part's footprint (half a stud of body
                    // beyond its edge is not a place a stud can enter).
                    let half = pitch / 2.0 - 0.5;
                    if cx - half < b.min[0]
                        || cx + half > b.max[0]
                        || cz - half < b.min[2]
                        || cz + half > b.max[2]
                    {
                        continue;
                    }
                    if let Some(open) = cell_ok(cx, cz) {
                        cells.push((cx, cz, open));
                        if cells.len() > MAX_STUDF {
                            break 'outer;
                        }
                    }
                }
            }
            if cells.len() > best.len() {
                best = cells;
            }
        }
    }
    if best.len() > MAX_STUDF {
        return Vec::new();
    }
    best.into_iter()
        .filter_map(|(x, z, y)| {
            let pos = lattice_point([x, y, z])?;
            Some(Conn {
                kind: ConnKind::TubeF,
                pos,
                dir: [0, 1, 0],
            })
        })
        .collect()
}

/// Everything the importer derives for one part from its geometry and primitives.
#[derive(Clone, Debug)]
pub struct Derived {
    pub mesh: IndexedMesh,
    pub body: Aabb,
    pub pick: Aabb,
    pub conns: Vec<Conn>,
    pub joints: Joints,
    pub tubes: usize,
    pub studs: usize,
    pub studf: usize,
    pub off_lattice: usize,
    pub uncertified_triangles: usize,
    pub missing_refs: usize,
}

/// Tessellate a part and derive its connectors and boxes.
pub fn derive(lib: &Library, path: &str, limits: Limits) -> Result<Option<Derived>, WalkError> {
    let sc = scan(lib, path, limits)?;
    let mut c = Collect::new(is_stud);
    let mut w = Walker::new(lib, limits);
    w.walk(&mut c, path, &Mat::IDENTITY)?;
    let Some((body, pick)) = boxes(&c.body, &c.studs) else {
        return Ok(None);
    };
    let mut conns = sc.conns.clone();
    let phase = sc
        .conns
        .iter()
        .find(|c| c.kind == ConnKind::StudM)
        .map(|c| [f64::from(c.pos[0]), f64::from(c.pos[2])]);
    let studf = synth_studf(&c.soup, phase);
    let studf_n = studf.len();
    conns.extend(studf);
    normalise(&mut conns);
    let mesh = IndexedMesh::from_soup(&c.soup);
    Ok(Some(Derived {
        mesh,
        body,
        pick,
        conns,
        joints: sc.joints,
        tubes: sc.tubes,
        studs: sc.studs,
        studf: studf_n,
        off_lattice: sc.off_lattice,
        uncertified_triangles: w.stats.uncertified_triangles,
        missing_refs: w.stats.missing_count,
    }))
}
