//! GDB server exposed as a library (OPI-GDB-LIB): this `mod.rs` re-exports the
//! stub entry points as `pub` so a worker serves GDB over its own borrowed
//! `Session` instead of reopening the probe. The `arch`/`stub`/`target` engine
//! subtree is extracted upstream code, kept lint-exempt so `-D warnings` reflects
//! only this file. See `../../EXTRACTION.md` for the extraction and its deltas.
#[allow(warnings, clippy::all)]
mod arch;
#[allow(warnings, clippy::all)]
mod stub;
#[allow(warnings, clippy::all)]
mod target;

pub use stub::{GdbInstanceConfiguration, run};
