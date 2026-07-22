// user/src/bin/ls.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::*;

#[no_mangle]
fn main() -> ! {
    println!("ls: not implemented yet\n");
    syscall::exit(0);
}