// kernel/src/proc/trapframe.rs
use crate::arch::trap::TrapFrame;

pub const TRAPFRAME_SIZE: usize = core::mem::size_of::<TrapFrame>();

pub fn alloc_trapframe(kstack: usize) -> *mut TrapFrame {
    // TrapFrame is placed at the top of the kernel stack
    let tf_ptr = (kstack + 4096 - TRAPFRAME_SIZE) as *mut TrapFrame;
    unsafe { core::ptr::write_bytes(tf_ptr, 0, 1) };
    tf_ptr
}

pub fn free_trapframe(_tf: *mut TrapFrame) {
    // TrapFrame is part of the kernel stack, so freeing the stack frees the trapframe
    // No action needed here
}