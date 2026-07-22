// user-lib/src/stdio.rs
use core::fmt::{self, Write};

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::syscall::write(1, s.as_bytes());
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::stdio::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::print!("{}\n", format_args!($($arg)*)) };
}

pub fn getc() -> Option<u8> {
    let mut buf = [0u8; 1];
    if crate::syscall::read(0, &mut buf) > 0 {
        Some(buf[0])
    } else {
        None
    }
}

pub fn putc(c: u8) {
    crate::syscall::write(1, &[c]);
}