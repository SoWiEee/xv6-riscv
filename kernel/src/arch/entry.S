/* kernel/src/arch/entry.S - Assembly entry point */
.section .text.entry
.global _start
.extern _stack_end
_start:
    /* Set up stack pointer */
    la sp, _stack_end
    /* Print 'A' to UART to verify we're running */
    li a0, 0x10000000
    li a1, 'A'
    sb a1, 0(a0)
    /* Initialize the allocator */
    call init_allocator
    /* Print 'B' after allocator */
    li a0, 0x10000000
    li a1, 'B'
    sb a1, 0(a0)
    /* Print 'C' before calling Rust init */
    li a0, 0x10000000
    li a1, 'C'
    sb a1, 0(a0)
    /* Call Rust init */
    call arch_init_init
    /* Print 'Z' after init returns (should never happen) */
    li a0, 0x10000000
    li a1, 'Z'
    sb a1, 0(a0)

.section .text
.global init_allocator
init_allocator:
    /* Initialize the linked_list_allocator heap */
    /* This is a stub - real implementation would set up the heap */
    ret