//! Times the built-in catalog: every part at both levels of detail.
//!
//! ```text
//! cargo run --release -p openbrick-catalog --example catalog_timing
//! ```

use std::time::Instant;

use openbrick_catalog::{builtin, Lod, BUILTIN};

fn main() {
    println!(
        "{:<22} {:>8} {:>9} {:>8} {:>9} {:>6}",
        "part", "detail ms", "tris", "low ms", "tris", "conns"
    );
    let (mut total_detail, mut total_low) = (0.0, 0.0);
    for spec in BUILTIN {
        let t = Instant::now();
        let d = builtin::build(spec, Lod::Detail).expect("builds");
        let dt = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let l = builtin::build(spec, Lod::Low).expect("builds");
        let lt = t.elapsed().as_secs_f64() * 1e3;
        total_detail += dt;
        total_low += lt;
        println!(
            "{:<22} {:>8.1} {:>9} {:>8.1} {:>9} {:>6}",
            spec.id,
            dt,
            d.mesh.triangle_count(),
            lt,
            l.mesh.triangle_count(),
            d.shape.connectors.len()
        );
    }
    println!("total: detail {total_detail:.0} ms, low {total_low:.0} ms");
}
