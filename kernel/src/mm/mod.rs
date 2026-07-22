// kernel/src/mm/mod.rs
pub mod address;
pub mod frame_allocator;
pub mod page_table;
pub mod heap;
pub mod page_fault;
pub mod paging;

// Re-export for convenience
pub use address::{PhysAddr, VirtAddr, PhysPageNum, VirtPageNum};
pub use frame_allocator::{alloc_page, free_page, kinit as frame_kinit};
pub use page_table::{PageTable, kernel_pagetable, kvminit, kvminithart, uvmcreate, uvmalloc, uvmfree, uvmcopy};
pub use crate::arch::paging::{PAGE_SIZE};

pub fn kinit() {
    // Get physical memory range from linker script
    unsafe extern "C" {
        fn end();
    }
    let start = end as *const () as usize;
    let end_addr = crate::arch::asm::PHYSTOP;
    frame_allocator::kinit(
        crate::mm::address::PhysAddr::new(start),
        crate::mm::address::PhysAddr::new(end_addr),
    );
}