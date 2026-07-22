// kernel/src/drivers/console.rs
use crate::drivers::uart::{uart_putc, uart_init};
use crate::sync::spinlock::SpinLock;
use core::fmt::{self, Write};

static CONSOLE_LOCK: SpinLock<()> = SpinLock::new((), "console");

pub fn console_init() {
    uart_init();
}

pub fn console_intr(c: u8) {
    // Handle input (echo, buffer, etc.)
    uart_putc(c);
}

pub fn consputc(c: i32) {
    let _guard = CONSOLE_LOCK.acquire();
    if c == '\n' as i32 {
        uart_putc('\r' as u8);
    }
    uart_putc(c as u8);
}

pub struct ConsoleWriter;

impl Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            consputc(c as i32);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! console_printk {
    ($($arg:tt)*) => {{
        let _guard = $crate::drivers::console::CONSOLE_LOCK.acquire();
        $crate::drivers::console::ConsoleWriter.write_fmt(format_args!($($arg)*)).unwrap();
    }};
}