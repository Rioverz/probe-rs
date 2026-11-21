//! GDB descriptions of every probe-rs register set and an empty map

use probe_rs::architecture::arm::core::registers::cortex_m::{
    CORTEX_M_CORE_REGISTERS, CORTEX_M_WITH_FP_CORE_REGISTERS,
};
use probe_rs::architecture::arm::core::registers::{aarch32, aarch64, armv8m};
use probe_rs::architecture::riscv::registers::{
    RISCV_CORE_REGISTERS, RISCV_RV32E_CORE_REGISTERS, RISCV_WITH_FP_CORE_REGISTERS,
};
use probe_rs::architecture::riscv::registers64::{
    RISCV64_CORE_REGISTERS, RISCV64_WITH_FP_CORE_REGISTERS,
};
use probe_rs::architecture::xtensa::registers::{
    XTENSA_CORE_REGISTERS, XTENSA_WITH_FP_CORE_REGISTERS,
};
use probe_rs::config::Registry;
use probe_rs::{
    CoreRegister, CoreRegisters, CoreType, InstructionSet, RegisterDataType, RegisterId,
    RegisterRole, UnwindRule, Xlen,
};
use probe_rs_gdb::{build_target_description, gdb_memory_map};

const RV32: InstructionSet = InstructionSet::Riscv {
    xlen: Xlen::X32,
    c: true,
    f: false,
    e: false,
};

const RV32F: InstructionSet = InstructionSet::Riscv {
    xlen: Xlen::X32,
    c: true,
    f: true,
    e: false,
};

const RV64: InstructionSet = InstructionSet::Riscv {
    xlen: Xlen::X64,
    c: true,
    f: false,
    e: false,
};

const RV64F: InstructionSet = InstructionSet::Riscv {
    xlen: Xlen::X64,
    c: true,
    f: true,
    e: false,
};

const WIDE_BITS: usize = 96;

// A register of a size GDB has no type for
static WIDE: CoreRegister = CoreRegister {
    id: RegisterId(0x7F00),
    dwarf_id: None,
    roles: &[RegisterRole::Core("wide")],
    data_type: RegisterDataType::UnsignedInteger(WIDE_BITS),
    unwind_rule: UnwindRule::Clear,
};

// Each static register set with its core types and ISAs
fn register_cases() -> Vec<(&'static CoreRegisters, CoreType, InstructionSet)> {
    let cortex_m = [CoreType::Armv6m, CoreType::Armv7m, CoreType::Armv7em];
    let aarch32_cores = [
        (CoreType::Armv7a, InstructionSet::A32),
        (CoreType::Armv7r, InstructionSet::A32),
        (CoreType::Armv8a, InstructionSet::A32),
        (CoreType::Armv8a, InstructionSet::Thumb2),
    ];
    let mut cases = Vec::new();
    for set in [&*CORTEX_M_CORE_REGISTERS, &*CORTEX_M_WITH_FP_CORE_REGISTERS] {
        cases.extend(cortex_m.map(|core| (set, core, InstructionSet::Thumb2)));
    }
    for set in [
        &*armv8m::V8M_BASE_SEC_REGISTERS,
        &*armv8m::V8M_BASE_SEC_FP_REGISTERS,
        &*armv8m::V8M_MAIN_REGISTERS,
        &*armv8m::V8M_MAIN_FP_REGISTERS,
        &*armv8m::V8M_MAIN_SEC_REGISTERS,
        &*armv8m::V8M_MAIN_SEC_FP_REGISTERS,
    ] {
        cases.push((set, CoreType::Armv8m, InstructionSet::Thumb2));
    }
    for set in [
        &*aarch32::AARCH32_CORE_REGISTERS,
        &*aarch32::AARCH32_WITH_FP_16_CORE_REGISTERS,
        &*aarch32::AARCH32_WITH_FP_32_CORE_REGISTERS,
    ] {
        cases.extend(aarch32_cores.map(|(core, isa)| (set, core, isa)));
    }
    let aarch64 = &*aarch64::AARCH64_CORE_REGISTERS;
    cases.push((aarch64, CoreType::Armv8a, InstructionSet::A64));
    cases.push((aarch64, CoreType::Armv8a, InstructionSet::Thumb2));
    cases.extend([
        (&*RISCV_RV32E_CORE_REGISTERS, CoreType::Riscv, RV32),
        (&*RISCV_CORE_REGISTERS, CoreType::Riscv, RV32),
        (&*RISCV_WITH_FP_CORE_REGISTERS, CoreType::Riscv, RV32F),
        (&*RISCV64_CORE_REGISTERS, CoreType::Riscv64, RV64),
        (&*RISCV64_WITH_FP_CORE_REGISTERS, CoreType::Riscv64, RV64F),
        (
            &*XTENSA_CORE_REGISTERS,
            CoreType::Xtensa,
            InstructionSet::Xtensa,
        ),
        (
            &*XTENSA_WITH_FP_CORE_REGISTERS,
            CoreType::Xtensa,
            InstructionSet::Xtensa,
        ),
    ]);
    cases
}

fn reg_elements(target_xml: &str) -> usize {
    target_xml.matches("<reg ").count()
}

#[test]
fn every_register_set_lists_each_described_register_once() {
    for (set, core_type, isa) in register_cases() {
        let description = build_target_description(set, core_type, isa);
        let target_xml = description.target_xml();
        assert!(!description.registers().is_empty(), "{core_type:?} {isa:?}");
        assert_eq!(
            description.registers().len(),
            reg_elements(&target_xml),
            "{core_type:?} {isa:?}"
        );
    }
}

#[test]
fn a_register_with_no_gdb_type_is_left_out() {
    for (set, core_type, isa) in register_cases() {
        let mut registers: Vec<&'static CoreRegister> = set.all_registers().collect();
        registers.push(&WIDE);
        let wide = CoreRegisters::new(registers);
        let plain = build_target_description(set, core_type, isa);
        let description = build_target_description(&wide, core_type, isa);
        let target_xml = description.target_xml();
        assert!(!target_xml.contains("wide"), "{core_type:?} {isa:?}");
        assert_eq!(target_xml, plain.target_xml(), "{core_type:?} {isa:?}");
        assert_eq!(description.registers().len(), reg_elements(&target_xml));
        assert!(
            description
                .registers()
                .iter()
                .all(|reg| reg.size() != WIDE_BITS)
        );
    }
}

#[test]
fn an_armv8a_core_with_thumb2_is_described_as_a32() {
    for set in [
        &*aarch32::AARCH32_CORE_REGISTERS,
        &*aarch64::AARCH64_CORE_REGISTERS,
    ] {
        let thumb = build_target_description(set, CoreType::Armv8a, InstructionSet::Thumb2);
        let a32 = build_target_description(set, CoreType::Armv8a, InstructionSet::A32);
        let target_xml = thumb.target_xml();
        assert!(target_xml.contains("<architecture>armv8-a</architecture>"));
        assert_eq!(target_xml, a32.target_xml());
        let sources: Vec<_> = thumb.registers().iter().map(|reg| reg.source()).collect();
        let a32_sources: Vec<_> = a32.registers().iter().map(|reg| reg.source()).collect();
        assert_eq!(sources, a32_sources);
    }
}

#[test]
fn a_target_with_no_regions_is_all_ram_at_its_width() {
    let mut target = Registry::from_builtin_families()
        .get_target_by_name("Cortex-M3")
        .unwrap();
    target.memory_map.clear();
    target.flash_algorithms.clear();
    let ram = |length: &str| format!(r#"<memory type="ram" start="0x0" length="{length}"/>"#);
    let narrow = gdb_memory_map(&target, CoreType::Armv7m, InstructionSet::Thumb2);
    assert!(narrow.contains(&ram("0xffffffff")), "{narrow}");
    let wide = gdb_memory_map(&target, CoreType::Riscv64, RV64);
    assert!(wide.contains(&ram("0xffffffffffffffff")), "{wide}");
    assert_eq!(narrow.matches("<memory ").count(), 1);
    assert_eq!(wide.matches("<memory ").count(), 1);
}
