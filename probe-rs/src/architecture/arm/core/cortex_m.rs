//! Common functions and data types for Cortex-M core variants

use super::Dfsr;
use crate::memory::{Operation, OperationKind};
use crate::{
    CoreInterface, Error, MemoryMappedRegister,
    architecture::arm::{ArmError, memory::ArmMemoryInterface},
    core::{RegisterId, VectorCatchCondition},
    memory_mapped_bitfield_register,
    semihosting::SemihostingCommand,
    semihosting::decode_semihosting_syscall,
};
use std::time::{Duration, Instant};

memory_mapped_bitfield_register! {
    pub struct Vtor(pub u32);
    0xE000_ED08, "VTOR",
    impl From;
    /// This fields holds bits `[31:7]` of the table offset.
    pub tbloff, set_tbloff: 31, 7;
}

memory_mapped_bitfield_register! {
    pub struct Dhcsr(pub u32);
    0xE000_EDF0, "DHCSR",
    impl From;
    pub s_reset_st, _: 25;
    pub s_retire_st, _: 24;
    pub s_lockup, _: 19;
    pub s_sleep, _: 18;
    pub s_halt, _: 17;
    pub s_regrdy, _: 16;
    pub c_maskints, set_c_maskints: 3;
    pub c_step, set_c_step: 2;
    pub c_halt, set_c_halt: 1;
    pub c_debugen, set_c_debugen: 0;
}

impl Dhcsr {
    /// This function sets the bit to enable writes to this register.
    ///
    /// C1.6.3 Debug Halting Control and Status Register, DHCSR:
    /// Debug key:
    /// Software must write 0xA05F to this field to enable write accesses to bits
    /// `[15:0]`, otherwise the processor ignores the write access.
    pub fn enable_write(&mut self) {
        self.0 &= !(0xffff << 16);
        self.0 |= 0xa05f << 16;
    }
}

memory_mapped_bitfield_register! {
    /// Debug Exception and Monitor Control Register, DEMCR
    ///
    /// Only the vector catch bits common to ARMv6-M and ARMv7-M are modelled here.
    pub struct Demcr(pub u32);
    0xE000_EDFC, "DEMCR",
    impl From;
    pub vc_harderr, set_vc_harderr: 10;
    pub vc_corereset, set_vc_corereset: 0;
}

memory_mapped_bitfield_register! {
    pub struct Dcrsr(pub u32);
    0xE000_EDF4, "DCRSR",
    impl From;
    pub _, set_regwnr: 16;
    // If the processor does not implement the FP extension the REGSEL field is bits `[4:0]`, and bits `[6:5]` are Reserved, SBZ.
    // Increased to 7 bits on v8-M
    pub _, set_regsel: 7,0;
}

memory_mapped_bitfield_register! {
    pub struct Dcrdr(pub u32);
    0xE000_EDF8, "DCRDR",
    impl From;
}

memory_mapped_bitfield_register! {
    ///  Coprocessor Access Control Register
    pub struct Cpacr(pub u32);
    0xE000_ED88, "CPACR",
    impl From;
    pub fpu_privilege, _: 21,20;
}

impl Cpacr {
    pub fn fpu_present(&self) -> bool {
        self.fpu_privilege() != 0
    }
}

memory_mapped_bitfield_register! {
    ///  Media and VFP Feature Register 0
    pub struct Mvfr0(pub u32);
    0xE000_EF40, "MVFR0",
    impl From;
    pub fpdp, _: 11, 8;
    pub fpsp, _: 7, 4;
}

impl Mvfr0 {
    pub fn fp_present(&self) -> bool {
        self.fpdp() != 0 || self.fpsp() != 0
    }
}

pub enum SecurityExtension {
    NotImplemented,
    Implemented,
    ImplementedWithStateHandling,
    Reserved,
}

impl From<u8> for SecurityExtension {
    fn from(value: u8) -> Self {
        match value {
            0b0000 => SecurityExtension::NotImplemented,
            0b0001 => SecurityExtension::Implemented,
            0b0011 => SecurityExtension::ImplementedWithStateHandling,
            _ => SecurityExtension::Reserved,
        }
    }
}

memory_mapped_bitfield_register! {
    /// Processor Feature Register 1
    pub struct IdPfr1(pub u32);
    0xE000_ED44, "ID_PFR1",
    impl From;
    /// Identifies support for the M-Profile programmer's model
    pub u8, m_prog_mod, _: 11, 8;
    /// Identifies whether the Security Extension is implemented
    pub u8, security, _: 7, 4;
}

impl IdPfr1 {
    pub fn security_present(&self) -> bool {
        matches!(
            self.security().into(),
            SecurityExtension::Implemented | SecurityExtension::ImplementedWithStateHandling
        )
    }
}

pub(crate) fn read_core_reg(
    memory: &mut dyn ArmMemoryInterface,
    addr: RegisterId,
) -> Result<u32, ArmError> {
    let mut dcrsr_val = Dcrsr(0);
    dcrsr_val.set_regwnr(false); // Perform a read.
    dcrsr_val.set_regsel(addr.into()); // The address of the register to read.

    // Select the register, then read the ready flag and the value. The flag is read first, so a
    // flag that comes back set means the value behind it had already settled.
    let mut ready = 0u32;
    let mut value = 0u32;
    memory.execute_operations(&mut [
        Operation::new(
            Dcrsr::get_mmio_address(),
            OperationKind::WriteWord32(dcrsr_val.into()),
        ),
        Operation::new(
            Dhcsr::get_mmio_address(),
            OperationKind::Read32(std::slice::from_mut(&mut ready)),
        ),
        Operation::new(
            Dcrdr::get_mmio_address(),
            OperationKind::Read32(std::slice::from_mut(&mut value)),
        ),
    ])?;

    if Dhcsr(ready).s_regrdy() {
        return Ok(value);
    }

    // Asked too early, which the flag is there to catch. Wait for it and read the value again.
    wait_for_core_register_transfer(memory, Duration::from_millis(100))?;

    memory.read_word_32(Dcrdr::get_mmio_address())
}

pub(crate) fn write_core_reg(
    memory: &mut dyn ArmMemoryInterface,
    addr: RegisterId,
    value: u32,
) -> Result<(), ArmError> {
    // write the DCRSR value to select the register we want to write.
    let mut dcrsr_val = Dcrsr(0);
    dcrsr_val.set_regwnr(true); // Perform a write.
    dcrsr_val.set_regsel(addr.into()); // The address of the register to write.

    // The DCRSR write clears the ready flag, so a flag that comes back set
    // means the transfer is done.
    let mut ready = 0u32;
    memory.execute_operations(&mut [
        Operation::new(Dcrdr::get_mmio_address(), OperationKind::WriteWord32(value)),
        Operation::new(
            Dcrsr::get_mmio_address(),
            OperationKind::WriteWord32(dcrsr_val.into()),
        ),
        Operation::new(
            Dhcsr::get_mmio_address(),
            OperationKind::Read32(std::slice::from_mut(&mut ready)),
        ),
    ])?;

    if Dhcsr(ready).s_regrdy() {
        return Ok(());
    }

    wait_for_core_register_transfer(memory, Duration::from_millis(100))
}

/// Leave Debug state, either running freely or stepping one instruction.
///
/// A single DHCSR write cannot clear C_HALT and change C_MASKINTS, so C_MASKINTS is settled in
/// its own write first. See ARM DDI 0419E C1.6.3, DDI 0403E.e C1.6.2 and DDI 0553B.v D1.2.38
pub(crate) fn exit_halt(memory: &mut dyn ArmMemoryInterface, step: bool) -> Result<(), ArmError> {
    let mut dhcsr = Dhcsr(memory.read_word_32(Dhcsr::get_mmio_address())?);

    if !dhcsr.c_debugen() {
        tracing::warn!("Leaving halt while DHCSR.C_DEBUGEN is false");
    }

    // C_HALT stays 1 in this write, which is what makes the C_MASKINTS change predictable.
    if dhcsr.c_maskints() != step {
        dhcsr.set_c_maskints(step);
        dhcsr.enable_write();
        memory.write_word_32(Dhcsr::get_mmio_address(), dhcsr.into())?;
        memory.flush()?;
    }

    dhcsr.set_c_step(step);
    dhcsr.set_c_debugen(true);
    release_halt(memory, dhcsr)
}

/// Write DHCSR with C_HALT 0, clearing DFSR just before it
fn release_halt(memory: &mut dyn ArmMemoryInterface, mut dhcsr: Dhcsr) -> Result<(), ArmError> {
    dhcsr.set_c_halt(false);
    dhcsr.enable_write();
    clear_dfsr(memory)?;
    memory.write_word_32(Dhcsr::get_mmio_address(), dhcsr.into())?;
    memory.flush()
}

/// Clear every DFSR bit (write one to clear)
pub(crate) fn clear_dfsr(memory: &mut dyn ArmMemoryInterface) -> Result<(), ArmError> {
    memory.write_word_32(Dfsr::get_mmio_address(), Dfsr::clear_all().into())
}

/// Clear C_STEP and C_MASKINTS left by a step since a system reset does not clear DHCSR.
///
/// C_HALT is set in the write that changes C_MASKINTS as exit_halt does.
/// When halt is false and the core was running a second write clears C_HALT again.
/// DFSR survives a local reset (DDI 0403E.e C1.6.1): clear it last
pub(crate) fn clear_step_before_reset(
    memory: &mut dyn ArmMemoryInterface,
    halt: bool,
) -> Result<(), ArmError> {
    let mut dhcsr = Dhcsr(memory.read_word_32(Dhcsr::get_mmio_address())?);
    if dhcsr.c_step() || dhcsr.c_maskints() {
        let was_halted = dhcsr.c_halt();

        dhcsr.set_c_step(false);
        dhcsr.set_c_maskints(false);
        dhcsr.set_c_halt(true);
        dhcsr.enable_write();
        memory.write_word_32(Dhcsr::get_mmio_address(), dhcsr.into())?;
        memory.flush()?;

        if !halt && !was_halted {
            release_halt(memory, dhcsr)?;
        }
    }
    clear_dfsr(memory)?;
    memory.flush()
}

/// Set DHCSR.C_DEBUGEN, keeping the debug key so the write is not ignored.
pub(crate) fn enable_halting_debug(memory: &mut dyn ArmMemoryInterface) -> Result<(), ArmError> {
    let mut dhcsr = Dhcsr(memory.read_word_32(Dhcsr::get_mmio_address())?);
    dhcsr.set_c_debugen(true);
    dhcsr.enable_write();
    memory.write_word_32(Dhcsr::get_mmio_address(), dhcsr.into())
}

/// Enable or disable a vector catch condition.
///
/// ARMv6-M and ARMv7-M only. ARMv8-M also has VC_SFERR and keeps its own copy.
pub(crate) fn set_vector_catch(
    memory: &mut dyn ArmMemoryInterface,
    condition: VectorCatchCondition,
    enable: bool,
) -> Result<(), Error> {
    let mut demcr = Demcr(memory.read_word_32(Demcr::get_mmio_address())?);
    match condition {
        VectorCatchCondition::HardFault => demcr.set_vc_harderr(enable),
        VectorCatchCondition::CoreReset => demcr.set_vc_corereset(enable),
        VectorCatchCondition::All => {
            demcr.set_vc_harderr(enable);
            demcr.set_vc_corereset(enable);
        }
        VectorCatchCondition::SecureFault => {
            return Err(Error::Arm(ArmError::ArchitectureRequired(&["ARMv8"])));
        }
        VectorCatchCondition::Svc | VectorCatchCondition::Hlt => {
            return Err(Error::NotImplemented("vector catch condition Svc/Hlt"));
        }
    };

    memory.write_word_32(Demcr::get_mmio_address(), demcr.into())?;
    Ok(())
}

/// Check if the current breakpoint is a semihosting call.
///
/// Call this if you get some kind of breakpoint. Works on ARMv6-M, ARMv7-M and ARMv8-M.
pub(crate) fn check_for_semihosting(
    cached_command: Option<SemihostingCommand>,
    core: &mut dyn CoreInterface,
) -> Result<Option<SemihostingCommand>, Error> {
    // The Arm Semihosting Specification, specifies that the instruction
    // "BKPT 0xAB" (encoded as 0xBEAB) triggers a semihosting call.
    // <https://github.com/ARM-software/abi-aa/blob/main/semihosting/semihosting.rst#the-semihosting-interface>
    const TRAP_INSTRUCTION: [u8; 2] = [
        // instruction encoded as little endian
        0xAB, 0xBE,
    ];

    // We only want to decode the semihosting command once, since answering it might change some of the registers
    if let Some(command) = cached_command {
        return Ok(Some(command));
    }

    let pc: u32 = core.read_core_reg(core.program_counter().id)?.try_into()?;

    let mut actual_instruction = [0u8; 2];
    core.read_8(pc as u64, &mut actual_instruction)?;
    let actual_instruction = actual_instruction.as_slice();

    tracing::debug!(
        "Semihosting check pc={pc:#x} instruction={0:#02x}{1:#02x}",
        actual_instruction[1],
        actual_instruction[0]
    );

    let command = if TRAP_INSTRUCTION == actual_instruction {
        Some(decode_semihosting_syscall(core)?)
    } else {
        None
    };

    Ok(command)
}

/// Returns true if `instruction` is a Thumb `BKPT` (`0xBExx`).
pub(crate) fn is_bkpt_instruction(instruction: u16) -> bool {
    const BKPT_MASK: u16 = 0xFF00;
    const BKPT_ENCODING: u16 = 0xBE00;

    instruction & BKPT_MASK == BKPT_ENCODING
}

/// Reads the halfword at `address` and checks if it is a `BKPT`.
///
/// If the read fails we treat it as not a `BKPT`, so we never skip an instruction we couldn't read.
pub(crate) fn is_bkpt_at(core: &mut dyn CoreInterface, address: u64) -> bool {
    let mut instruction = [0u8; 2];
    match core.read_8(address, &mut instruction) {
        Ok(()) => is_bkpt_instruction(u16::from_le_bytes(instruction)),
        Err(error) => {
            tracing::warn!(
                "Could not read the instruction at {address:#010x}, not advancing the PC: {error}"
            );
            false
        }
    }
}

fn wait_for_core_register_transfer(
    memory: &mut dyn ArmMemoryInterface,
    timeout: Duration,
) -> Result<(), ArmError> {
    // now we have to poll the dhcsr register, until the dhcsr.s_regrdy bit is set
    // (see C1-292, cortex m0 arm)
    let start = Instant::now();

    while start.elapsed() < timeout {
        let dhcsr_val = Dhcsr(memory.read_word_32(Dhcsr::get_mmio_address())?);

        if dhcsr_val.s_regrdy() {
            return Ok(());
        }
    }
    Err(ArmError::Timeout)
}

#[cfg(test)]
mod tests {
    use super::is_bkpt_instruction;

    use super::{clear_step_before_reset, exit_halt};
    use crate::CoreType;
    use crate::architecture::arm::ArmError;
    use crate::architecture::arm::core::mock::MockProbe;
    use crate::architecture::arm::memory::ArmMemoryInterface;
    use crate::architecture::arm::sequences::DefaultArmSequence;

    // Register addresses: DDI 0403E.e C1.6.2 (DHCSR) and C1.6.1 (DFSR)
    const DHCSR: u64 = 0xE000_EDF0;
    const DFSR: u64 = 0xE000_ED30;
    const DEMCR: u64 = 0xE000_EDFC;
    const KEY: u32 = 0xA05F << 16;
    const C_DEBUGEN: u32 = 1 << 0;
    const C_HALT: u32 = 1 << 1;
    const C_STEP: u32 = 1 << 2;
    const C_MASKINTS: u32 = 1 << 3;
    const S_HALT: u32 = 1 << 17;
    const DFSR_ALL: u32 = 0b1_1111;

    #[derive(Debug, Clone, Copy)]
    enum Op {
        Read(u64, u32),
        Write(u64, u32),
    }

    /// Run a call on a mock that allows exactly these operations
    fn recorded(
        ops: &[Op],
        call: impl FnOnce(&mut dyn ArmMemoryInterface) -> Result<(), ArmError>,
    ) {
        let mut probe = MockProbe::new(0);
        for op in ops {
            match *op {
                Op::Read(address, value) => probe.expected_read(address, value),
                Op::Write(address, value) => probe.expected_write(address, value),
            }
        }
        call(&mut probe).unwrap();
    }

    /// The release rule over an operation list
    ///
    /// DFSR starts as maybe set; a C_HALT=1 write makes it so again and
    /// may not come after the clear; only a write of all five bits
    /// clears it; a C_HALT=0 write needs it clear
    fn check_release_rule(ops: &[Op], ends_clear: bool) -> Result<(), String> {
        let mut clear = false;
        for (at, op) in ops.iter().enumerate() {
            match *op {
                Op::Write(DFSR, DFSR_ALL) => clear = true,
                Op::Write(DFSR, value) => return Err(format!("op {at}: DFSR write {value:#x}")),
                Op::Write(DHCSR, value) if value & C_HALT != 0 => {
                    if clear {
                        return Err(format!("op {at}: C_HALT=1 write after the clear"));
                    }
                }
                Op::Write(DHCSR, _) => {
                    if !clear {
                        return Err(format!("op {at}: release with DFSR maybe set"));
                    }
                    clear = false;
                }
                _ => {}
            }
        }
        if ends_clear && !clear {
            return Err("DFSR maybe set at the end".to_owned());
        }
        Ok(())
    }

    /// What exit_halt must do from a halted core
    fn exit_halt_ops(step: bool, maskints: bool) -> Vec<Op> {
        let masked = if maskints { C_MASKINTS } else { 0 };
        let mut ops = vec![Op::Read(DHCSR, S_HALT | C_HALT | C_DEBUGEN | masked)];
        let wanted = if step { C_MASKINTS } else { 0 };
        if maskints != step {
            ops.push(Op::Write(DHCSR, KEY | C_HALT | C_DEBUGEN | wanted));
        }
        let stepping = if step { C_STEP } else { 0 };
        ops.push(Op::Write(DFSR, DFSR_ALL));
        ops.push(Op::Write(DHCSR, KEY | C_DEBUGEN | wanted | stepping));
        ops
    }

    #[test]
    fn exit_halt_clears_dfsr_directly_before_the_release() {
        for step in [false, true] {
            for maskints in [false, true] {
                let ops = exit_halt_ops(step, maskints);
                recorded(&ops, |memory| exit_halt(memory, step));
                assert_eq!(
                    check_release_rule(&ops, false),
                    Ok(()),
                    "step {step} maskints {maskints}"
                );
            }
        }
    }

    #[test]
    fn clear_step_before_reset_leaves_dfsr_clear_on_every_path() {
        let stepped = C_DEBUGEN | C_STEP | C_MASKINTS;
        let halt_write = Op::Write(DHCSR, KEY | C_HALT | C_DEBUGEN);
        let clear = Op::Write(DFSR, DFSR_ALL);
        let paths = [
            (
                "neither bit",
                true,
                vec![Op::Read(DHCSR, S_HALT | C_HALT | C_DEBUGEN), clear],
            ),
            (
                "halt true",
                true,
                vec![Op::Read(DHCSR, stepped), halt_write, clear],
            ),
            (
                "halt false, halted",
                false,
                vec![
                    Op::Read(DHCSR, S_HALT | C_HALT | stepped),
                    halt_write,
                    clear,
                ],
            ),
            (
                "halt false, running",
                false,
                vec![
                    Op::Read(DHCSR, stepped),
                    halt_write,
                    clear,
                    Op::Write(DHCSR, KEY | C_DEBUGEN),
                    clear,
                ],
            ),
        ];
        for (path, halt, ops) in paths {
            recorded(&ops, |memory| clear_step_before_reset(memory, halt));
            assert_eq!(check_release_rule(&ops, true), Ok(()), "{path}");
        }
    }

    #[test]
    fn debug_core_stop_clears_dfsr_directly_before_the_dhcsr_write() {
        let ops = [
            Op::Write(DFSR, DFSR_ALL),
            Op::Write(DHCSR, KEY),
            Op::Write(DEMCR, 0),
        ];
        let sequence = DefaultArmSequence::create();
        recorded(&ops, |memory| {
            sequence.debug_core_stop(memory, CoreType::Armv7m)
        });
        assert_eq!(check_release_rule(&ops, false), Ok(()));
    }

    #[test]
    fn the_release_rule_fails_a_release_without_a_clear() {
        // exit_halt at 21198f15c with a C_MASKINTS change
        let ops = [
            Op::Read(DHCSR, S_HALT | C_HALT | C_DEBUGEN),
            Op::Write(DHCSR, KEY | C_HALT | C_DEBUGEN | C_MASKINTS),
            Op::Write(DHCSR, KEY | C_DEBUGEN | C_MASKINTS | C_STEP),
        ];
        assert!(check_release_rule(&ops, false).is_err());
    }

    #[test]
    fn the_release_rule_fails_a_halt_write_between_the_clear_and_the_release() {
        let ops = [
            Op::Read(DHCSR, S_HALT | C_HALT | C_DEBUGEN),
            Op::Write(DFSR, DFSR_ALL),
            Op::Write(DHCSR, KEY | C_HALT | C_DEBUGEN | C_MASKINTS),
            Op::Write(DHCSR, KEY | C_DEBUGEN | C_MASKINTS | C_STEP),
        ];
        assert!(check_release_rule(&ops, false).is_err());
    }

    #[test]
    fn the_release_rule_fails_a_clear_of_fewer_bits_and_an_end_maybe_set() {
        let partial = [Op::Write(DFSR, 0b0_1111), Op::Write(DHCSR, KEY | C_DEBUGEN)];
        assert!(check_release_rule(&partial, false).is_err());
        let released = [Op::Write(DFSR, DFSR_ALL), Op::Write(DHCSR, KEY | C_DEBUGEN)];
        assert_eq!(check_release_rule(&released, false), Ok(()));
        assert!(check_release_rule(&released, true).is_err());
    }

    #[test]
    fn is_bkpt_instruction_tells_bkpt_from_branch_to_self() {
        for (instruction, expected) in [
            (0xBE00, true),  // BKPT #0
            (0xBEAB, true),  // BKPT #0xAB (semihosting)
            (0xE7FE, false), // B . (branch to itself)
            (0xBF00, false), // NOP
        ] {
            assert_eq!(
                is_bkpt_instruction(instruction),
                expected,
                "{instruction:#06x}"
            );
        }
    }
}
