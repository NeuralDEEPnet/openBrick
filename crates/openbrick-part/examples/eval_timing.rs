//! Times the part evaluator on standard bricks and plates (backlog items 6 to 8 in
//! docs/plan/14). Debug builds are about eight times slower than release
//! (docs/spikes/q01-kernel-conformance.md), so run it in release:
//!
//! ```text
//! cargo run --release -p openbrick-part --example eval_timing
//! cargo run --release -p openbrick-part --example eval_timing -- 2x4:brick 6x16:plate
//! ```
//!
//! Each argument is `<x>x<y>:<tile|plate|brick>`. Every part is built with studs
//! (except tiles) and a tube field on the nominal fdm-0.4 process with the example
//! fit profile, three times; the table shows the fastest run.

use std::time::Instant;

use openbrick_geom::manifold_rust::ManifoldRust;
use openbrick_part::{evaluate, EvalInput, MemoryAssets, ParamValues, PartDoc};
use openbrick_print::{default_process_profiles, example_fit_profile};

fn doc(x: u32, y: u32, kind: &str) -> PartDoc {
    let studs = if kind == "tile" {
        String::new()
    } else {
        r#"{ "id": "studs", "op": "stud.field", "on": "body.top" },"#.to_string()
    };
    let text = format!(
        r#"{{
          "openbrick": "1.0", "id": "bench.part", "version": "0.1.0", "name": "bench",
          "license": "CC0-1.0", "credit": {{ "author": "OpenBrick3D contributors" }},
          "features": [
            {{ "id": "body", "op": "body.{kind}", "size": [{x}, {y}] }},
            {studs}
            {{ "id": "bottom", "op": "tube.field", "on": "body.bottom", "fitCritical": true }}
          ]
        }}"#
    );
    PartDoc::parse(&text).expect("bench document parses")
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = [
            "2x2:brick",
            "2x4:tile",
            "2x4:plate",
            "2x4:brick",
            "1x4:plate",
            "6x16:plate",
        ]
        .map(String::from)
        .to_vec();
    }
    let process = default_process_profiles()
        .into_iter()
        .find(|p| p.id == "fdm-0.4")
        .expect("fdm-0.4 is bundled");
    let fit = example_fit_profile();
    let kernel = ManifoldRust;
    println!("kernel: {}", openbrick_geom::Kernel::id(&kernel));
    println!(
        "{:<12} {:>9} {:>9} {:>12} {:>5} {:>5} {:>10}",
        "part", "best ms", "tris", "volume mm3", "genus", "comps", "connectors"
    );
    for arg in args {
        let (size, kind) = arg.split_once(':').unwrap_or((arg.as_str(), "brick"));
        let (x, y) = size.split_once('x').expect("size like 2x4");
        let d = doc(x.parse().expect("x"), y.parse().expect("y"), kind);
        let params = ParamValues::new();
        let assets = MemoryAssets::new();
        let input = EvalInput {
            doc: &d,
            params: &params,
            process: &process,
            fit: Some(&fit),
            assets: &assets,
            skip_unsupported: false,
        };
        let mut best = f64::INFINITY;
        let mut last = None;
        for _ in 0..3 {
            let t = Instant::now();
            let built = evaluate(&kernel, &input).expect("evaluates");
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
            last = Some(built);
        }
        let b = last.expect("built");
        let m = &b.measurements;
        println!(
            "{:<12} {:>9.1} {:>9} {:>12.3} {:>5} {:>5} {:>10}",
            arg,
            best,
            m.triangle_count,
            m.volume_mm3,
            m.genus,
            m.components,
            b.connectors.len()
        );
    }
}
