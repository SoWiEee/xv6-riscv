// user/src/bin/usertests.rs
//
// A Rust port of xv6's usertests. Each test runs in its own child process; the
// runner reports OK / FAILED based on the child's exit status. A test signals
// failure by calling `exit(1)` (printing a diagnostic first); returning
// normally means success and the runner's child then `exit(0)`s.
//
// This is an incremental port: it covers the core file-system and process
// tests the Rust kernel supports. Tests that need kernel features this port
// does not implement (lazy allocation, precise copyin/copyout boundary
// behaviour, kernel-memory probes, exact free-page accounting) or that cannot
// fit the small fs.img (writebig/bigfile need MAXFILE≈65803 blocks here — the
// kernel has double-indirect blocks — vs a 2000-block image) are omitted.
#![no_std]
#![no_main]

extern crate alloc;

use xv6_user_lib::fs::{O_CREATE, O_RDONLY, O_RDWR, O_WRONLY};
use xv6_user_lib::{print, println, syscall};

// (MAXOPBLOCKS + 2) * BSIZE, matching the C `buf`.
const BSIZE: usize = 1024;
const BUFSZ: usize = (10 + 2) * BSIZE;
static mut BUF: [u8; BUFSZ] = [0u8; BUFSZ];

/// Shared scratch buffer (the C global `char buf[BUFSZ]`). Single-threaded per
/// process, so a `static mut` view is race-free within one process.
fn buf() -> &'static mut [u8] {
    // SAFETY: one process, single-threaded; only this process touches BUF.
    unsafe { &mut *&raw mut BUF }
}

/// Write a plain &str to a specific fd (used where the C code fprintf()s to a
/// preserved stderr after redirecting fd 1).
fn fdputs(fd: i32, msg: &str) {
    syscall::write(fd, msg.as_bytes());
}

// ---------------------------------------------------------------------------
// file-system tests
// ---------------------------------------------------------------------------

fn opentest(s: &str) {
    let fd = syscall::open("echo", 0);
    if fd < 0 {
        println!("{}: open echo failed!", s);
        syscall::exit(1);
    }
    syscall::close(fd as i32);
    let fd = syscall::open("doesnotexist", 0);
    if fd >= 0 {
        println!("{}: open doesnotexist succeeded!", s);
        syscall::exit(1);
    }
}

fn writetest(s: &str) {
    const N: usize = 100;
    const SZ: usize = 10;

    let fd = syscall::open("small", O_CREATE | O_RDWR);
    if fd < 0 {
        println!("{}: error: creat small failed!", s);
        syscall::exit(1);
    }
    let fd = fd as i32;
    for i in 0..N {
        if syscall::write(fd, b"aaaaaaaaaa") != SZ as isize {
            println!("{}: error: write aa {} new file failed", s, i);
            syscall::exit(1);
        }
        if syscall::write(fd, b"bbbbbbbbbb") != SZ as isize {
            println!("{}: error: write bb {} new file failed", s, i);
            syscall::exit(1);
        }
    }
    syscall::close(fd);
    let fd = syscall::open("small", O_RDONLY);
    if fd < 0 {
        println!("{}: error: open small failed!", s);
        syscall::exit(1);
    }
    let fd = fd as i32;
    let n = syscall::read(fd, &mut buf()[..N * SZ * 2]);
    if n != (N * SZ * 2) as isize {
        println!("{}: read failed", s);
        syscall::exit(1);
    }
    syscall::close(fd);
    if syscall::unlink("small") < 0 {
        println!("{}: unlink small failed", s);
        syscall::exit(1);
    }
}

fn createtest(s: &str) {
    const N: usize = 52;
    let mut name = [b'a', b'0', 0];
    for i in 0..N {
        name[1] = b'0' + i as u8;
        let fd = syscall::open(core::str::from_utf8(&name[..2]).unwrap(), O_CREATE | O_RDWR);
        syscall::close(fd as i32);
    }
    for i in 0..N {
        name[1] = b'0' + i as u8;
        syscall::unlink(core::str::from_utf8(&name[..2]).unwrap());
    }
    let _ = s;
}

fn dirtest(s: &str) {
    if syscall::mkdir("dir0") < 0 {
        println!("{}: mkdir failed", s);
        syscall::exit(1);
    }
    if syscall::chdir("dir0") < 0 {
        println!("{}: chdir dir0 failed", s);
        syscall::exit(1);
    }
    if syscall::chdir("..") < 0 {
        println!("{}: chdir .. failed", s);
        syscall::exit(1);
    }
    if syscall::unlink("dir0") < 0 {
        println!("{}: unlink dir0 failed", s);
        syscall::exit(1);
    }
}

fn exectest(s: &str) {
    let echoargv: [*const u8; 3] = [b"echo\0".as_ptr(), b"OK\0".as_ptr(), core::ptr::null()];

    syscall::unlink("echo-ok");
    let pid = syscall::fork();
    if pid < 0 {
        println!("{}: fork failed", s);
        syscall::exit(1);
    }
    if pid == 0 {
        let errfd = syscall::dup(1);
        if errfd < 0 {
            println!("{}: dup failed", s);
            syscall::exit(1);
        }
        let errfd = errfd as i32;
        syscall::close(1);
        let fd = syscall::open("echo-ok", O_CREATE | O_WRONLY);
        if fd < 0 {
            fdputs(errfd, "exectest: create failed\n");
            syscall::exit(1);
        }
        if fd != 1 {
            fdputs(errfd, "exectest: wrong fd\n");
            syscall::exit(1);
        }
        if syscall::exec("echo", &echoargv) < 0 {
            fdputs(errfd, "exectest: exec echo failed\n");
            syscall::exit(1);
        }
        // not reached
    }
    let mut xstatus: i32 = 0;
    if syscall::wait(&mut xstatus) != pid {
        println!("{}: wait failed!", s);
    }
    if xstatus != 0 {
        println!("{}: nonzero wait status {}", s, xstatus);
        syscall::exit(1);
    }

    let fd = syscall::open("echo-ok", O_RDONLY);
    if fd < 0 {
        println!("{}: open failed", s);
        syscall::exit(1);
    }
    let fd = fd as i32;
    let mut rbuf = [0u8; 3];
    if syscall::read(fd, &mut rbuf[..2]) != 2 {
        println!("{}: read failed", s);
        syscall::exit(1);
    }
    syscall::close(fd);
    syscall::unlink("echo-ok");
    if rbuf[0] == b'O' && rbuf[1] == b'K' {
        syscall::exit(0);
    } else {
        println!("{}: wrong output", s);
        syscall::exit(1);
    }
}

// iput / cwd transaction tests

fn iputtest(s: &str) {
    if syscall::mkdir("iputdir") < 0 {
        println!("{}: mkdir failed", s);
        syscall::exit(1);
    }
    if syscall::chdir("iputdir") < 0 {
        println!("{}: chdir iputdir failed", s);
        syscall::exit(1);
    }
    if syscall::unlink("../iputdir") < 0 {
        println!("{}: unlink ../iputdir failed", s);
        syscall::exit(1);
    }
    if syscall::chdir("/") < 0 {
        println!("{}: chdir / failed", s);
        syscall::exit(1);
    }
}

fn exitiputtest(s: &str) {
    let pid = syscall::fork();
    if pid < 0 {
        println!("{}: fork failed", s);
        syscall::exit(1);
    }
    if pid == 0 {
        if syscall::mkdir("iputdir") < 0 {
            println!("{}: mkdir failed", s);
            syscall::exit(1);
        }
        if syscall::chdir("iputdir") < 0 {
            println!("{}: child chdir failed", s);
            syscall::exit(1);
        }
        if syscall::unlink("../iputdir") < 0 {
            println!("{}: unlink ../iputdir failed", s);
            syscall::exit(1);
        }
        syscall::exit(0);
    }
    let mut xstatus: i32 = 0;
    syscall::wait(&mut xstatus);
    syscall::exit(xstatus);
}

fn openiputtest(s: &str) {
    if syscall::mkdir("oidir") < 0 {
        println!("{}: mkdir oidir failed", s);
        syscall::exit(1);
    }
    let pid = syscall::fork();
    if pid < 0 {
        println!("{}: fork failed", s);
        syscall::exit(1);
    }
    if pid == 0 {
        let fd = syscall::open("oidir", O_RDWR);
        if fd >= 0 {
            println!("{}: open directory for write succeeded", s);
            syscall::exit(1);
        }
        syscall::exit(0);
    }
    syscall::sleep(1);
    if syscall::unlink("oidir") != 0 {
        println!("{}: unlink failed", s);
        syscall::exit(1);
    }
    let mut xstatus: i32 = 0;
    syscall::wait(&mut xstatus);
    syscall::exit(xstatus);
}

// ---------------------------------------------------------------------------
// test runner
// ---------------------------------------------------------------------------

type TestFn = fn(&str);

static QUICKTESTS: &[(TestFn, &str)] = &[
    (opentest, "opentest"),
    (writetest, "writetest"),
    (createtest, "createtest"),
    (dirtest, "dirtest"),
    (exectest, "exectest"),
    (iputtest, "iput"),
    (exitiputtest, "exitiput"),
    (openiputtest, "openiput"),
];

/// Run one test in its own child; returns true if the child exited 0.
fn run(f: TestFn, s: &str) -> bool {
    print!("test {}: ", s);
    let pid = syscall::fork();
    if pid < 0 {
        println!("runtest: fork error");
        syscall::exit(1);
    }
    if pid == 0 {
        f(s);
        syscall::exit(0);
    }
    let mut xstatus: i32 = 0;
    syscall::wait(&mut xstatus);
    if xstatus != 0 {
        println!("FAILED");
    } else {
        println!("OK");
    }
    xstatus == 0
}

fn runtests(tests: &[(TestFn, &str)], justone: Option<&str>) -> i32 {
    let mut ntests = 0;
    for &(f, s) in tests {
        if justone.is_none() || justone == Some(s) {
            ntests += 1;
            if !run(f, s) {
                println!("SOME TESTS FAILED");
                return -1;
            }
        }
    }
    ntests
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    // Parse an optional single-test name argument.
    let justone: Option<&str> = if argc >= 2 {
        // SAFETY: argv[1] is a NUL-terminated C string from the kernel.
        unsafe {
            let p = *argv.add(1);
            if p.is_null() {
                None
            } else {
                let mut len = 0usize;
                while *p.add(len) != 0 {
                    len += 1;
                }
                core::str::from_utf8(core::slice::from_raw_parts(p, len)).ok()
            }
        }
    } else {
        None
    };

    println!("usertests starting");
    let n = runtests(QUICKTESTS, justone);
    if n < 0 {
        syscall::exit(1);
    }
    if justone.is_some() && n == 0 {
        println!("NO TESTS EXECUTED");
        syscall::exit(1);
    }
    println!("ALL TESTS PASSED");
    syscall::exit(0);
}
