// user/src/bin/wc.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall};

/// Count lines/words/chars on `fd` and print them with `name`, matching C xv6.
fn wc(fd: i32, name: &str) {
    let mut buf = [0u8; 512];
    let (mut lines, mut words, mut chars) = (0usize, 0usize, 0usize);
    let mut inword = false;

    loop {
        let n = syscall::read(fd, &mut buf);
        if n < 0 {
            println!("wc: read error");
            return;
        }
        if n == 0 {
            break;
        }
        for &b in &buf[..n as usize] {
            chars += 1;
            if b == b'\n' {
                lines += 1;
            }
            // Whitespace set matches C xv6: " \r\t\n\v".
            if matches!(b, b' ' | b'\r' | b'\t' | b'\n' | 0x0b) {
                inword = false;
            } else if !inword {
                words += 1;
                inword = true;
            }
        }
    }

    println!("{} {} {} {}", lines, words, chars, name);
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    let args = unsafe { xv6_user_lib::args(argc, argv) };

    // No file arguments: count standard input (C xv6 prints an empty name).
    if args.len() < 2 {
        wc(0, "");
        return 0;
    }

    for path in &args[1..] {
        let fd = syscall::open(path, 0); // O_RDONLY
        if fd < 0 {
            println!("wc: cannot open {}", path);
            return 1;
        }
        wc(fd as i32, path);
        syscall::close(fd as i32);
    }
    0
}
