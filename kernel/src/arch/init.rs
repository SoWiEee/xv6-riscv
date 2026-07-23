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
        crate::arch::console::printk(format_args!("after kvminit, before kvminithart\n"));
        kvminithart();
        
        procinit();
        crate::arch::console::printk(format_args!("before trapinit\n"));
        trapinit();
        crate::arch::console::printk(format_args!("after trapinit\n"));
        crate::arch::console::printk(format_args!("before plic_init\n"));
plic_init();
        crate::arch::console::printk(format_args!("after plic_init\n"));
        crate::arch::console::printk(format_args!("before virtio_init\n"));
        crate::drivers::virtio::virtio_init();
        crate::arch::console::printk(format_args!("after virtio_init\n"));
        crate::arch::console::printk(format_args!("virtio init done\n"));
        crate::arch::console::printk(format_args!("calling binit...\n"));
        crate::arch::console::printk(format_args!("before binit ptr={:#x}\n", unsafe { crate::mm::page_table::KERNEL_PAGETABLE_PTR } as usize));
        crate::arch::console::printk(format_args!("KERNEL_PAGETABLE_PTR addr={:#x}\n", &raw const crate::mm::page_table::KERNEL_PAGETABLE_PTR as usize));
        crate::arch::console::printk(format_args!("BUF_CACHE addr={:#x}\n", crate::fs::buf::bcache_addr()));
        crate::fs::binit();
        crate::arch::console::printk(format_args!("after binit ptr={:#x}\n", unsafe { crate::mm::page_table::KERNEL_PAGETABLE_PTR } as usize));
        crate::arch::console::printk(format_args!("binit done\n"));
        crate::arch::console::printk(format_args!("calling iinit...\n"));
        crate::fs::iinit();
        crate::arch::console::printk(format_args!("iinit done\n"));
        crate::arch::console::printk(format_args!("calling fileinit...\n"));
        crate::fs::fileinit();
        crate::arch::console::printk(format_args!("fileinit done\n"));
        crate::arch::console::printk(format_args!("calling userinit...\n"));
        crate::proc::userinit();
        crate::arch::console::printk(format_args!("userinit done\n"));
        
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