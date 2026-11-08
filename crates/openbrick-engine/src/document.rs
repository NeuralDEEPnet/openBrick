//! The `.obrick` model document (docs/plan/08 sections 1 and 2), schema
//! `schemas/obrick.schema.json`.
//!
//! In this alpha a `.obrick` file is the `model.json` of docs/plan/08 on its
//! own, as plain JSON text: the built-in catalog needs no embedded parts, and
//! the zip container (`model.json`, `parts/`, `thumbnail.png`) arrives with
//! custom parts. A reader can tell the two apart by the first bytes (`{` or
//! the zip signature `PK`).
//!
//! Writing is deterministic: fields in a fixed order, one instance per line in
//! table order, the palette in palette order and the bill of materials sorted,
//! so saving the same model twice gives the same bytes and a diff shows what
//! changed. The bill of materials is written for people and other tools; the
//! reader recomputes it rather than trusting it.

use std::collections::BTreeMap;

use openbrick_catalog::{Catalog, Colour, Origin, Palette, CATALOG_ID, LDRAW_ID_PREFIX};
use openbrick_units::{Credit, LatticePose, Rot24};

use crate::model::{EngineError, Instance, InstanceId, Model};

/// The schema version this build writes.
pub const MODEL_PRAGMA: &str = "1.0";
const UNITS: &str = "ldu";
const FRAME: &str = "ldraw-y-down";
const MAIN: &str = "main";

/// The document as read and written.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelDoc {
    pub openbrick: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<Credit>,
    pub units: String,
    pub frame: String,
    /// The catalog the part ids name.
    pub catalog: String,
    /// The part packs the model needs besides the built-in catalog: written when a
    /// part of an LDraw pack is placed, so a reader can say which pack to load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<PackRequirement>,
    /// The colours the model uses, for tools without our palette.
    #[serde(default)]
    pub palette: Vec<Colour>,
    pub submodels: Vec<Submodel>,
    #[serde(default)]
    pub bom: Vec<BomLine>,
}

/// A part pack a model needs (`.obpack`, docs/plan/08 section 1): which library
/// version it was made from and its fingerprint (SHA-256 of the pack file).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PackRequirement {
    /// `ldraw`.
    pub pack: String,
    /// The LDraw library version, `2026-10`.
    pub library: String,
    /// The pack's fingerprint, 64 hex digits.
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Submodel {
    pub id: String,
    pub instances: Vec<InstanceDoc>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstanceDoc {
    pub id: InstanceId,
    pub part: String,
    pub pose: PoseDoc,
    pub colour: u16,
}

/// A lattice pose: integer LDU position and a rotation index 0 to 23.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PoseDoc {
    pub pos: [i32; 3],
    pub rot: u8,
}

/// One line of the bill of materials: how many of a part in a colour.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BomLine {
    pub part: String,
    pub name: String,
    pub colour: u16,
    pub colour_name: String,
    pub count: u32,
}

/// The bill of materials, sorted by catalog order then colour code.
pub type Bom = Vec<BomLine>;

/// Why a document could not be read. Each message says what to change.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DocError {
    #[error("not a model document: {0}")]
    Json(String),
    #[error(
        "schema version {0} is not readable by this build, which reads 1.x; update OpenBrick3D"
    )]
    Version(String),
    #[error("the document's kind is `{0}`, not `model`; open it in the matching tool")]
    Kind(String),
    #[error("`{field}` is `{got}`; this build reads only `{want}`")]
    Unsupported {
        field: &'static str,
        got: String,
        want: &'static str,
    },
    #[error(
        "the model uses the part catalog `{0}`; this build has `{CATALOG_ID}`; update OpenBrick3D"
    )]
    Catalog(String),
    #[error("instance {id}: part `{part}` comes from the LDraw pack `{pack}` (library {library}), which is not loaded; add it first (Build mode: \"Add parts from an LDraw library…\"; command line: --pack), see docs/guides/ldraw-parts.md")]
    MissingPack {
        id: InstanceId,
        part: String,
        pack: String,
        library: String,
    },
    #[error("the model has {0} submodels; this build reads exactly one, `main`")]
    Submodels(usize),
    #[error("instance {id}: part `{part}` is not in the catalog `{CATALOG_ID}`")]
    UnknownPart { id: InstanceId, part: String },
    #[error("instance {id}: {error}")]
    Instance { id: InstanceId, error: EngineError },
}

impl ModelDoc {
    /// Parse a document; the structural checks happen in [`Model::from_doc`].
    pub fn parse(text: &str) -> Result<ModelDoc, DocError> {
        serde_json::from_str(text).map_err(|e| DocError::Json(e.to_string()))
    }

    /// The deterministic text form (module documentation), ending in a newline.
    pub fn to_text(&self) -> String {
        let mut out = String::from("{\n");
        let mut field = |name: &str, value: String, last: bool| {
            out.push_str(&format!(
                "  {}: {value}{}\n",
                j(name),
                if last { "" } else { "," }
            ));
        };
        field("openbrick", j(&self.openbrick), false);
        field("kind", j(&self.kind), false);
        field("name", j(&self.name), false);
        if let Some(c) = &self.credit {
            field("credit", j(c), false);
        }
        field("units", j(&self.units), false);
        field("frame", j(&self.frame), false);
        field("catalog", j(&self.catalog), false);
        if !self.requires.is_empty() {
            field("requires", list(&self.requires, "    ", "  "), false);
        }
        field("palette", list(&self.palette, "    ", "  "), false);
        let submodels: Vec<String> = self
            .submodels
            .iter()
            .map(|s| {
                format!(
                    "{{\n      \"id\": {},\n      \"instances\": {}\n    }}",
                    j(&s.id),
                    list(&s.instances, "        ", "      ")
                )
            })
            .collect();
        field("submodels", items(&submodels, "    ", "  "), false);
        field("bom", list(&self.bom, "    ", "  "), true);
        out.push_str("}\n");
        out
    }
}

/// `[` items one per line `]`, or `[]`.
fn items(items: &[String], indent: &str, close: &str) -> String {
    if items.is_empty() {
        return "[]".into();
    }
    let body: Vec<String> = items.iter().map(|i| format!("{indent}{i}")).collect();
    format!("[\n{}\n{close}]", body.join(",\n"))
}

fn list<T: serde::Serialize>(values: &[T], indent: &str, close: &str) -> String {
    let lines: Vec<String> = values.iter().map(|v| j(v)).collect();
    items(&lines, indent, close)
}

/// Compact JSON of one value.
fn j<T: serde::Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).expect("document parts serialise")
}

impl<'c> Model<'c> {
    /// The bill of materials: parts by catalog order, then colour code.
    pub fn bom(&self) -> Bom {
        let palette = Palette::bundled();
        let mut counts: BTreeMap<(u32, u16), u32> = BTreeMap::new();
        for i in self.instances.iter() {
            *counts.entry((i.part, i.colour)).or_default() += 1;
        }
        counts
            .into_iter()
            .map(|((part, colour), count)| {
                let p = &self.catalog().parts[part as usize];
                BomLine {
                    part: p.id.clone(),
                    name: p.name.clone(),
                    colour,
                    colour_name: palette
                        .get(colour)
                        .map(|c| c.name.clone())
                        .unwrap_or_default(),
                    count,
                }
            })
            .collect()
    }

    /// The packs the placed parts need: one entry when any part comes from the loaded pack.
    pub fn requirements(&self) -> Vec<PackRequirement> {
        let catalog = self.catalog();
        let uses_pack = self
            .instances
            .part
            .iter()
            .any(|&p| catalog.parts[p as usize].origin == Origin::Ldraw);
        match (&catalog.pack, uses_pack) {
            (Some(pack), true) => vec![PackRequirement {
                pack: pack.id.clone(),
                library: pack.library.clone(),
                fingerprint: pack.fingerprint.clone(),
            }],
            _ => Vec::new(),
        }
    }

    /// The model as a document named `name`.
    pub fn to_doc(&self, name: &str, credit: Option<Credit>) -> ModelDoc {
        let palette = Palette::bundled();
        let used: Vec<Colour> = palette
            .colours
            .iter()
            .filter(|c| self.instances.colour.contains(&c.code))
            .cloned()
            .collect();
        let instances = self
            .instances
            .iter()
            .map(|i| InstanceDoc {
                id: i.id,
                part: self.catalog().parts[i.part as usize].id.clone(),
                pose: PoseDoc {
                    pos: i.pose.pos,
                    rot: i.pose.rot.0,
                },
                colour: i.colour,
            })
            .collect();
        ModelDoc {
            openbrick: MODEL_PRAGMA.into(),
            kind: "model".into(),
            name: name.into(),
            credit,
            units: UNITS.into(),
            frame: FRAME.into(),
            catalog: CATALOG_ID.into(),
            requires: self.requirements(),
            palette: used,
            submodels: vec![Submodel {
                id: MAIN.into(),
                instances,
            }],
            bom: self.bom(),
        }
    }

    /// A model from a document: every instance is checked as if placed by
    /// hand (known part and colour, on the stud grid, above the ground, no
    /// overlap), in document order. The history starts empty.
    pub fn from_doc(catalog: &'c Catalog, doc: &ModelDoc) -> Result<Model<'c>, DocError> {
        if doc.openbrick.split('.').next() != Some("1") {
            return Err(DocError::Version(doc.openbrick.clone()));
        }
        if doc.kind != "model" {
            return Err(DocError::Kind(doc.kind.clone()));
        }
        for (field, got, want) in [("units", &doc.units, UNITS), ("frame", &doc.frame, FRAME)] {
            if got != want {
                return Err(DocError::Unsupported {
                    field,
                    got: got.clone(),
                    want,
                });
            }
        }
        if doc.catalog != CATALOG_ID {
            return Err(DocError::Catalog(doc.catalog.clone()));
        }
        let [main] = doc.submodels.as_slice() else {
            return Err(DocError::Submodels(doc.submodels.len()));
        };
        if main.id != MAIN {
            return Err(DocError::Submodels(doc.submodels.len()));
        }
        let mut model = Model::new(catalog);
        for (index, i) in main.instances.iter().enumerate() {
            let part = catalog.find(&i.part).ok_or_else(|| {
                if i.part.starts_with(LDRAW_ID_PREFIX) {
                    let need = doc.requires.first();
                    DocError::MissingPack {
                        id: i.id,
                        part: i.part.clone(),
                        pack: need.map_or_else(|| "ldraw".into(), |r| r.pack.clone()),
                        library: need.map_or_else(|| "unknown".into(), |r| r.library.clone()),
                    }
                } else {
                    DocError::UnknownPart {
                        id: i.id,
                        part: i.part.clone(),
                    }
                }
            })?;
            if i.pose.rot >= 24 {
                return Err(DocError::Instance {
                    id: i.id,
                    error: EngineError::UnknownRotation(i.pose.rot),
                });
            }
            model
                .insert(
                    index,
                    Instance {
                        id: i.id,
                        part,
                        pose: LatticePose {
                            pos: i.pose.pos,
                            rot: Rot24(i.pose.rot),
                        },
                        colour: i.colour,
                    },
                )
                .map_err(|error| DocError::Instance { id: i.id, error })?;
        }
        Ok(model)
    }

    /// Read a `.obrick` document's text.
    pub fn from_text(catalog: &'c Catalog, text: &str) -> Result<Model<'c>, DocError> {
        Model::from_doc(catalog, &ModelDoc::parse(text)?)
    }

    /// The `.obrick` text of this model.
    pub fn to_text(&self, name: &str, credit: Option<Credit>) -> String {
        self.to_doc(name, credit).to_text()
    }
}
