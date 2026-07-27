// user/src/bin/kill.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::syscall;

/// Parse a leading run of ASCII digits into an i32, mirroring C xv6 `atoi`
/// (stops at the first non-digit, returns 0 if there are none).
fn atoi(s: &str) -> i32 {
    let mut n = 0i32;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        n = n * 10 + (b - b'0') as i32;
    }
    n
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    let args = unsafe { xv6_user_lib::args(argc, argv) };

    if args.len() < 2 {
        syscall::write(2, b"usage: kill pid...\n");
        return 1;
    }

    for pid in &args[1..] {
        syscall::kill(atoi(pid));
    }
    0
}
