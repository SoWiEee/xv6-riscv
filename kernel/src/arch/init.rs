// kernel/src/arch/init.rs
use super::asm::*;
use super::interrupt::*;
use crate::mm::frame_allocator::kinit;
use crate::proc::scheduler::procinit;
use crate::arch::console::consoleinit;
use crate::mm::page_table::{kvminit, kvminithart};
use crate::mm::address::PhysAddr;
use crate::proc::scheduler::scheduler;

static mut STARTED: bool = false;

#[unsafe(no_mangle)]
pub extern "C" fn init() -> ! {
    let hart_id = r_tp();
    if hart_id == 0 {
        consoleinit();
        crate::arch::console::printk(format_args!("\nxv6-rust kernel is booting\n\n"));
        // Get physical memory range from linker script
        unsafe extern "C" {
            fn end();
        }
        let start = end as usize;
        let end_addr = crate::arch::asm::PHYSTOP;
        
        // Initialize frame allocator first so kvminit can allocate pages
        kinit(
            PhysAddr::new(start),
            PhysAddr::new(end_addr),
        );
        
        // Initialize kernel page table
        kvminit();
        kvminithart();
        
        procinit();
        trapinit();
        plic_init();
        plic_init_hart();
        crate::drivers::virtio::virtio_init();
        crate::fs::iinit();
        crate::fs::fileinit();
        crate::proc::userinit();
        
        // Signal other harts
        unsafe { STARTED = true; }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    } else {
        // Wait for hart 0
        while !started() {
            core::hint::spin_loop();
        }
        kvminithart();
        trapinit();
        plic_init_hart();
    }
    
    scheduler();
}

fn started() -> bool {
    unsafe { core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst); STARTED }
}

pub fn trapinit() {
    // Set up trap vector for kernel mode
    w_stvec(super::trap::kernelvec_addr());
}