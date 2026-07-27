// user/src/bin/zombie.rs
// Create a zombie process that must be reparented at exit.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::syscall;

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    if syscall::fork() > 0 {
        syscall::sleep(5); // let the child exit before the parent (C `pause`)
    }
    syscall::exit(0);
}
