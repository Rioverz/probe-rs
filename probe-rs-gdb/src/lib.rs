//! OPI-GDB-LIB: the probe-rs GDB server as a library over a borrowed session.
//!
//! Upstream probe-rs `0.32.0` ships its GDB server as tool-internal
//! (`probe-rs-tools`) code whose CLI wrapper opens the probe with
//! `simple_attach`. The OPI Hub worker already owns the authoritative
//! `probe_rs::Session` behind a `FairMutex`; it must serve GDB by *borrowing*
//! that session, never by opening the probe a second time.
//!
//! This crate is the library-exposure patch: it re-exports the extracted
//! GDB stub entry points ([`gdb::run`], [`gdb::GdbInstanceConfiguration`]) as a
//! public library API. `run` takes a borrowed `&FairMutex<Session>` and
//! `GdbInstanceConfiguration::from_session` takes a borrowed `&Session`, so the
//! caller retains ownership. The crate exposes no enumerate/open/attach path of
//! its own (guarded by `tests/no_independent_attach.rs`).
//!
//! Removal condition: drop this extraction when a published upstream release
//! exposes the GDB server as a library the production pin can depend on
//! directly (see `../provenance` in the OPI Hub repository, patch OPI-GDB-LIB).

pub mod gdb;

pub use gdb::{GdbInstanceConfiguration, run};
