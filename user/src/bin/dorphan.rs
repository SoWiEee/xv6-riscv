// user/src/bin/dorphan.rs
// Create an orphaned (unlinked but cwd) directory, then sit until killed so a
// test harness can check that the inode is reclaimed.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    if syscall::mkdir("dd") != 0 {
        println!("dorphan: mkdir dd failed");
        syscall::exit(1);
    }
    if syscall::chdir("dd") != 0 {
        println!("dorphan: chdir dd failed");
        syscall::exit(1);
    }
    if syscall::unlink("../dd") < 0 {
        println!("dorphan: unlink failed");
        syscall::exit(1);
    }
    println!("wait for kill and reclaim");
    loop {
        syscall::sleep(1000);
    }
}
