//! Inputs shared by the 3MF and STL writers, the error type and the mesh checks
//! every writer runs before it emits a byte.

use openbrick_geom::Mesh;

/// Writer name and version, the default for [`ExportMeta::generator`] when the
/// caller has nothing more specific (the CLI passes its own crate name instead).
pub const GENERATOR: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));

/// The colour roles of the part designer (docs/plan/05 section 7). The writers
/// accept any non-empty role so the assembler can add its own groups; these are
/// the ones the part designer emits.
pub const ROLES: [&str; 3] = ["body", "accent", "serial"];

/// One solid to export: a mesh in millimetres, Z up, in the frame shared by every
/// object of the same export (the writers apply no transform). The evaluator
/// guarantees that objects of one export do not overlap, as the 3MF specification
/// asks of producers; the writers do not test it.
#[derive(Clone, Debug)]
pub struct ExportObject<'a> {
    /// Object name shown by slicers, for example "body" or "ndn.logo-tile body".
    pub name: String,
    /// Colour role: "body", "accent" or "serial" (see [`ROLES`]).
    pub role: String,
    /// Display colour as sRGB plus alpha, `[r, g, b, a]`. Every object of one role
    /// must carry the same colour (or all `None`); 3MF writes one colour per role.
    pub color_srgb: Option<[u8; 4]>,
    pub mesh: &'a Mesh,
}

impl<'a> ExportObject<'a> {
    pub fn new(
        name: impl Into<String>,
        role: impl Into<String>,
        color_srgb: Option<[u8; 4]>,
        mesh: &'a Mesh,
    ) -> ExportObject<'a> {
        ExportObject {
            name: name.into(),
            role: role.into(),
            color_srgb,
            mesh,
        }
    }
}

/// Traceability metadata embedded in every 3MF (docs/plan/05 sections 3 and 7), so
/// a printed part can be traced to its document, fit profile and generator.
///
/// [`write_3mf`](crate::write_3mf) requires every field except `title` to hold
/// more than white space (`serial` only when it is `Some`); it returns
/// [`IoError::MissingMeta`] otherwise. `Default` is therefore not a valid export
/// on its own; it exists for building values field by field.
///
/// The 3MF UUIDs are derived from all of these fields and from the objects (see
/// [`threemf::package_uuids`](crate::threemf::package_uuids)).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportMeta {
    /// Part id, for example "ndn.logo-tile".
    pub part_id: String,
    /// Part document version (semver).
    pub version: String,
    /// Content hash of the normalised part document and parameters
    /// (docs/plan/06 section 7).
    pub content_hash: String,
    /// Id of the fit profile whose offsets are baked into the geometry.
    pub fit_profile_id: String,
    /// Id of the process profile the part was validated against.
    pub process_id: String,
    /// Crate name and version of the program that wrote the file; see [`GENERATOR`].
    pub generator: String,
    /// Kernel id, for example "manifold-rust 0.19.1" (`Kernel::id`).
    pub kernel_id: String,
    /// Human title; written as the 3MF `Title` when non-empty.
    pub title: String,
    /// Serial number of this copy, when the part carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
}

impl ExportMeta {
    /// The fields with the names used in error messages, in a fixed order.
    pub(crate) fn fields(&self) -> [(&'static str, Option<&str>); 9] {
        [
            ("part_id", Some(self.part_id.as_str())),
            ("version", Some(self.version.as_str())),
            ("content_hash", Some(self.content_hash.as_str())),
            ("fit_profile_id", Some(self.fit_profile_id.as_str())),
            ("process_id", Some(self.process_id.as_str())),
            ("generator", Some(self.generator.as_str())),
            ("kernel_id", Some(self.kernel_id.as_str())),
            ("title", Some(self.title.as_str())),
            ("serial", self.serial.as_deref()),
        ]
    }
}

/// Why an edge breaks the closed-mesh rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeProblem {
    /// No triangle runs along the edge in the opposite direction: a hole.
    Open,
    /// Two triangles run along the edge in the same direction: a flipped triangle,
    /// or more than two triangles meet at the edge.
    Repeated,
}

impl std::fmt::Display for EdgeProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EdgeProblem::Open => "has no triangle running the other way (the mesh is open there)",
            EdgeProblem::Repeated => {
                "is used twice in the same direction (a flipped triangle, or more than two triangles share the edge)"
            }
        })
    }
}

/// Errors from the writers. Each message names the object or field and says what
/// to change; nothing is written when one is returned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IoError {
    #[error("nothing to export: the object list is empty; pass at least one object")]
    NoObjects,
    #[error("object {object:?} has no triangles; every exported object needs at least one")]
    EmptyMesh { object: String },
    #[error("object {object:?} has {len} triangle indices, which is not a multiple of 3; the mesh is truncated")]
    IndexCount { object: String, len: usize },
    #[error("object {object:?}: triangle {triangle} uses vertex {index}, but the mesh has only {vertices} vertices")]
    IndexOutOfRange {
        object: String,
        triangle: usize,
        index: u32,
        vertices: usize,
    },
    #[error("object {object:?}: vertex {vertex} has a coordinate that is not a finite number")]
    NonFiniteVertex { object: String, vertex: usize },
    #[error("object {object:?}: triangle {triangle} uses the same vertex twice; 3MF needs three distinct vertices per triangle")]
    DegenerateTriangle { object: String, triangle: usize },
    #[error("object {object:?} is not a closed, consistently oriented mesh: edge {from} -> {to} {problem}. 3MF model objects must be watertight; the MESH rules of the validator (docs/plan/05 section 5) find the cause")]
    NotClosed {
        object: String,
        from: u32,
        to: u32,
        problem: EdgeProblem,
    },
    #[error("object {object:?} has an empty role; use one of body, accent or serial")]
    EmptyRole { object: String },
    #[error("objects with role {role:?} carry different colours; 3MF writes one colour per role, so give them the same colour or different roles")]
    RoleColourConflict { role: String },
    #[error("{field} contains the character U+{code:04X}, which XML 1.0 cannot carry; remove it")]
    InvalidText { field: String, code: u32 },
    #[error("{field} is empty; every 3MF carries it as openbrick traceability metadata so a printed part can be traced back (docs/plan/05 sections 3 and 7). Pass the real value; a part without a serial number uses serial: None, not an empty string")]
    MissingMeta { field: String },
    #[error("{what} is too large for this writer ({size} bytes or entries); split the export into several files")]
    TooLarge { what: &'static str, size: u64 },
}

/// Structural checks shared by every writer: whole triangles, indices in range,
/// finite coordinates and at least one triangle.
pub(crate) fn check_mesh(object: &str, mesh: &Mesh) -> Result<(), IoError> {
    let len = mesh.indices.len();
    if !len.is_multiple_of(3) {
        return Err(IoError::IndexCount {
            object: object.to_string(),
            len,
        });
    }
    if len == 0 {
        return Err(IoError::EmptyMesh {
            object: object.to_string(),
        });
    }
    let vertices = mesh.positions.len();
    for (i, &index) in mesh.indices.iter().enumerate() {
        if index as usize >= vertices {
            return Err(IoError::IndexOutOfRange {
                object: object.to_string(),
                triangle: i / 3,
                index,
                vertices,
            });
        }
    }
    if let Some(vertex) = mesh
        .positions
        .iter()
        .position(|p| p.iter().any(|c| !c.is_finite()))
    {
        return Err(IoError::NonFiniteVertex {
            object: object.to_string(),
            vertex,
        });
    }
    Ok(())
}

/// The 3MF rule for objects of type "model" (3MF Core specification, chapter 4):
/// three distinct vertices per triangle, and every edge shared by exactly two
/// triangles that run along it in opposite directions, so the mesh is closed and
/// consistently oriented. Expects [`check_mesh`] to have passed.
pub(crate) fn check_closed(object: &str, mesh: &Mesh) -> Result<(), IoError> {
    let mut edges: Vec<(u32, u32)> = Vec::with_capacity(mesh.indices.len());
    let (triangles, _) = mesh.indices.as_chunks::<3>();
    for (triangle, &[a, b, c]) in triangles.iter().enumerate() {
        if a == b || b == c || a == c {
            return Err(IoError::DegenerateTriangle {
                object: object.to_string(),
                triangle,
            });
        }
        edges.extend([(a, b), (b, c), (c, a)]);
    }
    edges.sort_unstable();
    let fail = |(from, to): (u32, u32), problem| IoError::NotClosed {
        object: object.to_string(),
        from,
        to,
        problem,
    };
    if let Some(pair) = edges.windows(2).find(|w| w[0] == w[1]) {
        return Err(fail(pair[0], EdgeProblem::Repeated));
    }
    // Every directed edge is now unique; it needs exactly one partner running the
    // other way, and that partner is unique too.
    if let Some(&edge) = edges
        .iter()
        .find(|&&(a, b)| edges.binary_search(&(b, a)).is_err())
    {
        return Err(fail(edge, EdgeProblem::Open));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tetrahedron with outward (counter-clockwise from outside) triangles.
    /// Abstract test geometry, not a brick dimension.
    fn tetra() -> Mesh {
        Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            indices: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2],
        }
    }

    #[test]
    fn closed_tetrahedron_passes() {
        let t = tetra();
        assert_eq!(check_mesh("t", &t), Ok(()));
        assert_eq!(check_closed("t", &t), Ok(()));
    }

    #[test]
    fn structural_failures_are_reported() {
        let mut m = tetra();
        m.indices.pop();
        assert_eq!(
            check_mesh("t", &m),
            Err(IoError::IndexCount {
                object: "t".into(),
                len: 11
            })
        );

        let mut m = tetra();
        m.indices[4] = 9;
        assert_eq!(
            check_mesh("t", &m),
            Err(IoError::IndexOutOfRange {
                object: "t".into(),
                triangle: 1,
                index: 9,
                vertices: 4
            })
        );

        let mut m = tetra();
        m.positions[2][1] = f32::NAN;
        assert_eq!(
            check_mesh("t", &m),
            Err(IoError::NonFiniteVertex {
                object: "t".into(),
                vertex: 2
            })
        );

        let empty = Mesh::default();
        assert_eq!(
            check_mesh("e", &empty),
            Err(IoError::EmptyMesh { object: "e".into() })
        );
    }

    #[test]
    fn open_flipped_and_degenerate_meshes_fail_the_closed_check() {
        // Drop the last triangle (0, 3, 2): its three edges lose their partners.
        let mut open = tetra();
        open.indices.truncate(9);
        let err = check_closed("t", &open).unwrap_err();
        assert!(
            matches!(
                err,
                IoError::NotClosed {
                    problem: EdgeProblem::Open,
                    ..
                }
            ),
            "{err}"
        );

        // Flip the first triangle: (0, 1) then appears twice in one direction.
        let mut flipped = tetra();
        flipped.indices.swap(1, 2);
        let err = check_closed("t", &flipped).unwrap_err();
        assert!(
            matches!(
                err,
                IoError::NotClosed {
                    problem: EdgeProblem::Repeated,
                    ..
                }
            ),
            "{err}"
        );

        let mut degenerate = tetra();
        degenerate.indices[5] = 0;
        assert_eq!(
            check_closed("t", &degenerate),
            Err(IoError::DegenerateTriangle {
                object: "t".into(),
                triangle: 1
            })
        );
    }

    #[test]
    fn meta_round_trips_through_json_with_camel_case_names() {
        let meta = ExportMeta {
            part_id: "ndn.logo-tile".into(),
            serial: Some("A-0001".into()),
            ..ExportMeta::default()
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"partId\":\"ndn.logo-tile\""), "{json}");
        assert!(json.contains("\"fitProfileId\""), "{json}");
        let back: ExportMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back, meta);
    }
}
