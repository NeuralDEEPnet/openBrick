//! Native timings of the engine on a large model (bench/scale/README.md): builds a `city` of N
//! bricks like bench/scale/layouts.mjs does and times open, save, stats, pick, snap, add and remove.
//!
//!   cargo run --release -p openbrick-engine --example scale -- 100000

use std::time::Instant;

use openbrick_catalog::Catalog;
use openbrick_engine::{Command, Model, Ray};
use openbrick_units::Rot24;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let catalog = Catalog::builtin().expect("builtin catalog");
    // 16 x 16 baseplates, 32 columns of 2x4 bricks each, 12 layers a baseplate.
    let mut instances = Vec::new();
    let mut id = 1u32;
    let per_plate = 32 * 12;
    let plates = n.div_ceil(per_plate);
    let side = (plates as f64).sqrt().ceil() as i32;
    let mut placed = 0;
    let line = |id: u32, part: &str, x: i32, y: i32, z: i32, colour: u16| {
        format!(
            r#"{{"id":{id},"part":"{part}","pose":{{"pos":[{x},{y},{z}],"rot":0}},"colour":{colour}}}"#
        )
    };
    for b in 0..plates as i32 {
        let (ox, oz) = ((b % side) * 320, (b / side) * 320);
        instances.push(line(id, "ob.baseplate-16x16", ox, -8, oz, 2));
        id += 1;
        for x in [-120, -40, 40, 120] {
            for r in 0..8 {
                for layer in 0..12 {
                    if placed >= n {
                        break;
                    }
                    instances.push(line(
                        id,
                        "ob.brick-2x4",
                        ox + x,
                        -8 - 24 * (layer + 1),
                        oz - 140 + 40 * r,
                        4,
                    ));
                    id += 1;
                    placed += 1;
                }
            }
        }
    }
    let text = format!(
        r#"{{"openbrick":"1.0","kind":"model","name":"x","units":"ldu","frame":"ldraw-y-down","catalog":"openbrick-builtin@1","palette":[],"submodels":[{{"id":"main","instances":[{}]}}],"bom":[]}}"#,
        instances.join(",")
    );
    let time = |what: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        println!("{what:>28}: {:8.2} ms", t.elapsed().as_secs_f64() * 1000.0);
    };
    time("  of which JSON parse", &mut || {
        std::hint::black_box(openbrick_engine::ModelDoc::parse(&text).unwrap());
    });
    let mut model = None;
    time("open (parse + check)", &mut || {
        model = Some(Model::from_text(catalog, &text).expect("opens"));
    });
    let mut m = model.unwrap();
    println!("{} parts, index entries {:?}", m.len(), m.index_entries());
    time("save", &mut || {
        std::hint::black_box(m.to_text("x", None));
    });
    time("stats (from scratch)", &mut || {
        std::hint::black_box(m.stats());
    });
    time("stats_cached (cold)", &mut || {
        std::hint::black_box(m.stats_cached());
    });
    let rays: Vec<Ray> = (0..1000)
        .map(|k| Ray {
            origin: [(k * 37 % 3000) as f64, -3000.0, (k * 91 % 3000) as f64],
            dir: [0.05, 1.0, 0.02],
        })
        .collect();
    time("1000 picks", &mut || {
        for r in &rays {
            std::hint::black_box(m.pick(r));
        }
    });
    let brick = catalog.find("ob.brick-2x4").unwrap();
    time("1000 snaps", &mut || {
        for r in &rays {
            std::hint::black_box(m.snap(brick, Rot24::IDENTITY, r).unwrap());
        }
    });
    time("1000 remove+undo (middle)", &mut || {
        for k in 0..1000u32 {
            let id = n as u32 / 2 + k * 3;
            if m.apply(&Command::Remove { id }).is_ok() {
                m.undo().unwrap();
            }
        }
    });
}
