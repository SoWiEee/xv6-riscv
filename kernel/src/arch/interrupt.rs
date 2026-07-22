// kernel/src/arch/interrupt.rs
use super::registers::*;
use super::asm::*;
use core::ptr::write_volatile;
use core::ptr::read_volatile;
use core::fmt::Write;

pub const UART0: usize = 0x10000000;
pub const VIRTIO0: usize = 0x10001000;
pub const PLIC_BASE: usize = 0x0C000000;
pub const UART0_IRQ: u32 = 10;
pub const VIRTIO0_IRQ: u32 = 1;

pub fn plic_init() {
    // Set priorities
    for i in 1..=31 {
        unsafe { write_volatile((PLIC_BASE + PLIC_PRIORITY_BASE + i * 4) as *mut u32, 1); }
    }
    // Enable UART and Virtio for hart 0
    unsafe { write_volatile((PLIC_BASE + PLIC_ENABLE_BASE) as *mut u32, (1 << UART0_IRQ) | (1 << VIRTIO0_IRQ)); }
    unsafe { write_volatile((PLIC_BASE + PLIC_THRESHOLD_BASE) as *mut u32, 0); }
}

pub fn plic_init_hart() {
    let hart = r_tp() as usize;
    unsafe { write_volatile((PLIC_BASE + PLIC_ENABLE_BASE + hart * PLIC_HART_STRIDE) as *mut u32, (1 << UART0_IRQ) | (1 << VIRTIO0_IRQ)); }
    unsafe { write_volatile((PLIC_BASE + PLIC_THRESHOLD_BASE + hart * PLIC_HART_STRIDE) as *mut u32, 0); }
}

pub fn plic_claim() -> u32 {
    let hart = r_tp() as usize;
    unsafe { read_volatile((PLIC_BASE + PLIC_CLAIM_BASE + hart * PLIC_HART_STRIDE) as *const u32) }
}

pub fn plic_complete(irq: u32) {
    let hart = r_tp() as usize;
    unsafe { write_volatile((PLIC_BASE + PLIC_CLAIM_BASE + hart * PLIC_HART_STRIDE) as *mut u32, irq); }
}

pub fn devintr() -> u32 {
    let scause = r_scause();
    if scause == 0x8000000000000009 { // Supervisor external interrupt
        let irq = plic_claim();
        match irq {
            UART0_IRQ => crate::drivers::uart::uart_intr(),
            VIRTIO0_IRQ => crate::drivers::virtio::virtio_intr(),
            _ if irq != 0 => crate::arch::console::printk(format_args!("unexpected interrupt irq={}\n", irq)),
            _ => {}
        }
        if irq != 0 { plic_complete(irq); }
        1
    } else if scause == 0x8000000000000005 { // Timer interrupt
        clock_intr();
        2
    } else {
        0
    }
}

fn clock_intr() {
    if r_tp() == 0 {
        crate::proc::tick();
    }
    w_stimecmp(r_time() + 1000000);
}

pub fn uart_init() {
    // ... UART init
}

pub fn uart_intr() {
    crate::drivers::uart::uart_intr();
}