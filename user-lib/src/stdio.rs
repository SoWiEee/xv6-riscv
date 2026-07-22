// user-lib/src/stdio.rs
use crate::syscall::*;

pub fn printf(fmt: &str, args: &[&dyn core::fmt::Display]) {
    // Simplified printf - real implementation would format properly
    write(1, fmt.as_bytes());
}

pub fn println(fmt: &str) {
    write(1, fmt.as_bytes());
    write(1, b"\n");
}

pub fn getc() -> Option<u8> {
    let mut buf = [0u8; 1];
    if read(0, &mut buf) > 0 {
        Some(buf[0])
    } else {
        None
    }
}

pub fn putc(c: u8) {
    write(1, &[c]);
}