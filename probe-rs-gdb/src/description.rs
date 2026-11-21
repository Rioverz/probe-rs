//! GDB target description and memory map of a probe-rs core

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt::Write;

use probe_rs::config::MemoryRegion;
use probe_rs::{
    CoreRegister, CoreRegisters, CoreType, InstructionSet, RegisterId, Target, architecture,
};

/// GDB feature
struct GdbFeature {
    name: &'static str,
    reg_count: usize,
}

/// Source of a GDB register view
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegisterSource {
    /// One probe-rs register as one GDB register
    SingleRegister(RegisterId),
    /// Two probe-rs registers as one GDB register
    TwoWordRegister { low: RegisterId, high: RegisterId },
    /// In GDB layout but unreadable on target
    Unavailable,
}

/// Register described to GDB
pub struct DescribedRegister {
    name: String,
    size: usize,
    kind: &'static str,
    source: RegisterSource,
}

/// GDB target description and register info
pub struct TargetDescription {
    arch: &'static str,
    features: Vec<GdbFeature>,
    regs: Vec<DescribedRegister>,
}

impl TargetDescription {
    /// Create description for CPU type and instruction set
    fn new(core_type: CoreType, isa: InstructionSet) -> Self {
        let arch = match core_type {
            CoreType::Armv6m => "armv6-m",
            CoreType::Armv7a | CoreType::Armv7r | CoreType::Armv7m => "armv7",
            CoreType::Armv7em => "armv7e-m",
            CoreType::Armv8a => match isa {
                InstructionSet::A64 => "aarch64",
                _ => "armv8-a",
            },
            CoreType::Armv8m => "armv8-m.main",
            CoreType::Riscv => "riscv:rv32",
            CoreType::Riscv64 => "riscv:rv64",
            CoreType::Xtensa => "xtensa",
        };

        Self {
            arch,
            features: vec![],
            regs: vec![],
        }
    }

    /// Build target XML for GDB
    pub fn target_xml(&self) -> String {
        let mut target_description = r#"<?xml version="1.0"?>
        <!DOCTYPE target SYSTEM "gdb-target.dtd">
        <target version="1.0">
        "#
        .to_owned();

        let _ = write!(
            target_description,
            "<architecture>{}</architecture>",
            self.arch
        );

        let mut reg_start = 0usize;

        for feature in &self.features {
            let _ = write!(target_description, "<feature name='{}'>", feature.name);

            for reg in self.regs.iter().skip(reg_start).take(feature.reg_count) {
                let _ = write!(
                    target_description,
                    "<reg name='{}' bitsize='{}' type='{}'/>",
                    reg.name.to_lowercase(),
                    reg.size,
                    reg.kind
                );
            }

            reg_start += feature.reg_count;

            target_description.push_str("</feature>");
        }

        target_description.push_str("</target>");

        target_description
    }

    /// Registers in GDB order
    pub fn registers(&self) -> &[DescribedRegister] {
        &self.regs
    }

    /// Add GDB feature
    fn add_gdb_feature(&mut self, name: &'static str) {
        self.features.push(GdbFeature { name, reg_count: 0 });
    }

    // Add register to current GDB feature (left out when none open)
    fn push(&mut self, name: String, size: usize, kind: &'static str, source: RegisterSource) {
        if let Some(feature) = self.features.last_mut() {
            feature.reg_count += 1;
            self.regs.push(DescribedRegister {
                name,
                size,
                kind,
                source,
            });
        }
    }

    /// Add register to current GDB feature
    fn add_register(&mut self, reg: &CoreRegister) {
        let id: RegisterId = reg.into();

        self.add_register_from_details(reg.name().to_owned(), reg.size_in_bits(), id);
    }

    /// Add register by name, size and id (size without GDB type left out)
    fn add_register_from_details(&mut self, name: impl Into<String>, size: usize, id: RegisterId) {
        if let Some(kind) = size_to_type(size) {
            self.push(name.into(), size, kind, RegisterSource::SingleRegister(id));
        }
    }

    /// Add placeholder for unreadable register
    fn add_unavailable_register(&mut self, name: impl Into<String>, size: usize) {
        if let Some(kind) = size_to_type(size) {
            self.push(name.into(), size, kind, RegisterSource::Unavailable);
        }
    }

    /// Add registers to current GDB feature
    fn add_registers<'a>(&mut self, regs: impl Iterator<Item = &'a CoreRegister>) {
        for reg in regs {
            self.add_register(reg);
        }
    }

    /// Add registers merged in pairs into one GDB view
    ///
    /// s0 to s3 become d0 (s0 and s1) and d1 (s2 and s3)
    fn add_two_word_registers<'a>(
        &mut self,
        regs: impl Iterator<Item = &'a CoreRegister>,
        name_pattern: &'static str,
        reg_type: &'static str,
    ) {
        let regs: Vec<_> = regs.collect();
        for (i, &[first_reg, second_reg]) in regs.as_chunks::<2>().0.iter().enumerate() {
            let first_id: RegisterId = first_reg.into();
            let second_id: RegisterId = second_reg.into();

            self.push(
                format!("{name_pattern}{i}"),
                first_reg.size_in_bits() * 2,
                reg_type,
                RegisterSource::TwoWordRegister {
                    low: first_id,
                    high: second_id,
                },
            );
        }
    }

    /// Rename register
    fn update_register_name(&mut self, old_name: &'static str, new_name: &'static str) {
        for reg in &mut self.regs {
            if reg.name == old_name {
                new_name.clone_into(&mut reg.name);
            }
        }
    }

    /// Set register type
    fn update_register_type(&mut self, name: &'static str, new_type: &'static str) {
        for reg in &mut self.regs {
            if reg.name == name {
                reg.kind = new_type;
            }
        }
    }
}

fn size_to_type(size: usize) -> Option<&'static str> {
    match size {
        32 => Some("uint32"),
        64 => Some("uint64"),
        128 => Some("uint128"),
        _ => None,
    }
}

/// Describe the registers of a core to GDB
pub fn build_target_description(
    regs: &CoreRegisters,
    core_type: CoreType,
    isa: InstructionSet,
) -> TargetDescription {
    let mut desc = TargetDescription::new(core_type, isa);

    // Build main register group
    match core_type {
        CoreType::Armv6m | CoreType::Armv7em | CoreType::Armv7m | CoreType::Armv8m => {
            build_cortex_m_registers(&mut desc, regs);
        }
        CoreType::Armv7a | CoreType::Armv7r => build_aarch32_registers(&mut desc, regs),
        CoreType::Armv8a => match isa {
            InstructionSet::A64 => build_aarch64_registers(&mut desc, regs),
            // Other ISAs described as A32 (upstream panics)
            _ => build_aarch32_registers(&mut desc, regs),
        },
        CoreType::Riscv | CoreType::Riscv64 => build_riscv_registers(&mut desc, regs),
        CoreType::Xtensa => build_xtensa_registers(&mut desc, regs),
    }

    desc
}

/// RISC-V FPU control registers probe-rs lists as core registers
const RISCV_FP_CSR_NAMES: [&str; 2] = ["fflags", "frm"];

fn build_riscv_registers(desc: &mut TargetDescription, regs: &CoreRegisters) {
    let is_csr = |r: &&CoreRegister| {
        r.dwarf_id()
            .is_some_and(|id| id >= architecture::riscv::registers::DWARF_CSR_BASE)
    };
    // fflags and frm have a core role only, GDB reads them in FPU feature
    let is_fp_csr = |r: &&CoreRegister| RISCV_FP_CSR_NAMES.contains(&r.name());

    // Create main register group
    desc.add_gdb_feature("org.gnu.gdb.riscv.cpu");
    // GDB numbers x0-x31, pc and FPU by position in these features
    // Core registers hold the core pc (upstream adds a 32-bit one)
    desc.add_registers(
        regs.core_registers()
            .filter(|r| !is_csr(r) && !is_fp_csr(r)),
    );

    if let (Some(fpu), Some(status)) = (regs.fpu_registers(), regs.fpu_status_registers()) {
        desc.add_gdb_feature("org.gnu.gdb.riscv.fpu");
        desc.add_registers(fpu);
        desc.add_registers(status);
        desc.add_registers(regs.core_registers().filter(is_fp_csr));
    }

    desc.add_gdb_feature("org.gnu.gdb.riscv.csr");
    desc.add_registers(regs.core_registers().filter(is_csr));

    desc.update_register_type("pc", "code_ptr");
}

fn build_aarch64_registers(desc: &mut TargetDescription, regs: &CoreRegisters) {
    // Create main register group
    desc.add_gdb_feature("org.gnu.gdb.aarch64.core");
    desc.add_registers(regs.core_registers());
    if let Some(psr) = regs.psr() {
        desc.add_register(psr);
    }

    // AArch64 always has FP support
    desc.add_gdb_feature("org.gnu.gdb.aarch64.fpu");
    if let Some(fpu) = regs.fpu_registers() {
        desc.add_registers(fpu);
    }
    if let Some(control) = regs.other_by_name("Floating Point Control") {
        desc.add_register(control);
    }
    if let Some(fpsr) = regs.fpsr() {
        desc.add_register(fpsr);
    }

    // GDB expects PSTATE named CPSR (old v7 name)
    desc.update_register_name("PSTATE", "CPSR");

    desc.update_register_type("SP", "data_ptr");
    desc.update_register_type("PC", "code_ptr");
}

fn build_aarch32_registers(desc: &mut TargetDescription, regs: &CoreRegisters) {
    // Create main register group
    desc.add_gdb_feature("org.gnu.gdb.arm.core");
    desc.add_registers(regs.core_registers());
    if let Some(psr) = regs.psr() {
        desc.add_register(psr);
    }

    if let (Some(msp), Some(psp)) = (regs.msp(), regs.psp()) {
        // Optional m-system extension
        desc.add_gdb_feature("org.gnu.gdb.arm.m-system");
        desc.add_register(msp);
        desc.add_register(psp);
    }

    if let (Some(fpsr), Some(fpu)) = (regs.fpsr(), regs.fpu_registers()) {
        desc.add_gdb_feature("org.gnu.gdb.arm.vfp");
        desc.add_registers(fpu);
        desc.add_register(fpsr);
    }

    // Rename registers as GDB expects
    desc.update_register_name("R13", "SP");
    desc.update_register_name("R14", "LR");
    desc.update_register_name("R15", "PC");

    desc.update_register_type("SP", "data_ptr");
    desc.update_register_type("PC", "code_ptr");
}

fn build_cortex_m_registers(desc: &mut TargetDescription, regs: &CoreRegisters) {
    // Create main register group
    desc.add_gdb_feature("org.gnu.gdb.arm.m-profile");
    desc.add_registers(regs.core_registers());
    if let Some(psr) = regs.psr() {
        desc.add_register(psr);
    }

    if let (Some(msp), Some(psp)) = (regs.msp(), regs.psp()) {
        // Optional m-system extension
        desc.add_gdb_feature("org.gnu.gdb.arm.m-system");
        desc.add_register(msp);
        desc.add_register(psp);
    }

    if let (Some(fpsr), Some(fpu)) = (regs.fpsr(), regs.fpu_registers()) {
        desc.add_gdb_feature("org.gnu.gdb.arm.vfp");
        // probe-rs exposes single word registers s0-s31
        // GDB needs double word registers d0-d15
        // Each d register from two consecutive s registers
        desc.add_two_word_registers(fpu, "d", "ieee_double");
        desc.add_register(fpsr);
    }

    // Rename registers as GDB expects
    desc.update_register_name("R13", "SP");
    desc.update_register_name("R14", "LR");
    desc.update_register_name("R15", "PC");

    desc.update_register_type("SP", "data_ptr");
    desc.update_register_type("PC", "code_ptr");
}

fn build_xtensa_registers(desc: &mut TargetDescription, regs: &CoreRegisters) {
    // Xtensa GDB uses compiled-in register layout (no XML features)
    // Match exact register order and count of xtensa-*-elf-gdb
    // ESP32-class layout (contiguous format, 64 address registers)
    //
    // probe-rs Xtensa RegisterId encoding
    //   CPU register N:     RegisterId(N) for N 0 to 15
    //   Special register N: RegisterId(0x0100 | N)
    //   FP register N:      RegisterId(0x0200 | N)
    //   User register N:    RegisterId(0x0300 | N)
    //   Current PC:         RegisterId(0xFF00)
    //   Current PS:         RegisterId(0xFF01)
    let cpu = |n: u16| -> RegisterId { RegisterId(n) };
    let sr = |n: u16| -> RegisterId { RegisterId(0x0100 | n) };
    let fr = |n: u16| -> RegisterId { RegisterId(0x0200 | n) };
    let ur = |n: u16| -> RegisterId { RegisterId(0x0300 | n) };
    let program_counter = RegisterId(0xFF00);
    let processor_status = RegisterId(0xFF01);

    let has_fpu = regs.fpu_registers().is_some();

    desc.add_gdb_feature("org.gnu.gdb.xtensa.core");

    // Register 0 pc
    desc.add_register_from_details("pc", 32, program_counter);

    // Registers 1-16 ar0-ar15 (current window, from CPU a0-a15)
    for i in 0..16u16 {
        desc.add_register_from_details(format!("ar{i}"), 32, cpu(i));
    }

    // Registers 17-64 ar16-ar63 (physical, outside current window)
    for i in 16..64 {
        desc.add_unavailable_register(format!("ar{i}"), 32);
    }

    // Registers 65-68 Loop and Shift
    desc.add_register_from_details("lbeg", 32, sr(0));
    desc.add_register_from_details("lend", 32, sr(1));
    desc.add_register_from_details("lcount", 32, sr(2));
    desc.add_register_from_details("sar", 32, sr(3));

    // Registers 69-70 Window Control
    // Windowbase reported as 0 (only current window known, at ar0-ar15)
    // Real windowbase makes GDB read unavailable ar[windowbase*4..]
    desc.add_unavailable_register("windowbase", 32);
    desc.add_register_from_details("windowstart", 32, sr(73));

    // Registers 71-72 Config IDs (read-only silicon config, unavailable)
    desc.add_unavailable_register("configid0", 32);
    desc.add_unavailable_register("configid1", 32);

    // Register 73 Processor Status
    desc.add_register_from_details("ps", 32, processor_status);

    // Register 74 Thread Pointer (user register, not standard SR)
    desc.add_unavailable_register("threadptr", 32);

    // Register 75 Boolean Register File
    desc.add_register_from_details("br", 32, sr(4));

    // Register 76 Conditional Store Compare
    desc.add_register_from_details("scompare1", 32, sr(12));

    // Registers 77-78 MAC16 Accumulator
    desc.add_register_from_details("acclo", 32, sr(16));
    desc.add_register_from_details("acchi", 32, sr(17));

    // Registers 79-82 MAC16 Operand Registers
    desc.add_register_from_details("m0", 32, sr(32));
    desc.add_register_from_details("m1", 32, sr(33));
    desc.add_register_from_details("m2", 32, sr(34));
    desc.add_register_from_details("m3", 32, sr(35));

    // Register 83 GPIO and Trace State (Espressif-specific)
    desc.add_unavailable_register("expstate", 32);

    // Registers 84-86 Double-Precision FPU State
    desc.add_unavailable_register("f64r_lo", 32);
    desc.add_unavailable_register("f64r_hi", 32);
    desc.add_unavailable_register("f64s", 32);

    // Registers 87-102 Single-Precision FPU Registers
    for i in 0..16u16 {
        if has_fpu {
            desc.add_register_from_details(format!("f{i}"), 32, fr(i));
        } else {
            desc.add_unavailable_register(format!("f{i}"), 32);
        }
    }

    // Registers 103-104 FPU Control and Status
    if has_fpu {
        desc.add_register_from_details("fcr", 32, ur(232));
        desc.add_register_from_details("fsr", 32, ur(233));
    } else {
        desc.add_unavailable_register("fcr", 32);
        desc.add_unavailable_register("fsr", 32);
    }

    // Register 105 Memory Management ID
    desc.add_unavailable_register("mmid", 32);

    // Registers 106-109 Debug and Memory Control
    desc.add_register_from_details("ibreakenable", 32, sr(96));
    desc.add_register_from_details("memctl", 32, sr(97));
    desc.add_register_from_details("atomctl", 32, sr(99));
    desc.add_register_from_details("ddr", 32, sr(104));

    // Registers 110-111 Instruction Breakpoint Addresses
    desc.add_register_from_details("ibreaka0", 32, sr(128));
    desc.add_register_from_details("ibreaka1", 32, sr(129));

    // Registers 112-115 Data Breakpoint Addresses and Control
    desc.add_register_from_details("dbreaka0", 32, sr(144));
    desc.add_register_from_details("dbreaka1", 32, sr(145));
    desc.add_register_from_details("dbreakc0", 32, sr(160));
    desc.add_register_from_details("dbreakc1", 32, sr(161));

    // Registers 116-122 Exception Program Counters
    for i in 1..=7u16 {
        desc.add_register_from_details(format!("epc{i}"), 32, sr(176 + i));
    }

    // Register 123 Double Exception Program Counter
    desc.add_register_from_details("depc", 32, sr(192));

    // Registers 124-129 Exception Processor Status
    for i in 2..=7u16 {
        desc.add_register_from_details(format!("eps{i}"), 32, sr(192 + i));
    }

    // Registers 130-136 Exception Save Registers
    for i in 1..=7u16 {
        desc.add_register_from_details(format!("excsave{i}"), 32, sr(208 + i));
    }

    // Register 137 Coprocessor Enable
    desc.add_register_from_details("cpenable", 32, sr(224));

    // Registers 138-139 Interrupt Status (both read SR 226)
    desc.add_register_from_details("interrupt", 32, sr(226));
    desc.add_register_from_details("intset", 32, sr(226));

    // Register 140 Interrupt Clear (write-only)
    desc.add_unavailable_register("intclear", 32);

    // Register 141 Interrupt Enable
    desc.add_register_from_details("intenable", 32, sr(228));

    // Registers 142-149 Exception and Debug State
    desc.add_register_from_details("vecbase", 32, sr(231));
    desc.add_register_from_details("exccause", 32, sr(232));
    desc.add_register_from_details("debugcause", 32, sr(233));
    desc.add_register_from_details("ccount", 32, sr(234));
    desc.add_register_from_details("prid", 32, sr(235));
    desc.add_register_from_details("icount", 32, sr(236));
    desc.add_register_from_details("icountlevel", 32, sr(237));
    desc.add_register_from_details("excvaddr", 32, sr(238));

    // Registers 150-152 Cycle Comparators
    desc.add_register_from_details("ccompare0", 32, sr(240));
    desc.add_register_from_details("ccompare1", 32, sr(241));
    desc.add_register_from_details("ccompare2", 32, sr(242));

    // Registers 153-156 Miscellaneous
    desc.add_register_from_details("misc0", 32, sr(244));
    desc.add_register_from_details("misc1", 32, sr(245));
    desc.add_register_from_details("misc2", 32, sr(246));
    desc.add_register_from_details("misc3", 32, sr(247));

    desc.update_register_type("pc", "code_ptr");
}

impl DescribedRegister {
    /// Size in bits
    pub fn size(&self) -> usize {
        self.size
    }

    /// Where GDB reads the register from
    pub fn source(&self) -> RegisterSource {
        self.source
    }
}

#[derive(Ord, PartialOrd, Eq, PartialEq)]
struct FlashRegion {
    start: u64,
    length: u64,
    blocksize: u64,
}

/// Whole address space as RAM (any virtual address may be valid)
fn full_ram_memory_map(address_size: usize) -> String {
    let length = match address_size {
        64 => u64::MAX,
        _ => u64::from(u32::MAX),
    };

    format!(
        r#"<?xml version="1.0"?>
<!DOCTYPE memory-map PUBLIC "+//IDN gnu.org//DTD GDB Memory Map V1.0//EN" "http://sourceware.org/gdb/gdb-memory-map.dtd">
<memory-map>
<memory type="ram" start="0x0" length="{length:#x}"/>
</memory-map>"#
    )
}

/// Compute GDB memory map for a target and primary core
pub fn gdb_memory_map(target: &Target, core_type: CoreType, isa: InstructionSet) -> String {
    // Address size from core type (upstream reads live PC size)
    let address_size = if matches!(
        (core_type, isa),
        (CoreType::Armv8a, InstructionSet::A64) | (CoreType::Riscv64, _)
    ) {
        64
    } else {
        32
    };

    // Cortex-A cores use virtual addressing (any address may be valid)
    if matches!(core_type, CoreType::Armv7a | CoreType::Armv8a) {
        return full_ram_memory_map(address_size);
    }

    let sectors = flash_sectors(target);
    if target.memory_map.is_empty() && sectors.is_empty() {
        return full_ram_memory_map(address_size);
    }

    let mut xml_map = r#"<?xml version="1.0"?>
<!DOCTYPE memory-map PUBLIC "+//IDN gnu.org//DTD GDB Memory Map V1.0//EN" "http://sourceware.org/gdb/gdb-memory-map.dtd">
<memory-map>
"#.to_owned();

    let has_flash = !sectors.is_empty();
    for region in &target.memory_map {
        let region_kind = match region {
            MemoryRegion::Ram(_) => "ram",
            MemoryRegion::Generic(_) => "rom",
            MemoryRegion::Nvm(_) => {
                if has_flash {
                    continue;
                }
                "rom"
            }
        };
        let range = region.address_range();
        let start = range.start;
        let length = range.end - range.start;
        let region_entry =
            format!(r#"<memory type="{region_kind}" start="{start:#x}" length="{length:#x}"/>\n"#);

        xml_map.push_str(&region_entry);
    }

    // Print deduplicated flash regions (GDB fails on overlapping regions)
    for FlashRegion {
        start,
        length,
        blocksize,
    } in &sectors
    {
        let region_entry = format!(
            r#"<memory type="flash" start="{start:#x}" length="{length:#x}"><property name="blocksize">{blocksize:#x}</property></memory>\n"#
        );
        xml_map.push_str(&region_entry);
    }

    xml_map.push_str("</memory-map>");

    xml_map
}

/// Deduplicated absolute flash sectors, clipped to NVM regions
/// (algorithm range may pass the flash the target has)
fn flash_sectors(target: &Target) -> Vec<FlashRegion> {
    let mut regions = BTreeMap::new();
    for algo in &target.flash_algorithms {
        let start = algo.flash_properties.address_range.start;
        let end = algo.flash_properties.address_range.end;

        let windows = target
            .memory_map
            .iter()
            .filter_map(|region| match region {
                MemoryRegion::Nvm(nvm) => {
                    let window = nvm.range.start.max(start)..nvm.range.end.min(end);
                    (window.start < window.end).then_some(window)
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut sectors = algo.flash_properties.sectors.clone();
        sectors.sort_by_key(|s| s.address);
        // Add end dummy sector (gives length of last real sector)
        sectors.push(probe_rs::config::SectorDescription {
            size: 0,
            address: end - start,
        });
        // Pair sectors (next sector start gives current sector size)
        for (current, next) in sectors.iter().zip(sectors.iter().skip(1)) {
            for window in &windows {
                // Algorithm sector addresses are relative to region base
                // Absolute address as GDB expects (vFlashErase, vFlashWrite)
                let sector_start = (start + current.address).max(window.start);
                let sector_end = (start + next.address).min(window.end);
                if sector_start >= sector_end {
                    continue;
                }
                let region = FlashRegion {
                    start: sector_start,
                    length: sector_end - sector_start,
                    blocksize: current.size,
                };
                // Duplicate regions keep smaller block size
                match regions.entry(region.start) {
                    Entry::Vacant(e) => {
                        e.insert(region);
                    }
                    Entry::Occupied(mut e) => {
                        if region.blocksize < e.get().blocksize {
                            e.insert(region);
                        }
                    }
                }
            }
        }
    }
    regions.into_values().collect()
}
