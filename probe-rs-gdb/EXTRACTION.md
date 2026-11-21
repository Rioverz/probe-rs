# GDB target description source note

This crate holds the GDB target description and memory map code of the probe-rs
GDB server, exposed as a library. It holds no GDB server: the RSP engine that an
earlier version of this crate extracted is removed.

## Source

- Upstream repo: `https://github.com/probe-rs/probe-rs`
- Base commit: `0da5ec5c5e3d43f66f8eb4cbf5e7106158110c7c` (`v0.32.0-360-g0da5ec5c5`)
- Upstream files:
  - `probe-rs-tools/src/bin/probe-rs/cmd/gdb_server/target/desc/data.rs`
  - `probe-rs-tools/src/bin/probe-rs/cmd/gdb_server/target/desc/mod.rs`
  - `probe-rs-tools/src/bin/probe-rs/rpc/functions/info.rs` (`wire_flash_sectors`)

All the code is in `src/description.rs`. It is hand-maintained: no test compares
it with upstream. Upstream fixes to these files reach the crate only by hand.

## Deltas from upstream

- Slice `as_chunks::<2>()` replaces itertools `chunks(2)`.
- No panics: a register of a size with no GDB type, a feature-less register and
  a missing FPU are left out.
- An Armv8-A core with an ISA that is neither A32 nor A64 is described as A32.
- Each described register carries its probe-rs source (one register, two
  registers as one, or unavailable). The caller maps it to its own reply.
- RISC-V: one pc, from the core registers (upstream adds a 32-bit pc).
- RISC-V: `fflags` and `frm` sit in the FPU feature after `fcsr`, not in cpu.
- The memory map takes a `Target` and a core type, not RPC wire types.
- The address size comes from the core type: 64 for Armv8-A with A64 and for
  RV64, else 32.
- Items used only by the upstream server are removed.
- Pedantic lint fixes without `allow` (renames, merged armv7 arms).

## Licensing

probe-rs is dual-licensed `MIT OR Apache-2.0` (`Copyright (c) 2019 probe-rs`).
This crate inherits the workspace `license = "MIT OR Apache-2.0"`, and the
license texts live at the fork root (`LICENSE-MIT`, `LICENSE-APACHE`).

## Tests

`tests/description.rs` describes every static register set probe-rs exports
with its core types and ISAs, checks the no-panic cases above, and checks the
memory map of a target with no regions.

## Removal condition

Drop this crate when a published upstream release exposes the GDB target
description as a library the production pin can depend on directly.
