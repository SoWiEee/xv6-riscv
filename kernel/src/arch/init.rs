// kernel/src/arch/init.rs
use super::asm::*;
use super::paging::{kvm_init, kvm_init_hart};
use super::interrupt::*;
use crate::mm::frame_allocator::kinit;
use crate::proc::procinit;
use crate::arch::console::consoleinit;

static mut STARTED: bool = false;

#[unsafe(no_mangle)]
pub extern "C" fn init() -> ! {
    let hart_id = r_tp();
    if hart_id == 0 {
        consoleinit();
        crate::arch::console::printk(format_args!("\nxv6-rust kernel is booting\n\n"));
        kinit();
        let root = kvm_init();
        kvm_init_hart(root);
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
        kvm_init_hart(crate::mm::paging::kernel_pagetable());
        trapinit();
        plic_init_hart();
    }
    
    crate::proc::scheduler();
}

fn started() -> bool {
    unsafe { core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst); STARTED }
}

pub fn trapinit() {
    // Set up trap vector for kernel mode
    w_stvec(super::trap::kernelvec_addr());
}