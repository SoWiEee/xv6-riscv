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

    /* Only hart 0 emits the 'M' M-mode boot marker to the UART. */
    bnez t0, 1f
    li a0, 0x10000000
    li a1, 'M'
    sb a1, 0(a0)
1:
    /* Machine-mode setup, then mret into init() in S-mode. Never returns. */
    call mstart
    /* Should never get here */
2:  wfi
    j 2b
