//! The LDraw importer (docs/plan/07 section 3, docs/plan/08 section 3): our own
//! parser, a bounded library resolver over a folder or the official zip,
//! tessellation, connector derivation from primitives and the pack writer.
//!
//! Behind the `import` feature: the command line turns it on, the WebAssembly
//! build does not. The LDraw Parts Library is data the user fetches at runtime
//! (CC BY 2.0 / CC BY 4.0); none of it is in this repository, and the tests use
//! small synthetic files written for them.

pub mod connect;
pub mod flatten;
pub mod import;
pub mod library;
pub mod parse;

pub use import::{import, ImportError, ImportOptions, ImportReport, DEFAULT_CATEGORIES};
pub use library::{Library, LibraryError, Resolution};
