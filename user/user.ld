/* user.ld - Linker script for user programs */

OUTPUT_ARCH(riscv)
ENTRY(main)

SECTIONS
{
    . = 0x10000;
    
    .text : {
        *(.text .text.*)
    }
    
    .rodata : {
        *(.rodata .rodata.*)
    }
    
    .data : {
        *(.data .data.*)
    }
    
    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    }
    
    /DISCARD/ : {
        *(.eh_frame)
        *(.eh_frame_hdr)
    }
}