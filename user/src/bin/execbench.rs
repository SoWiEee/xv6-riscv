// user/src/bin/execbench.rs
//
// Heavy-path microbenchmark: N iterations of fork() + exec("nop") + wait().
// Subtract forkbench's marginal cost to isolate exec (ELF load + address-space
// rebuild). Counterpart to user/execbench.c. Difference two N values.
#![no_std]
#![no_main]

extern crate alloc;

use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();
    let args = unsafe { xv6_user_lib::args(argc, argv) };
    let n: usize = if args.len() > 1 {
        args[1].parse().unwrap_or(1000)
    } else {
        1000
    };

    let exec_argv: [*const u8; 2] = [b"nop\0".as_ptr(), core::ptr::null()];
    let t0 = syscall::uptime();
    for _ in 0..n {
        let pid = syscall::fork();
        if pid < 0 {
            println!("execbench: fork failed");
            syscall::exit(1);
        }
        if pid == 0 {
            syscall::exec("nop", &exec_argv);
            syscall::exit(1); // only reached if exec failed
        }
        let mut st: i32 = 0;
        syscall::wait(&mut st);
    }
    let t1 = syscall::uptime();

    println!("EXECBENCH n={} ticks={}", n, t1 - t0);
    0
}
