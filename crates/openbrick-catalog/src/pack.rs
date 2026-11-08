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
//! [`MAX_PARTS`], [`MAX_BLOB_BYTES`]) and checks the hash, the header and every
//! mesh range and index before it hands out anything, so a damaged or hostile
//! file is an error and never a panic.

use sha2::{Digest, Sha256};

use crate::builtin::PartMesh;
use crate::shape::{Aabb, Conn};

/// The pack format this build writes and reads.
pub const PACK_FORMAT: u32 = 1;
/// The first bytes of every pack.
pub const MAGIC: &[u8; 8] = b"OBPACK1\n";
/// The prefix of the part ids of an LDraw pack: `ld.3001`.
pub const LDRAW_ID_PREFIX: &str = "ld.";
/// The largest pack file read, in bytes.
pub const MAX_PACK_BYTES: usize = 256 * 1024 * 1024;
/// The largest header, in bytes.
pub const MAX_HEADER_BYTES: usize = 32 * 1024 * 1024;
/// The most parts in one pack.
pub const MAX_PARTS: usize = 20_000;
/// The largest mesh blob after inflating, in bytes.
pub const MAX_BLOB_BYTES: usize = 512 * 1024 * 1024;
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

fn damaged(m: impl Into<String>) -> PackError {
    PackError::Damaged(m.into())
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
        let comp = body
            .get(at + 12..at + 12 + clen)
            .ok_or_else(|| damaged("the mesh blob is cut short"))?;
        if at + 12 + clen != body.len() {
            return Err(damaged("there are extra bytes after the mesh blob"));
        }
        let blob = miniz_oxide::inflate::decompress_to_vec_with_limit(comp, MAX_BLOB_BYTES)
            .map_err(|e| damaged(format!("the mesh blob does not inflate: {e:?}")))?;
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

    /// The structural checks of a pack read from a file.
    fn check(&self) -> Result<(), PackError> {
        let h = &self.header;
        if h.kind != "pack" || h.openbrick.split('.').next() != Some("1") {
            return Err(damaged("the header is not a pack header of version 1"));
        }
        if h.parts.len() > MAX_PARTS {
            return Err(damaged("too many parts"));
        }
        let mut seen = std::collections::HashSet::new();
        let mut next = 0u64;
        for p in &h.parts {
            if !p.id.starts_with(LDRAW_ID_PREFIX)
                || p.id.len() > 64
                || p.id.chars().any(char::is_control)
            {
                return Err(damaged(format!(
                    "part id `{}` is not valid",
                    p.id.escape_debug()
                )));
            }
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
