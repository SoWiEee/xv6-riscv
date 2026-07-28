/* kernel/src/arch/entry.S - Assembly entry point (runs in M-mode) */
.section .text.entry
.global _start
.extern _stack_end
_start:
    /* Every hart enters here. Give each its own boot stack so they don't
       clobber each other's frames:  sp = _stack_end - mhartid * STACKSIZE,
       with STACKSIZE = 16384 = 1<<14. The .stack region in memory.x reserves
       NCPU * STACKSIZE for this. */
    csrr t0, mhartid
    la sp, _stack_end
    slli t1, t0, 14          /* t1 = mhartid * 16384 */
    sub sp, sp, t1

    /* Machine-mode setup, then mret into init() in S-mode. Never returns.
       (An early 'M' UART poke used to live here as a pre-Rust boot marker;
       dropped now that boot is stable so the first output is the banner.) */
    call mstart
    /* Should never get here */
2:  wfi
    j 2b
