// kernel/src/mm/frame_allocator.rs
use super::address::{PhysPageNum, PhysAddr};
use crate::sync::spinlock::SpinLock;
use buddy_system_allocator::LockedHeap;

const KERNEL_HEAP_SIZE: usize = 16 * 1024 * 1024; // 16MB
static HEAP: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::empty();

// Maximum number of physical pages (128MB / 4KB = 32768)
const MAX_PHYS_PAGES: usize = 32768;
static mut FREE_LIST_STORAGE: [PhysPageNum; MAX_PHYS_PAGES] = [PhysPageNum::new(0); MAX_PHYS_PAGES];

pub static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::new(), "frame_allocator");

pub struct FrameAllocator {
    start_ppn: PhysPageNum,
    end_ppn: PhysPageNum,
    free_list: &'static mut [PhysPageNum],
    free_count: usize,
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self { start_ppn: PhysPageNum::new(0), end_ppn: PhysPageNum::new(0), free_list: &mut [], free_count: 0 }
    }
    
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
    
    pub fn alloc(&mut self) -> Result<PhysPageNum, &'static str> {
        if self.free_count == 0 {
            return Err("out of memory");
        }
        self.free_count -= 1;
        Ok(self.free_list[self.free_count])
    }
    
    pub fn dealloc(&mut self, ppn: PhysPageNum) {
        if self.free_count >= self.free_list.len() {
            panic!("frame allocator free list overflow");
        }
        self.free_list[self.free_count] = ppn;
        self.free_count += 1;
    }
}

pub fn kinit(start: PhysAddr, end: PhysAddr) {
    FRAME_ALLOCATOR.acquire().init(start, end);
}

pub fn kalloc() -> Option<PhysPageNum> {
    FRAME_ALLOCATOR.acquire().alloc().ok()
}

pub fn kfree(ppn: PhysPageNum) {
    FRAME_ALLOCATOR.acquire().dealloc(ppn);
}

pub fn alloc_page() -> Result<PhysPageNum, &'static str> {
    kalloc().ok_or("out of memory")
}

pub fn free_page(ppn: PhysPageNum) {
    kfree(ppn);
}