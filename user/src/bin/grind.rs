// user/src/bin/grind.rs
// Run random system calls in parallel forever. A faithful port of the C xv6
// `grind` stress test: two children per iteration each hammer the FS, fork,
// pipe, exec and sbrk paths with a pseudo-random mix of syscalls. Runs the
// same soft-float lp64 / riscv64imac ABI as the rest of the Rust userland, so
// unlike the old prebuilt C binary it actually exercises the Rust kernel.
#![no_std]
#![no_main]

use xv6_user_lib::fs::{O_CREATE, O_RDWR};
use xv6_user_lib::syscall::Stat;
use xv6_user_lib::{println, syscall};

// Per-process PRNG state. Each process gets its own copy after fork, matching
// the C global `rand_next`; a user program is single-threaded so a plain
// `static mut` accessed from one process is race-free.
static mut RAND_NEXT: u64 = 1;

// Scratch buffer used by the read/write cases (C: `static char buf[999]`).
static mut BUF: [u8; 999] = [0u8; 999];

// From FreeBSD. Compute x = (7^5 * x) mod (2^31 - 1) without overflowing 31 bits.
fn do_rand(ctx: &mut u64) -> i64 {
    let mut x: i64 = ((*ctx % 0x7fff_fffe) + 1) as i64;
    let hi = x / 127773;
    let lo = x % 127773;
    x = 16807 * lo - 2836 * hi;
    if x < 0 {
        x += 0x7fff_ffff;
    }
    x -= 1;
    *ctx = x as u64;
    x
}

fn rand() -> i64 {
    // SAFETY: single-threaded user process; only this process touches RAND_NEXT.
    unsafe {
        let ctx = &raw mut RAND_NEXT;
        do_rand(&mut *ctx)
    }
}

fn buf_ref() -> &'static mut [u8] {
    // SAFETY: single-threaded user process; the scratch buffer has no aliasing.
    unsafe { &mut *(&raw mut BUF) }
}

fn go(which_child: i32) -> ! {
    let mut fd: i32 = -1;
    let break0 = syscall::sbrk(0) as usize;
    let mut iters: u64 = 0;

    syscall::mkdir("grindir");
    if syscall::chdir("grindir") != 0 {
        println!("grind: chdir grindir failed");
        syscall::exit(1);
    }
    syscall::chdir("/");

    loop {
        iters += 1;
        if iters % 500 == 0 {
            syscall::putc(if which_child != 0 { b'B' } else { b'A' });
        }
        let what = rand() % 23;
        match what {
            1 => {
                let f = syscall::open("grindir/../a", O_CREATE | O_RDWR) as i32;
                syscall::close(f);
            }
            2 => {
                let f = syscall::open("grindir/../grindir/../b", O_CREATE | O_RDWR) as i32;
                syscall::close(f);
            }
            3 => {
                syscall::unlink("grindir/../a");
            }
            4 => {
                if syscall::chdir("grindir") != 0 {
                    println!("grind: chdir grindir failed");
                    syscall::exit(1);
                }
                syscall::unlink("../b");
                syscall::chdir("/");
            }
            5 => {
                syscall::close(fd);
                fd = syscall::open("/grindir/../a", O_CREATE | O_RDWR) as i32;
            }
            6 => {
                syscall::close(fd);
                fd = syscall::open("/./grindir/./../b", O_CREATE | O_RDWR) as i32;
            }
            7 => {
                syscall::write(fd, buf_ref());
            }
            8 => {
                syscall::read(fd, buf_ref());
            }
            9 => {
                syscall::mkdir("grindir/../a");
                let f = syscall::open("a/../a/./a", O_CREATE | O_RDWR) as i32;
                syscall::close(f);
                syscall::unlink("a/a");
            }
            10 => {
                syscall::mkdir("/../b");
                let f = syscall::open("grindir/../b/b", O_CREATE | O_RDWR) as i32;
                syscall::close(f);
                syscall::unlink("b/b");
            }
            11 => {
                syscall::unlink("b");
                syscall::link("../grindir/./../a", "../b");
            }
            12 => {
                syscall::unlink("../grindir/../a");
                syscall::link(".././b", "/grindir/../a");
            }
            13 => {
                let pid = syscall::fork();
                if pid == 0 {
                    syscall::exit(0);
                } else if pid < 0 {
                    println!("grind: fork failed");
                    syscall::exit(1);
                }
                syscall::wait(core::ptr::null_mut());
            }
            14 => {
                let pid = syscall::fork();
                if pid == 0 {
                    syscall::fork();
                    syscall::fork();
                    syscall::exit(0);
                } else if pid < 0 {
                    println!("grind: fork failed");
                    syscall::exit(1);
                }
                syscall::wait(core::ptr::null_mut());
            }
            15 => {
                syscall::sbrk(6011);
            }
            16 => {
                let cur = syscall::sbrk(0) as usize;
                if cur > break0 {
                    syscall::sbrk(-((cur - break0) as isize));
                }
            }
            17 => {
                let pid = syscall::fork();
                if pid == 0 {
                    let f = syscall::open("a", O_CREATE | O_RDWR) as i32;
                    syscall::close(f);
                    syscall::exit(0);
                } else if pid < 0 {
                    println!("grind: fork failed");
                    syscall::exit(1);
                }
                if syscall::chdir("../grindir/..") != 0 {
                    println!("grind: chdir failed");
                    syscall::exit(1);
                }
                syscall::kill(pid as i32);
                syscall::wait(core::ptr::null_mut());
            }
            18 => {
                let pid = syscall::fork();
                if pid == 0 {
                    syscall::kill(syscall::getpid() as i32);
                    syscall::exit(0);
                } else if pid < 0 {
                    println!("grind: fork failed");
                    syscall::exit(1);
                }
                syscall::wait(core::ptr::null_mut());
            }
            19 => {
                let mut fds = [0i32; 2];
                if syscall::pipe(&mut fds) < 0 {
                    println!("grind: pipe failed");
                    syscall::exit(1);
                }
                let pid = syscall::fork();
                if pid == 0 {
                    syscall::fork();
                    syscall::fork();
                    if syscall::write(fds[1], b"x") != 1 {
                        println!("grind: pipe write failed");
                    }
                    let mut c = [0u8; 1];
                    if syscall::read(fds[0], &mut c) != 1 {
                        println!("grind: pipe read failed");
                    }
                    syscall::exit(0);
                } else if pid < 0 {
                    println!("grind: fork failed");
                    syscall::exit(1);
                }
                syscall::close(fds[0]);
                syscall::close(fds[1]);
                syscall::wait(core::ptr::null_mut());
            }
            20 => {
                let pid = syscall::fork();
                if pid == 0 {
                    syscall::unlink("a");
                    syscall::mkdir("a");
                    syscall::chdir("a");
                    syscall::unlink("../a");
                    // Open only for its side effect (create "x"); the child
                    // exit(0) below closes the fd, matching C xv6 grind.
                    let _ = syscall::open("x", O_CREATE | O_RDWR);
                    syscall::unlink("x");
                    syscall::exit(0);
                } else if pid < 0 {
                    println!("grind: fork failed");
                    syscall::exit(1);
                }
                syscall::wait(core::ptr::null_mut());
            }
            21 => {
                syscall::unlink("c");
                // Should always succeed: verifies free inodes, fds, and blocks.
                let fd1 = syscall::open("c", O_CREATE | O_RDWR) as i32;
                if fd1 < 0 {
                    println!("grind: create c failed");
                    syscall::exit(1);
                }
                if syscall::write(fd1, b"x") != 1 {
                    println!("grind: write c failed");
                    syscall::exit(1);
                }
                let mut st = Stat::default();
                if syscall::fstat(fd1, &mut st) != 0 {
                    println!("grind: fstat failed");
                    syscall::exit(1);
                }
                if st.size != 1 {
                    println!("grind: fstat reports wrong size {}", st.size);
                    syscall::exit(1);
                }
                if st.ino > 200 {
                    println!("grind: fstat reports crazy i-number {}", st.ino);
                    syscall::exit(1);
                }
                syscall::close(fd1);
                syscall::unlink("c");
            }
            22 => {
                // echo hi | cat
                let mut aa = [0i32; 2];
                let mut bb = [0i32; 2];
                if syscall::pipe(&mut aa) < 0 {
                    println!("grind: pipe failed");
                    syscall::exit(1);
                }
                if syscall::pipe(&mut bb) < 0 {
                    println!("grind: pipe failed");
                    syscall::exit(1);
                }
                let pid1 = syscall::fork();
                if pid1 == 0 {
                    syscall::close(bb[0]);
                    syscall::close(bb[1]);
                    syscall::close(aa[0]);
                    syscall::close(1);
                    if syscall::dup(aa[1]) != 1 {
                        println!("grind: dup failed");
                        syscall::exit(1);
                    }
                    syscall::close(aa[1]);
                    let args = [b"echo\0".as_ptr(), b"hi\0".as_ptr(), core::ptr::null()];
                    syscall::exec("grindir/../echo", &args);
                    println!("grind: echo: not found");
                    syscall::exit(2);
                } else if pid1 < 0 {
                    println!("grind: fork failed");
                    syscall::exit(3);
                }
                let pid2 = syscall::fork();
                if pid2 == 0 {
                    syscall::close(aa[1]);
                    syscall::close(bb[0]);
                    syscall::close(0);
                    if syscall::dup(aa[0]) != 0 {
                        println!("grind: dup failed");
                        syscall::exit(4);
                    }
                    syscall::close(aa[0]);
                    syscall::close(1);
                    if syscall::dup(bb[1]) != 1 {
                        println!("grind: dup failed");
                        syscall::exit(5);
                    }
                    syscall::close(bb[1]);
                    let args = [b"cat\0".as_ptr(), core::ptr::null()];
                    syscall::exec("/cat", &args);
                    println!("grind: cat: not found");
                    syscall::exit(6);
                } else if pid2 < 0 {
                    println!("grind: fork failed");
                    syscall::exit(7);
                }
                syscall::close(aa[0]);
                syscall::close(aa[1]);
                syscall::close(bb[1]);
                let mut buf = [0u8; 4];
                syscall::read(bb[0], &mut buf[0..1]);
                syscall::read(bb[0], &mut buf[1..2]);
                syscall::read(bb[0], &mut buf[2..3]);
                syscall::close(bb[0]);
                let mut st1 = 0i32;
                let mut st2 = 0i32;
                syscall::wait(&mut st1);
                syscall::wait(&mut st2);
                if st1 != 0 || st2 != 0 || &buf[0..3] != b"hi\n" {
                    println!(
                        "grind: exec pipeline failed {} {} {:?}",
                        st1,
                        st2,
                        &buf[0..3]
                    );
                    syscall::exit(1);
                }
            }
            _ => {}
        }
    }
}

fn iter() -> ! {
    syscall::unlink("a");
    syscall::unlink("b");

    let pid1 = syscall::fork();
    if pid1 < 0 {
        println!("grind: fork failed");
        syscall::exit(1);
    }
    if pid1 == 0 {
        // SAFETY: single-threaded user process.
        unsafe { RAND_NEXT ^= 31 };
        go(0);
    }

    let pid2 = syscall::fork();
    if pid2 < 0 {
        println!("grind: fork failed");
        syscall::exit(1);
    }
    if pid2 == 0 {
        // SAFETY: single-threaded user process.
        unsafe { RAND_NEXT ^= 7177 };
        go(1);
    }

    let mut st1 = -1i32;
    syscall::wait(&mut st1);
    if st1 != 0 {
        syscall::kill(pid1 as i32);
        syscall::kill(pid2 as i32);
    }
    let mut st2 = -1i32;
    syscall::wait(&mut st2);

    syscall::exit(0);
}

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    loop {
        let pid = syscall::fork();
        if pid == 0 {
            iter();
        }
        if pid > 0 {
            syscall::wait(core::ptr::null_mut());
        }
        syscall::sleep(20);
        // SAFETY: single-threaded user process.
        unsafe { RAND_NEXT += 1 };
    }
}
