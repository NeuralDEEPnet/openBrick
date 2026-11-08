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

use openbrick_catalog::{builtin, Catalog, Lod, Origin, Pack, PartMesh, BUILTIN, CATALOG_ID};
use openbrick_engine::{Command, Model, ModelDoc, Ray};
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

/// The LDraw pack this core has loaded, and the catalog built from it.
struct Loaded {
    pack: Arc<Pack>,
    catalog: &'static Catalog,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
/// Catalogs made so far, by pack fingerprint: a model borrows its catalog for as
/// long as it lives, so a catalog is leaked once per distinct pack and reused when
/// the same pack is loaded again (a worker restart). At most [`MAX_PACK_LOADS`]
/// distinct packs per core; a catalog is only the parts' boxes and connectors,
/// never their meshes.
static CATALOGS: Mutex<Vec<(String, &'static Catalog)>> = Mutex::new(Vec::new());
const MAX_PACK_LOADS: usize = 16;

fn lock<T>(m: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, CoreError> {
    m.lock()
        .map_err(|_| CoreError("the assembler's state is poisoned; reload the page".into()))
}

fn catalog() -> Result<&'static Catalog, CoreError> {
    if let Some(l) = lock(&LOADED)?.as_ref() {
        return Ok(l.catalog);
    }
    Catalog::builtin().map_err(|e| CoreError(e.to_string()))
}

fn loaded_pack() -> Result<Option<Arc<Pack>>, CoreError> {
    Ok(lock(&LOADED)?.as_ref().map(|l| l.pack.clone()))
}

/// Load an LDraw pack (`.obpack` bytes) into this core: its parts join the catalog
/// after the built-in ones (part numbers of the built-ins and every built-in id stay
/// as they are). New assemblers (`new Assembler()`) place its parts; one made before
/// keeps the catalog it was made with, so the page makes a new one and loads its
/// model text into it. Returns the pack's info as JSON: `{ id, library,
/// fingerprint, attribution, licences: [{ name, text }], counts, parts, resolution,
/// generator }`. A file that is not a pack, or is damaged, is an error and changes nothing.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_load_pack(bytes: &[u8]) -> Result<String, CoreError> {
    let pack = Pack::from_bytes(bytes).map_err(engine)?;
    let base = Catalog::builtin().map_err(|e| CoreError(e.to_string()))?;
    let fingerprint = pack.fingerprint().to_string();
    let cat = {
        let mut made = lock(&CATALOGS)?;
        if let Some((_, c)) = made.iter().find(|(f, _)| *f == fingerprint) {
            *c
        } else {
            if made.len() >= MAX_PACK_LOADS {
                return Err(CoreError(format!(
                    "{MAX_PACK_LOADS} different packs were loaded in this session; reload the page to load another"
                )));
            }
            let leaked: &'static Catalog = Box::leak(Box::new(base.with_pack(&pack)));
            made.push((fingerprint.clone(), leaked));
            leaked
        }
    };
    let info = pack_info(&pack);
    *lock(&LOADED)? = Some(Loaded {
        pack: Arc::new(pack),
        catalog: cat,
    });
    Ok(info.to_string())
}

/// Forget the loaded LDraw pack; new assemblers have the built-in catalog only.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_unload_pack() -> Result<(), CoreError> {
    *lock(&LOADED)? = None;
    Ok(())
}

fn pack_info(pack: &Pack) -> serde_json::Value {
    let h = &pack.header;
    json!({
        "id": h.id, "library": h.library, "fingerprint": pack.fingerprint(),
        "attribution": h.attribution, "licences": h.licences, "counts": h.counts,
        "parts": h.parts.len(), "resolution": h.resolution, "generator": h.generator,
        "format": h.format,
    })
}

/// What a `.obrick` document needs that this core cannot give: `{ requires: [{ pack,
/// library, fingerprint }], missing: [part ids, first 20], missingCount }`. The page
/// asks before it loads a model, so a model that needs a pack can offer to add it
/// instead of failing.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_model_needs(text: &str) -> Result<String, CoreError> {
    if text.len() > MAX_MODEL_BYTES {
        return Err(CoreError(format!(
            "the file is {} bytes; a model file can be at most {MAX_MODEL_BYTES} bytes",
            text.len()
        )));
    }
    let doc = ModelDoc::parse(text).map_err(engine)?;
    let c = catalog()?;
    let mut missing: Vec<String> = Vec::new();
    let mut count = 0usize;
    for s in &doc.submodels {
        for i in &s.instances {
            if c.find(&i.part).is_none() {
                count += 1;
                if missing.len() < 20 && !missing.contains(&i.part) {
                    missing.push(i.part.clone());
                }
            }
        }
    }
    Ok(json!({ "requires": doc.requires, "missing": missing, "missingCount": count }).to_string())
}

fn engine<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError(e.to_string())
}

/// The catalog the assembler places parts from, its palette, the 24 rotation
/// matrices and the yaw rotations the page offers, as JSON: `{ catalog, lduMm,
/// maxModelBytes, studPitchLdu, plateLdu,
/// parts: [{ id, name, body, origin, category, connectivity, joints, studs: [x, z],
/// heightLdu, base, bodyBox, pickBox, connectors, author?, license? }], pack (the
/// loaded LDraw pack's info, or null), palette, rotations (24 row-major 3x3 matrices), yaws (the
/// rotation index of 0 to 3 quarter turns), samples: [{ id, label, file }] }`.
/// Boxes are `{ min, max }` in integer LDU in the part's frame.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_catalog_json() -> Result<String, CoreError> {
    let c = catalog()?;
    let pack = loaded_pack()?;
    let first_pack = c
        .parts
        .iter()
        .take_while(|p| p.origin == Origin::Builtin)
        .count();
    let parts: Vec<_> = c
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut v = json!({
                "id": p.id, "name": p.name, "body": p.body, "origin": p.origin,
                "category": p.category, "connectivity": p.connectivity, "joints": p.joints,
                "studs": p.studs, "heightLdu": p.height_ldu, "base": p.base,
                "bodyBox": p.shape.body, "pickBox": p.shape.pick,
                "connectors": p.shape.connectors.len(),
            });
            if let Some(pp) = pack.as_ref().and_then(|k| {
                i.checked_sub(first_pack)
                    .and_then(|j| k.header.parts.get(j))
            }) {
                v["author"] = json!(pp.author);
                v["license"] = json!(pp.license);
                v["triangles"] = json!(pp.triangles);
            }
            v
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
        "maxModelBytes": MAX_MODEL_BYTES,
        "maxPackBytes": openbrick_catalog::pack::MAX_PACK_BYTES,
        "studPitchLdu": openbrick_units::STUD_PITCH_LDU,
        "plateLdu": openbrick_units::PLATE_LDU,
        "parts": parts,
        "pack": pack.as_ref().map(|p| pack_info(p)),
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

/// The render mesh of catalog part number `index`: a built-in part is built once by
/// the part evaluator and cached (`detail` false gives the cheap level, 8 segments
/// per stud instead of 24; the page uses it for the baseplate and for big models);
/// a part of the loaded LDraw pack is read from the pack, which has one level
/// (chosen at import), so `detail` does not matter for it.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn assembler_part_mesh(index: u32, detail: bool) -> Result<AsmMesh, CoreError> {
    if index as usize >= BUILTIN.len() {
        let pack = loaded_pack()?.ok_or_else(|| {
            CoreError(format!(
                "there is no catalog part number {index}; no LDraw pack is loaded"
            ))
        })?;
        let m = pack
            .mesh(index as usize - BUILTIN.len())
            .ok_or_else(|| CoreError(format!("there is no catalog part number {index}")))?;
        return Ok(AsmMesh {
            positions: m.positions.iter().flatten().copied().collect(),
            triangles: (m.indices.len() / 3) as u32,
            indices: m.indices,
        });
    }
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
    if o.iter()
        .chain(&d)
        .any(|v| !v.is_finite() || v.abs() > openbrick_engine::MAX_RAY)
        || d == [0.0; 3]
    {
        return Err(CoreError(format!(
            "the pointer ray needs a finite origin and direction, none beyond {} LDU, and a direction that is not zero",
            openbrick_engine::MAX_RAY
        )));
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
    /// A document that does not load leaves the model as it was. Returns the notices of a
    /// model that opened but with a pack other than the one it was made with, as a JSON array
    /// of sentences (`[]` for none).
    pub fn load(&mut self, text: &str) -> Result<String, CoreError> {
        if text.len() > MAX_MODEL_BYTES {
            return Err(CoreError(format!(
                "the file is {} bytes; a model file can be at most {MAX_MODEL_BYTES} bytes ({} MiB)",
                text.len(),
                MAX_MODEL_BYTES / (1024 * 1024)
            )));
        }
        let (m, notices) = Model::from_text_with_notices(catalog()?, text).map_err(engine)?;
        self.model = m;
        self.revision += 1;
        Ok(json!(notices).to_string())
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
