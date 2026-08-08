// user/src/bin/init_test.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main() -> ! {
    // Initialize heap
    xv6_user_lib::init_heap();
    
    println!("=== Rust User Space Test ===");
    
    // Test getpid
    let pid = syscall::getpid();
    println!("PID: {}", pid);
    
    // Test fork
    println!("Testing fork...");
    let pid = syscall::fork();
    if pid == 0 {
        println!("Child process: PID = {}", syscall::getpid());
        syscall::exit(42);
    } else if pid > 0 {
        println!("Parent process: child PID = {}", pid);
        let mut status: i32 = 0;
        let wpid = syscall::wait(&mut status);
        println!("Waited for PID {}: status = {}", wpid, status);
    } else {
        println!("Fork failed!");
    }
    
    // Test sbrk
    println!("Testing sbrk...");
    let old_heap = syscall::sbrk(0);
    let new_heap = syscall::sbrk(4096);
    println!("Old heap: {:p}, New heap: {:p}", old_heap, new_heap);
    
    // Test sleep
    println!("Testing sleep...");
    syscall::sleep(10);
    println!("Slept for 10 ticks");
    
    // Test uptime
    let uptime = syscall::uptime();
    println!("Uptime: {}", uptime);
    
    // Test file operations (if open/read/write work)
    println!("Testing file operations...");
    let fd = syscall::open("README", 0); // O_RDONLY
    if fd >= 0 {
        println!("Opened README, fd = {}", fd);
        let mut buf = [0u8; 64];
        let n = syscall::read(fd as i32, &mut buf);
        if n > 0 {
            println!("Read {} bytes: {}", n, core::str::from_utf8(&buf[..n as usize]).unwrap_or("invalid UTF-8"));
        }
        syscall::close(fd as i32);
    } else {
        println!("Failed to open README (open not implemented in kernel)");
    }
    
    println!("=== Test complete ===");
    syscall::exit(0);
}