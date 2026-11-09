//! The built-in catalog of the assembler alpha: standard bricks, plates, tiles
//! and a baseplate, each built by the part evaluator (`openbrick_part::
//! evaluate`) from a generated `openbrick/part@1` document, so its mesh, its
//! collision box and its connectors come from the same features the part
//! designer uses (CLAUDE.md: connectivity is derived from features, never
//! hand-authored). The LDraw catalog import (docs/plan/07 section 3) comes
//! later; these parts need no download.
//!
//! Geometry is the nominal process geometry of the `fdm-0.4` process profile
//! with no fit profile (fit offsets belong to printing, not to display), at
//! two levels of detail that differ only in how many segments a stud, tube or
//! pin circle has ([`Lod`]). The boxes and connectors do not depend on the
//! level of detail (a test checks it), so collision never depends on what is
//! drawn (docs/plan/07 section 6).
//!
//! Frames: the evaluator writes meshes in the kernel frame (millimetres, +Z
//! up, body minimum corner at the origin) and connectors in integer LDU in the
//! LDraw frame (minus Y up, origin centred on the top stud group, stud bases at
//! Y = 0). [`PartMesh`] converts the mesh into that LDraw frame, in LDU, with
//! the numbers of the Dimensional Standard (`module` and `undersize_per_side`
//! through `openbrick-units`), so a part's mesh, box and connectors share one
//! frame and the renderer only places instances.

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_geom::Mesh;
use openbrick_part::{evaluate, EvalInput, MemoryAssets, MeshChecks, ParamValues, PartDoc};
use openbrick_print::{default_process_profiles, ProcessProfile, ProcessTessellation};
use openbrick_units::{HALF_STUD_LDU, STANDARD};

use crate::shape::{Aabb, Conn, ConnKind, PartShape};

/// What kind of body a built-in part has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Body {
    Brick,
    Plate,
    Tile,
}

impl Body {
    fn op(self) -> &'static str {
        match self {
            Body::Brick => "body.brick",
            Body::Plate => "body.plate",
            Body::Tile => "body.tile",
        }
    }
}

/// One built-in part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinSpec {
    /// Catalog id, `ob.<kind>-<x>x<z>`.
    pub id: &'static str,
    pub name: &'static str,
    pub body: Body,
    /// Footprint in studs along LDraw X then Z (the long side along X, as in
    /// the LDraw library's orientation of its bricks).
    pub studs: [u32; 2],
    /// A baseplate: a large plate the palette lists apart and new models start on.
    pub base: bool,
}

const fn spec(id: &'static str, name: &'static str, body: Body, x: u32, z: u32) -> BuiltinSpec {
    BuiltinSpec {
        id,
        name,
        body,
        studs: [x, z],
        base: false,
    }
}

/// The built-in parts, in palette order. Ids and names are ours; the parts
/// are generic brick-compatible shapes (docs/plan/12: no trademarked names).
pub const BUILTIN: &[BuiltinSpec] = &[
    spec("ob.brick-1x1", "Brick 1 x 1", Body::Brick, 1, 1),
    spec("ob.brick-1x2", "Brick 1 x 2", Body::Brick, 2, 1),
    spec("ob.brick-1x4", "Brick 1 x 4", Body::Brick, 4, 1),
    spec("ob.brick-2x2", "Brick 2 x 2", Body::Brick, 2, 2),
    spec("ob.brick-2x4", "Brick 2 x 4", Body::Brick, 4, 2),
    spec("ob.plate-1x1", "Plate 1 x 1", Body::Plate, 1, 1),
    spec("ob.plate-1x2", "Plate 1 x 2", Body::Plate, 2, 1),
    spec("ob.plate-1x4", "Plate 1 x 4", Body::Plate, 4, 1),
    spec("ob.plate-2x2", "Plate 2 x 2", Body::Plate, 2, 2),
    spec("ob.plate-2x4", "Plate 2 x 4", Body::Plate, 4, 2),
    spec("ob.plate-2x6", "Plate 2 x 6", Body::Plate, 6, 2),
    spec("ob.tile-1x2", "Tile 1 x 2", Body::Tile, 2, 1),
    spec("ob.tile-2x2", "Tile 2 x 2", Body::Tile, 2, 2),
    BuiltinSpec {
        id: "ob.baseplate-16x16",
        name: "Baseplate 16 x 16",
        body: Body::Plate,
        studs: [16, 16],
        base: true,
    },
];

/// The catalog this build carries, named in `.obrick` documents.
pub const CATALOG_ID: &str = "openbrick-builtin@1";

/// The process profile the built-in geometry is built on.
pub const PROCESS_ID: &str = "fdm-0.4";

/// Level of detail of a built-in mesh: segments per stud, tube and pin circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lod {
    /// Close-up rendering: 24 segments, round enough at a few studs' distance.
    Detail,
    /// Far away, large parts (the baseplate) and the catalog's own shape
    /// derivation: 8 segments, the cheapest build with everything under the body too.
    Low,
    /// Mid distance (render level 2 of the page): studs of 8 segments, a solid underside (no tubes,
    /// pins or ribs), so nothing a person sees from above or the side is lost.
    Simple,
    /// Far away (render level 3): the stud-less hull of the body, a box of 12 triangles made from
    /// the body vertices of the [`Lod::Low`] mesh ([`body_hull`]).
    Hull,
}

impl Lod {
    /// Segments per full circle of studs, tubes and pins.
    pub fn segments(self) -> u32 {
        match self {
            Lod::Detail => 24,
            Lod::Low | Lod::Simple | Lod::Hull => 8,
        }
    }

    /// The four render levels the page draws, nearest first: full studs, cheap studs and tubes,
    /// cheap studs on a solid body, and the stud-less hull.
    pub const RENDER: [Lod; 4] = [Lod::Detail, Lod::Low, Lod::Simple, Lod::Hull];
}

/// The part document of a built-in part: one body, a stud field on top unless
/// it is a tile, and the tube field underneath (tubes, pins and wall ribs).
pub fn part_doc_json(spec: &BuiltinSpec) -> String {
    part_doc_json_with(spec, true)
}

/// [`part_doc_json`], with or without the tube field under the body (the [`Lod::Simple`] mesh
/// leaves it out; the connectors never come from that build).
fn part_doc_json_with(spec: &BuiltinSpec, bottom: bool) -> String {
    let studs = if spec.body == Body::Tile {
        String::new()
    } else {
        r#"{ "id": "studs", "op": "stud.field", "on": "body.top" },"#.to_string()
    };
    let features = if bottom {
        format!(
            r#"{{ "id": "body", "op": "{op}", "size": [{x}, {z}] }},
    {studs}
    {{ "id": "bottom", "op": "tube.field", "on": "body.bottom" }}"#,
            op = spec.body.op(),
            x = spec.studs[0],
            z = spec.studs[1],
        )
    } else {
        let tail = if studs.is_empty() {
            String::new()
        } else {
            format!(",\n    {}", studs.trim_end_matches(','))
        };
        format!(
            r#"{{ "id": "body", "op": "{op}", "size": [{x}, {z}] }}{tail}"#,
            op = spec.body.op(),
            x = spec.studs[0],
            z = spec.studs[1],
        )
    };
    format!(
        r#"{{
  "openbrick": "1.0",
  "kind": "part",
  "id": "{id}",
  "version": "1.0.0",
  "name": "{name}",
  "license": "CC0-1.0",
  "credit": {{ "author": "OpenBrick3D contributors" }},
  "features": [
    {features}
  ]
}}"#,
        id = spec.id,
        name = spec.name,
    )
}

/// A built-in part mesh in the part's LDraw frame: LDU, minus Y up, origin
/// centred on the top stud group. Triangles are counter-clockwise seen from
/// outside (the kernel's winding; the frame change is a proper rotation).
#[derive(Clone, Debug, PartialEq)]
pub struct PartMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl PartMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// A built part: what the engine needs and what the renderer draws.
#[derive(Clone, Debug)]
pub struct BuiltPart {
    pub shape: PartShape,
    pub mesh: PartMesh,
    /// Body height in LDU.
    pub height_ldu: i32,
}

/// Why a built-in part could not be built (a bug: the specs are fixed).
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("built-in part {id}: {message}")]
pub struct CatalogError {
    pub id: String,
    pub message: String,
}

fn process(lod: Lod) -> ProcessProfile {
    let mut p = default_process_profiles()
        .into_iter()
        .find(|p| p.id == PROCESS_ID)
        .expect("the fdm-0.4 process profile is bundled");
    let mut t = p.tessellation.unwrap_or(ProcessTessellation::default());
    t.stud_segments = Some(lod.segments());
    p.tessellation = Some(t);
    p
}

/// Build one built-in part with the evaluator. Every level has the same `shape` (the collision box,
/// pick box and connectors come from the [`Lod::Low`] build and never from what is drawn); only the
/// mesh differs.
pub fn build(spec: &BuiltinSpec, lod: Lod) -> Result<BuiltPart, CatalogError> {
    match lod {
        Lod::Detail | Lod::Low => build_full(spec, lod, true, None),
        Lod::Simple => {
            let low = build_full(spec, Lod::Low, true, None)?;
            let solid = build_full(spec, Lod::Low, false, Some(low.height_ldu))?;
            Ok(BuiltPart {
                mesh: solid.mesh,
                ..low
            })
        }
        Lod::Hull => {
            let low = build_full(spec, Lod::Low, true, None)?;
            Ok(BuiltPart {
                mesh: body_hull(&low.mesh, &low.shape.body),
                ..low
            })
        }
    }
}

fn build_full(
    spec: &BuiltinSpec,
    lod: Lod,
    bottom: bool,
    height: Option<i32>,
) -> Result<BuiltPart, CatalogError> {
    let err = |message: String| CatalogError {
        id: spec.id.to_string(),
        message,
    };
    let doc = PartDoc::parse(&part_doc_json_with(spec, bottom)).map_err(|e| err(e.to_string()))?;
    let process = process(lod);
    let params = ParamValues::new();
    let assets = MemoryAssets::new();
    let built = evaluate(
        &ManifoldRust,
        &EvalInput {
            doc: &doc,
            params: &params,
            process: &process,
            fit: None,
            assets: &assets,
            skip_unsupported: false,
            mesh_checks: MeshChecks::Preview,
        },
    )
    .map_err(|e| err(e.to_string()))?;
    let body = built
        .objects
        .first()
        .ok_or_else(|| err("the evaluator built no object".into()))?;

    let mut connectors: Vec<Conn> = Vec::new();
    for c in &built.connectors {
        let kind = ConnKind::from_name(&c.kind)
            .ok_or_else(|| err(format!("connector kind {} is not handled yet", c.kind)))?;
        connectors.push(Conn {
            kind,
            pos: c.pos,
            dir: c.dir,
        });
    }
    // The body height is where the receptacles are: every built-in part has
    // a tube field on its bottom face.
    let height_ldu = match height {
        Some(h) => h,
        None => connectors
            .iter()
            .filter(|c| c.kind == ConnKind::TubeF)
            .map(|c| c.pos[1])
            .max()
            .ok_or_else(|| err("the part has no receptacle under it".into()))?,
    };
    let mesh = to_ldraw(&body.mesh, spec.studs, height_ldu);
    let (lo, hi) = bounds(&mesh);
    // The pick box is the mesh bounds rounded to whole LDU (the body is inset
    // by the undersize, a quarter LDU, from its lattice cell on every side).
    let pick = Aabb {
        min: lo.map(|v| v.round() as i32),
        max: hi.map(|v| v.round() as i32),
    };
    // The body box stops at the stud bases (Y = 0 in the part frame).
    let stud_base = connectors
        .iter()
        .filter(|c| c.kind == ConnKind::StudM)
        .map(|c| c.pos[1])
        .min();
    let mut body_box = pick;
    if let Some(y) = stud_base {
        body_box.min[1] = y;
    }
    Ok(BuiltPart {
        shape: PartShape {
            body: body_box,
            pick,
            connectors,
        },
        mesh,
        height_ldu,
    })
}

/// Kernel frame (mm, +Z up, body minimum corner at the origin) to the part's
/// LDraw frame (LDU, minus Y up, centred on the stud group, top at Y = 0):
/// kernel (x, y, z) is LDraw (x, -z, y) up to the origin shift, a proper
/// rotation, so the winding stays counter-clockwise from outside.
fn to_ldraw(mesh: &Mesh, studs: [u32; 2], height_ldu: i32) -> PartMesh {
    let s = STANDARD.get();
    let ldu = s.ldu_mm;
    let gap = s.mm("undersize_per_side");
    // The lattice cell corner is one undersize outside the body's corner, and
    // the cell spans n studs of HALF_STUD_LDU * 2 each, centred on the origin.
    let half = |n: u32| f64::from(HALF_STUD_LDU) * f64::from(n);
    let (hx, hz) = (half(studs[0]), half(studs[1]));
    let positions = mesh
        .positions
        .iter()
        .map(|p| {
            let [x, y, z] = p.map(f64::from);
            [
                ((x + gap) / ldu - hx) as f32,
                (f64::from(height_ldu) - z / ldu) as f32,
                ((y + gap) / ldu - hz) as f32,
            ]
        })
        .collect();
    PartMesh {
        positions,
        indices: mesh.indices.clone(),
    }
}

fn bounds(mesh: &PartMesh) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(f64::from(p[k]));
            hi[k] = hi[k].max(f64::from(p[k]));
        }
    }
    (lo, hi)
}

/// The stud-less hull of a part: the box around the vertices of its body (those at or below the
/// stud base line `body.min[1]`), 8 vertices and 12 triangles, counter-clockwise from outside like
/// every mesh here. This is the lowest render level ([`Lod::Hull`], docs/plan/07 section 3: "LOD2
/// stud-less hull, about 12 triangles"); it is made from the mesh, never typed in, and it is never
/// used for collision (the engine's boxes come from the part's features, not from any mesh).
pub fn body_hull(mesh: &PartMesh, body: &Aabb) -> PartMesh {
    let floor = body.min[1] as f32 - 0.25;
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in mesh.positions.iter().filter(|p| p[1] >= floor) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if lo[0] > hi[0] {
        lo = body.min.map(|v| v as f32);
        hi = body.max.map(|v| v as f32);
    }
    let corner = |i: usize| {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    let positions: Vec<[f32; 3]> = (0..8).map(corner).collect();
    // Two triangles per face; each is turned round when its normal points into the box.
    let faces: [[u32; 4]; 6] = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    let centre = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let mut indices = Vec::with_capacity(36);
    for f in faces {
        for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
            let [a, b, c] = tri.map(|i| positions[i as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let out = [a[0] - centre[0], a[1] - centre[1], a[2] - centre[2]];
            let outward = n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0;
            if outward {
                indices.extend(tri);
            } else {
                indices.extend([tri[0], tri[2], tri[1]]);
            }
        }
    }
    PartMesh { positions, indices }
}
