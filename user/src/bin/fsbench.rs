// user/src/bin/fsbench.rs
//
// FS write throughput benchmark: create a file and write N KiB (1024-byte
// writes), which drives the block layer (write_block via log commits) hard.
// Difference two N values (T(2N)-T(N)) to isolate the marginal per-block cost.
// Counterpart to user/fsbench.c.
#![no_std]
#![no_main]

extern crate alloc;

use xv6_user_lib::fs::{O_CREATE, O_RDWR};
use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();
    let args = unsafe { xv6_user_lib::args(argc, argv) };
    let n: usize = if args.len() > 1 {
        args[1].parse().unwrap_or(300)
    } else {
        300
    };

    let buf = [0x61u8; 1024];
    syscall::unlink("fsbench.tmp");
    let t0 = syscall::uptime();
    let fd = syscall::open("fsbench.tmp", O_CREATE | O_RDWR);
    if fd < 0 {
        println!("fsbench: open failed");
        syscall::exit(1);
    }
    let fd = fd as i32;
    for _ in 0..n {
        if syscall::write(fd, &buf) != 1024 {
            println!("fsbench: write failed");
            syscall::exit(1);
        }
    }
    syscall::close(fd);
    let t1 = syscall::uptime();
    syscall::unlink("fsbench.tmp");

    println!("FSBENCH n={} ticks={}", n, t1 - t0);
    0
}
