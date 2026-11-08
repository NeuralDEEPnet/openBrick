//! Writes the sample models in `models/` (`cargo run -p openbrick-engine --example
//! samples`). Every placement goes through the engine's checks, so a sample that
//! does not stand (overlap, off the stud grid) fails here, not in the app.

use openbrick_catalog::Catalog;
use openbrick_engine::{Command, Model};
use openbrick_units::{Credit, PLATE_LDU};

const BASE: i32 = -PLATE_LDU; // top of the baseplate
const BLUE: u16 = 10000;
const WHITE: u16 = 15;
const RED: u16 = 4;

fn add(m: &mut Model, part: &str, x: i32, y: i32, z: i32, rot: u8, colour: u16) {
    m.apply(&Command::Add {
        part: part.into(),
        pos: [x, y, z],
        rot,
        colour,
    })
    .unwrap_or_else(|e| panic!("{part} at {x},{y},{z}: {e}"));
}

fn start(catalog: &Catalog) -> Model<'_> {
    let mut m = Model::new(catalog);
    add(&mut m, "ob.baseplate-16x16", 0, BASE, 0, 0, 2);
    m
}

fn house(catalog: &Catalog) -> Model<'_> {
    let mut m = start(catalog);
    let side = openbrick_units::Rot24::yaw_quarter_turns(1).0;
    for course in 0..4 {
        let y = BASE - 24 * (course + 1);
        let odd = course % 2 == 1;
        let colour = if course == 3 { WHITE } else { BLUE };
        for z in [-50, 50] {
            let door = z == -50 && course < 3;
            match (door, odd) {
                (false, false) => {
                    add(&mut m, "ob.brick-1x4", -40, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x4", 40, y, z, 0, colour);
                }
                (false, true) => {
                    add(&mut m, "ob.brick-1x2", -60, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x4", 0, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x2", 60, y, z, 0, colour);
                }
                (true, false) => {
                    add(&mut m, "ob.brick-1x2", -60, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x1", -30, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x1", 30, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x2", 60, y, z, 0, colour);
                }
                (true, true) => {
                    add(&mut m, "ob.brick-1x1", -70, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x2", -40, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x2", 40, y, z, 0, colour);
                    add(&mut m, "ob.brick-1x1", 70, y, z, 0, colour);
                }
            }
        }
        for x in [-70, 70] {
            if odd {
                add(&mut m, "ob.brick-1x2", x, y, -20, side, colour);
                add(&mut m, "ob.brick-1x2", x, y, 20, side, colour);
            } else {
                add(&mut m, "ob.brick-1x4", x, y, 0, side, colour);
            }
        }
    }
    // A window: two yellow tiles' worth of colour in the side walls would need
    // transparent parts; the alpha keeps solid colours only.
    let roof = BASE - 24 * 4 - PLATE_LDU;
    for z in [-40, 0, 40] {
        for x in [-40, 40] {
            add(&mut m, "ob.plate-2x4", x, roof, z, 0, RED);
        }
    }
    // A second, smaller roof layer and a chimney.
    for x in [-40, 40] {
        add(&mut m, "ob.plate-2x4", x, roof - PLATE_LDU, 0, 0, RED);
    }
    for k in 0..3 {
        add(
            &mut m,
            "ob.brick-1x1",
            70,
            roof - PLATE_LDU - 24 * (k + 1),
            -10,
            0,
            72,
        );
    }
    m
}

fn stairs(catalog: &Catalog) -> Model<'_> {
    let mut m = start(catalog);
    let colours = [BLUE, 1, 15, 14, 25, 4];
    for (k, &c) in colours.iter().enumerate() {
        let k = k as i32;
        add(
            &mut m,
            "ob.brick-2x4",
            -80 + 40 * k,
            BASE - 24 * (k + 1),
            0,
            0,
            c,
        );
    }
    add(&mut m, "ob.plate-2x4", 120, BASE - 24 * 7 + 16, 0, 0, 70);
    m
}

fn main() {
    let catalog = Catalog::builtin().expect("built-in catalog");
    let credit = Some(Credit {
        author: "OpenBrick3D contributors".into(),
        url: String::new(),
    });
    for (file, name, m) in [
        ("house", "Neuraldeep-blue house", house(catalog)),
        ("staircase", "Rainbow staircase", stairs(catalog)),
    ] {
        let text = m.to_text(name, credit.clone());
        std::fs::write(format!("models/{file}.obrick.json"), text).expect("write");
        let s = m.stats();
        println!(
            "{file}: {} parts, {} connections, {} floating",
            s.parts,
            s.connections,
            s.floating.len()
        );
    }
}
