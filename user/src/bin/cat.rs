// user/src/bin/cat.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall};

fn cat(fd: i32) {
    let mut buf = [0u8; 512];
    loop {
        let n = syscall::read(fd, &mut buf);
        if n <= 0 {
            break;
        }
        syscall::write(1, &buf[..n as usize]);
    }
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    let args = unsafe { xv6_user_lib::args(argc, argv) };

    // With no file arguments, cat standard input (like C xv6).
    if args.len() < 2 {
        cat(0);
        return 0;
    }

    for path in &args[1..] {
        let fd = syscall::open(path, 0); // O_RDONLY
        if fd < 0 {
            println!("cat: cannot open {}", path);
            return -1;
        }
        cat(fd as i32);
        syscall::close(fd as i32);
    }
    0
}
