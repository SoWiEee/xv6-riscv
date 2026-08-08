// user/src/bin/init.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main() -> ! {
    // Initialize heap
    xv6_user_lib::init_heap();
    
    // Open the console device. Like xv6, open WITHOUT O_CREATE so the first
    // attempt fails when /console does not yet exist; then mknod creates it as
    // a device node (major 1 = console) and the reopen succeeds. Passing
    // O_CREATE here would instead create a regular file, and writes would go to
    // disk rather than the UART.
    let fd = syscall::open("console", 0x002); // O_RDWR
    if fd < 0 {
        syscall::mknod("console", 1, 0);
        syscall::open("console", 0x002);
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