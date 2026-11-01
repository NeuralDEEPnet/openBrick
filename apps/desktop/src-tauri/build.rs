fn main() {
    // Without the `app` feature the crate is only the file rules (src/files.rs,
    // src/shell.rs) and needs no Tauri build step.
    #[cfg(feature = "app")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        // The app's own commands, each with an `allow-...` permission that
        // capabilities/main.json grants. Listing them here makes them
        // denied unless a capability allows them.
        tauri_build::AppManifest::new().commands(&["pick_file", "read_picked_file", "save_file"]),
    ))
    .expect("tauri-build failed: see the message above");
}
