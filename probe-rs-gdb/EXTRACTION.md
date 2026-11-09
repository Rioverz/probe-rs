# GDB server library extraction manifest

This crate exposes the probe-rs GDB server as a library so a caller can
serve GDB by borrowing its already-owned `&FairMutex<Session>`, instead of the
CLI path that opens the probe a second time via `simple_attach`.

## Source

- Upstream repo: `https://github.com/probe-rs/probe-rs`
- Commit: `48f5e4d53c690a1d40c2454033c6f785b4f4f95c` (probe-rs v0.32.0)
- Source directory: `probe-rs-tools/src/bin/probe-rs/cmd/gdb_server/` at that
  commit, read from git (at this fork's base the folder holds upstream's RPC
  rewrite of the GDB server, not this source)

The engine files are extracted from that pinned upstream source with the
documented deltas below. They are not hand-maintained: fidelity is enforced
deterministically by `tests/extraction_drift.rs`, which reads the pinned source
from git. A full `cargo xtask probe-rs update` regenerator
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
| `src/gdb/mod.rs` | `gdb_server/mod.rs` | rewritten — authored in this fork (delta #1) |
| `src/lib.rs` | — | library entry authored in this fork |

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

## Port to upstream `0da5ec5c5e3d43f66f8eb4cbf5e7106158110c7c`

The fork's base moved from v0.32.0 to that upstream master commit. The crate is
ported to the new core API with **no edits**: every file under `src/` is
unchanged, and the extraction source stays the commit above. Two upstream changes
sit under the crate without an edit: `DownloadOptions` gained `ram_chunk_size`
(default off) and `preverify` now compares sectors (default off); the crate sets
neither. Upstream GDB-server fixes made after the commit above no longer reach
this crate. Port edits for a later base are listed here, each with the upstream
API it follows.

## Re-verifying "no silent drift"

`tests/extraction_drift.rs` is the authoritative, deterministic check. It reads
the pinned source from git (`git ls-tree` and `git show` at the commit above, run
in this repository), applies the two deltas above and compares the result with
`src/gdb` file by file, newline-normalized: an unrecorded edit fails it. A clone
without that commit fails the test with the fix in its message (a full clone, or
`git fetch origin <commit>`); it never skips. A second check binds the whole
ported crate: `PORTED_CRATE_SHA256` is a SHA-256 over every `src/**/*.rs`, each
path relative to the crate root with `/` separators, so Windows and Linux agree.
Its negative tests prove that an extracted-only edit, a crate edit and a source
file the crate lacks are caught. The same result is reproducible by hand:

```bash
mkdir -p /tmp/pinned && git archive 48f5e4d53c690a1d40c2454033c6f785b4f4f95c probe-rs-tools/src/bin/probe-rs/cmd/gdb_server | tar -x -C /tmp/pinned
UP=/tmp/pinned/probe-rs-tools/src/bin/probe-rs/cmd/gdb_server
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
`src/gdb/target/`). `src/gdb/mod.rs` and `src/lib.rs` are authored in this fork under the
same dual license.

## Lint scoping

The borrowed subtree carries a narrowly-scoped module-level
`#[allow(warnings, clippy::all)]` on `mod arch/stub/target` in `src/gdb/mod.rs`,
so `-D warnings` reflects only the library exposure written in this fork. No broad
crate-level warning suppression is used.

## Guarantee tests

- `tests/borrowed_session_seam.rs` — compile/link proof that `run` borrows
  `&FairMutex<Session>` and `GdbInstanceConfiguration::from_session` borrows
  `&Session` against this fork's core.
- `tests/no_independent_attach.rs` — fails if any non-comment source line
  contains an enumerate/open/attach path (`simple_attach`, `.open(`, `.attach(`,
  `Lister`, …).
- `tests/extraction_drift.rs` — deterministic drift verifier against the pinned
  source read from git; fails if the extracted crate diverges from the pin beyond
  the two documented deltas or the ported crate differs from `PORTED_CRATE_SHA256`.

## Removal condition

Drop this extraction when a published upstream release exposes the GDB server as
a library the production pin can depend on directly.
