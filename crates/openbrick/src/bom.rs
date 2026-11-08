//! `openbrick bom <model.obrick>`: validate an assembler model and print its
//! bill of materials (part by colour counts) and connection statistics
//! (docs/plan/07 sections 7 and 13).
//!
//! The model is read as the assembler reads it: every part must be in the
//! built-in catalog or in the LDraw pack given with `--pack`, every colour in the palette, every part on the stud grid,
//! above the ground and overlapping nothing. Floating parts (no chain of
//! connections to a part on the ground) are a warning, never a blocker.
//! Exit codes follow the other commands: 0 success, 2 a missing or invalid
//! file.

use std::path::PathBuf;

use clap::Args;
use openbrick_catalog::Catalog;
use openbrick_engine::Model;
use serde_json::json;

use crate::ui::{Fail, Outcome, Ui, GENERATOR};

#[derive(Args)]
pub struct BomArgs {
    /// The model document (.obrick).
    pub file: PathBuf,
    /// The LDraw part pack (.obpack) the model's `ld.` parts come from, made by
    /// `openbrick catalog import`.
    #[arg(long, value_name = "FILE")]
    pub pack: Option<PathBuf>,
    /// Print one JSON object on stdout instead of the table.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: &BomArgs, ui: Ui) -> Result<Outcome, Fail> {
    let text = std::fs::read_to_string(&args.file).map_err(|e| {
        Fail::Input(format!(
            "cannot read {}: {e}; give the path of a .obrick model",
            args.file.display()
        ))
    })?;
    let builtin = Catalog::builtin().map_err(|e| Fail::Internal(e.to_string()))?;
    let with_pack;
    let catalog = match &args.pack {
        None => builtin,
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| {
                Fail::Input(format!(
                    "cannot read {}: {e}; give the path of an .obpack made by `openbrick catalog import`",
                    path.display()
                ))
            })?;
            let pack = openbrick_catalog::Pack::from_bytes(&bytes)
                .map_err(|e| Fail::Input(format!("{}: {e}", path.display())))?;
            with_pack = builtin.with_pack(&pack);
            &with_pack
        }
    };
    let model = Model::from_text(catalog, &text)
        .map_err(|e| Fail::Input(format!("{} is not a valid model: {e}", args.file.display())))?;
    let bom = model.bom();
    let stats = model.stats();

    ui.say(format!(
        "{}: {} parts, {} connections, {} connected group(s), {} standing on the ground",
        args.file.display(),
        stats.parts,
        stats.connections,
        stats.components,
        stats.grounded
    ));
    ui.say(format!("{:>5}  {:<22} colour", "count", "part"));
    for l in &bom {
        ui.say(format!(
            "{:>5}  {:<22} {} ({})",
            l.count, l.name, l.colour_name, l.colour
        ));
    }
    if !stats.floating.is_empty() {
        let ids: Vec<String> = stats.floating.iter().map(u32::to_string).collect();
        ui.err(format!(
            "warning FLOATING: {} part(s) have no chain of connections to the ground (ids {}); measured {}, threshold 0; fix: press them onto studs of a part that stands on the ground, or on the baseplate",
            stats.floating.len(),
            ids.join(", "),
            stats.floating.len()
        ));
    }
    Ok(Outcome {
        code: 0,
        json: json!({
            "command": "bom",
            "generator": GENERATOR,
            "ok": true,
            "exitCode": 0,
            "file": args.file.display().to_string(),
            "stats": stats,
            "bom": bom,
        }),
    })
}
