//! DWT data watchpoints shared by the Armv6-M and Armv7-M cores

use crate::{
    architecture::arm::memory::ArmMemoryInterface,
    core::WatchpointAccess,
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

/// NUMCOMP, the number of comparators, from DWT_CTRL
pub(super) fn units(memory: &mut dyn ArmMemoryInterface) -> Result<u32, Error> {
    Ok(memory.read_word_32(DWT_CTRL)? >> 28)
}

/// Watched address per comparator, `None` when FUNCTION is disabled
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
