// user/src/bin/forkbench_slim.rs
//
// Size-optimized twin of forkbench.rs. Identical work, but avoids the two big
// sources of Rust user-binary bloat:
//   * core::fmt (println!)  -> hand-rolled itoa + raw syscall::write
//   * alloc/Vec (args())    -> manual argv[1] parse, no init_heap
// Demonstrates that the footprint is fmt/alloc, not Rust: see the .text drop
// vs forkbench.rs (and `nop` = 32 bytes).
#![no_std]
#![no_main]

use xv6_user_lib::syscall;

/// Parse a NUL-terminated C string of ASCII digits into usize.
fn parse_usize(p: *const u8) -> usize {
    let mut n = 0usize;
    let mut i = 0isize;
    loop {
        let c = unsafe { *p.offset(i) };
        if c < b'0' || c > b'9' {
            break;
        }
        n = n * 10 + (c - b'0') as usize;
        i += 1;
    }
    n
}

/// Write a usize as decimal to fd 1 (no core::fmt).
fn put_usize(v: usize) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = v;
    if v == 0 {
        syscall::write(1, b"0");
        return;
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    syscall::write(1, &buf[i..]);
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    let n = if argc > 1 {
        parse_usize(unsafe { *argv.add(1) })
    } else {
        1000
    };

    let t0 = syscall::uptime();
    for _ in 0..n {
        let pid = syscall::fork();
        if pid < 0 {
            syscall::write(1, b"forkbench_slim: fork failed\n");
            syscall::exit(1);
        }
        if pid == 0 {
            syscall::exit(0);
        }
        let mut st: i32 = 0;
        syscall::wait(&mut st);
    }
    let t1 = syscall::uptime();

    syscall::write(1, b"FORKBENCH n=");
    put_usize(n);
    syscall::write(1, b" ticks=");
    put_usize((t1 - t0) as usize);
    syscall::write(1, b"\n");
    0
}
