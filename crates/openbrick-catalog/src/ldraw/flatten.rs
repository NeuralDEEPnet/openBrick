//! Walking a part's sub-file references with composed transforms and BFC
//! winding, and tessellating it to an indexed triangle mesh in LDU.
//!
//! The walk is bounded: a depth limit, a limit on type-1 expansions and one on
//! triangles, plus a cycle check, so a hostile or damaged library ends in an
//! error and never in a hang or a stack overflow (the recursion depth is at
//! most [`Limits::max_depth`]).
//!
//! Winding (LDraw spec, "Back Face Culling"): a polygon's vertex order seen
//! from its front is the file's winding (CCW unless `BFC CERTIFY CW` / `CW`
//! says otherwise), flipped once for every matrix with a negative
//! determinant on the way down and once for every `BFC INVERTNEXT` that
//! applies to a reference on the way down. The meshes we write are
//! counter-clockwise seen from outside, so a polygon whose effective winding
//! is clockwise has its vertices reversed. A polygon of a file that is not
//! BFC certified has unknown winding: it is written in both orders so it shows
//! from either side, and counted.

use std::collections::HashMap;

use super::library::{Library, LibraryError};
use super::parse::{Bfc, Line, SubFile, Winding};
use crate::text::safe_msg;

/// A 3 x 3 matrix and a translation: `p -> m p + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat {
    pub m: [[f64; 3]; 3],
    pub t: [f64; 3],
}

impl Mat {
    pub const IDENTITY: Mat = Mat {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0; 3],
    };

    pub fn of(s: &SubFile) -> Mat {
        Mat { m: s.m, t: s.t }
    }

    /// `self` after `child`: the matrix that takes a point of the child's frame to this frame's parent.
    pub fn mul(&self, child: &Mat) -> Mat {
        let mut m = [[0.0; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| self.m[i][k] * child.m[k][j]).sum();
            }
        }
        Mat {
            m,
            t: self.apply(child.t),
        }
    }

    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        [
            self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.m[0][2] * p[2] + self.t[0],
            self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.m[1][2] * p[2] + self.t[1],
            self.m[2][0] * p[0] + self.m[2][1] * p[1] + self.m[2][2] * p[2] + self.t[2],
        ]
    }

    /// The matrix applied to a direction (no translation).
    pub fn apply_dir(&self, d: [f64; 3]) -> [f64; 3] {
        [
            self.m[0][0] * d[0] + self.m[0][1] * d[1] + self.m[0][2] * d[2],
            self.m[1][0] * d[0] + self.m[1][1] * d[1] + self.m[1][2] * d[2],
            self.m[2][0] * d[0] + self.m[2][1] * d[1] + self.m[2][2] * d[2],
        ]
    }

    pub fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

/// Bounds on one walk.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Nesting depth of sub-files below the part.
    pub max_depth: usize,
    /// Type-1 lines expanded in all.
    pub max_refs: usize,
    /// Triangles produced in all.
    pub max_triangles: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_depth: 32,
            max_refs: 300_000,
            max_triangles: 1_500_000,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error("sub-files are nested deeper than {} levels at {}; the file is damaged or hostile", .0, safe_msg(.1))]
    TooDeep(usize, String),
    #[error("{} refers to itself through its sub-files", safe_msg(.0))]
    Cycle(String),
    #[error("more than {0} sub-file references in one part")]
    TooManyRefs(usize),
    #[error("more than {0} triangles in one part")]
    TooManyTriangles(usize),
}

/// What a walk reports to its user.
pub trait Visitor {
    /// False to skip the triangles (a scan that only wants references).
    fn wants_triangles(&self) -> bool {
        true
    }
    /// A reference is about to be expanded: its resolved library path, the
    /// transform of its frame in the part's frame and whether its winding is
    /// flipped. Return false to not descend (the visitor has used it).
    fn enter(&mut self, path: &str, m: &Mat, flip: bool) -> bool;
    /// The matching end of a descended reference.
    fn leave(&mut self, _path: &str) {}
    /// A triangle in the part's frame, counter-clockwise from outside.
    fn triangle(&mut self, tri: [[f64; 3]; 3]);
}

/// Counts a walk keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalkStats {
    pub refs: usize,
    pub triangles: usize,
    /// Triangles of files that are not BFC certified (written in both orders).
    pub uncertified_triangles: usize,
    /// References that could not be resolved (first few names).
    pub missing: Vec<String>,
    pub missing_count: usize,
}

pub struct Walker<'a> {
    lib: &'a Library,
    limits: Limits,
    pub stats: WalkStats,
    stack: Vec<String>,
}

impl<'a> Walker<'a> {
    pub fn new(lib: &'a Library, limits: Limits) -> Walker<'a> {
        Walker {
            lib,
            limits,
            stats: WalkStats::default(),
            stack: Vec::new(),
        }
    }

    /// Walk the part at library path `path` (`parts/3001.dat`) placed by `root`.
    pub fn walk<V: Visitor>(&mut self, v: &mut V, path: &str, root: &Mat) -> Result<(), WalkError> {
        let r = self.lib.load(path)?;
        self.stack.push(path.to_string());
        let out = self.file(v, &r.file, root, false, 0);
        self.stack.pop();
        out
    }

    fn file<V: Visitor>(
        &mut self,
        v: &mut V,
        file: &super::parse::ParsedFile,
        m: &Mat,
        flip: bool,
        depth: usize,
    ) -> Result<(), WalkError> {
        let mut winding = file.winding;
        let mut invert_next = false;
        for line in &file.lines {
            match line {
                Line::Bfc(b) => match b {
                    Bfc::Certify(w) => winding = *w,
                    Bfc::Ccw => winding = Winding::Ccw,
                    Bfc::Cw => winding = Winding::Cw,
                    Bfc::InvertNext => invert_next = true,
                    Bfc::NoCertify | Bfc::Clip | Bfc::NoClip => {}
                },
                Line::SubFile(s) => {
                    let inv = std::mem::take(&mut invert_next);
                    self.stats.refs += 1;
                    self.lib.charge(1)?;
                    if self.stats.refs > self.limits.max_refs {
                        return Err(WalkError::TooManyRefs(self.limits.max_refs));
                    }
                    let local = Mat::of(s);
                    let child_m = m.mul(&local);
                    let child_flip = flip ^ (local.det() < 0.0) ^ inv;
                    let Some(r) = self.lib.resolve(&s.file)? else {
                        self.stats.missing_count += 1;
                        if self.stats.missing.len() < 8 {
                            self.stats.missing.push(s.file.clone());
                        }
                        continue;
                    };
                    if !v.enter(&r.path, &child_m, child_flip) {
                        continue;
                    }
                    if depth + 1 > self.limits.max_depth {
                        return Err(WalkError::TooDeep(self.limits.max_depth, r.path));
                    }
                    if self.stack.contains(&r.path) {
                        return Err(WalkError::Cycle(r.path));
                    }
                    self.stack.push(r.path.clone());
                    let out = self.file(v, &r.file, &child_m, child_flip, depth + 1);
                    self.stack.pop();
                    v.leave(&r.path);
                    out?;
                }
                Line::Tri { p, .. } if v.wants_triangles() => {
                    self.polygon(v, file.certified, winding, flip, m, &p[..])?;
                }
                Line::Quad { p, .. } if v.wants_triangles() => {
                    self.polygon(v, file.certified, winding, flip, m, &p[..])?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn polygon<V: Visitor>(
        &mut self,
        v: &mut V,
        certified: bool,
        winding: Winding,
        flip: bool,
        m: &Mat,
        p: &[[f64; 3]],
    ) -> Result<(), WalkError> {
        let world: Vec<[f64; 3]> = p.iter().map(|q| m.apply(*q)).collect();
        let ccw = (winding == Winding::Ccw) ^ flip;
        let ordered = |forward: bool| -> Vec<[f64; 3]> {
            if forward {
                world.clone()
            } else {
                world.iter().rev().copied().collect()
            }
        };
        let orders: Vec<Vec<[f64; 3]>> = if certified {
            vec![ordered(ccw)]
        } else {
            vec![ordered(true), ordered(false)]
        };
        for o in orders {
            for tri in 1..o.len() - 1 {
                self.stats.triangles += 1;
                self.lib.charge(1)?;
                if !certified {
                    self.stats.uncertified_triangles += 1;
                }
                if self.stats.triangles > self.limits.max_triangles {
                    return Err(WalkError::TooManyTriangles(self.limits.max_triangles));
                }
                v.triangle([o[0], o[tri], o[tri + 1]]);
            }
        }
        Ok(())
    }
}

/// Axis-aligned bounds of points, in LDU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub any: bool,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
            any: false,
        }
    }
}

impl Bounds {
    pub fn add(&mut self, p: [f64; 3]) {
        self.any = true;
        for (k, v) in p.into_iter().enumerate() {
            self.min[k] = self.min[k].min(v);
            self.max[k] = self.max[k].max(v);
        }
    }

    pub fn union(&mut self, o: &Bounds) {
        if o.any {
            self.add(o.min);
            self.add(o.max);
        }
    }
}

/// A flat list of triangles with a flag for the ones inside stud primitives.
#[derive(Clone, Debug, Default)]
pub struct Soup {
    pub tris: Vec<[[f64; 3]; 3]>,
    /// Parallel to `tris`: inside a stud primitive.
    pub stud: Vec<bool>,
}

/// The visitor that collects triangles. `is_stud(path)` says which primitives are
/// studs, so that the body box can leave them out.
pub struct Collect<F: Fn(&str) -> bool> {
    pub soup: Soup,
    pub body: Bounds,
    pub studs: Bounds,
    stud_depth: usize,
    is_stud: F,
}

impl<F: Fn(&str) -> bool> Collect<F> {
    pub fn new(is_stud: F) -> Collect<F> {
        Collect {
            soup: Soup::default(),
            body: Bounds::default(),
            studs: Bounds::default(),
            stud_depth: 0,
            is_stud,
        }
    }
}

impl<F: Fn(&str) -> bool> Visitor for Collect<F> {
    fn enter(&mut self, path: &str, _m: &Mat, _flip: bool) -> bool {
        if (self.is_stud)(path) {
            self.stud_depth += 1;
        } else if self.stud_depth > 0 {
            // A sub-primitive of a stud: keep counting so `leave` balances.
            self.stud_depth += 1;
        }
        true
    }

    fn leave(&mut self, _path: &str) {
        self.stud_depth = self.stud_depth.saturating_sub(1);
    }

    fn triangle(&mut self, tri: [[f64; 3]; 3]) {
        let stud = self.stud_depth > 0;
        for p in tri {
            if stud {
                self.studs.add(p);
            } else {
                self.body.add(p);
            }
        }
        self.soup.tris.push(tri);
        self.soup.stud.push(stud);
    }
}

/// An indexed mesh in f32 LDU, vertices welded to 1/1000 LDU.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexedMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl IndexedMesh {
    /// Weld a triangle soup; drops triangles with (almost) no area.
    pub fn from_soup(soup: &Soup) -> IndexedMesh {
        let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
        let mut mesh = IndexedMesh::default();
        for t in &soup.tris {
            let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
            let w = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
            let c = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            if c.iter().map(|x| x * x).sum::<f64>() < 1e-9 {
                continue;
            }
            let ids: Vec<u32> = t
                .iter()
                .map(|p| {
                    let key = [
                        (p[0] * 1000.0).round() as i64,
                        (p[1] * 1000.0).round() as i64,
                        (p[2] * 1000.0).round() as i64,
                    ];
                    *weld.entry(key).or_insert_with(|| {
                        mesh.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                        (mesh.positions.len() - 1) as u32
                    })
                })
                .collect();
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
                continue;
            }
            mesh.indices.extend(ids);
        }
        mesh
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}
