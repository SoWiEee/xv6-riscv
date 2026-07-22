// kernel/src/mm/mod.rs
pub mod frame_allocator;
pub mod paging;
pub mod page_fault;

// Re-export for convenience
pub use frame_allocator::{alloc_page, free_frame};
pub use paging::{kernel_pagetable, set_kernel_pagetable};
pub use crate::arch::paging::{PhysPageNum, VirtAddr, PhysAddr, PAGE_SIZE};

pub fn kinit() {
    frame_allocator::kinit();
}