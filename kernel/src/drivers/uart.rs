// kernel/src/drivers/uart.rs
use crate::arch::console;

pub fn uart_intr() {
    // Handle UART interrupt
    while let Some(c) = console::getchar() {
        console::putchar(c); // Echo back
    }
}