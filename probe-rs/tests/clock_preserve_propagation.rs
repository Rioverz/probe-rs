#![cfg(feature = "builtin-targets")]
//! `DownloadOptions::preserve_prepared_target_clock` must propagate through
//! `FlashLoader::commit` to the `Flasher::load` path — proven because a non-dry-run
//! preserve=on commit against a running core fails closed with
//! `PreparedTargetNotHalted`.

use probe_rs::Permissions;
use probe_rs::flashing::{DownloadOptions, FlashError};
use probe_rs::integration::FakeProbe;
use probe_rs::probe::Probe;

const CHIP: &str = "nrf51822_xxAC";

#[test]
fn preserve_on_propagates_from_download_options() {
    let probe = Probe::from_specific_probe(Box::new(FakeProbe::with_mocked_core()));
    let mut session = probe
        .attach(CHIP, Permissions::default())
        .expect("attach fake probe");

    // Ensure the core is running so the prepared-clock guard would reject it.
    let _ = session.core(0).unwrap().run();

    let mut loader = session.target().flash_loader();
    loader
        .add_data(0x0, &[0x1, 0x2, 0x3, 0x4])
        .expect("add flash data");

    let mut options = DownloadOptions::new();
    options.preserve_prepared_target_clock = true;
    // dry_run stays false: the propagated flag is only observable on the real load path.

    let err = loader
        .commit(&mut session, options)
        .expect_err("a running core must be rejected under preserve=on");
    assert!(
        matches!(err, FlashError::PreparedTargetNotHalted),
        "preserve=on must propagate to the flasher and fail closed on a running core; got {err:?}"
    );
}
