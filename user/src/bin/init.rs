// user/src/bin/init.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{print, println, syscall};
use alloc::string::String;

#[unsafe(no_mangle)]
fn main() -> ! {
    // Initialize heap
    xv6_user_lib::init_heap();
    
    // Open console
    let fd = syscall::open("console", 0x202); // O_RDWR | O_CREATE
    if fd < 0 {
        syscall::mknod("console", 1, 0);
        syscall::open("console", 0x202);
    }
    syscall::dup(0); // stdout
    syscall::dup(0); // stderr
    
    let argv: [*const u8; 2] = [
        b"sh\0".as_ptr(),
        core::ptr::null(),
    ];
    
    loop {
        println!("init: starting sh");
        let pid = syscall::fork();
        if pid < 0 {
            println!("init: fork failed");
            syscall::exit(1);
        }
        if pid == 0 {
            syscall::exec("sh", &argv);
            println!("init: exec sh failed");
            syscall::exit(1);
        }
        
        loop {
            let mut status: i32 = 0;
            let wpid = syscall::wait(&mut status);
            if wpid == pid {
                break;
            } else if wpid < 0 {
                println!("init: wait returned an error");
                syscall::exit(1);
            }
        }
    }
}