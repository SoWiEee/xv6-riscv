// user/src/bin/forphan.rs
// Create an orphaned (unlinked but open) file, then sit until killed so a test
// harness can check that the inode is reclaimed.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::fs::{O_CREATE, O_RDONLY, O_WRONLY};
use xv6_user_lib::syscall::{self, Stat};
use xv6_user_lib::println;

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    let ff = "file0";

    let fd = syscall::open(ff, O_CREATE | O_WRONLY) as i32;
    if fd < 0 {
        println!("forphan: open failed");
        syscall::exit(1);
    }

    let mut st = Stat::default();
    if syscall::fstat(fd, &mut st) < 0 {
        println!("forphan: cannot stat {}", ff);
        syscall::exit(1);
    }

    if syscall::unlink(ff) < 0 {
        println!("forphan: unlink failed");
        syscall::exit(1);
    }

    if syscall::open(ff, O_RDONLY) != -1 {
        println!("forphan: open succeeded");
        syscall::exit(1);
    }

    println!("wait for kill and reclaim {}", st.ino);
    loop {
        syscall::sleep(1000);
    }
}
