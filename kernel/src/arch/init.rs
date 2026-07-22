// kernel/src/arch/init.rs
use super::console;
use super::asm::*;
use core::arch::asm;

#[unsafe(no_mangle)]
pub extern "C" fn init() {
    // Print 'D' directly to UART using inline asm to verify Rust code is running
    unsafe { asm!("li t0, 0x10000000; sb {}, 0(t0)", in(reg) b'D') };
    unsafe { asm!("li t0, 0x10000000; sb {}, 0(t0)", in(reg) b'\n') };
    
    console::init();
    
    // Direct UART test
    console::putchar(b'E');
    console::putchar(b'\n');
    
    // Set up trap vector
    w_stvec(trap_handler as *const () as usize);
    
    // Enable timer interrupts
    w_sie(r_sie() | (1 << 5) | (1 << 1)); // STIE | SSIE
    
    // Enable interrupts globally
    intr_on();
    
    // Print banner
    console::printk_fmt(format_args!("xv6-riscv kernel starting...\n"));
    console::printk_fmt(format_args!("Hart ID: {}\n", r_tp()));
    
    // Loop forever - scheduler will be implemented later
    loop {
        wfi();
    }
}

// Trap handler called from assembly
#[unsafe(no_mangle)]
pub extern "C" fn trap_handler() -> ! {
    console::printk_fmt(format_args!("TRAP: scause={:#x} sepc={:#x} stval={:#x}\n", r_scause(), r_sepc(), r_stval()));
    loop {
        wfi();
    }
}