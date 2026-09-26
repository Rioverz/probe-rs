# OPI-GDB-LIB extraction manifest

This crate exposes the probe-rs GDB server as a library so an OPI worker can
serve GDB by borrowing its already-owned `&FairMutex<Session>`, instead of the
CLI path that opens the probe a second time via `simple_attach`.

## Source

- Upstream repo: `https://github.com/probe-rs/probe-rs`
- Commit: `48f5e4d53c690a1d40c2454033c6f785b4f4f95c` (probe-rs v0.32.0)
- Source directory (in this same fork): `probe-rs-tools/src/bin/probe-rs/cmd/gdb_server/`

The engine files are regenerated from that pinned upstream source by the
engine-update tooling on every bump; they are not hand-maintained here.

| Crate path | Upstream path | Change |
| --- | --- | --- |
| `src/gdb/stub.rs` | `gdb_server/stub.rs` | byte-verbatim |
| `src/gdb/arch.rs` | `gdb_server/arch.rs` | byte-verbatim |
| `src/gdb/target/**` | `gdb_server/target/**` | byte-verbatim, except `target/desc/mod.rs` (delta #2) |
| `src/gdb/mod.rs` | `gdb_server/mod.rs` | rewritten — OPI-authored (delta #1) |
| `src/lib.rs` | — | OPI-authored library entry |

## Two documented deltas from verbatim

1. **`mod.rs` — library exposure.** Upstream `mod.rs` defines a clap `Cmd` whose
   `run` calls `ProbeOptions::simple_attach(..)` (a second probe open) and
   re-exports the stub as `pub(crate)` (visible only inside the `probe-rs`
   binary). This crate drops the `Cmd`/`simple_attach` ownership path and
   re-exports `stub::{run, GdbInstanceConfiguration}` as `pub`. No line of the
   RSP engine logic is edited.
2. **`target/desc/mod.rs` — removed snapshot tests.** The upstream
   `#[cfg(test)] mod test;` line, `target/desc/test.rs`, and
   `target/desc/snapshots/` are removed (they exercise target-description
   rendering, not the borrow seam, and would pull an `insta` dev-dependency).

## Re-verifying "no silent drift"

Because the engine files are otherwise verbatim, drift is detectable by
re-diffing against the pinned source (the two deltas above are the only expected
differences):

```bash
UP=<fork>/probe-rs-tools/src/bin/probe-rs/cmd/gdb_server
diff "$UP/stub.rs" src/gdb/stub.rs   # empty
diff "$UP/arch.rs" src/gdb/arch.rs   # empty
diff -r "$UP/target" src/gdb/target  # only removed desc/test.rs + desc/snapshots/ and the desc/mod.rs test-line note
```

## Licensing

probe-rs is dual-licensed `MIT OR Apache-2.0` (`Copyright (c) 2019 probe-rs`).
The upstream texts are preserved verbatim under `licenses/probe-rs/`. They cover
the copied files (`src/gdb/stub.rs`, `src/gdb/arch.rs`, all of `src/gdb/target/`).
`src/gdb/mod.rs` and `src/lib.rs` are OPI-authored.

## Lint scoping

The borrowed subtree carries a narrowly-scoped module-level
`#[allow(warnings, clippy::all)]` on `mod arch/stub/target` in `src/gdb/mod.rs`,
so `-D warnings` reflects only the OPI-authored library exposure. No broad
crate-level warning suppression is used.

## Guarantee tests

- `tests/borrowed_session_seam.rs` — compile/link proof that `run` borrows
  `&FairMutex<Session>` and `GdbInstanceConfiguration::from_session` borrows
  `&Session` against this fork's core.
- `tests/no_independent_attach.rs` — fails if any non-comment source line
  contains an enumerate/open/attach path (`simple_attach`, `.open(`, `.attach(`,
  `Lister`, …).

## Removal condition

Drop this extraction when a published upstream release exposes the GDB server as
a library the production pin can depend on directly.
