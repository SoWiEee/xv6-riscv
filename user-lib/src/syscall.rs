// user-lib/src/syscall.rs
use crate::*;

pub fn fork() -> isize {
    syscall!(SYS_FORK) as isize
}

pub fn exit(code: i32) -> ! {
    syscall!(SYS_EXIT, code as usize);
    loop {}
}

pub fn wait() -> isize {
    syscall!(SYS_WAIT) as isize
}

pub fn pipe(fd: &mut [usize; 2]) -> isize {
    syscall!(SYS_PIPE, fd.as_mut_ptr() as usize) as isize
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    syscall!(SYS_READ, fd, buf.as_mut_ptr() as usize, buf.len()) as isize
}

pub fn write(fd: usize, buf: &[u8]) -> isize {
    syscall!(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) as isize
}

pub fn close(fd: usize) -> isize {
    syscall!(SYS_CLOSE, fd) as isize
}

pub fn kill(pid: usize) -> isize {
    syscall!(SYS_KILL, pid) as isize
}

pub fn exec(path: &str, argv: &[&str]) -> isize {
    // Simplified - real implementation needs string conversion
    syscall!(SYS_EXEC, path.as_ptr() as usize, argv.as_ptr() as usize) as isize
}

pub fn fstat(fd: usize, stat: &mut Stat) -> isize {
    syscall!(SYS_FSTAT, fd, stat as *mut _ as usize) as isize
}

pub fn chdir(path: &str) -> isize {
    syscall!(SYS_CHDIR, path.as_ptr() as usize) as isize
}

pub fn dup(fd: usize) -> isize {
    syscall!(SYS_DUP, fd) as isize
}

pub fn getpid() -> isize {
    syscall!(SYS_GETPID) as isize
}

pub fn sbrk(incr: isize) -> *mut u8 {
    syscall!(SYS_SBRK, incr as usize) as *mut u8
}

pub fn sleep(ticks: usize) -> isize {
    syscall!(SYS_SLEEP, ticks) as isize
}

pub fn uptime() -> usize {
    syscall!(SYS_UPTIME)
}

pub fn open(path: &str, flags: usize) -> isize {
    syscall!(SYS_OPEN, path.as_ptr() as usize, flags) as isize
}

pub fn mknod(path: &str, major: usize, minor: usize) -> isize {
    syscall!(SYS_MKNOD, path.as_ptr() as usize, major, minor) as isize
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