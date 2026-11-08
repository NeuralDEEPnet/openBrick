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
    /// derivation: 8 segments, the cheapest build.
    Low,
}

impl Lod {
    /// Segments per full circle of studs, tubes and pins.
    pub fn segments(self) -> u32 {
        match self {
            Lod::Detail => 24,
            Lod::Low => 8,
        }
    }
}

/// The part document of a built-in part: one body, a stud field on top unless
/// it is a tile, and the tube field underneath (tubes, pins and wall ribs).
pub fn part_doc_json(spec: &BuiltinSpec) -> String {
    let studs = if spec.body == Body::Tile {
        String::new()
    } else {
        r#"{ "id": "studs", "op": "stud.field", "on": "body.top" },"#.to_string()
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
    {{ "id": "body", "op": "{op}", "size": [{x}, {z}] }},
    {studs}
    {{ "id": "bottom", "op": "tube.field", "on": "body.bottom" }}
  ]
}}"#,
        id = spec.id,
        name = spec.name,
        op = spec.body.op(),
        x = spec.studs[0],
        z = spec.studs[1],
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

/// Build one built-in part with the evaluator.
pub fn build(spec: &BuiltinSpec, lod: Lod) -> Result<BuiltPart, CatalogError> {
    let err = |message: String| CatalogError {
        id: spec.id.to_string(),
        message,
    };
    let doc = PartDoc::parse(&part_doc_json(spec)).map_err(|e| err(e.to_string()))?;
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
    let height_ldu = connectors
        .iter()
        .filter(|c| c.kind == ConnKind::TubeF)
        .map(|c| c.pos[1])
        .max()
        .ok_or_else(|| err("the part has no receptacle under it".into()))?;
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
