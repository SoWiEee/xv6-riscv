// user-lib/src/lib.rs
//! User-space library for xv6-riscv Rust programs.
//!
//! Provides syscall interface, standard I/O, string operations,
//! file system access, and process management for user programs.

#![no_std]
extern crate alloc;

pub mod syscall;
pub mod stdio;
pub mod string;
pub mod fs;
pub mod process;

// Re-export macros (they're already exported via #[macro_export])
// pub use stdio::{print, println};

/// Syscall numbers matching C xv6.
/// 
/// These must match the kernel's syscall numbers exactly for binary compatibility.
pub const SYS_FORK: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_WAIT: usize = 3;
pub const SYS_PIPE: usize = 4;
pub const SYS_READ: usize = 5;
pub const SYS_WRITE: usize = 6;
pub const SYS_CLOSE: usize = 7;
pub const SYS_KILL: usize = 8;
pub const SYS_EXEC: usize = 9;
pub const SYS_FSTAT: usize = 10;
pub const SYS_CHDIR: usize = 11;
pub const SYS_DUP: usize = 12;
pub const SYS_GETPID: usize = 13;
pub const SYS_SBRK: usize = 14;
pub const SYS_SLEEP: usize = 15;
pub const SYS_UPTIME: usize = 16;
pub const SYS_OPEN: usize = 17;
pub const SYS_MKNOD: usize = 18;
pub const SYS_UNLINK: usize = 19;
pub const SYS_LINK: usize = 20;
pub const SYS_MKDIR: usize = 21;

/// Syscall macro for making system calls.
/// 
/// Generates inline assembly `ecall` instructions with the appropriate
/// register setup. Supports 0-6 arguments (a0-a5) with syscall number in a7.
/// 
/// # Example
/// ```
/// let pid = syscall!(SYS_GETPID);
/// let fd = syscall!(SYS_OPEN, path_ptr, flags);
/// ```
#[macro_export]
macro_rules! syscall {
    ($num:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr, $a1:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                in("a1") $a1,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr, $a1:expr, $a2:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                in("a1") $a1,
                in("a2") $a2,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                in("a1") $a1,
                in("a2") $a2,
                in("a3") $a3,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                in("a1") $a1,
                in("a2") $a2,
                in("a3") $a3,
                in("a4") $a4,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                in("a1") $a1,
                in("a2") $a2,
                in("a3") $a3,
                in("a4") $a4,
                in("a5") $a5,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
}

// Panic handler
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Global allocator
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the user heap.
/// 
/// Allocates 1MB of heap space via `sbrk` syscall and initializes
/// the global allocator. Must be called before any allocation.
pub fn init_heap() {
    use crate::syscall::sbrk;
    const HEAP_SIZE: usize = 1024 * 1024; // 1MB
    let heap_start = sbrk(HEAP_SIZE as isize);
    unsafe {
        ALLOCATOR.lock().init(heap_start, HEAP_SIZE);
    }
}