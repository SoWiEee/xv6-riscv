// user-lib/src/process.rs

pub fn fork() -> isize {
    crate::syscall::fork()
}

pub fn exit(code: i32) -> ! {
    crate::syscall::exit(code)
}

pub fn wait() -> isize {
    let mut status: i32 = 0;
    crate::syscall::wait(&mut status)
}

pub fn waitpid(_pid: isize) -> isize {
    let mut status: i32 = 0;
    crate::syscall::wait(&mut status)
}

pub fn exec(path: &str, args: &[&str]) -> isize {
    // Convert args to C-style argv
    let mut argv: alloc::vec::Vec<*const u8> = args.iter()
        .map(|s| s.as_ptr())
        .collect();
    argv.push(core::ptr::null());
    crate::syscall::exec(path, &argv)
}

pub fn sleep(ticks: usize) -> isize {
    crate::syscall::sleep(ticks)
}

pub fn getpid() -> isize {
    crate::syscall::getpid()
}

pub fn kill(pid: i32) -> isize {
    crate::syscall::kill(pid)
}

pub fn sbrk(n: isize) -> *mut u8 {
    crate::syscall::sbrk(n)
}