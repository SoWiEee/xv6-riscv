// user/src/bin/syscallbench.rs
//
// Microbenchmark: tight loop of getpid() — the cheapest syscall — to isolate
// syscall entry/exit cost. Prints a marker line the host harness watches for,
// plus the guest uptime-tick delta (deterministic under QEMU -icount shift=0).
// Run with two N values and take the difference (T(2N)-T(N)) to cancel boot,
// load, and fixed overhead, leaving marginal per-syscall cost.
#![no_std]
#![no_main]

extern crate alloc;

use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();
    let args = unsafe { xv6_user_lib::args(argc, argv) };
    let n: usize = if args.len() > 1 {
        args[1].parse().unwrap_or(1_000_000)
    } else {
        1_000_000
    };

    let t0 = syscall::uptime();
    // `acc` folds every getpid() return so the loop can't be elided; the ecall
    // is opaque to the optimizer anyway, but this makes the intent explicit.
    let mut acc: isize = 0;
    for _ in 0..n {
        acc = acc.wrapping_add(syscall::getpid());
    }
    let t1 = syscall::uptime();

    println!("SYSCALLBENCH n={} ticks={} acc={}", n, t1 - t0, acc);
    0
}
