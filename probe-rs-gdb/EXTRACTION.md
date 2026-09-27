# OPI-GDB-LIB extraction manifest

This crate exposes the probe-rs GDB server as a library so an OPI worker can
serve GDB by borrowing its already-owned `&FairMutex<Session>`, instead of the
CLI path that opens the probe a second time via `simple_attach`.

## Source

- Upstream repo: `https://github.com/probe-rs/probe-rs`
- Commit: `48f5e4d53c690a1d40c2454033c6f785b4f4f95c` (probe-rs v0.32.0)
- Source directory (in this same fork): `probe-rs-tools/src/bin/probe-rs/cmd/gdb_server/`

The engine files are extracted from that pinned upstream source (the source
directory above is the fork's own pinned copy at the commit named above) with the
documented deltas below. They are not hand-maintained: fidelity is enforced
deterministically by `tests/extraction_drift.rs` against a committed SHA-256
baseline of the pinned contents. A full `cargo xtask probe-rs update` regenerator
that re-extracts this subtree from a fetched upstream revision is **planned future
work and does not exist yet**; today extraction is *verified*, not regenerated.

Comparisons are newline-normalized, so entries below are text-identical modulo
line endings (not byte-for-byte on a CRLF checkout), which is what the verifier
checks.

| Crate path | Upstream path | Change |
| --- | --- | --- |
| `src/gdb/stub.rs` | `gdb_server/stub.rs` | text-identical (modulo line endings) |
| `src/gdb/arch.rs` | `gdb_server/arch.rs` | text-identical (modulo line endings) |
| `src/gdb/target/**` | `gdb_server/target/**` | text-identical, except `target/desc/mod.rs` (delta #2) |
| `src/gdb/mod.rs` | `gdb_server/mod.rs` | rewritten — OPI-authored (delta #1) |
| `src/lib.rs` | — | OPI-authored library entry |

## Two documented deltas from the pinned source

1. **`mod.rs` — library exposure.** Upstream `mod.rs` defines a clap `Cmd` whose
   `run` calls `ProbeOptions::simple_attach(..)` (a second probe open) and
   re-exports the stub as `pub(crate)` (visible only inside the `probe-rs`
   binary). This crate drops the `Cmd`/`simple_attach` ownership path and
   re-exports `stub::{run, GdbInstanceConfiguration}` as `pub`. No line of the
   RSP engine logic is edited.
2. **`target/desc/mod.rs` — removed snapshot tests.** The upstream trailing
   test-module block (the blank line, `#[cfg(test)]` and `mod test;`),
   `target/desc/test.rs`, and `target/desc/snapshots/` are removed (they exercise
   target-description rendering, not the borrow seam, and would pull an `insta`
   dev-dependency). No explanatory comment is added in the extracted engine file
   itself, so the only difference from upstream is the deletion; the explanation
   lives here.

## Re-verifying "no silent drift"

`tests/extraction_drift.rs` is the authoritative, deterministic check. It hashes
the in-fork pinned source and the extracted crate (newline-normalized) and
compares both against the committed baseline in `tests/pinned_gdb_server.sha256`
— reviewed SHA-256 values of the pinned upstream contents at the commit above,
not a live diff of the two trees. It also re-derives the `desc/mod.rs` transform
from the source and confirms it yields the committed extracted hash, and its
negative tests prove that both an extracted-only edit and an identical edit made
to *both* trees are caught. The two deltas above are the only expected
differences; the same result is reproducible by hand (newline-normalized):

```bash
UP=<fork>/probe-rs-tools/src/bin/probe-rs/cmd/gdb_server
diff "$UP/stub.rs" src/gdb/stub.rs    # empty
diff "$UP/arch.rs" src/gdb/arch.rs    # empty
diff -r "$UP/target" src/gdb/target   # only: removed desc/test.rs, removed
                                      # desc/snapshots/, and desc/mod.rs minus its
                                      # trailing `#[cfg(test)] mod test;` block
```

## Licensing

probe-rs is dual-licensed `MIT OR Apache-2.0` (`Copyright (c) 2019 probe-rs`).
This crate inherits the workspace `license = "MIT OR Apache-2.0"`, and the license
texts live at the fork root (`LICENSE-MIT`, `LICENSE-APACHE`) — the same probe-rs
terms that cover the copied files (`src/gdb/stub.rs`, `src/gdb/arch.rs`, all of
`src/gdb/target/`). `src/gdb/mod.rs` and `src/lib.rs` are OPI-authored under the
same dual license.

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
- `tests/extraction_drift.rs` — deterministic drift verifier against the committed
  SHA-256 baseline of the pinned source; fails if the in-fork source or the
  extracted crate diverges from the pin beyond the two documented deltas, and
  proves (via negative tests) that simultaneous source+copy edits are caught.
- `tests/pinned_gdb_server.sha256` — the reviewed, committed baseline hashes.

## Removal condition

Drop this extraction when a published upstream release exposes the GDB server as
a library the production pin can depend on directly.
