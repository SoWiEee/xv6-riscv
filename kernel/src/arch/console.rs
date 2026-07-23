// kernel/src/arch/console.rs
use core::fmt::{self, Write};
use core::arch::asm;

// UART16550 base address for QEMU virt machine
const UART_BASE: usize = 0x1000_0000;

// UART register offsets
const RBR: usize = 0;  // Receive Buffer Register (read) / Transmit Holding Register (write) when DLAB=0
const IER: usize = 1;  // Interrupt Enable Register when DLAB=0
const IIR: usize = 2;  // Interrupt Identification Register (read) / FIFO Control Register (write)
const LCR: usize = 3;  // Line Control Register
const MCR: usize = 4;  // Modem Control Register
const LSR: usize = 5;  // Line Status Register
const DLL: usize = 0;  // Divisor Latch Low when DLAB=1
const DLH: usize = 1;  // Divisor Latch High when DLAB=1

pub fn init() {
    // Initialize UART
    // Set DLAB = 1 to access divisor latches
    write_reg(LCR, 0x80);
    // Set divisor for 115200 baud @ 1.8432 MHz (divisor = 1)
    write_reg(DLL, 0x01);
    write_reg(DLH, 0x00);
    // Clear DLAB, set 8 bits, no parity, 1 stop bit (0x03)
    write_reg(LCR, 0x03);
    // Enable FIFO, clear, 14-byte threshold
    write_reg(IIR, 0xC7);
    // Enable interrupts, RTS/DSR
    write_reg(MCR, 0x0B);
    // Enable interrupts
    write_reg(IER, 0x01);
}

fn write_reg(offset: usize, value: u8) {
    let addr = UART_BASE + offset;
    unsafe {
        core::ptr::write_volatile(addr as *mut u8, value);
    }
}

fn read_reg(offset: usize) -> u8 {
    let addr = UART_BASE + offset;
    unsafe {
        core::ptr::read_volatile(addr as *const u8)
    }
}

pub fn putchar(c: u8) {
    while (read_reg(LSR) & 0x20) == 0 {} // Wait for THR empty
    write_reg(RBR, c);
}

pub fn getchar() -> Option<u8> {
    if (read_reg(LSR) & 0x01) != 0 {
        Some(read_reg(RBR))
    } else {
        None
    }
}

pub struct UartWriter;

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            putchar(c);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! printk {
    ($($arg:tt)*) => {
        let _ = core::fmt::write(&mut $crate::arch::console::UartWriter, format_args!($($arg)*));
    };
}

pub fn printk_fmt(fmt: fmt::Arguments) {
    let _ = UartWriter.write_fmt(fmt);
}

pub fn consoleinit() {
    init();
}

pub fn printk(args: fmt::Arguments) {
    let _ = UartWriter.write_fmt(args);
}