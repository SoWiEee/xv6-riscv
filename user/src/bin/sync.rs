// user/src/bin/sync.rs
// Flush the file system to disk.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::syscall;

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    syscall::sync();
    syscall::exit(0);
}
