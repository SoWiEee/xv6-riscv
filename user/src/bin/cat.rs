// user/src/bin/cat.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{print, println, syscall};
use alloc::string::String;

#[unsafe(no_mangle)]
fn main() -> isize {
    // Initialize heap
    xv6_user_lib::init_heap();
    
    // For simplicity, just cat a test file
    let path = "README";
    
    let fd = syscall::open(path, 0); // O_RDONLY
    if fd < 0 {
        println!("cat: cannot open {}", path);
        return -1;
    }
    
    let mut buf = [0u8; 512];
    loop {
        let n = syscall::read(fd as i32, &mut buf);
        if n <= 0 {
            break;
        }
        syscall::write(1, &buf[..n as usize]);
    }
    
    syscall::close(fd as i32);
    0
}