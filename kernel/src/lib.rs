// kernel/src/lib.rs
#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod arch;
pub mod mm;
pub mod sync;
pub mod proc;
pub mod fs;
pub mod drivers;
pub mod syscall;
pub mod trap;
pub mod elf;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::console::printk_fmt(format_args!("KERNEL PANIC: {}\n", info));
    loop {
        riscv::asm::wfi();
    }
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Allocation failed: {:?}", layout);
}

// _start is now in entry.S, this is just a placeholder
// arch_init_init is now in asm.S