/* kernel/src/arch/entry.S - Assembly entry point (runs in M-mode) */
.section .text.entry
.global _start
.extern _stack_end
_start:
    /* Set up a stack (shared for now; per-hart stacks come with SMP support) */
    la sp, _stack_end
    /* 'M' marks that we reached M-mode entry */
    li a0, 0x10000000
    li a1, 'M'
    sb a1, 0(a0)
    /* Machine-mode setup, then mret into init() in S-mode. Never returns. */
    call mstart
    /* Should never get here */
1:  wfi
    j 1b
