// user/src/bin/rm.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    let args = unsafe { xv6_user_lib::args(argc, argv) };
    if args.len() < 2 {
        println!("Usage: rm files...");
        return 1;
    }

    let mut status = 0;
    for path in &args[1..] {
        if syscall::unlink(path) < 0 {
            println!("rm: {} failed to delete", path);
            status = 1;
        }
    }
    status
}
