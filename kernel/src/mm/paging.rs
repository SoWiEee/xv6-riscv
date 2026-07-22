// kernel/src/mm/paging.rs
use crate::arch::paging::{PhysPageNum};

pub fn kernel_pagetable() -> PhysPageNum {
    unsafe { KERNEL_PAGETABLE }
}

pub fn set_kernel_pagetable(ppn: PhysPageNum) {
    unsafe { KERNEL_PAGETABLE = ppn; }
}

static mut KERNEL_PAGETABLE: PhysPageNum = 0;