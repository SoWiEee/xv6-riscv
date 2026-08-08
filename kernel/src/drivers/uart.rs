// kernel/src/drivers/uart.rs
//! 16550 UART driver for console output and input.
//!
//! Memory-mapped at 0x10000000. Used for kernel printk and user console I/O.

use crate::arch::asm::{intr_on, intr_off};
use core::fmt::{self, Write};

const UART0: usize = 0x10000000;
const UART_THR: usize = 0;      // Transmitter Holding Register
const UART_RHR: usize = 0;      // Receiver Holding Register
const UART_IER: usize = 1;      // Interrupt Enable Register
const UART_FCR: usize = 2;      // FIFO Control Register
const UART_LCR: usize = 3;      // Line Control Register
#[allow(dead_code)] // part of the 16550 register map; kept for reference
const UART_MCR: usize = 4;      // Modem Control Register
const UART_LSR: usize = 5;      // Line Status Register
const UART_LSR_RX: u8 = 1;      // Data ready
const UART_LSR_TX: u8 = 32;     // Transmitter empty

/// Initialize the UART.
/// 
/// Sets up 38400 baud, 8N1, enables FIFO and RX interrupt.
pub fn uart_init() {
    unsafe {
        let uart = UART0 as *mut u8;
        // Disable interrupts
        uart.add(UART_IER).write_volatile(0x00);
        // Set DLAB=1 for baud rate
        uart.add(UART_LCR).write_volatile(0x80);
        // 38400 baud: divisor = 3 (for 115200 base)
        uart.add(0).write_volatile(0x03);
        uart.add(1).write_volatile(0x00);
        // 8 bits, no parity, 1 stop bit, DLAB=0
        uart.add(UART_LCR).write_volatile(0x03);
        // Enable FIFO
        uart.add(UART_FCR).write_volatile(0x07);
        // Enable interrupts
        uart.add(UART_IER).write_volatile(0x01);
    }
}

/// Output a character to the UART (polling).
/// 
/// Spins until transmitter is ready.
pub fn uart_putc(c: u8) {
    unsafe {
        let uart = UART0 as *mut u8;
        while (uart.add(UART_LSR).read_volatile() & UART_LSR_TX) == 0 {
            core::hint::spin_loop();
        }
        uart.add(UART_THR).write_volatile(c);
    }
}

/// Read a character from the UART (non-blocking).
/// 
/// Returns `Some(c)` if a character is available, `None` otherwise.
pub fn uart_getc() -> Option<u8> {
    unsafe {
        let uart = UART0 as *mut u8;
        if (uart.add(UART_LSR).read_volatile() & UART_LSR_RX) != 0 {
            Some(uart.add(UART_RHR).read_volatile())
        } else {
            None
        }
    }
}

/// UART interrupt handler.
/// 
/// Reads all available characters and passes them to the console.
pub fn uart_intr() {
    while let Some(c) = uart_getc() {
        crate::drivers::console::console_intr(c);
    }
}

/// Writer implementation for `core::fmt::Write`.
/// 
/// Used by `printk` for formatted output.
pub struct UartWriter;

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            uart_putc(c);
        }
        Ok(())
    }
}

/// Print formatted string to UART (with interrupts disabled).
/// 
/// Used for kernel debugging output.
pub fn printk(args: fmt::Arguments) {
    intr_off();
    UartWriter.write_fmt(args).unwrap();
    intr_on();
}