//! Public-seam proof: the GDB library operates over a *borrowed*, worker-owned
//! session. We cannot attach a probe in a host test, so the proof is that these
//! real call sites against the fork's `Session` API type-check and link — if
//! that API drifted from what the GDB server borrows, this test crate would
//! fail to compile.

use parking_lot::FairMutex;
use probe_rs::Session;
use probe_rs_gdb::{GdbInstanceConfiguration, run};

// Real call sites, never executed (no probe is owned here). Ownership of the
// `Session` stays with the caller throughout: `from_session` borrows `&Session`
// and `run` borrows `&FairMutex<Session>`.
fn borrowed_session_seam(
    session: &FairMutex<Session>,
    cfgs: &[GdbInstanceConfiguration],
) -> anyhow::Result<()> {
    let _cfgs: Vec<GdbInstanceConfiguration> =
        GdbInstanceConfiguration::from_session(&session.lock(), None::<&str>);
    run(session, cfgs.iter(), None)
}

#[test]
fn borrowed_session_entry_points_type_check() {
    // Coercing to a fn pointer forces the borrow signature above to be fully
    // type-checked and retained; owning a real probe is not required.
    let seam: fn(&FairMutex<Session>, &[GdbInstanceConfiguration]) -> anyhow::Result<()> =
        borrowed_session_seam;
    assert!(seam as usize != 0);
}
