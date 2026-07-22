// user/src/bin/sh.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::*;

#[no_mangle]
fn main() -> ! {
    println!("xv6-rust shell\n");
    loop {
        print!("$ ");
        // Simple shell loop - will be expanded later
        syscall::sleep(100);
    }
}