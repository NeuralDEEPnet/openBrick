//! `openbrick` CLI. M0 ships `standard`, `profiles`, `check` and the disclaimer;
//! `build`, `calibrate`, `validate`, `convert`, `run`, `catalog` and `serve` land
//! in M1 to M4 per docs/plan/11.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

const DISCLAIMER: &str = "OpenBrick3D is an independent, community-driven open-source project published by Neural Deep Network Ltd (Malta). It is not affiliated with, sponsored by, authorized by, or endorsed by the LEGO Group. LEGO®, the LEGO® logo and the Minifigure are trademarks of the LEGO Group of companies, which does not sponsor, authorize or endorse this project. Works with LEGO®-compatible bricks. LDraw™ is a trademark of the Estate of James Jessiman. This software uses the LDraw Parts Library (https://www.ldraw.org).";

#[derive(Parser)]
#[command(
    name = "openbrick",
    version,
    about = "OpenBrick3D: brick-compatible part designer, assembler and print pipeline"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the Dimensional Standard v1 (nominal mm, LDU, confidence).
    Standard {
        /// Output JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// List bundled process profiles.
    Profiles,
    /// Structurally check a part document (.obpart.json).
    Check { file: PathBuf },
    /// Print the non-affiliation disclaimer and attribution.
    About,
    /// Build a part into 3MF or STL (M1).
    Build { file: PathBuf },
    /// Generate or record the calibration coupon (M1).
    Calibrate,
    /// Validate a part against a process profile (M1).
    Validate { file: PathBuf },
    /// Convert between .obrick, .ldr/.mpd, 3MF, STL, glTF (M3).
    Convert { file: PathBuf },
    /// Run a batch merch job (M4).
    Run { file: PathBuf },
    /// Serve the PWA on the LAN with a QR code (M4).
    Serve,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Standard { json } => {
            let s = openbrick_units::STANDARD.get();
            if json {
                println!("{}", serde_json::to_string_pretty(s).expect("serialise"));
            } else {
                println!(
                    "OpenBrick3D Dimensional Standard v{} ({}), 1 LDU = {} mm",
                    s.version, s.license, s.ldu_mm
                );
                println!(
                    "{:<34} {:>10} {:>9}  {:<9} note",
                    "feature", "nominal mm", "LDU", "conf."
                );
                for f in &s.features {
                    let mm = f
                        .nominal_mm
                        .map(|v| format!("{v:.3}"))
                        .unwrap_or_else(|| "-".into());
                    let ldu = f
                        .ldraw_ldu
                        .map(|v| format!("{v}"))
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "{:<34} {:>10} {:>9}  {:<9} {}",
                        f.id,
                        mm,
                        ldu,
                        format!("{:?}", f.confidence).to_lowercase(),
                        f.note
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Cmd::Profiles => {
            for p in openbrick_print::default_process_profiles() {
                println!(
                    "{:<12} {:<48} line {:.2} mm, min wall {:.2}, min feature {:.2}{}",
                    p.id,
                    p.name,
                    p.line_width_mm,
                    p.min_wall_mm,
                    p.min_feature_mm,
                    if p.experimental {
                        " (experimental)"
                    } else {
                        ""
                    }
                );
            }
            ExitCode::SUCCESS
        }
        Cmd::Check { file } => match std::fs::read_to_string(&file) {
            Ok(text) => match openbrick_part::PartDoc::parse(&text) {
                Ok(doc) => {
                    println!(
                        "ok: {} v{} ({} params, {} features, fit-critical: {})",
                        doc.id,
                        doc.version,
                        doc.params.len(),
                        doc.features.len(),
                        doc.fit_critical_features().join(", ")
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            },
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", file.display());
                ExitCode::from(2)
            }
        },
        Cmd::About => {
            println!(
                "OpenBrick3D {}\n\n{}\n\n{}",
                env!("CARGO_PKG_VERSION"),
                DISCLAIMER,
                openbrick_catalog_attribution()
            );
            ExitCode::SUCCESS
        }
        Cmd::Build { .. }
        | Cmd::Calibrate
        | Cmd::Validate { .. }
        | Cmd::Convert { .. }
        | Cmd::Run { .. }
        | Cmd::Serve => {
            eprintln!("not implemented yet: see docs/plan/11-roadmap.md for the milestone that delivers this command");
            ExitCode::from(3)
        }
    }
}

fn openbrick_catalog_attribution() -> &'static str {
    "Part geometry for the assembler comes from the LDraw Parts Library (CC BY 2.0 / CC BY 4.0), downloaded at first run; credits are generated from each file's header."
}
