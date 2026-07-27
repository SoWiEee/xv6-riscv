/* kernel/memory.x */
MEMORY
{
    KERNEL : ORIGIN = 0x80000000, LENGTH = 128M
}

ENTRY(_start)

SECTIONS
{
    .text : {
        *(.text.entry)
        *(.text)
        *(.text.*)
        *(.rodata)
        *(.rodata.*)
        /* Page-align the text/data boundary so the kernel can map [KERNBASE,
           etext) read-execute and the rest read-write without a shared page. */
        . = ALIGN(4096);
    } > KERNEL

    etext = .;
    
    .data : {
        *(.data)
        *(.data.*)
    } > KERNEL
    
    .bss : {
        *(.bss)
        *(.bss.*)
        *(.sbss)
        *(.sbss.*)
        *(COMMON)
        *(.eh_frame)
        *(.eh_frame.*)
    } > KERNEL
    
    end = .;
    
    .stack (NOLOAD) : {
        . = ALIGN(16);
        _stack_start = .;
        /* One 16 KiB boot stack per hart (NCPU = 8). entry.S slices this by
           mhartid; the frame allocator starts at _kernel_end, past this region,
           so these stacks are never handed out as free pages. */
        . += 16384 * 8;
        _stack_end = .;
    } > KERNEL
    
    _kernel_end = .;
}