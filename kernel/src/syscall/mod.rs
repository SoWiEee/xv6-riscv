// kernel/src/syscall/mod.rs
//! System call dispatch and implementation.
//!
//! Provides the syscall entry point called from trap handler.
//! Individual syscall implementations are in `crate::proc::syscall`.

use crate::proc::syscall::proc_syscall;

/// Main system call dispatcher.
/// 
/// Called from `usertrap` after saving user registers. Reads syscall number
/// and arguments from the current process's trap frame, executes the syscall,
/// and stores the return value in a0.
pub fn syscall() {
    proc_syscall();
}

// Re-export syscall numbers for external use
pub use crate::proc::syscall::{
    SYS_FORK, SYS_EXIT, SYS_WAIT, SYS_PIPE, SYS_READ, SYS_WRITE,
    SYS_CLOSE, SYS_KILL, SYS_EXEC, SYS_FSTAT, SYS_CHDIR, SYS_DUP,
    SYS_GETPID, SYS_SBRK, SYS_SLEEP, SYS_UPTIME, SYS_OPEN,
    SYS_MKNOD, SYS_UNLINK, SYS_LINK, SYS_MKDIR,
};