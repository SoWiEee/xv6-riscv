// kernel/src/mm/heap.rs
// Already initialized in frame_allocator.rs via LockedHeap
// This file just re-exports and documents

pub use buddy_system_allocator::LockedHeap;

/// Kernel heap allocator.
/// Initialized in kinit() with 16MB from .bss section.
/// Used for: PageTable, Inode, File, Proc allocations, etc.
pub fn init_heap() {
    // Done in frame_allocator::kinit()
}