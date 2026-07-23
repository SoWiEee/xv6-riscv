// kernel/src/arch/init.rs
use super::asm::*;
use super::interrupt::*;
use crate::mm::frame_allocator::kinit;
use crate::proc::scheduler::procinit;
use crate::arch::console::consoleinit;
use crate::mm::page_table::{kvminit, kvminithart};
use crate::mm::address::PhysAddr;
use crate::proc::scheduler::scheduler;
use core::sync::atomic::{AtomicBool, Ordering};

static STARTED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
pub extern "C" fn init() -> ! {
    let hart_id = r_tp();
    
    // All harts wait here until hart 0 signals
    if hart_id != 0 {
        while !started() {
            core::hint::spin_loop();
        }
    }
    
    if hart_id == 0 {
        // Hart 0 does full initialization
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
        STARTED.store(true, Ordering::SeqCst);
    }
    
    // All non-zero harts initialize their page tables and traps
    if hart_id != 0 {
        kvminithart();
        trapinit();
        plic_init_hart();
    }
    
    scheduler();
}

fn started() -> bool {
    STARTED.load(Ordering::SeqCst)
}

pub fn trapinit() {
    // Set up trap vector for kernel mode
    w_stvec(super::trap::kernelvec_addr());
    // Set initial timer interrupt
    w_stimecmp(r_time() + 1000000);
}