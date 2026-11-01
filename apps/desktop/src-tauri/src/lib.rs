//! The OpenBrick3D desktop app (docs/plan/09, decision D13): the browser
//! playground (apps/web) in a Tauri 2 window, with native open and save
//! dialogs instead of browser downloads and file inputs.
//!
//! Through M2 the desktop app computes nothing natively: the page runs the
//! Rust core as WebAssembly in a Web Worker exactly as in the browser
//! (docs/plan/02 section 3), so desktop and web give the same meshes, hashes
//! and files. This crate adds only the window and three commands that show a
//! native dialog and read or write the one file the user chose.
//!
//! - [`files`]: the file rules (formats, suggested names, bounded reads,
//!   atomic writes), tested on every OS by `cargo test --workspace`.
//! - [`shell`]: which links open in the system browser, and Linux safe
//!   rendering.
//! - `app` (feature `app`): the Tauri application itself.

pub mod files;
pub mod shell;

#[cfg(feature = "app")]
pub mod app;
