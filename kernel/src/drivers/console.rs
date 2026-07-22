// kernel/src/drivers/console.rs
//! Console driver for kernel output.
//!
//! Provides synchronized access to the UART for kernel printk and panic messages.

use crate::drivers::uart::{uart_putc, uart_init};
use crate::sync::spinlock::SpinLock;
use core::fmt::{self, Write};

/// Global console lock for synchronized output.
static CONSOLE_LOCK: SpinLock<()> = SpinLock::new((), "console");

/// Initialize the console (calls UART init).
pub fn console_init() {
    uart_init();
}

/// Console interrupt handler.
/// 
/// Called from UART interrupt. Currently just echoes input.
pub fn console_intr(c: u8) {
    uart_putc(c);
}

/// Output a character to console with locking.
/// 
/// Converts `\n` to `\r\n` for terminal compatibility.
pub fn consputc(c: i32) {
    let _guard = CONSOLE_LOCK.acquire();
    if c == '\n' as i32 {
        uart_putc('\r' as u8);
    }
    uart_putc(c as u8);
}

/// Writer implementation for formatted console output.
pub struct ConsoleWriter;

impl Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            consputc(c as i32);
        }
        Ok(())
    }
}

/// Macro for printing to console with automatic locking.
/// 
/// Usage: `console_printk!("hello {}", 42);`
#[macro_export]
macro_rules! console_printk {
    ($($arg:tt)*) => {{
        let _guard = $crate::drivers::console::CONSOLE_LOCK.acquire();
        $crate::drivers::console::ConsoleWriter.write_fmt(format_args!($($arg)*)).unwrap();
    }};
}