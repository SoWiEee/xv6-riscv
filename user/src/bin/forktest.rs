// user/src/bin/forktest.rs
// Test that fork fails gracefully once the proc table fills.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{print, syscall};

const N: usize = 1000;

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    print!("fork test\n");

    let mut n = 0;
    while n < N {
        let pid = syscall::fork();
        if pid < 0 {
            break;
        }
        if pid == 0 {
            syscall::exit(0);
        }
        n += 1;
    }

    if n == N {
        print!("fork claimed to work N times!\n");
        syscall::exit(1);
    }

    while n > 0 {
        if syscall::wait(core::ptr::null_mut()) < 0 {
            print!("wait stopped early\n");
            syscall::exit(1);
        }
        n -= 1;
    }

    if syscall::wait(core::ptr::null_mut()) != -1 {
        print!("wait got too many\n");
        syscall::exit(1);
    }

    print!("fork test OK\n");
    syscall::exit(0);
}
