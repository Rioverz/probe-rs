//! DWT data watchpoints shared by the Armv6-M and Armv7-M cores

use super::armv7m::Demcr;
use crate::{
    architecture::arm::memory::ArmMemoryInterface,
    core::{MemoryMappedRegister, WatchpointAccess},
    error::{Error, WatchpointError},
};

// The DWT data address watchpoints: ARMv6-M Architecture Reference Manual C1.8 and ARMv7-M
// Architecture Reference Manual (DDI 0403E.e) C1.8; the address-match values are the same on both
/// DWT_CTRL; `NUMCOMP`, the number of comparators, is bits [31:28]
pub(super) const DWT_CTRL: u64 = 0xE000_1000;
/// DWT_COMP0; each comparator's COMP, MASK and FUNCTION registers follow at +0, +4 and +8,
/// and comparator `n` is `DWT_COMPARATOR_STRIDE * n` further on
pub(super) const DWT_COMP0: u64 = 0xE000_1020;
pub(super) const DWT_COMPARATOR_STRIDE: u64 = 0x10;
pub(super) const DWT_MASK_OFFSET: u64 = 0x4;
pub(super) const DWT_FUNCTION_OFFSET: u64 = 0x8;
/// MASK: the number of low address bits the comparison ignores; 2 watches one aligned word
pub(super) const DWT_MASK_WORD: u32 = 2;
/// FUNCTION[3:0] for a disabled comparator
pub(super) const DWT_FUNCTION_DISABLED: u32 = 0b0000;
pub(super) const DWT_FUNCTION_MASK: u32 = 0b1111;

/// The COMP, MASK and FUNCTION values of a watchpoint on the word at `address`
pub(super) fn watchpoint_registers(
    address: u32,
    access: WatchpointAccess,
) -> Result<(u32, u32, u32), Error> {
    if !address.is_multiple_of(4) {
        return Err(WatchpointError::NotWordAligned(address.into()).into());
    }
    let function = match access {
        WatchpointAccess::Read => 0b0101,
        WatchpointAccess::Write => 0b0110,
        WatchpointAccess::ReadWrite => 0b0111,
    };
    Ok((address, DWT_MASK_WORD, function))
}

/// The address of comparator `unit`'s COMP register
pub(super) fn dwt_comparator(unit: usize) -> u64 {
    DWT_COMP0 + DWT_COMPARATOR_STRIDE * unit as u64
}

/// Set DEMCR.TRCENA when clear, the bit Armv6-M calls DWTENA: comparators idle without it
///
/// It stays set until detach writes DEMCR 0, a trace user may need it (ARMv7-M ARM C1.6.5)
pub(super) fn enable_dwt(memory: &mut dyn ArmMemoryInterface) -> Result<(), Error> {
    let mut demcr = Demcr(memory.read_word_32(Demcr::get_mmio_address())?);
    if !demcr.trcena() {
        demcr.set_trcena(true);
        memory.write_word_32(Demcr::get_mmio_address(), demcr.into())?;
    }
    Ok(())
}

/// NUMCOMP, the number of comparators, from DWT_CTRL
pub(super) fn units(memory: &mut dyn ArmMemoryInterface) -> Result<u32, Error> {
    Ok(memory.read_word_32(DWT_CTRL)? >> 28)
}

/// Watched address per comparator, `None` when FUNCTION is disabled
///
/// On Armv7-M each FUNCTION read clears MATCHED, so read MATCHED before any listing (DDI 0403E.e C1.8.17)
pub(super) fn in_use(
    memory: &mut dyn ArmMemoryInterface,
    units: u32,
) -> Result<Vec<Option<u64>>, Error> {
    let units = units as usize;
    let mut watchpoints = Vec::with_capacity(units);
    for unit in 0..units {
        let comparator = dwt_comparator(unit);
        let function = memory.read_word_32(comparator + DWT_FUNCTION_OFFSET)?;
        if function & DWT_FUNCTION_MASK == DWT_FUNCTION_DISABLED {
            watchpoints.push(None);
        } else {
            watchpoints.push(Some(memory.read_word_32(comparator)?.into()));
        }
    }
    Ok(watchpoints)
}

/// Program comparator `unit` with FUNCTION written last, then flush
pub(super) fn write(
    memory: &mut dyn ArmMemoryInterface,
    unit: usize,
    (comp, mask, function): (u32, u32, u32),
) -> Result<(), Error> {
    let comparator = dwt_comparator(unit);
    // Disable first so a half-set comparator never matches
    memory.write_word_32(comparator + DWT_FUNCTION_OFFSET, DWT_FUNCTION_DISABLED)?;
    memory.write_word_32(comparator, comp)?;
    memory.write_word_32(comparator + DWT_MASK_OFFSET, mask)?;
    memory.write_word_32(comparator + DWT_FUNCTION_OFFSET, function)?;
    memory.flush()?;
    Ok(())
}

/// Disable comparator `unit` (FUNCTION 0), flush
pub(super) fn clear(memory: &mut dyn ArmMemoryInterface, unit: usize) -> Result<(), Error> {
    memory.write_word_32(
        dwt_comparator(unit) + DWT_FUNCTION_OFFSET,
        DWT_FUNCTION_DISABLED,
    )?;
    memory.flush()?;
    Ok(())
}

/// Disable each of the `units` comparators (FUNCTION 0) unread, then flush once
pub(super) fn clear_all(memory: &mut dyn ArmMemoryInterface, units: u32) -> Result<(), Error> {
    for unit in 0..units as usize {
        memory.write_word_32(
            dwt_comparator(unit) + DWT_FUNCTION_OFFSET,
            DWT_FUNCTION_DISABLED,
        )?;
    }
    memory.flush()?;
    Ok(())
}

/// Armv6-M and Armv7-M test fixtures, addresses as literals
#[cfg(test)]
pub(super) mod test_support {
    use crate::architecture::arm::core::mock::MockProbe;

    // ARMv6-M ARM (DDI 0419E) and ARMv7-M ARM (DDI 0403E.e) C1.6.5 and C1.8
    pub(crate) const DEMCR: u64 = 0xE000_EDFC;
    pub(crate) const DWT_CTRL: u64 = 0xE000_1000;
    #[cfg(feature = "builtin-targets")]
    pub(crate) const TWO_COMPARATORS: u32 = 2 << 28;
    pub(crate) const ADDRESS: u32 = 0x2000_0100;
    const MASK_WORD: u32 = 2;

    pub(crate) fn comp(unit: u64) -> u64 {
        0xE000_1020 + 16 * unit
    }

    fn mask(unit: u64) -> u64 {
        comp(unit) + 4
    }

    pub(crate) fn function(unit: u64) -> u64 {
        comp(unit) + 8
    }

    // FUNCTION 0, COMP, MASK, then the access code
    pub(crate) fn expect_set(probe: &mut MockProbe, unit: u64, address: u32, function_value: u32) {
        probe.expected_write(function(unit), 0);
        probe.expected_write(comp(unit), address);
        probe.expected_write(mask(unit), MASK_WORD);
        probe.expected_write(function(unit), function_value);
    }

    #[cfg(feature = "builtin-targets")]
    pub(crate) fn builtin_target(name: &str) -> crate::Target {
        crate::config::Registry::from_builtin_families()
            .get_target_by_name(name)
            .unwrap()
    }
}
