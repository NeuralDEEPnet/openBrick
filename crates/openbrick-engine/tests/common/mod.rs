//! Shared helpers for the engine tests: the built-in catalog, a seeded
//! random generator (SplitMix64, so failures name a seed to rerun) and rays.

#![allow(dead_code)]

use openbrick_catalog::{Catalog, PartId};
use openbrick_engine::{Command, Model, Ray};

pub fn catalog() -> &'static Catalog {
    Catalog::builtin().expect("the built-in catalog builds")
}

pub fn part(id: &str) -> PartId {
    catalog().find(id).expect("built-in part")
}

/// SplitMix64 (Steele, Lea and Flood 2014): small, fast, reproducible.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next() as usize % items.len()]
    }
}

/// A ray from above, at a slant, towards an integer point near the origin.
pub fn ray_towards(rng: &mut Rng, span: i32) -> Ray {
    let target = [
        rng.range(-span, span),
        rng.range(-120, 0),
        rng.range(-span, span),
    ];
    let origin = [
        target[0] + rng.range(-200, 200),
        -400,
        target[2] + rng.range(-200, 200),
    ];
    Ray {
        origin: origin.map(f64::from),
        dir: [
            f64::from(target[0] - origin[0]),
            f64::from(target[1] - origin[1]),
            f64::from(target[2] - origin[2]),
        ],
    }
}

/// A straight-down ray onto the point (x, z).
pub fn down(x: i32, z: i32) -> Ray {
    Ray {
        origin: [f64::from(x), -1000.0, f64::from(z)],
        dir: [0.0, 1.0, 0.0],
    }
}

/// A model with a baseplate at the origin, its top at Y = -8.
pub fn on_baseplate() -> Model<'static> {
    let mut m = Model::new(catalog());
    m.apply(&Command::Add {
        part: "ob.baseplate-16x16".into(),
        pos: [0, -8, 0],
        rot: 0,
        colour: 2,
    })
    .expect("the baseplate goes on the ground");
    m
}

/// The yaw rotations the assembler offers.
pub fn yaws() -> [u8; 4] {
    [0, 1, 2, 3].map(|n| openbrick_units::Rot24::yaw_quarter_turns(n).0)
}

/// Snap `part` along `ray` and add it when valid; the new id, if placed.
pub fn place(m: &mut Model, part: &str, rot: u8, ray: &Ray, colour: u16) -> Option<u32> {
    let p = m.catalog().find(part).unwrap();
    let s = m.snap(p, openbrick_units::Rot24(rot), ray).expect("snap")?;
    if !s.valid {
        return None;
    }
    Some(
        m.apply(&Command::Add {
            part: part.into(),
            pos: s.pos,
            rot: s.rot,
            colour,
        })
        .expect("a valid snap applies")
        .expect("add returns the id"),
    )
}
