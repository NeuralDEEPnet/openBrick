//! The `.obpack` file (docs/plan/08 section 1): a curated subset of the LDraw
//! Parts Library, turned into what the assembler needs (integer-LDU boxes,
//! derived connectors and a render mesh per part) and nothing else.
//!
//! **A pack is derived from the LDraw Parts Library (CC BY 2.0 / CC BY 4.0),
//! made by the user on their own machine and kept there.** It is never
//! committed to this repository nor bundled in a release (docs/plan/12). It
//! carries the library's attribution and licence text, and each part's author
//! and licence line, so a copy keeps its credits.
//!
//! Layout, little-endian, format version 1:
//!
//! | bytes | content |
//! |-------|---------|
//! | 8 | magic `OBPACK1\n` |
//! | 4 | `header_len` |
//! | `header_len` | the header, JSON text ([`PackHeader`]) |
//! | 4 | `blob_len`, the compressed size |
//! | 8 | `blob_raw_len`, the size after inflating |
//! | `blob_len` | the mesh blob, raw DEFLATE: for each part in header order its vertices (`vertices` x 3 `f32`) then its indices (`triangles` x 3 `u32`) |
//! | 32 | SHA-256 of every byte before it (the pack's fingerprint) |
//!
//! Writing is deterministic: the same parts in the same order give the same
//! bytes. Reading is bounded ([`MAX_PACK_BYTES`], [`MAX_HEADER_BYTES`],
//! [`MAX_PARTS`], [`MAX_BLOB_BYTES`], and a length for every text field) and
//! checks the hash, the header and every mesh range and index before it hands
//! out anything, so a damaged or hostile file is an error and never a panic.
//! The size the blob inflates to is checked against what the header says the
//! parts need before anything is inflated, and the inflater is never allowed
//! to go beyond it.

use sha2::{Digest, Sha256};

use crate::builtin::PartMesh;
use crate::shape::{Aabb, Conn, MAX_BOX_CELLS, MAX_BOX_EXTENT_LDU};
use crate::text::{has_unsafe, safe};
use openbrick_units::STUD_PITCH_LDU;

/// The pack format this build writes and reads.
pub const PACK_FORMAT: u32 = 1;
/// The first bytes of every pack.
pub const MAGIC: &[u8; 8] = b"OBPACK1\n";
/// The prefix of the part ids of an LDraw pack: `ld.3001`.
pub const LDRAW_ID_PREFIX: &str = "ld.";
/// The largest pack file read, in bytes. A pack of the 2026-10 library's 1,526 usable parts is 21 MB.
pub const MAX_PACK_BYTES: usize = 128 * 1024 * 1024;
/// The largest header, in bytes (that pack's is 2.9 MB).
pub const MAX_HEADER_BYTES: usize = 16 * 1024 * 1024;
/// The most parts in one pack.
pub const MAX_PARTS: usize = 20_000;
/// The largest mesh blob after inflating, in bytes (that pack's is 62 MB). A reader holds the
/// compressed pack and the blob at once, in a WebAssembly page that is 4 GiB at most.
pub const MAX_BLOB_BYTES: usize = 128 * 1024 * 1024;
/// The longest part name (the LDraw description), in characters.
pub const MAX_NAME_CHARS: usize = 256;
/// The longest category, in characters.
pub const MAX_CATEGORY_CHARS: usize = 64;
/// The longest author or licence line of a part, in characters.
pub const MAX_CREDIT_CHARS: usize = 512;
/// The longest attribution line, in bytes.
pub const MAX_ATTRIBUTION_BYTES: usize = 64 * 1024;
/// The most licence texts, and their names and texts together, in bytes.
pub const MAX_LICENCES: usize = 16;
pub const MAX_LICENCE_BYTES: usize = 1024 * 1024;
/// The longest library version, generator and licence name, in characters.
pub const MAX_LABEL_CHARS: usize = 256;
/// The joint kinds a part may list.
pub const JOINT_KINDS: [&str; 4] = ["clip", "hinge", "ball", "socket"];

/// The footprint of a body box in whole studs along X then Z (at least one each).
pub fn footprint_studs(body: &Aabb) -> [u32; 2] {
    let w = |a: usize| {
        let d = i64::from(body.max[a]) - i64::from(body.min[a]);
        let c = i64::from(STUD_PITCH_LDU);
        ((d + c - 1) / c).clamp(1, i64::from(u32::MAX)) as u32
    };
    [w(0), w(2)]
}
/// The most triangles in one part's mesh.
pub const MAX_TRIANGLES_PER_PART: u32 = 200_000;

/// Whether a part has connectors derived from the LDraw geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Connectivity {
    /// At least one stud, receptacle, pin or axle connector was derived.
    Derived,
    /// None was: the part is placeable on the ground or at the pointer only.
    None,
}

/// A licence text carried in the pack.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LicenceText {
    pub name: String,
    pub text: String,
}

/// How an import went, kept in the pack for `openbrick catalog info`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCounts {
    /// Top-level part files in the library.
    pub scanned: u32,
    /// Part files that passed the metadata filters (real parts, not minifigure, print or alias).
    pub eligible: u32,
    /// Eligible parts of the chosen categories.
    pub in_categories: u32,
    /// Of those, parts with at least one derived connector.
    pub with_connectors: u32,
    /// Parts written to the pack.
    pub imported: u32,
    /// Imported parts without connectors (free placement only).
    pub free_placement: u32,
    /// Imported parts that carry clip, hinge, ball or socket joints this slice does not model.
    pub with_joints: u32,
}

/// One part of a pack: what the engine and the palette need, and where its mesh is.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPart {
    /// `ld.3001`: the pack prefix and the LDraw part number.
    pub id: String,
    /// The LDraw description.
    pub name: String,
    pub category: String,
    pub author: String,
    /// The part file's `!LICENSE` line.
    pub license: String,
    pub connectivity: Connectivity,
    /// Joint kinds found that are not modelled (`clip`, `hinge`, `ball`, `socket`).
    #[serde(default)]
    pub joints: Vec<String>,
    /// Footprint in studs along X then Z, from the body box.
    pub studs: [u32; 2],
    pub height_ldu: i32,
    pub body: Aabb,
    pub pick: Aabb,
    pub connectors: Vec<Conn>,
    pub vertices: u32,
    pub triangles: u32,
    /// Byte offset of this part's mesh in the inflated blob.
    pub offset: u64,
}

/// The pack's JSON header.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackHeader {
    pub openbrick: String,
    pub kind: String,
    pub format: u32,
    /// The pack's name in `.obrick` files: `ldraw`.
    pub id: String,
    /// The library version (`2026-10`): the newest `UPDATE` tag among the imported files.
    pub library: String,
    /// Which primitives the meshes use: `low`, `standard` or `high`.
    pub resolution: String,
    pub generator: String,
    /// The attribution line every client shows while the pack is loaded.
    pub attribution: String,
    pub licences: Vec<LicenceText>,
    pub counts: PackCounts,
    pub parts: Vec<PackPart>,
}

/// Why a pack could not be read or built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    #[error("this is not an OpenBrick3D pack (it does not start with the pack signature); choose an .obpack made by `openbrick catalog import`")]
    NotAPack,
    #[error("the pack is {0} bytes; the limit is {MAX_PACK_BYTES} bytes")]
    TooBig(usize),
    #[error("the pack is damaged: {0}")]
    Damaged(String),
    #[error("the pack's checksum does not match its content; it was changed or cut short, import it again")]
    Checksum,
    #[error("the pack has format version {0}; this build reads version {PACK_FORMAT}; update OpenBrick3D or import the pack again")]
    Version(u32),
}

/// A damaged-pack error. The text is cut and made safe to print: it may repeat parts of the file.
fn damaged(m: impl Into<String>) -> PackError {
    PackError::Damaged(safe(&m.into(), 300))
}

/// A pack in memory: the header and the inflated mesh blob.
#[derive(Clone, Debug)]
pub struct Pack {
    pub header: PackHeader,
    blob: Vec<u8>,
    fingerprint: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Pack {
    /// Build a pack from its parts: `parts[i]` is the part's metadata (its
    /// `vertices`, `triangles` and `offset` are set here) and `meshes[i]` its mesh.
    pub fn build(mut header: PackHeader, meshes: &[PartMesh]) -> Result<Pack, PackError> {
        if header.parts.len() != meshes.len() {
            return Err(damaged("one mesh per part is needed"));
        }
        if header.parts.len() > MAX_PARTS {
            return Err(damaged(format!("more than {MAX_PARTS} parts")));
        }
        let mut blob = Vec::new();
        for (p, m) in header.parts.iter_mut().zip(meshes) {
            p.offset = blob.len() as u64;
            p.vertices = m.positions.len() as u32;
            p.triangles = (m.indices.len() / 3) as u32;
            for v in &m.positions {
                for c in v {
                    blob.extend(c.to_le_bytes());
                }
            }
            for i in &m.indices {
                blob.extend(i.to_le_bytes());
            }
            if blob.len() > MAX_BLOB_BYTES {
                return Err(damaged(
                    "the meshes are too large for one pack; import fewer parts",
                ));
            }
        }
        let mut pack = Pack {
            header,
            blob,
            fingerprint: String::new(),
        };
        let bytes = pack.to_bytes()?;
        pack.fingerprint = hex(&bytes[bytes.len() - 32..]);
        Ok(pack)
    }

    /// The SHA-256 of the pack file without its last 32 bytes, in hex: the
    /// pack's identity, written into `.obrick` files that use it.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The part's mesh, decoded.
    pub fn mesh(&self, index: usize) -> Option<PartMesh> {
        let p = self.header.parts.get(index)?;
        let (nv, nt) = (p.vertices as usize, p.triangles as usize);
        let start = p.offset as usize;
        let end = start + nv * 12 + nt * 12;
        let bytes = self.blob.get(start..end)?;
        let (vb, ib) = bytes.split_at(nv * 12);
        let positions = vb
            .as_chunks::<12>()
            .0
            .iter()
            .map(|c| {
                [
                    f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                    f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                    f32::from_le_bytes([c[8], c[9], c[10], c[11]]),
                ]
            })
            .collect();
        let indices = ib
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        Some(PartMesh { positions, indices })
    }

    /// The deterministic file bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, PackError> {
        let header = serde_json::to_vec(&self.header).map_err(|e| damaged(e.to_string()))?;
        if header.len() > MAX_HEADER_BYTES {
            return Err(damaged("the header is too large for a pack; import fewer parts"));
        }
        let blob = miniz_oxide::deflate::compress_to_vec(&self.blob, 6);
        let mut out = Vec::with_capacity(8 + 4 + header.len() + 12 + blob.len() + 32);
        out.extend(MAGIC);
        out.extend((header.len() as u32).to_le_bytes());
        out.extend(&header);
        out.extend((blob.len() as u32).to_le_bytes());
        out.extend((self.blob.len() as u64).to_le_bytes());
        out.extend(&blob);
        let sum = Sha256::digest(&out);
        out.extend(sum);
        if out.len() > MAX_PACK_BYTES {
            return Err(PackError::TooBig(out.len()));
        }
        Ok(out)
    }

    /// Read a pack, checking everything (module documentation).
    pub fn from_bytes(bytes: &[u8]) -> Result<Pack, PackError> {
        if bytes.len() > MAX_PACK_BYTES {
            return Err(PackError::TooBig(bytes.len()));
        }
        if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
            return Err(PackError::NotAPack);
        }
        if bytes.len() < 8 + 4 + 12 + 32 {
            return Err(damaged("the file is cut short"));
        }
        let (body, sum) = bytes.split_at(bytes.len() - 32);
        if Sha256::digest(body).as_slice() != sum {
            return Err(PackError::Checksum);
        }
        let rd32 = |at: usize| -> Result<usize, PackError> {
            body.get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or_else(|| damaged("the file is cut short"))
        };
        let hlen = rd32(8)?;
        if hlen > MAX_HEADER_BYTES {
            return Err(damaged("the header is too large"));
        }
        let hstart = 12;
        let hbytes = body
            .get(hstart..hstart + hlen)
            .ok_or_else(|| damaged("the header is cut short"))?;
        let header: PackHeader =
            serde_json::from_slice(hbytes).map_err(|e| damaged(format!("the header: {e}")))?;
        if header.format != PACK_FORMAT {
            return Err(PackError::Version(header.format));
        }
        let at = hstart + hlen;
        let clen = rd32(at)?;
        let raw_len = body
            .get(at + 4..at + 12)
            .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
            .ok_or_else(|| damaged("the file is cut short"))? as usize;
        if raw_len > MAX_BLOB_BYTES {
            return Err(damaged("the mesh blob is too large"));
        }
        // What the header's parts need: the blob must be exactly that, so a stream that inflates
        // to more (a bomb) is refused at the declared size, before any more is allocated.
        let needed = header.parts.iter().try_fold(0u64, |n, p| {
            n.checked_add(u64::from(p.vertices) * 12 + u64::from(p.triangles) * 12)
        });
        if needed != Some(raw_len as u64) {
            return Err(damaged(
                "the size of the mesh blob is not the size the parts need",
            ));
        }
        let comp = body
            .get(at + 12..at + 12 + clen)
            .ok_or_else(|| damaged("the mesh blob is cut short"))?;
        if at + 12 + clen != body.len() {
            return Err(damaged("there are extra bytes after the mesh blob"));
        }
        let blob = miniz_oxide::inflate::decompress_to_vec_with_limit(comp, raw_len)
            .map_err(|e| damaged(format!("the mesh blob does not inflate ({:?})", e.status)))?;
        if blob.len() != raw_len {
            return Err(damaged("the mesh blob has the wrong size"));
        }
        let pack = Pack {
            header,
            blob,
            fingerprint: hex(sum),
        };
        pack.check()?;
        Ok(pack)
    }

    /// The text of the header: lengths, and no control, bidirectional or format character in what
    /// the page shows. The licence texts may hold line breaks and tabs, nothing else of the kind.
    fn check_header_text(&self) -> Result<(), PackError> {
        let h = &self.header;
        let plain = |what: &str, v: &str, max_chars: usize| -> Result<(), PackError> {
            if v.chars().count() > max_chars {
                return Err(damaged(format!("the {what} is longer than {max_chars} characters")));
            }
            if has_unsafe(v) {
                return Err(damaged(format!(
                    "the {what} has a control, direction or other invisible character"
                )));
            }
            Ok(())
        };
        let id_ok = !h.id.is_empty()
            && h.id.len() <= 32
            && h.id.starts_with(|c: char| c.is_ascii_lowercase())
            && h.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(damaged(
                "the pack id must be 1 to 32 characters: lower-case letters, digits and `-`, starting with a letter",
            ));
        }
        if !["low", "standard", "high"].contains(&h.resolution.as_str()) {
            return Err(damaged("the resolution is not low, standard or high"));
        }
        plain("openbrick version", &h.openbrick, 16)?;
        plain("library version", &h.library, 32)?;
        plain("generator", &h.generator, MAX_LABEL_CHARS)?;
        if h.attribution.len() > MAX_ATTRIBUTION_BYTES || has_unsafe(&h.attribution) {
            return Err(damaged(
                "the attribution is longer than 64 KiB or has a control or invisible character",
            ));
        }
        if h.licences.len() > MAX_LICENCES {
            return Err(damaged(format!("more than {MAX_LICENCES} licence texts")));
        }
        let mut total = 0usize;
        for l in &h.licences {
            plain("licence name", &l.name, MAX_LABEL_CHARS)?;
            total = total.saturating_add(l.name.len()).saturating_add(l.text.len());
            if l.text.chars().any(|c| !matches!(c, '\n' | '\r' | '\t') && crate::text::is_unsafe_char(c)) {
                return Err(damaged(format!(
                    "the licence text `{}` has a control, direction or other invisible character",
                    safe(&l.name, 60)
                )));
            }
        }
        if total > MAX_LICENCE_BYTES {
            return Err(damaged("the licence texts are longer than 1 MiB in all"));
        }
        Ok(())
    }

    /// The text a part shows in the palette.
    fn check_part_text(&self, p: &PackPart) -> Result<(), PackError> {
        for (what, v, max) in [
            ("name", p.name.as_str(), MAX_NAME_CHARS),
            ("category", p.category.as_str(), MAX_CATEGORY_CHARS),
            ("author", p.author.as_str(), MAX_CREDIT_CHARS),
            ("licence line", p.license.as_str(), MAX_CREDIT_CHARS),
        ] {
            if v.chars().count() > max {
                return Err(damaged(format!(
                    "the {what} of part {} is longer than {max} characters",
                    p.id
                )));
            }
            if has_unsafe(v) {
                return Err(damaged(format!(
                    "the {what} of part {} has a control, direction or other invisible character",
                    p.id
                )));
            }
        }
        if p.joints.len() > JOINT_KINDS.len() || p.joints.iter().any(|j| !JOINT_KINDS.contains(&j.as_str())) {
            return Err(damaged(format!(
                "part {} lists a joint kind other than clip, hinge, ball or socket",
                p.id
            )));
        }
        Ok(())
    }

    /// The structural checks of a pack read from a file.
    fn check(&self) -> Result<(), PackError> {
        let h = &self.header;
        if h.kind != "pack" || h.openbrick.split('.').next() != Some("1") {
            return Err(damaged("the header is not a pack header of version 1"));
        }
        if h.parts.len() > MAX_PARTS {
            return Err(damaged("too many parts"));
        }
        self.check_header_text()?;
        let mut seen = std::collections::HashSet::new();
        let mut next = 0u64;
        for p in &h.parts {
            // Plain ASCII only: no control, bidirectional or other invisible character, and no
            // look-alike letters, in a name that is typed into files and shown in messages.
            if !p.id.starts_with(LDRAW_ID_PREFIX)
                || p.id.len() <= LDRAW_ID_PREFIX.len()
                || p.id.len() > 64
                || !p.id.chars().all(|c| c.is_ascii_graphic())
            {
                return Err(damaged(format!(
                    "part id `{}` is not valid (`ld.` and a part number, at most 64 printable ASCII characters)",
                    p.id.escape_debug()
                )));
            }
            self.check_part_text(p)?;
            if !seen.insert(p.id.as_str()) {
                return Err(damaged(format!("part id `{}` appears twice", p.id)));
            }
            if p.triangles > MAX_TRIANGLES_PER_PART {
                return Err(damaged(format!("part {} has too many triangles", p.id)));
            }
            let size = u64::from(p.vertices) * 12 + u64::from(p.triangles) * 12;
            if p.offset != next || p.offset + size > self.blob.len() as u64 {
                return Err(damaged(format!(
                    "part {} has its mesh outside the blob",
                    p.id
                )));
            }
            next = p.offset + size;
            if p.connectors.len() > 8192 {
                return Err(damaged(format!("part {} has too many connectors", p.id)));
            }
            for (lo, hi) in p
                .body
                .min
                .iter()
                .zip(&p.body.max)
                .chain(p.pick.min.iter().zip(&p.pick.max))
            {
                if lo > hi || lo.unsigned_abs() > 10_000_000 || hi.unsigned_abs() > 10_000_000 {
                    return Err(damaged(format!("part {} has a bad box", p.id)));
                }
                // The widest real part is 960 LDU. A box this wide is a hostile or damaged
                // file, and would make the engine's spatial hash do absurd work.
                if i64::from(*hi) - i64::from(*lo) > i64::from(MAX_BOX_EXTENT_LDU) {
                    return Err(damaged(format!(
                        "part {} has a box wider than {MAX_BOX_EXTENT_LDU} LDU (the widest part of the LDraw library is 960)",
                        p.id
                    )));
                }
            }
            if p.studs != footprint_studs(&p.body)
                || i64::from(p.height_ldu) != i64::from(p.body.max[1]) - i64::from(p.body.min[1])
            {
                return Err(damaged(format!(
                    "part {} says it is {} x {} studs and {} LDU high, but its body box is {:?} to {:?}",
                    p.id, p.studs[0], p.studs[1], p.height_ldu, p.body.min, p.body.max
                )));
            }
            if p.body.lattice_cells() > MAX_BOX_CELLS {
                return Err(damaged(format!(
                    "part {} has a body covering {} lattice cells; the limit is {MAX_BOX_CELLS}",
                    p.id,
                    p.body.lattice_cells()
                )));
            }
            for c in &p.connectors {
                let axis = c.dir.iter().map(|v| v.abs()).sum::<i32>() == 1
                    && c.dir.iter().all(|v| v.abs() <= 1);
                if !axis || c.pos.iter().any(|v| v.unsigned_abs() > 10_000_000) {
                    return Err(damaged(format!(
                        "part {} has a connector off the lattice",
                        p.id
                    )));
                }
            }
        }
        if next != self.blob.len() as u64 {
            return Err(damaged("the mesh blob has bytes no part uses"));
        }
        // Every index must name a vertex of its part, and every coordinate be finite.
        for (i, p) in h.parts.iter().enumerate() {
            let m = self
                .mesh(i)
                .ok_or_else(|| damaged("a mesh is out of range"))?;
            if m.positions.iter().flatten().any(|v| !v.is_finite()) {
                return Err(damaged(format!(
                    "part {} has a coordinate that is not a number",
                    p.id
                )));
            }
            if m.indices.iter().any(|&ix| ix >= p.vertices) {
                return Err(damaged(format!(
                    "part {} has an index past its vertices",
                    p.id
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::ConnKind;

    pub(crate) fn sample() -> Pack {
        let part = |id: &str| PackPart {
            id: id.into(),
            name: "Test part".into(),
            category: "Brick".into(),
            author: "Me".into(),
            license: "Licensed under CC BY 4.0".into(),
            connectivity: Connectivity::Derived,
            joints: vec![],
            studs: [1, 1],
            height_ldu: 24,
            body: Aabb {
                min: [-10, 0, -10],
                max: [10, 24, 10],
            },
            pick: Aabb {
                min: [-10, -4, -10],
                max: [10, 24, 10],
            },
            connectors: vec![Conn {
                kind: ConnKind::StudM,
                pos: [0, 0, 0],
                dir: [0, -1, 0],
            }],
            vertices: 0,
            triangles: 0,
            offset: 0,
        };
        let mesh = PartMesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
        };
        let header = PackHeader {
            openbrick: "1.0".into(),
            kind: "pack".into(),
            format: PACK_FORMAT,
            id: "ldraw".into(),
            library: "2026-10".into(),
            resolution: "standard".into(),
            generator: "test".into(),
            attribution: "attr".into(),
            licences: vec![LicenceText {
                name: "x".into(),
                text: "y".into(),
            }],
            counts: PackCounts::default(),
            parts: vec![part("ld.1"), part("ld.2")],
        };
        Pack::build(header, &[mesh.clone(), mesh]).unwrap()
    }

    #[test]
    fn round_trips_and_is_deterministic() {
        let p = sample();
        let a = p.to_bytes().unwrap();
        let b = sample().to_bytes().unwrap();
        assert_eq!(a, b);
        let q = Pack::from_bytes(&a).unwrap();
        assert_eq!(q.header, p.header);
        assert_eq!(q.fingerprint(), p.fingerprint());
        assert_eq!(q.mesh(1).unwrap().indices, vec![0, 1, 2]);
        assert_eq!(q.mesh(0).unwrap().positions[1], [1.0, 0.0, 0.0]);
        assert!(q.mesh(2).is_none());
    }

    /// A pack file whose first part is changed by `edit`, with the checksum fixed up.
    fn forged(edit: impl FnOnce(&mut PackPart)) -> Vec<u8> {
        let mut p = sample();
        edit(&mut p.header.parts[0]);
        p.to_bytes().unwrap()
    }

    #[test]
    fn a_huge_box_is_refused_at_once() {
        let t = std::time::Instant::now();
        let big = 10_000_000;
        let e = Pack::from_bytes(&forged(|p| {
            p.body = Aabb {
                min: [-big, -big, -big],
                max: [big, big, big],
            };
            p.pick = p.body;
        }))
        .unwrap_err();
        assert!(e.to_string().contains("wider than"), "{e}");
        // One axis over the extent limit, the pick box alone, and a body over the cell limit
        // with every axis within the extent limit.
        assert!(Pack::from_bytes(&forged(|p| p.pick.max[0] = p.pick.min[0] + 8193)).is_err());
        let e = Pack::from_bytes(&forged(|p| {
            p.body = Aabb {
                min: [-4000, -8000, -4000],
                max: [4000, 0, 4000],
            };
            p.pick = p.body;
            p.studs = [400, 400];
            p.height_ldu = 8000;
        }))
        .unwrap_err();
        assert!(e.to_string().contains("lattice cells"), "{e}");
        // The widest real part (960 LDU) and the limit itself still load.
        assert!(Pack::from_bytes(&forged(|p| {
            p.body = Aabb {
                min: [-480, -4, -480],
                max: [480, 0, 480],
            };
            p.pick = p.body;
            p.studs = [48, 48];
            p.height_ldu = 4;
        }))
        .is_ok());
        assert!(t.elapsed().as_millis() < 1000);
    }

    /// A pack file assembled by hand from header JSON, a compressed blob and the declared inflated
    /// size, with a correct checksum: what a hostile writer can make.
    fn raw_file(header: &[u8], comp: &[u8], raw_len: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend((header.len() as u32).to_le_bytes());
        out.extend(header);
        out.extend((comp.len() as u32).to_le_bytes());
        out.extend(raw_len.to_le_bytes());
        out.extend(comp);
        let sum = Sha256::digest(&out);
        out.extend(sum);
        out
    }

    fn header_of(p: &Pack) -> serde_json::Value {
        serde_json::to_value(&p.header).unwrap()
    }

    #[test]
    fn text_fields_are_limited_and_free_of_invisible_characters() {
        let refused = |what: &str, bytes: Vec<u8>, needle: &str| {
            let e = Pack::from_bytes(&bytes).err().unwrap_or_else(|| panic!("{what}: accepted"));
            let m = e.to_string();
            assert!(m.contains(needle), "{what}: {m}");
            assert!(m.len() < 600, "{what}: a {} byte message", m.len());
        };
        let long = |n: usize| "x".repeat(n);
        // Lengths: the limit passes, one over is refused.
        for (what, edit, ok_len, bad_len) in [
            ("name", 0, MAX_NAME_CHARS, MAX_NAME_CHARS + 1),
            ("category", 1, MAX_CATEGORY_CHARS, MAX_CATEGORY_CHARS + 1),
            ("author", 2, MAX_CREDIT_CHARS, MAX_CREDIT_CHARS + 1),
            ("licence line", 3, MAX_CREDIT_CHARS, MAX_CREDIT_CHARS + 1),
        ] {
            let set = |n: usize| {
                forged(|p| match edit {
                    0 => p.name = long(n),
                    1 => p.category = long(n),
                    2 => p.author = long(n),
                    _ => p.license = long(n),
                })
            };
            assert!(Pack::from_bytes(&set(ok_len)).is_ok(), "{what} at the limit");
            refused(what, set(bad_len), "longer than");
        }
        // A 2 MB name inside a header that is itself within bounds.
        refused("2 MB name", forged(|p| p.name = long(2 << 20)), "longer than");
        // A header over its bound is refused when written and when read.
        let mut p = sample();
        p.header.parts[0].name = long(20 << 20);
        assert!(p.to_bytes().is_err());
        let json = serde_json::to_vec(&p.header).unwrap();
        refused(
            "20 MB header",
            raw_file(&json, &miniz_oxide::deflate::compress_to_vec(&p.blob, 1), p.blob.len() as u64),
            "header is too large",
        );
        // Invisible characters: a right-to-left override, a zero width space, an escape, a newline.
        for bad in ["a\u{202E}b", "a\u{200B}b", "a\u{1b}[31mb", "a\nb", "a\u{2066}b"] {
            refused("name", forged(|p| p.name = bad.into()), "invisible");
            refused("author", forged(|p| p.author = bad.into()), "invisible");
            refused("licence line", forged(|p| p.license = bad.into()), "invisible");
            refused("category", forged(|p| p.category = bad.into()), "invisible");
        }
        // Ids are plain ASCII.
        for bad in ["ld.\u{202E}1", "ld.\u{430}1", "ld.1\n", "ld.a b", "x.1", "ld.", &format!("ld.{}", long(70))] {
            refused("id", forged(|p| p.id = bad.into()), "not valid");
        }
        // Joints are the four kinds.
        assert!(Pack::from_bytes(&forged(|p| p.joints = vec!["clip".into(), "ball".into()])).is_ok());
        refused("joint", forged(|p| p.joints = vec!["x".into()]), "joint kind");
    }

    #[test]
    fn header_text_is_limited_too() {
        let edit = |f: &dyn Fn(&mut serde_json::Value)| {
            let p = sample();
            let mut h = header_of(&p);
            f(&mut h);
            raw_file(
                &serde_json::to_vec(&h).unwrap(),
                &miniz_oxide::deflate::compress_to_vec(&p.blob, 6),
                p.blob.len() as u64,
            )
        };
        let refused = |bytes: Vec<u8>, needle: &str| {
            let m = Pack::from_bytes(&bytes).unwrap_err().to_string();
            assert!(m.contains(needle) && m.len() < 600, "{m}");
        };
        assert!(Pack::from_bytes(&edit(&|_| {})).is_ok());
        refused(edit(&|h| h["attribution"] = ("a".repeat(70_000)).into()), "attribution");
        refused(edit(&|h| h["attribution"] = "a\u{202E}b".into()), "attribution");
        refused(edit(&|h| h["library"] = "a".repeat(33).into()), "library version");
        refused(edit(&|h| h["library"] = "20\u{202E}26".into()), "library version");
        refused(edit(&|h| h["generator"] = "a\u{1b}[2J".into()), "generator");
        refused(edit(&|h| h["id"] = "LDRAW".into()), "pack id");
        refused(edit(&|h| h["id"] = "ld/../x".into()), "pack id");
        refused(edit(&|h| h["id"] = "".into()), "pack id");
        refused(edit(&|h| h["resolution"] = "ultra".into()), "resolution");
        // Licences: a text with a line break and a tab is fine, an override is not, and the total is bounded.
        assert!(Pack::from_bytes(&edit(&|h| h["licences"][0]["text"] = "a\n\tb\r\n".into())).is_ok());
        refused(edit(&|h| h["licences"][0]["text"] = "a\u{202E}b".into()), "licence text");
        refused(edit(&|h| h["licences"][0]["name"] = "a\nb".into()), "licence name");
        let big = "a".repeat(600 * 1024);
        refused(
            edit(&|h| h["licences"] = serde_json::json!([{"name":"a","text":big},{"name":"b","text":big}])),
            "licence texts",
        );
        refused(
            edit(&|h| h["licences"] = serde_json::json!(vec![serde_json::json!({"name":"a","text":"b"}); 17])),
            "licence texts",
        );
        // The footprint and height must be the body box's.
        refused(forged(|p| p.studs = [2, 1]), "studs");
        refused(forged(|p| p.height_ldu = 25), "studs");
        refused(forged(|p| p.studs = [0, 1]), "studs");
    }

    #[test]
    fn a_blob_that_inflates_beyond_what_the_header_declares_is_refused_without_inflating() {
        let p = sample();
        let json = serde_json::to_vec(&p.header).unwrap();
        // Declares 500 MB: refused before anything is inflated, however small the stream.
        let zeros = vec![0u8; 8 << 20];
        let comp = miniz_oxide::deflate::compress_to_vec(&zeros, 1);
        let t = std::time::Instant::now();
        let e = Pack::from_bytes(&raw_file(&json, &comp, 500 << 20)).unwrap_err();
        assert!(e.to_string().contains("too large"), "{e}");
        // Declares the right size (what the two parts need) but the stream inflates to 8 MiB.
        let e = Pack::from_bytes(&raw_file(&json, &comp, p.blob.len() as u64)).unwrap_err();
        assert!(e.to_string().contains("does not inflate"), "{e}");
        // Declares a size the parts do not need.
        let e = Pack::from_bytes(&raw_file(&json, &comp, 1 << 20)).unwrap_err();
        assert!(e.to_string().contains("parts need"), "{e}");
        assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
    }

    #[test]
    fn error_messages_are_short_whatever_the_file_says() {
        // serde repeats the value it did not expect: a 2 MB string where a number goes.
        let p = sample();
        let mut h = header_of(&p);
        h["format"] = "x".repeat(2 << 20).into();
        let bytes = raw_file(
            &serde_json::to_vec(&h).unwrap(),
            &miniz_oxide::deflate::compress_to_vec(&p.blob, 6),
            p.blob.len() as u64,
        );
        let m = Pack::from_bytes(&bytes).unwrap_err().to_string();
        assert!(m.len() < 500, "{} bytes: {}", m.len(), &m[..80]);
        // A stream that stops half way inflates to a big partial output, which must not be in the message.
        let mut p = sample();
        p.header.parts.truncate(1);
        let mut h = header_of(&p);
        h["parts"][0]["vertices"] = 700_000.into();
        h["parts"][0]["triangles"] = 0.into();
        let need = 700_000u64 * 12;
        let mut comp = miniz_oxide::deflate::compress_to_vec(&vec![0u8; need as usize], 1);
        let half = comp.len() / 2;
        comp.truncate(half);
        let m = Pack::from_bytes(&raw_file(&serde_json::to_vec(&h).unwrap(), &comp, need))
            .unwrap_err()
            .to_string();
        assert!(m.contains("does not inflate") && m.len() < 300, "{} bytes: {m}", m.len());
    }

    #[test]
    fn damage_is_an_error_not_a_panic() {
        let bytes = sample().to_bytes().unwrap();
        assert_eq!(Pack::from_bytes(b"nope").unwrap_err(), PackError::NotAPack);
        assert!(Pack::from_bytes(&bytes[..bytes.len() - 5]).is_err());
        let mut flipped = bytes.clone();
        flipped[40] ^= 1;
        assert_eq!(Pack::from_bytes(&flipped).unwrap_err(), PackError::Checksum);
        // Every truncation and a sweep of single-byte changes (with the checksum fixed up).
        for n in 0..bytes.len().min(400) {
            let _ = Pack::from_bytes(&bytes[..n]);
        }
        for i in (0..bytes.len() - 32).step_by(3) {
            let mut b = bytes.clone();
            b[i] = b[i].wrapping_add(0x55);
            let sum = Sha256::digest(&b[..b.len() - 32]);
            let n = b.len();
            b[n - 32..].copy_from_slice(&sum);
            let _ = Pack::from_bytes(&b);
        }
    }
}
