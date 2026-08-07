// user/src/bin/forkbench.rs
//
// Heavy-path microbenchmark: N iterations of fork() + wait(), child exits
// immediately. Exercises uvmcopy (address-space copy), proc allocation, the
// scheduler round-trip, and uvmfree on exit. Counterpart to user/forkbench.c.
// Use two N values and difference (T(2N)-T(N)) to cancel fixed overhead.
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

    let t0 = syscall::uptime();
    for _ in 0..n {
        let pid = syscall::fork();
        if pid < 0 {
            println!("forkbench: fork failed");
            syscall::exit(1);
        }
        if pid == 0 {
            syscall::exit(0);
        }
        let mut st: i32 = 0;
        syscall::wait(&mut st);
    }
    let t1 = syscall::uptime();

    println!("FORKBENCH n={} ticks={}", n, t1 - t0);
    0
}
