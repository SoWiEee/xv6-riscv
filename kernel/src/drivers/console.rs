// kernel/src/drivers/console.rs
//! Console driver for kernel output.
//!
//! Provides synchronized access to the UART for kernel printk and panic messages.

use crate::drivers::uart::{uart_putc, uart_init, uart_getc};
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

/// Write bytes from a user buffer to the console (device write path).
///
/// Backs `filewrite` for the console device (`major == CONSOLE`). Returns the
/// number of bytes written (always the whole slice here).
pub fn console_write(src: &[u8]) -> usize {
    for &b in src {
        consputc(b as i32);
    }
    src.len()
}

/// Read a line of input from the console into `dst` (device read path).
///
/// Backs `fileread` for the console device. Blocks (polling the UART, yielding
/// to the scheduler between polls) until input arrives, echoing characters as
/// they are typed. Returns after a newline or when `dst` is full — cooked line
/// discipline, matching xv6's `consoleread` behaviour closely enough for the
/// shell. `\r` (Enter) is normalised to `\n`.
pub fn console_read(dst: &mut [u8]) -> usize {
    let mut i = 0;
    while i < dst.len() {
        // Block until a character is available.
        let c = loop {
            if let Some(c) = uart_getc() {
                break c;
            }
            if crate::proc::started() {
                crate::proc::yield_now();
            } else {
                core::hint::spin_loop();
            }
        };

        if c == b'\r' || c == b'\n' {
            consputc('\n' as i32);
            dst[i] = b'\n';
            i += 1;
            break;
        }

        consputc(c as i32);
        dst[i] = c;
        i += 1;
    }
    i
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