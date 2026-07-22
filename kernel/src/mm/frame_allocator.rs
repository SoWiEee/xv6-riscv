// kernel/src/mm/frame_allocator.rs
//! Physical page frame allocator.
//!
//! Manages the pool of free physical page frames using a simple stack-based
//! free list. All allocations are 4KB page-aligned.

use super::address::{PhysPageNum, PhysAddr};
use crate::sync::spinlock::SpinLock;
use buddy_system_allocator::LockedHeap;

/// Kernel heap size (16MB) for global allocator.
const KERNEL_HEAP_SIZE: usize = 16 * 1024 * 1024;
static HEAP: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::empty();

/// Maximum number of physical pages (128MB / 4KB = 32768).
const MAX_PHYS_PAGES: usize = 32768;
static mut FREE_LIST_STORAGE: [PhysPageNum; MAX_PHYS_PAGES] = [PhysPageNum::new(0); MAX_PHYS_PAGES];

/// Global frame allocator protected by a spinlock.
pub static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::new(), "frame_allocator");

/// Physical page frame allocator.
/// 
/// Uses a simple stack-based free list for O(1) allocation and deallocation.
/// Protected by `FRAME_ALLOCATOR` spinlock for thread safety.
pub struct FrameAllocator {
    start_ppn: PhysPageNum,
    end_ppn: PhysPageNum,
    free_list: &'static mut [PhysPageNum],
    free_count: usize,
}

impl FrameAllocator {
    /// Create a new uninitialized frame allocator.
    pub const fn new() -> Self {
        Self { start_ppn: PhysPageNum::new(0), end_ppn: PhysPageNum::new(0), free_list: &mut [], free_count: 0 }
    }
    
    /// Initialize the allocator with a physical memory range.
    /// 
    /// # Arguments
    /// * `start` - Starting physical address (inclusive)
    /// * `end` - Ending physical address (exclusive)
    /// 
    /// The range is page-aligned: start is rounded up, end is rounded down.
    pub fn init(&mut self, start: PhysAddr, end: PhysAddr) {
        self.start_ppn = start.ceil();
        self.end_ppn = end.floor();
        let total = self.end_ppn.0 - self.start_ppn.0;
        
        // Use static array for free list
        let free_list = unsafe { &mut FREE_LIST_STORAGE[..total] };
        self.free_list = free_list;
        self.free_count = 0;
        for i in self.start_ppn.0..self.end_ppn.0 {
            self.free_list[self.free_count] = PhysPageNum::new(i);
            self.free_count += 1;
        }
        // Initialize global allocator
        unsafe {
            ALLOCATOR.lock().init(HEAP.as_ptr() as usize, KERNEL_HEAP_SIZE);
        }
    }
    
    /// Allocate a single physical page frame.
    /// 
    /// Returns the physical page number on success, or an error if out of memory.
    pub fn alloc(&mut self) -> Result<PhysPageNum, &'static str> {
        if self.free_count == 0 {
            return Err("out of memory");
        }
        self.free_count -= 1;
        Ok(self.free_list[self.free_count])
    }
    
    /// Deallocate a physical page frame.
    /// 
    /// # Panics
    /// Panics if the free list overflows (double-free detection).
    pub fn dealloc(&mut self, ppn: PhysPageNum) {
        if self.free_count >= self.free_list.len() {
            panic!("frame allocator free list overflow");
        }
        self.free_list[self.free_count] = ppn;
        self.free_count += 1;
    }
}

/// Initialize the frame allocator with the kernel's physical memory range.
/// Called once during kernel initialization.
/// 
/// # Arguments
/// * `start` - Start of free physical memory (after kernel image)
/// * `end` - End of physical memory (PHYSTOP)
pub fn kinit(start: PhysAddr, end: PhysAddr) {
    FRAME_ALLOCATOR.acquire().init(start, end);
}

/// Allocate a physical page frame.
/// 
/// Returns `Some(PhysPageNum)` on success, `None` if out of memory.
pub fn kalloc() -> Option<PhysPageNum> {
    FRAME_ALLOCATOR.acquire().alloc().ok()
}

/// Free a physical page frame.
/// 
/// # Safety
/// Caller must ensure `ppn` was previously allocated and is not in use.
pub fn kfree(ppn: PhysPageNum) {
    FRAME_ALLOCATOR.acquire().dealloc(ppn);
}

/// Allocate a page frame, returning a Result.
pub fn alloc_page() -> Result<PhysPageNum, &'static str> {
    kalloc().ok_or("out of memory")
}

/// Free a page frame (alias for `kfree`).
pub fn free_page(ppn: PhysPageNum) {
    kfree(ppn);
}