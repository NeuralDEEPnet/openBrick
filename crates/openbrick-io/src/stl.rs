//! Binary STL writer (docs/plan/08 section 5): millimetres, Z up, no colour.
//!
//! Layout, all little-endian: an 80-byte header, a `u32` triangle count, then 50
//! bytes per triangle (facet normal and three vertices as `f32` x, y, z, and a
//! zero `u16` attribute). Triangles keep the mesh winding, counter-clockwise seen
//! from outside, and the facet normal is the unit normal of that winding
//! (computed in `f64`), or zero for a degenerate triangle. The header never starts
//! with `solid`, so readers that sniff for ASCII STL are not misled.

use crate::export::check_mesh;
use crate::{ExportObject, IoError};

/// The fixed 80-byte header, padded with spaces.
pub const STL_HEADER: [u8; 80] = {
    let text = b"OpenBrick3D binary STL; units: millimetre; up: +Z";
    let mut header = [b' '; 80];
    let mut i = 0;
    while i < text.len() {
        header[i] = text[i];
        i += 1;
    }
    header
};

/// Size in bytes of a binary STL with `triangles` triangles (for counts whose
/// file fits in memory; it does not check for overflow).
pub fn stl_binary_len(triangles: usize) -> usize {
    84 + 50 * triangles
}

/// Unit normal of the triangle (a, b, c) following its winding, or zero when the
/// triangle has no area.
fn facet_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let d = |p: [f32; 3], q: [f32; 3]| {
        [
            f64::from(q[0]) - f64::from(p[0]),
            f64::from(q[1]) - f64::from(p[1]),
            f64::from(q[2]) - f64::from(p[2]),
        ]
    };
    let (u, v) = (d(a, b), d(a, c));
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 && len.is_finite() {
        n.map(|x| (x / len) as f32)
    } else {
        [0.0; 3]
    }
}

/// All objects merged into one binary STL, in input order. Fails on an empty
/// object list or a structurally broken mesh (see [`IoError`]); unlike 3MF, STL
/// does not require closed meshes, so open ones are written as they are.
///
/// It returns a `Result` rather than bare bytes so that bad input (no objects,
/// an index out of range, a NaN) is an error, never a panic in WebAssembly.
///
/// The file has [`stl_binary_len`] bytes. At the import triangle cap
/// [`MESH_MAX_TRIANGLES`](crate::MESH_MAX_TRIANGLES) that is about 100 MB, twice
/// the import size cap [`MESH_MAX_BYTES`](crate::MESH_MAX_BYTES): until the two
/// caps are reconciled (docs/plan/08 section 7), a binary STL above about one
/// million triangles cannot be imported back.
pub fn write_stl_binary(objects: &[ExportObject<'_>]) -> Result<Vec<u8>, IoError> {
    if objects.is_empty() {
        return Err(IoError::NoObjects);
    }
    for o in objects {
        check_mesh(&o.name, o.mesh)?;
    }
    let triangles: usize = objects.iter().map(|o| o.mesh.triangle_count()).sum();
    let too_large = || IoError::TooLarge {
        what: "STL triangle count",
        size: triangles as u64,
    };
    let count = u32::try_from(triangles).map_err(|_| too_large())?;
    // Checked, because usize is 32 bits on wasm32.
    let size = triangles
        .checked_mul(50)
        .and_then(|n| n.checked_add(84))
        .ok_or_else(too_large)?;
    let mut out = Vec::new();
    out.try_reserve_exact(size).map_err(|_| too_large())?;
    out.extend_from_slice(&STL_HEADER);
    out.extend_from_slice(&count.to_le_bytes());
    for o in objects {
        let p = &o.mesh.positions;
        let (tris, _) = o.mesh.indices.as_chunks::<3>();
        for &[i, j, k] in tris {
            let (a, b, c) = (p[i as usize], p[j as usize], p[k as usize]);
            for v in [facet_normal(a, b, c), a, b, c] {
                for x in v {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
            out.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    debug_assert_eq!(out.len(), size);
    Ok(out)
}

/// One binary STL per role, for single-material printers that take one file per
/// colour (docs/plan/08 section 5). Roles appear in order of first appearance;
/// each file merges that role's objects in input order.
pub fn write_stl_binary_per_role(
    objects: &[ExportObject<'_>],
) -> Result<Vec<(String, Vec<u8>)>, IoError> {
    if objects.is_empty() {
        return Err(IoError::NoObjects);
    }
    let mut roles: Vec<&str> = Vec::new();
    for o in objects {
        if !roles.contains(&o.role.as_str()) {
            roles.push(&o.role);
        }
    }
    roles
        .into_iter()
        .map(|role| {
            let group: Vec<ExportObject<'_>> =
                objects.iter().filter(|o| o.role == role).cloned().collect();
            Ok((role.to_string(), write_stl_binary(&group)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbrick_geom::Mesh;

    /// Outward tetrahedron, shifted along x. Abstract test geometry in millimetres.
    fn tetra(dx: f32) -> Mesh {
        Mesh {
            positions: vec![
                [dx, 0.0, 0.0],
                [dx + 1.0, 0.0, 0.0],
                [dx, 1.0, 0.0],
                [dx, 0.0, 1.0],
            ],
            indices: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2],
        }
    }

    #[test]
    fn header_count_and_length() {
        let (a, b) = (tetra(0.0), tetra(2.0));
        let objects = [
            ExportObject::new("a", "body", None, &a),
            ExportObject::new("b", "accent", None, &b),
        ];
        let bytes = write_stl_binary(&objects).unwrap();
        assert_eq!(bytes.len(), stl_binary_len(8));
        assert_eq!(bytes.len(), 84 + 8 * 50);
        assert_eq!(&bytes[..80], &STL_HEADER);
        assert!(!bytes.starts_with(b"solid"));
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 8);
        assert_eq!(write_stl_binary(&objects).unwrap(), bytes, "deterministic");
    }

    /// Read back with `stl_io`, an independent reader.
    #[test]
    fn an_independent_reader_gets_the_triangles_back() {
        let (a, b) = (tetra(0.0), tetra(2.0));
        let objects = [
            ExportObject::new("a", "body", None, &a),
            ExportObject::new("b", "accent", None, &b),
        ];
        let bytes = write_stl_binary(&objects).unwrap();
        let tris = stl_io::create_stl_reader(&mut std::io::Cursor::new(&bytes))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(tris.len(), 8);
        let expected: Vec<[[f32; 3]; 3]> = [&a, &b]
            .iter()
            .flat_map(|m| {
                let (triangles, _) = m.indices.as_chunks::<3>();
                triangles.iter().map(|t| t.map(|i| m.positions[i as usize]))
            })
            .collect();
        let got: Vec<[[f32; 3]; 3]> = tris
            .iter()
            .map(|t| t.vertices.map(|v| [v[0], v[1], v[2]]))
            .collect();
        assert_eq!(got, expected);
        // Facet normals are the outward unit normals: the base triangle (0, 2, 1)
        // of the first tetrahedron lies in z = 0 and faces down.
        let n = tris[0].normal;
        assert_eq!([n[0], n[1], n[2]], [0.0, 0.0, -1.0]);
        // The slanted face (1, 2, 3) has normal (1, 1, 1) / sqrt(3).
        let s = (1.0 / 3f64.sqrt()) as f32;
        let n = tris[2].normal;
        assert_eq!([n[0], n[1], n[2]], [s, s, s]);
    }

    #[test]
    fn degenerate_triangles_get_a_zero_normal() {
        assert_eq!(
            facet_normal([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
            [0.0; 3]
        );
    }

    #[test]
    fn one_file_per_role_in_order_of_first_appearance() {
        let (a, b, c) = (tetra(0.0), tetra(2.0), tetra(4.0));
        let objects = [
            ExportObject::new("a", "accent", None, &a),
            ExportObject::new("b", "body", None, &b),
            ExportObject::new("c", "accent", None, &c),
        ];
        let files = write_stl_binary_per_role(&objects).unwrap();
        let roles: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(roles, ["accent", "body"]);
        assert_eq!(files[0].1.len(), stl_binary_len(8));
        assert_eq!(files[1].1.len(), stl_binary_len(4));
        assert_eq!(
            files[0].1,
            write_stl_binary(&[objects[0].clone(), objects[2].clone()]).unwrap()
        );
    }

    #[test]
    fn broken_input_is_refused() {
        assert_eq!(write_stl_binary(&[]), Err(IoError::NoObjects));
        assert_eq!(write_stl_binary_per_role(&[]), Err(IoError::NoObjects));
        let mut m = tetra(0.0);
        m.indices[0] = 7;
        let objects = [ExportObject::new("bad", "body", None, &m)];
        assert!(matches!(
            write_stl_binary(&objects),
            Err(IoError::IndexOutOfRange { index: 7, .. })
        ));
        // Open meshes are fine for STL.
        let mut open = tetra(0.0);
        open.indices.truncate(9);
        let objects = [ExportObject::new("open", "body", None, &open)];
        assert_eq!(write_stl_binary(&objects).unwrap().len(), stl_binary_len(3));
    }
}
