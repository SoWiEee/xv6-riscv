// user/src/bin/grep.rs
//
// Simple grep. Only supports the ^ . * $ operators, matching C xv6. The regex
// engine is the Kernighan & Pike matcher (The Practice of Programming, ch. 9)
// ported from pointer walks to byte slices.
#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use xv6_user_lib::{println, syscall};

/// Search for `re` anywhere in `text`.
fn match_re(re: &[u8], text: &[u8]) -> bool {
    if !re.is_empty() && re[0] == b'^' {
        return match_here(&re[1..], text);
    }
    // Try to match at each position, including the empty suffix at the end.
    let mut t = text;
    loop {
        if match_here(re, t) {
            return true;
        }
        if t.is_empty() {
            return false;
        }
        t = &t[1..];
    }
}

/// Match `re` at the beginning of `text`.
fn match_here(re: &[u8], text: &[u8]) -> bool {
    if re.is_empty() {
        return true;
    }
    if re.len() >= 2 && re[1] == b'*' {
        return match_star(re[0], &re[2..], text);
    }
    if re[0] == b'$' && re.len() == 1 {
        return text.is_empty();
    }
    if !text.is_empty() && (re[0] == b'.' || re[0] == text[0]) {
        return match_here(&re[1..], &text[1..]);
    }
    false
}

/// Match `c*re` at the beginning of `text` (zero or more `c`, then `re`).
fn match_star(c: u8, re: &[u8], text: &[u8]) -> bool {
    let mut t = text;
    loop {
        if match_here(re, t) {
            return true;
        }
        if t.is_empty() || !(t[0] == c || c == b'.') {
            return false;
        }
        t = &t[1..];
    }
}

/// Read all of `fd` and write each newline-terminated line that matches.
fn grep(pattern: &[u8], fd: i32) {
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = syscall::read(fd, &mut buf);
        if n <= 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
    }

    // Process complete lines only (up to the last '\n'), as C xv6 does.
    let mut start = 0;
    while let Some(rel) = data[start..].iter().position(|&b| b == b'\n') {
        let line = &data[start..start + rel]; // line without the newline
        if match_re(pattern, line) {
            syscall::write(1, &data[start..start + rel + 1]); // include newline
        }
        start += rel + 1;
    }
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    let args = unsafe { xv6_user_lib::args(argc, argv) };

    if args.len() < 2 {
        syscall::write(2, b"usage: grep pattern [file ...]\n");
        return 1;
    }
    let pattern = args[1].as_bytes();

    // No file arguments: grep standard input.
    if args.len() < 3 {
        grep(pattern, 0);
        return 0;
    }

    for path in &args[2..] {
        let fd = syscall::open(path, 0); // O_RDONLY
        if fd < 0 {
            println!("grep: cannot open {}", path);
            return 1;
        }
        grep(pattern, fd as i32);
        syscall::close(fd as i32);
    }
    0
}
