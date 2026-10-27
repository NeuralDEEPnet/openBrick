//! The 24 lattice-preserving rotations of the cube, stored as a `u8` index with
//! precomputed matrices and a 24 x 24 composition table.

use std::sync::OnceLock;

/// One of the 24 rotations that map the integer lattice onto itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Rot24(pub u8);

type Mat = [[i32; 3]; 3];

struct Tables {
    mats: [Mat; 24],
    compose: [[u8; 24]; 24],
    inverse: [u8; 24],
}

fn mat_mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [[0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn det(m: &Mat) -> i32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        // Enumerate signed permutation matrices in a deterministic order and keep det = +1.
        // Index 0 is the identity.
        let perms: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let mut mats: Vec<Mat> = Vec::with_capacity(24);
        for p in perms {
            for signs in 0..8u8 {
                let s = [
                    if signs & 1 == 0 { 1 } else { -1 },
                    if signs & 2 == 0 { 1 } else { -1 },
                    if signs & 4 == 0 { 1 } else { -1 },
                ];
                let mut m = [[0; 3]; 3];
                for r in 0..3 {
                    m[r][p[r]] = s[r];
                }
                if det(&m) == 1 {
                    mats.push(m);
                }
            }
        }
        assert_eq!(mats.len(), 24);
        let mats: [Mat; 24] = mats.try_into().expect("24 rotations");
        let index_of =
            |m: &Mat| -> u8 { mats.iter().position(|x| x == m).expect("closed group") as u8 };
        let mut compose = [[0u8; 24]; 24];
        let mut inverse = [0u8; 24];
        let identity: Mat = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
        for (i, a) in mats.iter().enumerate() {
            for (j, b) in mats.iter().enumerate() {
                let c = mat_mul(a, b);
                compose[i][j] = index_of(&c);
                if c == identity {
                    inverse[i] = j as u8;
                }
            }
        }
        Tables {
            mats,
            compose,
            inverse,
        }
    })
}

impl Rot24 {
    pub const IDENTITY: Rot24 = Rot24(0);

    /// All 24 rotations in index order.
    pub fn all() -> impl Iterator<Item = Rot24> {
        (0u8..24).map(Rot24)
    }

    /// The integer 3x3 matrix of this rotation (row-major).
    pub fn matrix(self) -> [[i32; 3]; 3] {
        tables().mats[self.0 as usize]
    }

    /// Compose: `self` applied after `other`.
    pub fn compose(self, other: Rot24) -> Rot24 {
        Rot24(tables().compose[self.0 as usize][other.0 as usize])
    }

    /// The inverse rotation.
    pub fn inverse(self) -> Rot24 {
        Rot24(tables().inverse[self.0 as usize])
    }

    /// Rotate an integer vector.
    pub fn apply(self, v: [i32; 3]) -> [i32; 3] {
        let m = self.matrix();
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// Find the rotation with this exact matrix, if it is one of the 24.
    pub fn from_matrix(m: [[i32; 3]; 3]) -> Option<Rot24> {
        tables()
            .mats
            .iter()
            .position(|x| *x == m)
            .map(|i| Rot24(i as u8))
    }

    /// Quarter turns about the vertical (Y) axis, the common assembler rotation.
    pub fn yaw_quarter_turns(n: u8) -> Rot24 {
        // Rotation about Y by 90 degrees in a right-handed frame: x -> -z, z -> x.
        let q: [[i32; 3]; 3] = [[0, 0, 1], [0, 1, 0], [-1, 0, 0]];
        let one = Rot24::from_matrix(q).expect("quarter turn is a lattice rotation");
        (0..n % 4).fold(Rot24::IDENTITY, |acc, _| one.compose(acc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_zero_and_group_is_closed() {
        assert_eq!(Rot24::IDENTITY.matrix(), [[1, 0, 0], [0, 1, 0], [0, 0, 1]]);
        for a in Rot24::all() {
            assert_eq!(a.compose(a.inverse()), Rot24::IDENTITY);
            assert_eq!(a.compose(Rot24::IDENTITY), a);
            for b in Rot24::all() {
                let c = a.compose(b);
                assert_eq!(c.matrix(), mat_mul(&a.matrix(), &b.matrix()));
                assert_eq!(det(&c.matrix()), 1);
            }
        }
    }

    #[test]
    fn four_yaw_turns_are_identity() {
        assert_eq!(Rot24::yaw_quarter_turns(4), Rot24::IDENTITY);
        assert_ne!(Rot24::yaw_quarter_turns(1), Rot24::IDENTITY);
        assert_eq!(Rot24::yaw_quarter_turns(1).apply([20, 0, 0]), [0, 0, -20]);
    }
}
