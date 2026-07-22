// user/src/bin/sh.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{print, println, syscall};
use alloc::string::String;
use alloc::vec::Vec;

#[unsafe(no_mangle)]
fn main() -> ! {
    // Initialize heap
    xv6_user_lib::init_heap();
    
    println!("xv6-rust shell");
    let mut input = String::new();
    
    loop {
        print!("$ ");
        
        // Read line from stdin
        input.clear();
        loop {
            match syscall::getc() {
                Some(b'\n') | Some(b'\r') => {
                    println!("");
                    break;
                }
                Some(c) => {
                    input.push(c as char);
                    syscall::putc(c);
                }
                None => {}
            }
        }
        
        if input.trim().is_empty() {
            continue;
        }
        
        // Parse command
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        
        let cmd = parts[0];
        let args = &parts[1..];
        
        match cmd {
            "exit" => {
                println!("Goodbye!");
                syscall::exit(0);
            }
            "echo" => {
                println!("{}", args.join(" "));
            }
            "ls" => {
                let path = if args.is_empty() { "." } else { args[0] };
                run_program("ls", &[path]);
            }
            "cat" => {
                if args.is_empty() {
                    println!("Usage: cat <file>");
                } else {
                    run_program("cat", args);
                }
            }
            _ => {
                run_program(cmd, args);
            }
        }
    }
}

fn run_program(cmd: &str, args: &[&str]) {
    let mut full_args = Vec::with_capacity(args.len() + 1);
    full_args.push(cmd);
    full_args.extend_from_slice(args);
    
    let pid = syscall::fork();
    if pid == 0 {
        // Child process
        let mut c_args: Vec<*const u8> = full_args.iter()
            .map(|s| s.as_ptr())
            .collect();
        c_args.push(core::ptr::null());
        syscall::exec(cmd, &c_args);
        // If exec returns, it failed
        println!("exec failed");
        syscall::exit(-1);
    } else if pid > 0 {
        // Parent process
        let mut status: i32 = 0;
        syscall::wait(&mut status);
    } else {
        println!("fork failed");
    }
}