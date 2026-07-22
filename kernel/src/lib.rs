// kernel/src/lib.rs
#![no_std]
#![no_main]

extern crate alloc;

use linked_list_allocator::LockedHeap;

pub mod arch;
pub mod mm;
pub mod sync;
pub mod proc;
pub mod fs;
pub mod drivers;
pub mod syscall;
pub mod trap;

use core::panic::PanicInfo;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::console::printk_fmt(format_args!("KERNEL PANIC: {}\n", info));
    loop {
        riscv::asm::wfi();
    }
}

// Use a default allocator that never fails for now
// #[alloc_error_handler] - unstable, will use linked_list_allocator which handles OOM
// fn alloc_error(layout: core::alloc::Layout) -> ! {
//     panic!("Allocation failed: {:?}", layout);
// }

// _start is now in entry.S, this is just a placeholder
// arch_init_init is now in asm.S