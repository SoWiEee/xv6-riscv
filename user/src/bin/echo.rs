// user/src/bin/echo.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{print, println};

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    // Print args[1..] separated by spaces, followed by a newline — matching
    // the C xv6 echo.
    let args = unsafe { xv6_user_lib::args(argc, argv) };
    for (i, arg) in args[1..].iter().enumerate() {
        if i > 0 {
            print!(" ");
        }
        print!("{}", arg);
    }
    println!("");
    0
}
