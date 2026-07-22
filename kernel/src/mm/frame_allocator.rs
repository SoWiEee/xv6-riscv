// kernel/src/mm/frame_allocator.rs
use crate::arch::registers::{PGSIZE, PGROUNDUP, PGROUNDDOWN};
use core::ptr::NonNull;

const KERNBASE: usize = 0x80000000;
const PHYSTOP: usize = KERNBASE + 128 * 1024 * 1024;
const END: usize = KERNBASE + 1024 * 1024; // Approximate end of kernel

static mut INITIALIZED: bool = false;
static mut FREE_LIST: *mut usize = core::ptr::null_mut();

pub fn kinit() {
    unsafe {
        let mut p = PGROUNDUP(END);
        let end = PGROUNDDOWN(PHYSTOP);
        while p + PGSIZE <= end {
            free_frame(p);
            p += PGSIZE;
        }
        INITIALIZED = true;
    }
}

pub fn alloc_page() -> Result<usize, &'static str> {
    unsafe {
        if !INITIALIZED {
            return Err("frame allocator not initialized");
        }
        let frame = FREE_LIST;
        if frame.is_null() {
            return Err("out of memory");
        }
        FREE_LIST = *frame as *mut usize;
        Ok(frame as usize)
    }
}

pub fn free_frame(pa: usize) {
    unsafe {
        let frame = pa as *mut usize;
        *frame = FREE_LIST as usize;
        FREE_LIST = frame;
    }
}

pub fn kernel_pagetable() -> usize {
    crate::mm::paging::kernel_pagetable()
}