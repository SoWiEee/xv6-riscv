// user-lib/src/syscall.rs
use crate::*;

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
    syscall!(SYS_EXEC, path.as_ptr() as usize, argv.as_ptr() as usize) as isize
}

pub fn fstat(fd: i32, st: &mut Stat) -> isize {
    syscall!(SYS_FSTAT, fd as usize, st as *mut _ as usize) as isize
}

pub fn chdir(path: &str) -> isize {
    syscall!(SYS_CHDIR, path.as_ptr() as usize) as isize
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
    syscall!(SYS_OPEN, path.as_ptr() as usize, flags as usize) as isize
}

pub fn mknod(path: &str, major: i32, minor: i32) -> isize {
    syscall!(SYS_MKNOD, path.as_ptr() as usize, major as usize, minor as usize) as isize
}

pub fn unlink(path: &str) -> isize {
    syscall!(SYS_UNLINK, path.as_ptr() as usize) as isize
}

pub fn link(old: &str, new: &str) -> isize {
    syscall!(SYS_LINK, old.as_ptr() as usize, new.as_ptr() as usize) as isize
}

pub fn mkdir(path: &str) -> isize {
    syscall!(SYS_MKDIR, path.as_ptr() as usize) as isize
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