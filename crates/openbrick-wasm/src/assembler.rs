//! The assembler (docs/plan/07) as seen from JavaScript: the built-in catalog
//! with its meshes, and [`Assembler`], one model with snapping, commands, undo
//! and redo, the bill of materials, connection statistics and `.obrick`
//! save and load. The page only displays what comes back and sends pointer
//! rays and commands; snapping, collision and connectivity are the engine's
//! (CLAUDE.md).
//!
//! Small inputs and outputs cross as JSON text; the placed parts cross as one
//! `Int32Array` of seven numbers each ([`Assembler::instances`]) and meshes as
//! [`AsmMesh`], whose arrays are moved out of Rust like [`crate::MeshOut`]'s.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use openbrick_catalog::{builtin, Catalog, Lod, PartMesh, BUILTIN, CATALOG_ID};
use openbrick_engine::{Command, Model, Ray};
use openbrick_units::{Credit, Rot24, STANDARD};
use serde_json::json;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::CoreError;

/// The sample models of `models/`, as committed.
const SAMPLES: [(&str, &str, &str, &str); 2] = [
    (
        "house",
        "Neuraldeep-blue house",
        "house.obrick.json",
        include_str!("../../../models/house.obrick.json"),
    ),
    (
        "staircase",
        "Rainbow staircase",
        "staircase.obrick.json",
        include_str!("../../../models/staircase.obrick.json"),
    ),
];

fn catalog() -> Result<&'static Catalog, CoreError> {
    Catalog::builtin().map_err(|e| CoreError(e.to_string()))
}

fn engine<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError(e.to_string())
}

/// The catalog the assembler places parts from, its palette, the 24 rotation
/// matrices and the yaw rotations the page offers, as JSON: `{ catalog, lduMm,
/// parts: [{ id, name, body, studs: [x, z], heightLdu, base, bodyBox, pickBox,
/// connectors }], palette, rotations (24 row-major 3x3 matrices), yaws (the
/// rotation index of 0 to 3 quarter turns), samples: [{ id, label, file }] }`.
/// Boxes are `{ min, max }` in integer LDU in the part's frame.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_catalog_json() -> Result<String, CoreError> {
    let c = catalog()?;
    let parts: Vec<_> = c
        .parts
        .iter()
        .map(|p| {
            json!({
                "id": p.id, "name": p.name, "body": p.body, "studs": p.studs,
                "heightLdu": p.height_ldu, "base": p.base,
                "bodyBox": p.shape.body, "pickBox": p.shape.pick,
                "connectors": p.shape.connectors.len(),
            })
        })
        .collect();
    let rotations: Vec<_> = Rot24::all().map(|r| r.matrix()).collect();
    let yaws: Vec<u8> = (0..4).map(|n| Rot24::yaw_quarter_turns(n).0).collect();
    let samples: Vec<_> = SAMPLES
        .iter()
        .map(|(id, label, file, _)| json!({ "id": id, "label": label, "file": file }))
        .collect();
    Ok(json!({
        "catalog": CATALOG_ID,
        "lduMm": STANDARD.get().ldu_mm,
        "parts": parts,
        "palette": openbrick_catalog::Palette::bundled(),
        "rotations": rotations,
        "yaws": yaws,
        "samples": samples,
    })
    .to_string())
}

/// The text of a sample model (`id` from [`assembler_catalog_json`]).
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_sample(id: &str) -> Result<String, CoreError> {
    SAMPLES
        .iter()
        .find(|s| s.0 == id)
        .map(|s| s.3.to_string())
        .ok_or_else(|| CoreError(format!("there is no sample model `{id}`")))
}

/// A part mesh in the part's LDraw frame (LDU, minus Y up), counter-clockwise
/// from outside. In JavaScript a class: take its arrays once, then `free()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct AsmMesh {
    positions: Vec<f32>,
    indices: Vec<u32>,
    triangles: u32,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl AsmMesh {
    /// Move the vertex positions out (x, y, z in LDU).
    pub fn take_positions(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.positions)
    }

    /// Move the triangle indices out.
    pub fn take_indices(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.indices)
    }

    pub fn triangle_count(&self) -> u32 {
        self.triangles
    }
}

type MeshCache = Mutex<HashMap<(u32, Lod), Arc<PartMesh>>>;

fn cached(index: u32, lod: Lod) -> Result<Arc<PartMesh>, CoreError> {
    static CACHE: OnceLock<MeshCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(m) = cache
        .lock()
        .map_err(|_| CoreError("mesh cache poisoned".into()))?
        .get(&(index, lod))
    {
        return Ok(m.clone());
    }
    let spec = BUILTIN
        .get(index as usize)
        .ok_or_else(|| CoreError(format!("there is no catalog part number {index}")))?;
    let mesh = Arc::new(builtin::build(spec, lod).map_err(engine)?.mesh);
    cache
        .lock()
        .map_err(|_| CoreError("mesh cache poisoned".into()))?
        .insert((index, lod), mesh.clone());
    Ok(mesh)
}

/// The render mesh of catalog part number `index`, built once by the part
/// evaluator and cached. `detail` false gives the cheap level (8 segments per
/// stud instead of 24): the page uses it for the baseplate and for big models.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_part_mesh(index: u32, detail: bool) -> Result<AsmMesh, CoreError> {
    let m = cached(index, if detail { Lod::Detail } else { Lod::Low })?;
    Ok(AsmMesh {
        positions: m.positions.iter().flatten().copied().collect(),
        indices: m.indices.clone(),
        triangles: m.triangle_count() as u32,
    })
}

/// A model with its history: the engine's [`Model`] over the built-in catalog.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Assembler {
    model: Model<'static>,
    /// Bumped by every change, so the page can tell its copy is current.
    revision: u32,
}

fn ray(o: [f64; 3], d: [f64; 3]) -> Result<Ray, CoreError> {
    if o.iter().chain(&d).any(|v| !v.is_finite()) || d == [0.0; 3] {
        return Err(CoreError(
            "the pointer ray needs a finite origin and a direction that is not zero".into(),
        ));
    }
    Ok(Ray { origin: o, dir: d })
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Assembler {
    /// An empty model.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn create() -> Result<Assembler, CoreError> {
        Ok(Assembler {
            model: Model::new(catalog()?),
            revision: 0,
        })
    }

    /// Replace the model with a `.obrick` document; the history starts empty.
    /// A document that does not load leaves the model as it was.
    pub fn load(&mut self, text: &str) -> Result<(), CoreError> {
        if text.len() > MAX_MODEL_BYTES {
            return Err(CoreError(format!(
                "the file is {} bytes; a model file can be at most {MAX_MODEL_BYTES} bytes ({} MiB)",
                text.len(),
                MAX_MODEL_BYTES / (1024 * 1024)
            )));
        }
        let m = Model::from_text(catalog()?, text).map_err(engine)?;
        self.model = m;
        self.revision += 1;
        Ok(())
    }

    /// The model as `.obrick` text named `name` (deterministic).
    pub fn save(&self, name: &str) -> String {
        let credit = Credit {
            author: "OpenBrick3D assembler".into(),
            url: String::new(),
        };
        self.model.to_text(name, Some(credit))
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// The placed parts: seven numbers each, in placement order: id, catalog
    /// part number, x, y, z (LDU), rotation index, colour code.
    pub fn instances(&self) -> Vec<i32> {
        let mut out = Vec::with_capacity(self.model.len() * 7);
        for i in self.model.instances().iter() {
            out.extend([
                i.id as i32,
                i.part as i32,
                i.pose.pos[0],
                i.pose.pos[1],
                i.pose.pos[2],
                i32::from(i.pose.rot.0),
                i32::from(i.colour),
            ]);
        }
        out
    }

    /// The best pose for catalog part `part` turned by rotation index `rot`
    /// where the ray points: JSON of a `Snap` (`{ pos, rot, valid, mates,
    /// grounded, collides, target }`) or `null` when the ray hits nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn snap(
        &self,
        part: &str,
        rot: u8,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> Result<String, CoreError> {
        let p = self
            .model
            .catalog()
            .find(part)
            .ok_or_else(|| CoreError(format!("there is no part `{part}` in the catalog")))?;
        let s = self
            .model
            .snap(p, Rot24(rot), &ray([ox, oy, oz], [dx, dy, dz])?)
            .map_err(engine)?;
        Ok(serde_json::to_string(&s).expect("snap serialises"))
    }

    /// The first thing the ray hits: JSON of a `Hit` (`{ instance, point,
    /// normal, t }`, `instance` null for the ground) or `null`.
    pub fn pick(
        &self,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> Result<String, CoreError> {
        let h = self.model.pick(&ray([ox, oy, oz], [dx, dy, dz])?);
        Ok(serde_json::to_string(&h).expect("hit serialises"))
    }

    /// Apply a command (JSON, `openbrick_engine::Command`) and record it for
    /// undo. Returns `{ id }`, the part added, moved or recoloured, or null.
    /// A refused command is an error whose message says what to do instead.
    pub fn apply(&mut self, command_json: &str) -> Result<String, CoreError> {
        let cmd: Command = serde_json::from_str(command_json)
            .map_err(|e| CoreError(format!("not a command: {e}")))?;
        let id = self.model.apply(&cmd).map_err(engine)?;
        self.revision += 1;
        Ok(json!({ "id": id }).to_string())
    }

    pub fn undo(&mut self) -> Result<(), CoreError> {
        self.model.undo().map_err(engine)?;
        self.revision += 1;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), CoreError> {
        self.model.redo().map_err(engine)?;
        self.revision += 1;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        self.model.history().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.model.history().can_redo()
    }

    /// Part by colour counts as JSON: `[{ part, name, colour, colourName, count }]`.
    pub fn bom(&self) -> String {
        serde_json::to_string(&self.model.bom()).expect("bom serialises")
    }

    /// Parts, connections, components, grounded and floating parts as JSON
    /// (`openbrick_engine::Stats`).
    pub fn stats(&self) -> String {
        serde_json::to_string(&self.model.stats()).expect("stats serialise")
    }
}

/// The largest model file the page opens: 16 MiB, about 100,000 parts.
pub const MAX_MODEL_BYTES: usize = 16 * 1024 * 1024;

/// The rotation index of `rot` turned `quarter_turns` about the vertical axis.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_turn_rotation(rot: u8, quarter_turns: i32) -> Result<u8, CoreError> {
    if rot >= 24 {
        return Err(CoreError(format!(
            "rotation {rot} is not one of the 24 lattice rotations (0 to 23)"
        )));
    }
    Ok(Rot24::yaw_quarter_turns(quarter_turns.rem_euclid(4) as u8)
        .compose(Rot24(rot))
        .0)
}
