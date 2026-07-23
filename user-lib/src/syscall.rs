// user-lib/src/syscall.rs
use crate::*;

/// Maximum path length passed to the kernel (including the NUL terminator).
const PATH_MAX: usize = 128;

/// Copy `s` into a NUL-terminated stack buffer and invoke `f` with a pointer to
/// it. Rust `&str`s are NOT NUL-terminated, but every path-based syscall expects
/// a C string and the kernel reads until it sees a `\0`. Passing `s.as_ptr()`
/// directly makes the kernel run off the end of the string into whatever bytes
/// follow it in `.rodata` (e.g. reading "console" as "consolesh"). Terminating
/// here, before crossing into the kernel, is the single choke point that keeps
/// all callers correct.
fn with_cstr<R>(s: &str, f: impl FnOnce(*const u8) -> R) -> R {
    let mut buf = [0u8; PATH_MAX];
    let n = s.len().min(PATH_MAX - 1);
    buf[..n].copy_from_slice(&s.as_bytes()[..n]);
    // buf[n] is already 0 -> NUL terminator.
    f(buf.as_ptr())
}

pub fn fork() -> isize {
    syscall!(SYS_FORK) as isize
}

pub fn exit(code: i32) -> ! {
    syscall!(SYS_EXIT, code as usize);
    loop {}
}

pub fn wait(addr: *mut i32) -> isize {
    syscall!(SYS_WAIT, addr as usize) as isize
}

pub fn pipe(fds: &mut [i32; 2]) -> isize {
    syscall!(SYS_PIPE, fds.as_mut_ptr() as usize) as isize
}

pub fn read(fd: i32, buf: &mut [u8]) -> isize {
    syscall!(SYS_READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) as isize
}

pub fn write(fd: i32, buf: &[u8]) -> isize {
    syscall!(SYS_WRITE, fd as usize, buf.as_ptr() as usize, buf.len()) as isize
}

pub fn close(fd: i32) -> isize {
    syscall!(SYS_CLOSE, fd as usize) as isize
}

pub fn kill(pid: i32) -> isize {
    syscall!(SYS_KILL, pid as usize) as isize
}

pub fn exec(path: &str, argv: &[*const u8]) -> isize {
    with_cstr(path, |p| syscall!(SYS_EXEC, p as usize, argv.as_ptr() as usize) as isize)
}

pub fn fstat(fd: i32, st: &mut Stat) -> isize {
    syscall!(SYS_FSTAT, fd as usize, st as *mut _ as usize) as isize
}

pub fn chdir(path: &str) -> isize {
    with_cstr(path, |p| syscall!(SYS_CHDIR, p as usize) as isize)
}

pub fn dup(fd: i32) -> isize {
    syscall!(SYS_DUP, fd as usize) as isize
}

pub fn getpid() -> isize {
    syscall!(SYS_GETPID) as isize
}

pub fn sbrk(n: isize) -> *mut u8 {
    syscall!(SYS_SBRK, n as usize) as *mut u8
}

pub fn sleep(ticks: usize) -> isize {
    syscall!(SYS_SLEEP, ticks) as isize
}

pub fn uptime() -> isize {
    syscall!(SYS_UPTIME) as isize
}

pub fn getc() -> Option<u8> {
    let mut buf = [0u8; 1];
    if read(0, &mut buf) > 0 {
        Some(buf[0])
    } else {
        None
    }
}

pub fn putc(c: u8) {
    write(1, &[c]);
}

pub fn open(path: &str, flags: i32) -> isize {
    with_cstr(path, |p| syscall!(SYS_OPEN, p as usize, flags as usize) as isize)
}

pub fn mknod(path: &str, major: i32, minor: i32) -> isize {
    with_cstr(path, |p| syscall!(SYS_MKNOD, p as usize, major as usize, minor as usize) as isize)
}

pub fn unlink(path: &str) -> isize {
    with_cstr(path, |p| syscall!(SYS_UNLINK, p as usize) as isize)
}

pub fn link(old: &str, new: &str) -> isize {
    with_cstr(old, |o| with_cstr(new, |n| syscall!(SYS_LINK, o as usize, n as usize) as isize))
}

pub fn mkdir(path: &str) -> isize {
    with_cstr(path, |p| syscall!(SYS_MKDIR, p as usize) as isize)
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Stat {
    pub dev: usize,
    pub ino: usize,
    pub mode: usize,
    pub nlink: usize,
    pub uid: usize,
    pub gid: usize,
    pub rdev: usize,
    pub size: usize,
    pub atime: usize,
    pub mtime: usize,
    pub ctime: usize,
}

impl Stat {
    pub fn type_(&self) -> u16 {
        (self.mode >> 12) as u16
    }
}