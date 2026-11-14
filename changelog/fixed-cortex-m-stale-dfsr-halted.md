Cortex-M: clear DFSR before every release from halt, before a reset and at detach, so a watchpoint or vector catch after a run no longer reads as several halt reasons.
