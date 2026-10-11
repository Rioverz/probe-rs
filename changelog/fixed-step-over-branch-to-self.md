Fixed stepping or resuming from a cleared breakpoint on a branch to itself (`b .`) on ARMv6-M, ARMv7-M and ARMv8-M: the PC was advanced by 2 as if over a `BKPT`, skipping an instruction.
