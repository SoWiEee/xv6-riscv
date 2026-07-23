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
        . += 16384;
        _stack_end = .;
    } > KERNEL
    
    _kernel_end = .;
}