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

use openbrick_catalog::text::{has_unsafe, safe_msg};
use openbrick_catalog::{Catalog, Colour, Origin, Palette, PartId, CATALOG_ID, LDRAW_ID_PREFIX};
use openbrick_units::{Credit, LatticePose, Rot24};

use crate::model::{EngineError, Instance, InstanceId, Model};

/// The schema version this build writes.
pub const MODEL_PRAGMA: &str = "1.0";
const UNITS: &str = "ldu";
const FRAME: &str = "ldraw-y-down";
const MAIN: &str = "main";
/// The most entries `requires` may have (one pack kind exists; the schema says the same).
pub const MAX_REQUIRES: usize = 16;

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
    #[error("not a model document: {}", safe_msg(.0))]
    Json(String),
    #[error(
        "schema version {} is not readable by this build, which reads 1.x; update OpenBrick3D",
        safe_msg(.0)
    )]
    Version(String),
    #[error("the document's kind is `{}`, not `model`; open it in the matching tool", safe_msg(.0))]
    Kind(String),
    #[error("`{field}` is `{}`; this build reads only `{want}`", safe_msg(got))]
    Unsupported {
        field: &'static str,
        got: String,
        want: &'static str,
    },
    #[error(
        "the model uses the part catalog `{}`; this build has `{CATALOG_ID}`; update OpenBrick3D",
        safe_msg(.0)
    )]
    Catalog(String),
    #[error("`requires` {0}")]
    Requires(String),
    #[error("instance {id}: part `{}` comes from the LDraw pack `{}` (library {}), which is not loaded; add it first (Build mode: \"Add parts from an LDraw library…\"; command line: --pack), see docs/guides/ldraw-parts.md", safe_msg(part), safe_msg(pack), safe_msg(library))]
    MissingPack {
        id: InstanceId,
        part: String,
        pack: String,
        library: String,
    },
    #[error("the model has {0} submodels; this build reads exactly one, `main`")]
    Submodels(usize),
    #[error("the model has {count} parts; this build holds at most {limit}. Split it into smaller models")]
    TooManyInstances { count: usize, limit: usize },
    #[error(
        "instance {id}: part `{}` is not in the catalog `{CATALOG_ID}`",
        safe_msg(part)
    )]
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
        self.text_with(None)
    }

    /// [`ModelDoc::to_text`], with the instances of the one submodel written by `instances` when
    /// given (the model streams them without making a document of them first).
    fn text_with(&self, instances: Option<&dyn Fn(&mut String)>) -> String {
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
        let submodels: Vec<String> = match instances {
            Some(write) => {
                let mut body = String::new();
                write(&mut body);
                vec![format!(
                    "{{\n      \"id\": {},\n      \"instances\": {body}\n    }}",
                    j(MAIN)
                )]
            }
            None => self
                .submodels
                .iter()
                .map(|s| {
                    format!(
                        "{{\n      \"id\": {},\n      \"instances\": {}\n    }}",
                        j(&s.id),
                        list(&s.instances, "        ", "      ")
                    )
                })
                .collect(),
        };
        field("submodels", items(&submodels, "    ", "  "), false);
        field("bom", list(&self.bom, "    ", "  "), true);
        out.push_str("}\n");
        out
    }
}

/// `requires` is bounded and well formed here, as the schema says: a pack named in lower case, a
/// library version of at most 32 printable characters, a fingerprint of 64 lower-case hex digits.
fn check_requires(requires: &[PackRequirement]) -> Result<(), DocError> {
    let bad = |n: usize, why: &str| DocError::Requires(format!("entry {}: {why}", n + 1));
    if requires.len() > MAX_REQUIRES {
        return Err(DocError::Requires(format!(
            "has {} entries; at most {MAX_REQUIRES} are read",
            requires.len()
        )));
    }
    for (n, r) in requires.iter().enumerate() {
        let name_ok = !r.pack.is_empty()
            && r.pack.len() <= 32
            && r.pack
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !name_ok {
            return Err(bad(
                n,
                "`pack` must be 1 to 32 lower-case letters, digits or `-`",
            ));
        }
        if r.library.is_empty() || r.library.chars().count() > 32 || has_unsafe(&r.library) {
            return Err(bad(
                n,
                "`library` must be 1 to 32 printable characters, like `2026-10`",
            ));
        }
        let hex = r.fingerprint.len() == 64
            && r.fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex {
            return Err(bad(
                n,
                "`fingerprint` must be 64 lower-case hexadecimal digits (the SHA-256 of the pack)",
            ));
        }
    }
    Ok(())
}

/// The notices for a model that opened with a pack other than the one it was made with.
fn pack_notices(catalog: &Catalog, doc: &ModelDoc, model: &Model) -> Vec<String> {
    let Some(loaded) = &catalog.pack else {
        return Vec::new();
    };
    let uses_pack = model
        .instances()
        .parts()
        .any(|p| catalog.parts[p as usize].origin == Origin::Ldraw);
    if !uses_pack {
        return Vec::new();
    }
    let short = |f: &str| f.chars().take(12).collect::<String>();
    doc.requires
        .iter()
        .filter(|r| r.pack == loaded.id && r.fingerprint != loaded.fingerprint)
        .map(|r| {
            if r.library == loaded.library {
                format!(
                    "This model was made with a different build of the `{}` pack (library {}, fingerprint {}…); the loaded pack is the same library version but another build ({}…). Every part it uses was found, but their shapes may differ slightly from the ones the model was built with.",
                    r.pack, safe_msg(&r.library), short(&r.fingerprint), short(&loaded.fingerprint)
                )
            } else {
                format!(
                    "This model was made with the `{}` pack of library {} (fingerprint {}…); the loaded pack is library {} (fingerprint {}…). Every part it uses was found, but their shapes may differ from the ones the model was built with.",
                    r.pack, safe_msg(&r.library), short(&r.fingerprint), loaded.library, short(&loaded.fingerprint)
                )
            }
        })
        .collect()
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
    /// The bill of materials: parts by catalog order, then colour code. Kept in step by every
    /// edit, so this is a walk over the distinct (part, colour) pairs, not over the parts.
    pub fn bom(&self) -> Bom {
        let palette = Palette::bundled();
        self.bom_counts()
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
            .bom_counts()
            .any(|((p, _), _)| catalog.parts[p as usize].origin == Origin::Ldraw);
        match (&catalog.pack, uses_pack) {
            (Some(pack), true) => vec![PackRequirement {
                pack: pack.id.clone(),
                library: pack.library.clone(),
                fingerprint: pack.fingerprint.clone(),
            }],
            _ => Vec::new(),
        }
    }

    /// The document without its instances (the header, the palette, the bill of materials and
    /// an empty `main` submodel).
    fn doc_header(&self, name: &str, credit: Option<Credit>) -> ModelDoc {
        let palette = Palette::bundled();
        let used: Vec<Colour> = palette
            .colours
            .iter()
            .filter(|c| self.bom_counts().any(|((_, colour), _)| colour == c.code))
            .cloned()
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
                instances: Vec::new(),
            }],
            bom: self.bom(),
        }
    }

    /// The model as a document named `name`.
    pub fn to_doc(&self, name: &str, credit: Option<Credit>) -> ModelDoc {
        let mut doc = self.doc_header(name, credit);
        doc.submodels[0].instances = self
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
        doc
    }

    /// The instances as the text of the `instances` array, one per line in table order, exactly
    /// as the document form writes them but without making a document of 100,000 parts first.
    fn write_instances(&self, out: &mut String) {
        use std::fmt::Write;
        if self.instances.is_empty() {
            out.push_str("[]");
            return;
        }
        out.reserve(self.len() * 96);
        let mut part_json: Vec<Option<String>> = vec![None; self.catalog().len()];
        out.push_str("[\n");
        for (n, i) in self.instances.iter().enumerate() {
            if n > 0 {
                out.push_str(",\n");
            }
            let part = part_json[i.part as usize]
                .get_or_insert_with(|| j(&self.catalog().parts[i.part as usize].id));
            let _ = write!(
                out,
                "        {{\"id\":{},\"part\":{part},\"pose\":{{\"pos\":[{},{},{}],\"rot\":{}}},\"colour\":{}}}",
                i.id, i.pose.pos[0], i.pose.pos[1], i.pose.pos[2], i.pose.rot.0, i.colour
            );
        }
        out.push_str("\n      ]");
    }

    /// A model from a document: every instance is checked as if placed by
    /// hand (known part and colour, on the stud grid, above the ground, no
    /// overlap), in document order. The history starts empty. See
    /// [`Model::from_doc_with_notices`] for what the loaded pack may differ in.
    pub fn from_doc(catalog: &'c Catalog, doc: &ModelDoc) -> Result<Model<'c>, DocError> {
        Model::from_doc_with_notices(catalog, doc).map(|(m, _)| m)
    }

    /// [`Model::from_doc`], and the notices a person should see though the model opened: the
    /// document names a pack by library version and fingerprint, and the loaded pack is another
    /// build of it (so the parts it has may be drawn or bounded differently from the ones the
    /// model was built with). A pack of another version still opens a model as long as it has every
    /// part (docs/guides/ldraw-parts.md); this is how the difference is not silent.
    pub fn from_doc_with_notices(
        catalog: &'c Catalog,
        doc: &ModelDoc,
    ) -> Result<(Model<'c>, Vec<String>), DocError> {
        let mut loader = Loader::new(catalog, doc.clone())?;
        loader.step(usize::MAX)?;
        loader.finish()
    }

    /// Read a `.obrick` document's text.
    pub fn from_text(catalog: &'c Catalog, text: &str) -> Result<Model<'c>, DocError> {
        Model::from_doc(catalog, &ModelDoc::parse(text)?)
    }

    /// Read a `.obrick` document's text, with the notices of [`Model::from_doc_with_notices`].
    pub fn from_text_with_notices(
        catalog: &'c Catalog,
        text: &str,
    ) -> Result<(Model<'c>, Vec<String>), DocError> {
        Model::from_doc_with_notices(catalog, &ModelDoc::parse(text)?)
    }

    /// The `.obrick` text of this model.
    pub fn to_text(&self, name: &str, credit: Option<Credit>) -> String {
        self.doc_header(name, credit)
            .text_with(Some(&|out| self.write_instances(out)))
    }

    /// A model with these parts in this order (a page putting back the table it holds): each is
    /// checked as if placed by hand, as in [`Model::from_doc`]. The history starts empty.
    pub fn from_instances(
        catalog: &'c Catalog,
        instances: impl IntoIterator<Item = Instance>,
    ) -> Result<Model<'c>, DocError> {
        let mut model = Model::new(catalog);
        for (n, inst) in instances.into_iter().enumerate() {
            if n >= crate::MAX_INSTANCES {
                return Err(DocError::TooManyInstances {
                    count: n + 1,
                    limit: crate::MAX_INSTANCES,
                });
            }
            model
                .insert(n, inst)
                .map_err(|error| DocError::Instance { id: inst.id, error })?;
        }
        model.take_changes();
        Ok(model)
    }

    /// Start reading a `.obrick` document's text in steps ([`Loader`]): the page shows progress and
    /// a cancelled open costs nothing.
    pub fn loader(catalog: &'c Catalog, text: &str) -> Result<Loader<'c>, DocError> {
        Loader::new(catalog, ModelDoc::parse(text)?)
    }
}

/// A document being read into a model, a few instances at a time. Every instance is checked as if
/// placed by hand, in document order (see [`Model::from_doc`]); a failure names the instance and
/// drops the loader, and nothing else changed.
pub struct Loader<'c> {
    catalog: &'c Catalog,
    doc: ModelDoc,
    model: Model<'c>,
    next: usize,
    /// Part names already looked up (a pack's catalog is searched by name, which costs a scan).
    parts: std::collections::HashMap<String, PartId>,
}

impl<'c> Loader<'c> {
    /// Check the document's header and size; no instance is read yet.
    pub fn new(catalog: &'c Catalog, doc: ModelDoc) -> Result<Loader<'c>, DocError> {
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
        check_requires(&doc.requires)?;
        let [main] = doc.submodels.as_slice() else {
            return Err(DocError::Submodels(doc.submodels.len()));
        };
        if main.id != MAIN {
            return Err(DocError::Submodels(doc.submodels.len()));
        }
        if main.instances.len() > crate::MAX_INSTANCES {
            return Err(DocError::TooManyInstances {
                count: main.instances.len(),
                limit: crate::MAX_INSTANCES,
            });
        }
        let mut model = Model::new(catalog);
        if main.instances.len() > 2_000 {
            model.reserve(main.instances.len());
        }
        Ok(Loader {
            catalog,
            doc,
            model,
            next: 0,
            parts: std::collections::HashMap::new(),
        })
    }

    /// How many instances the document has.
    pub fn total(&self) -> usize {
        self.doc.submodels[0].instances.len()
    }

    /// How many have been read.
    pub fn done(&self) -> usize {
        self.next
    }

    /// Read up to `max` more instances; returns how many have been read in all.
    pub fn step(&mut self, max: usize) -> Result<usize, DocError> {
        let instances = &self.doc.submodels[0].instances;
        let end = self.next.saturating_add(max).min(instances.len());
        while self.next < end {
            let i = &instances[self.next];
            let part = match self.parts.get(i.part.as_str()) {
                Some(&p) => p,
                None => {
                    let p = self.catalog.find(&i.part).ok_or_else(|| {
                        if i.part.starts_with(LDRAW_ID_PREFIX) {
                            let need = self.doc.requires.first();
                            DocError::MissingPack {
                                id: i.id,
                                part: i.part.clone(),
                                pack: need.map_or_else(|| "ldraw".into(), |r| r.pack.clone()),
                                library: need
                                    .map_or_else(|| "unknown".into(), |r| r.library.clone()),
                            }
                        } else {
                            DocError::UnknownPart {
                                id: i.id,
                                part: i.part.clone(),
                            }
                        }
                    })?;
                    self.parts.insert(i.part.clone(), p);
                    p
                }
            };
            if i.pose.rot >= 24 {
                return Err(DocError::Instance {
                    id: i.id,
                    error: EngineError::UnknownRotation(i.pose.rot),
                });
            }
            self.model
                .insert(
                    self.next,
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
            self.next += 1;
        }
        Ok(self.next)
    }

    /// The model, once every instance has been read, and the notices of
    /// [`Model::from_doc_with_notices`]. The history starts empty and the change list too.
    pub fn finish(mut self) -> Result<(Model<'c>, Vec<String>), DocError> {
        debug_assert_eq!(
            self.next,
            self.total(),
            "finish before every instance was read"
        );
        let notices = pack_notices(self.catalog, &self.doc, &self.model);
        self.model.take_changes();
        Ok((self.model, notices))
    }
}
