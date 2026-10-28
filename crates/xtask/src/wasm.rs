//! `cargo xtask wasm`: build the core for the browser (docs/plan/02 section 3).
//!
//! 1. Check that the `wasm-bindgen` CLI on the PATH has exactly the version that
//!    Cargo.lock pins for the `wasm-bindgen` crate: the generated JavaScript glue
//!    and the bindings compiled into the module must come from the same version.
//! 2. `cargo build --release --target wasm32-unknown-unknown -p openbrick-wasm`.
//! 3. `wasm-bindgen --target web` into `apps/web/src/wasm/` (generated, gitignored).
//! 4. Print the module size raw and gzipped, and fail above the size budget of the
//!    `wasm` CI job in docs/plan/10 section 5 (core under 2.5 MB gzip). The gzip
//!    size is flate2 (miniz_oxide) at its best compression; GNU `gzip -9` uses a
//!    different deflate encoder and differs by a few kilobytes either way, which
//!    is far inside the budget.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the browser app imports the module from.
pub const OUT_DIR: &str = "apps/web/src/wasm";
/// Base name of the generated files: `openbrick_wasm.js`, `openbrick_wasm_bg.wasm`.
const OUT_NAME: &str = "openbrick_wasm";
const TARGET: &str = "wasm32-unknown-unknown";
/// docs/plan/10 section 5: "size budget (core under 2.5 MB gzip)", read as SI
/// megabytes so the check errs on the strict side.
const GZIP_BUDGET_BYTES: usize = 2_500_000;

/// The single version of `name` in Cargo.lock, or an error naming what was found.
pub fn locked_version(lock: &str, name: &str) -> Result<String, String> {
    let wanted = format!("name = \"{name}\"");
    let versions: Vec<String> = lock
        .split("[[package]]")
        .filter(|block| block.lines().any(|l| l.trim() == wanted))
        .filter_map(|block| {
            block.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("version = \"")
                    .and_then(|v| v.strip_suffix('"'))
                    .map(str::to_string)
            })
        })
        .collect();
    match versions.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("Cargo.lock has no {name} package")),
        many => Err(format!(
            "Cargo.lock has {} versions of {name} ({}); the wasm-bindgen CLI can match only one",
            many.len(),
            many.join(", ")
        )),
    }
}

/// The version printed by `wasm-bindgen --version` ("wasm-bindgen 0.2.129").
pub fn cli_version(output: &str) -> Option<&str> {
    output
        .trim()
        .strip_prefix("wasm-bindgen ")?
        .split_whitespace()
        .next()
}

fn target_dir(root: &Path) -> Result<PathBuf, String> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err("cargo metadata failed".into());
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
    meta["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata has no target_directory".into())
}

/// Size of `bytes` gzipped by flate2 at `Compression::best()` (level 9 of
/// miniz_oxide, not GNU gzip; see the module documentation).
pub fn gzip_len(bytes: &[u8]) -> usize {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(bytes).expect("gzip to memory");
    gz.finish().expect("gzip to memory").len()
}

fn build(root: &Path) -> Result<(), String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|e| format!("cannot read Cargo.lock: {e}"))?;
    let pinned = locked_version(&lock, "wasm-bindgen")?;
    let install = format!("cargo install wasm-bindgen-cli --version ={pinned} --locked");
    let cli = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|e| format!("wasm-bindgen CLI not found ({e}); install it with: {install}"))?;
    let shown = String::from_utf8_lossy(&cli.stdout).to_string();
    match cli_version(&shown) {
        Some(v) if v == pinned => {}
        _ => {
            return Err(format!(
                "wasm-bindgen CLI is {:?} but Cargo.lock pins {pinned}; install it with: {install}",
                shown.trim()
            ))
        }
    }

    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--target",
            TARGET,
            "-p",
            "openbrick-wasm",
        ])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build for wasm32 failed".into());
    }
    let input = target_dir(root)?
        .join(TARGET)
        .join("release")
        .join(format!("{OUT_NAME}.wasm"));

    let out_dir = root.join(OUT_DIR);
    // Generated output only; clear it so no stale file from an older build survives.
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir)
            .map_err(|e| format!("cannot clear {}: {e}", out_dir.display()))?;
    }
    let status = Command::new("wasm-bindgen")
        .arg("--target")
        .arg("web")
        .arg("--out-name")
        .arg(OUT_NAME)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg(&input)
        .status()
        .map_err(|e| format!("cannot run wasm-bindgen: {e}"))?;
    if !status.success() {
        return Err("wasm-bindgen failed".into());
    }

    let module = out_dir.join(format!("{OUT_NAME}_bg.wasm"));
    let bytes =
        std::fs::read(&module).map_err(|e| format!("cannot read {}: {e}", module.display()))?;
    let gz = gzip_len(&bytes);
    println!(
        "wasm: {} is {} bytes, {} bytes gzipped by flate2 at best compression (budget {} bytes, docs/plan/10 section 5)",
        Path::new(OUT_DIR)
            .join(format!("{OUT_NAME}_bg.wasm"))
            .display(),
        bytes.len(),
        gz,
        GZIP_BUDGET_BYTES
    );
    if gz > GZIP_BUDGET_BYTES {
        return Err(format!(
            "the gzipped module is {gz} bytes, over the {GZIP_BUDGET_BYTES}-byte budget"
        ));
    }
    println!("wasm: ok (wasm-bindgen {pinned}, --target web, {OUT_DIR}/)");
    Ok(())
}

/// Run the task; prints the reason and returns false on failure.
pub fn wasm(root: &Path) -> bool {
    match build(root) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("wasm: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locked_version_is_found_and_ambiguity_is_an_error() {
        let lock = "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n\n[[package]]\nname = \"wasm-bindgen-shared\"\nversion = \"0.2.129\"\n";
        assert_eq!(locked_version(lock, "wasm-bindgen").unwrap(), "0.2.129");
        assert!(locked_version(lock, "js-sys")
            .unwrap_err()
            .contains("no js-sys"));
        let two = format!("{lock}\n[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.100\"\n");
        assert!(locked_version(&two, "wasm-bindgen")
            .unwrap_err()
            .contains("2 versions"));
    }

    #[test]
    fn the_workspace_pins_one_wasm_bindgen() {
        let lock = include_str!("../../../Cargo.lock");
        let manifest = include_str!("../../openbrick-wasm/Cargo.toml");
        let locked = locked_version(lock, "wasm-bindgen").expect("one wasm-bindgen");
        assert!(
            manifest.contains(&format!("wasm-bindgen = \"={locked}\"")),
            "crates/openbrick-wasm/Cargo.toml must pin wasm-bindgen = \"={locked}\""
        );
    }

    #[test]
    fn cli_versions_are_parsed() {
        assert_eq!(cli_version("wasm-bindgen 0.2.129\n"), Some("0.2.129"));
        assert_eq!(
            cli_version("wasm-bindgen 0.2.129 (abc123)"),
            Some("0.2.129")
        );
        assert_eq!(cli_version("something else"), None);
    }

    #[test]
    fn gzip_shrinks_repetitive_input() {
        let bytes = vec![7u8; 10_000];
        let n = gzip_len(&bytes);
        assert!(n > 0 && n < 200, "{n}");
    }
}
