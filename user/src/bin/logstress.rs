// user/src/bin/logstress.rs
// Stress the write-ahead log: each named file gets its own writer process
// (e.g. `logstress f1 f2 f3 f4`), all committing transactions concurrently.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::fs::{O_CREATE, O_RDWR};
use xv6_user_lib::{println, syscall};

const N: usize = 250;
const SZ: usize = 2000;

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();
    let args = unsafe { xv6_user_lib::args(argc, argv) };

    for (idx, name) in args.iter().enumerate().skip(1) {
        let pid = syscall::fork();
        if pid < 0 {
            println!("{}: fork failed", args[0]);
            syscall::exit(1);
        }
        if pid == 0 {
            let fd = syscall::open(name, O_CREATE | O_RDWR) as i32;
            if fd < 0 {
                println!("{}: create {} failed", args[0], name);
                syscall::exit(1);
            }
            let buf = [b'0' + idx as u8; SZ];
            for _ in 0..N {
                let n = syscall::write(fd, &buf);
                if n != SZ as isize {
                    println!("write failed {}", n);
                    syscall::exit(1);
                }
            }
            syscall::exit(0);
        }
    }

    let mut xstatus: i32 = 0;
    for _ in 1..args.len() {
        syscall::wait(&mut xstatus);
        if xstatus != 0 {
            syscall::exit(xstatus);
        }
    }
    syscall::exit(0);
}
