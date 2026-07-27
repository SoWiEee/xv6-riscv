// user/src/bin/stressfs.rs
// Concurrent FS stress: several processes each write then read a 10 KiB file.
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::fs::{O_CREATE, O_RDONLY, O_RDWR};
use xv6_user_lib::{println, syscall};

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    println!("stressfs starting");
    let data = [b'a'; 512];

    // Fork up to 4 children; each (and the parent) picks a distinct index `i`.
    let mut i = 0usize;
    while i < 4 {
        if syscall::fork() > 0 {
            break;
        }
        i += 1;
    }

    println!("write {}", i);

    // Per-process file "stressfs<i>".
    let mut path = *b"stressfs0";
    path[8] = b'0' + i as u8;
    let path = core::str::from_utf8(&path).unwrap();

    let fd = syscall::open(path, O_CREATE | O_RDWR) as i32;
    for _ in 0..20 {
        syscall::write(fd, &data);
    }
    syscall::close(fd);

    println!("read");

    let fd = syscall::open(path, O_RDONLY) as i32;
    let mut buf = [0u8; 512];
    for _ in 0..20 {
        syscall::read(fd, &mut buf);
    }
    syscall::close(fd);

    syscall::wait(core::ptr::null_mut());
    syscall::exit(0);
}
