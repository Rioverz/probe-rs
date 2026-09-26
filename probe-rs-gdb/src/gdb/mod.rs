//! GDB server, exposed as a library (OPI-GDB-LIB).
//!
//! `stub.rs`, `arch.rs`, and `target/` are byte-verbatim copies of the probe-rs
//! `probe-rs-tools` GDB server at the pinned upstream commit
//! `48f5e4d53c690a1d40c2454033c6f785b4f4f95c` (see `../../EXTRACTION.md`), with
//! two documented, mechanical exceptions:
//!
//!   1. This file. Upstream's `mod.rs` additionally defines a clap `Cmd` whose
//!      `run` calls `ProbeOptions::simple_attach(..)` — a second probe
//!      enumeration + open + attach — and re-exports the stub as `pub(crate)`,
//!      visible only inside the `probe-rs` binary. We drop the `Cmd` /
//!      `simple_attach` ownership path and re-export the stub entry points as
//!      `pub`, so a worker that already owns the `Session` borrows it into the
//!      GDB server instead of opening the probe a second time.
//!   2. `target/desc/mod.rs` has its `#[cfg(test)] mod test;` line removed (the
//!      snapshot tests and their `insta` dev-dependency are out of scope for a
//!      library that serves RSP); see `../../EXTRACTION.md`, delta #2.
//!
//! The borrowed subtree is upstream code, not OPI code: it is lint-exempt so
//! `-D warnings` reflects only the OPI-authored library exposure (this file),
//! and it is regenerated from the pinned upstream source by the engine-update
//! tooling, never hand-maintained here.
#[allow(warnings, clippy::all)]
mod arch;
#[allow(warnings, clippy::all)]
mod stub;
#[allow(warnings, clippy::all)]
mod target;

pub use stub::{GdbInstanceConfiguration, run};
