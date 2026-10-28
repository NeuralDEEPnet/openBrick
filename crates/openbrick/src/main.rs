//! `openbrick` CLI (docs/plan/05 section 9). M0 shipped `standard`, `profiles`,
//! `check` and the disclaimer. M1 adds `build` (a part document to a printable
//! 3MF or STL, validated first), `validate` (the validator report) and
//! `calibrate coupon` / `calibrate record` (the calibration coupon and the fit
//! profile recorded from it). `convert`, `run`, `catalog` and `serve` land in
//! M3 and M4 per docs/plan/11.
//!
//! Exit codes: 0 success; 1 the validator found blockers (nothing was
//! written); 2 a usage or input error (a missing or unreadable file, a bad
//! parameter, an op the evaluator does not build yet); 3 an internal error,
//! or a command that is not implemented yet. With `--json`, stdout carries one
//! JSON object (also on errors) and human text goes to stderr.

mod calibrate;
mod export;
mod files;
mod params;
mod part;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

use crate::ui::{Fail, Outcome, Ui};

const DISCLAIMER: &str = "OpenBrick3D is an independent, community-driven open-source project published by Neural Deep Network Ltd (Malta). It is not affiliated with, sponsored by, authorized by, or endorsed by the LEGO Group. LEGO®, the LEGO® logo and the Minifigure are trademarks of the LEGO Group of companies, which does not sponsor, authorize or endorse this project. Works with LEGO®-compatible bricks. LDraw™ is a trademark of the Estate of James Jessiman. This software uses the LDraw Parts Library (https://www.ldraw.org).";

#[derive(Parser)]
#[command(
    name = "openbrick",
    version,
    about = "OpenBrick3D: brick-compatible part designer, assembler and print pipeline",
    after_help = EXIT_CODES
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
    /// Build a part into a printable 3MF or STL (validated first).
    ///
    /// The part is validated first: with blockers nothing is written and the
    /// exit code is 1.
    #[command(after_help = BUILD_EXAMPLES)]
    Build(Box<part::BuildArgs>),
    /// Print the calibration coupon and record the cells that fit best.
    #[command(subcommand, after_help = CALIBRATE_EXAMPLES)]
    Calibrate(Box<calibrate::CalibrateCmd>),
    /// Validate a part against a process and fit profile.
    ///
    /// Prints the validator report; the exit code is 1 when it has blockers.
    Validate(Box<part::PartArgs>),
    /// Convert between .obrick, .ldr/.mpd, 3MF, STL, glTF (M3).
    Convert { file: PathBuf },
    /// Run a batch merch job (M4).
    Run { file: PathBuf },
    /// Serve the PWA on the LAN with a QR code (M4).
    Serve,
}

const EXIT_CODES: &str = "Exit codes: 0 success; 1 the validator found blockers (nothing written); 2 usage or input error; 3 internal error or not implemented yet.";

const BUILD_EXAMPLES: &str = "Examples:
  openbrick build parts/ndn.logo-tile.obpart.json --fit example --out tile.3mf
  openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg --param relief=inlay --fit my-fit.json --out inlay.3mf
  openbrick build parts/ndn.logo-tile.obpart.json --params size=2x2 height=brick --fit my-fit.json --out tile.stl --json

Exit codes: 0 written; 1 blockers (nothing written; each blocker is printed with its rule, measured value, threshold and fix); 2 usage or input error; 3 internal error.";

const CALIBRATE_EXAMPLES: &str = "Examples:
  openbrick calibrate coupon --process fdm-0.4 --out coupon.3mf --manifest coupon.json
  openbrick calibrate coupon --pockets full --bed 256x256 --out coupon-full.3mf
  openbrick calibrate record --process fdm-0.4 --printer \"Prusa MINI+\" --material PLA --stud 4.80 --tube 4.95 --pocket 6.65/0.30 --out my-fit.json

Guide: docs/guides/print-test.md";

/// Run a command that has a `--json` mode: print its JSON object on stdout in
/// that mode, its error otherwise, and return its exit code. A panic is
/// reported as an internal error rather than a crash.
fn run_json(command: &str, json: bool, f: impl FnOnce(Ui) -> Result<Outcome, Fail>) -> ExitCode {
    let ui = Ui { json };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(ui)))
        .unwrap_or_else(|_| {
            Err(Fail::Internal(
                "the command stopped on an internal error (see the message above); this is a bug, please report it with the command line and input files".into(),
            ))
        });
    match result {
        Ok(outcome) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&outcome.json).unwrap_or_else(|_| "{}".into())
                );
            }
            ExitCode::from(outcome.code)
        }
        Err(fail) => {
            eprintln!("error: {}", fail.message());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ui::failure_json(command, &fail))
                        .unwrap_or_else(|_| "{}".into())
                );
            }
            ExitCode::from(fail.code())
        }
    }
}

/// Parse the command line. A usage error under `--json` still prints one
/// JSON object on stdout (exit 2), so scripts always get one.
fn parse() -> Result<Cli, ExitCode> {
    match Cli::try_parse() {
        Ok(cli) => Ok(cli),
        Err(e) => {
            let usage = !matches!(
                e.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            if usage && std::env::args().any(|a| a == "--json") {
                let _ = e.print();
                let message = e.render().to_string();
                // The error paragraph, before the usage line, on one line.
                let first = message.split("\n\n").next().unwrap_or("usage error");
                let first = first.split_whitespace().collect::<Vec<_>>().join(" ");
                let fail = Fail::Input(first.trim_start_matches("error: ").to_string());
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ui::failure_json("usage", &fail))
                        .unwrap_or_else(|_| "{}".into())
                );
                return Err(ExitCode::from(fail.code()));
            }
            e.exit()
        }
    }
}

fn main() -> ExitCode {
    let cli = match parse() {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    match cli.cmd {
        Cmd::Build(args) => run_json("build", args.part.json, |ui| part::build(&args, ui)),
        Cmd::Validate(args) => run_json("validate", args.json, |ui| part::validate_cmd(&args, ui)),
        Cmd::Calibrate(cmd) => run_json(cmd.name(), cmd.json(), |ui| cmd.run(ui)),
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
        Cmd::Convert { .. } | Cmd::Run { .. } | Cmd::Serve => {
            eprintln!("not implemented yet: see docs/plan/11-roadmap.md for the milestone that delivers this command");
            ExitCode::from(3)
        }
    }
}

fn openbrick_catalog_attribution() -> &'static str {
    "Part geometry for the assembler comes from the LDraw Parts Library (CC BY 2.0 / CC BY 4.0), downloaded at first run; credits are generated from each file's header."
}
