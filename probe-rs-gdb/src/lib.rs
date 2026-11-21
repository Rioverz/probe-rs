//! GDB target description and memory map of probe-rs cores
//!
//! Upstream builds these bytes inside its GDB server only
//! A caller that serves GDB itself gets the same bytes here
//! The crate opens no probe and serves no protocol
//!
//! Drop it once an upstream release offers the description as a library

pub mod description;

pub use description::{
    DescribedRegister, RegisterSource, TargetDescription, build_target_description, gdb_memory_map,
};
