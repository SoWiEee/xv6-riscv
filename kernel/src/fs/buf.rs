// kernel/src/fs/buf.rs
//! Buffer cache for disk blocks.
//!
//! Implements a buffer cache with LRU eviction. Each buffer holds one
//! 1024-byte disk block (two 512-byte virtio sectors). The cache uses
//! reference counting for pinning and a condition variable for waiting
//! when all buffers are busy.

use crate::drivers::virtio::virtio_rw;
use crate::sync::spinlock::SpinLock;
use crate::sync::sleeplock::SleepLock;
use crate::sync::condvar::Condvar;
use crate::proc::{sleep, wakeup, started};

/// Block size in bytes (1024 = 2 virtio sectors).
pub const BSIZE: usize = 1024;

/// Buffer metadata protected by sleep lock.
struct BufData {
    blockno: u32,
    dev: u32,
    refcnt: usize,
    valid: bool,
    data: [u8; BSIZE],
}

/// A cached disk block.
/// 
/// Protected by a sleep lock since I/O operations may block.
pub struct Buf {
    lock: SleepLock<BufData>,
}

impl Buf {
    /// Create a new uninitialized buffer.
    pub fn new(blockno: u32, dev: u32) -> Self {
        Self {
            lock: SleepLock::new(BufData {
                blockno,
                dev,
                refcnt: 0,
                valid: false,
                data: [0; BSIZE],
            }, "buf"),
        }
    }
    
    /// Acquire the buffer lock for I/O.
    pub fn lock(&self) -> BufGuard<'_> { 
        BufGuard { guard: self.lock.acquire() }
    }
    
    /// Acquire the buffer lock (alias for `lock`).
    pub fn lock_with_data(&self) -> BufGuard<'_> {
        BufGuard { guard: self.lock.acquire() }
    }
}

/// RAII guard for a locked buffer.
/// 
/// Provides access to buffer data and metadata.
pub struct BufGuard<'a> {
    guard: crate::sync::sleeplock::SleepLockGuard<'a, BufData>,
}

impl<'a> BufGuard<'a> {
    /// Get immutable reference to buffer data.
    pub fn data(&self) -> &[u8] { &self.guard.data }
    
    /// Get mutable reference to buffer data.
    pub fn data_mut(&mut self) -> &mut [u8] { &mut self.guard.data }
    
    /// Get the block number.
    pub fn blockno(&self) -> u32 { self.guard.blockno }
    
    /// Get the device number.
    pub fn dev(&self) -> u32 { self.guard.dev }
    
    /// Check if buffer contains valid data.
    pub fn valid(&self) -> bool { self.guard.valid }
    
    /// Set validity flag.
    pub fn set_valid(&mut self, v: bool) { self.guard.valid = v; }
    
    /// Get reference count.
    pub fn refcnt(&self) -> usize { self.guard.refcnt }
    
    /// Increment reference count (pin).
    pub fn inc_ref(&mut self) { self.guard.refcnt += 1; }
    
    /// Decrement reference count (unpin).
    pub fn dec_ref(&mut self) { self.guard.refcnt -= 1; }
}

/// Reference to a buffer in the cache.
/// 
/// Holds an index into the global buffer cache. The actual buffer
/// is looked up when `lock()` is called.
#[derive(Clone, Copy)]
pub struct BufRef {
    index: usize,
}

impl BufRef {
    /// Create a new buffer reference.
    pub fn new(index: usize) -> Self { Self { index } }
    
    /// Get the cache index.
    pub fn index(&self) -> usize { self.index }
    
    /// Lock the referenced buffer.
    /// 
    /// Returns a guard for accessing the buffer data.
    pub fn lock(&self) -> BufGuard<'_> {
        // SAFETY: BUF_CACHE is a static, so the buffer lives for the entire program.
        // We briefly acquire the cache lock to get a pointer to the buffer,
        // then release the cache lock. The returned BufGuard borrows from the Buf
        // which is stored in the static BUF_CACHE and thus has 'static lifetime.
        let cache = BUF_CACHE.acquire();
        let buf_ptr: *const Buf = cache.buffers[self.index].as_ref().unwrap() as *const Buf;
        drop(cache);
        unsafe { &*buf_ptr }.lock_with_data()
    }
}

/// Number of buffers in the cache.
const NBUF: usize = 30; // MAXOPBLOCKS * 3 = 10 * 3 = 30

/// Global buffer cache.
pub static BUF_CACHE: SpinLock<BufCache> = SpinLock::new(BufCache::new(), "bcache");

pub fn bcache_addr() -> usize {
    &raw const BUF_CACHE as usize
}

/// Buffer cache with LRU replacement policy.
struct BufCache {
    buffers: [Option<Buf>; NBUF],
    head: Option<usize>, // LRU list - index of most recently used
    tail: Option<usize>, // LRU list - index of least recently used
    prev: [Option<usize>; NBUF], // Previous buffer in LRU
    next: [Option<usize>; NBUF], // Next buffer in LRU
    wait_cond: Condvar,
}

impl BufCache {
    const fn new() -> Self { 
        Self { 
            buffers: [const { None }; NBUF], 
            head: None,
            tail: None,
            prev: [const { None }; NBUF],
            next: [const { None }; NBUF],
            wait_cond: Condvar::new("bcache"),
        } 
    }
    
    fn remove_from_lru(&mut self, idx: usize) {
        let p = self.prev[idx];
        let n = self.next[idx];
        
        if let Some(p) = p {
            self.next[p] = n;
        } else {
            self.head = n;
        }
        
        if let Some(n) = n {
            self.prev[n] = p;
        } else {
            self.tail = p;
        }
        
        self.prev[idx] = None;
        self.next[idx] = None;
    }
    
    fn add_to_head(&mut self, idx: usize) {
        self.next[idx] = self.head;
        self.prev[idx] = None;
        
        if let Some(h) = self.head {
            self.prev[h] = Some(idx);
        }
        
        self.head = Some(idx);
        
        if self.tail.is_none() {
            self.tail = Some(idx);
        }
    }
    
    fn move_to_head(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return; // Already at head
        }
        self.remove_from_lru(idx);
        self.add_to_head(idx);
    }
    
    fn get_lru(&self) -> Option<usize> {
        self.tail
    }
}

/// Initialize the buffer cache.
pub fn binit() {
    let sp: usize;
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
    crate::arch::console::printk(format_args!("binit: start sp={:#x}\n", sp));
    let mut cache = BUF_CACHE.acquire();
    for i in 0..NBUF {
        crate::arch::console::printk(format_args!("binit: i={}, before ptr={:#x}\n", i, unsafe { crate::mm::page_table::KERNEL_PAGETABLE_PTR } as usize));
        cache.buffers[i] = Some(Buf::new(0, 0));
        crate::arch::console::printk(format_args!("binit: i={}, after ptr={:#x}\n", i, unsafe { crate::mm::page_table::KERNEL_PAGETABLE_PTR } as usize));
    }
    // Initialize LRU with all buffers
    for i in 0..NBUF {
        cache.add_to_head(i);
    }
    crate::arch::console::printk(format_args!("binit: done, head={:?}, tail={:?}\n", cache.head, cache.tail));
}

/// Get a buffer for a disk block, allocating or evicting as needed.
/// 
/// Returns a `BufRef` that can be locked to access the data.
fn bget(dev: u32, blockno: u32) -> BufRef {
    crate::arch::console::printk(format_args!("bget: ENTER dev={} blockno={} started={}\n", dev, blockno, crate::proc::started()));
    crate::arch::console::printk(format_args!("bget: before acquire\n"));
    let mut cache = BUF_CACHE.acquire();
    crate::arch::console::printk(format_args!("bget: after acquire\n"));
    
    loop {
        // Search for existing buffer
        let mut found_idx = None;
        for i in 0..NBUF {
            if let Some(buf) = &cache.buffers[i] {
                let mut guard = buf.lock();
                if guard.dev() == dev && guard.blockno() == blockno {
                    guard.inc_ref();
                    found_idx = Some(i);
                    break;
                }
            }
        }
        
        if let Some(i) = found_idx {
            cache.move_to_head(i);
            return BufRef::new(i);
        }
        
        // Not found - find a free buffer (refcnt == 0)
        let lru_idx = cache.get_lru();
        if let Some(lru_idx) = lru_idx {
            if let Some(buf) = &cache.buffers[lru_idx] {
                let mut guard = buf.lock();
                crate::arch::console::printk(format_args!("bget: lru_idx={} refcnt={} valid={}\n", lru_idx, guard.refcnt(), guard.valid()));
                if guard.refcnt() == 0 {
                    // Reuse this buffer
                    let idx = lru_idx;
                    guard.guard.blockno = blockno;
                    guard.guard.dev = dev;
                    guard.guard.valid = false;
                    guard.guard.refcnt = 1;
                    drop(guard);
                    cache.move_to_head(idx);
                    return BufRef::new(idx);
                }
            }
        }
        
        // All buffers busy - sleep and retry (but not before scheduler starts)
        crate::arch::console::printk(format_args!("bget: all busy, started={}\n", crate::proc::started()));
        if crate::proc::started() {
            cache.wait_cond.sleep(&BUF_CACHE);
        } else {
            core::hint::spin_loop();
        }
    }
}

/// Read a disk block into the cache.
/// 
/// Returns a `BufRef` to the locked buffer containing the data.
/// If the block is not in cache, it is read from disk.
pub fn bread(dev: u32, blockno: u32) -> BufRef {
    let buf_ref = bget(dev, blockno);
    
    // Check if valid while holding cache lock
    let mut needs_read = false;
    {
        let mut cache = BUF_CACHE.acquire();
        let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
        let mut guard = buf.lock();
        if !guard.valid() {
            needs_read = true;
        }
    }
    
    if needs_read {
        // Need to read from disk - acquire lock again for I/O
        let mut cache = BUF_CACHE.acquire();
        let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
        let mut guard = buf.lock();
        read_block(dev, blockno, guard.data_mut());
        guard.set_valid(true);
    }
    buf_ref
}

/// Release a buffer reference.
/// 
/// Decrements the reference count. If it reaches zero, the buffer
/// is moved to the head of the LRU list and waiters are woken.
pub fn brelse(buf_ref: BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    guard.dec_ref();
    let refcnt = guard.refcnt();
    drop(guard);
    if refcnt == 0 {
        // Move to head of LRU (most recently used)
        cache.move_to_head(buf_ref.index());
        cache.wait_cond.wakeup();
    }
}

/// Write a buffer to disk.
/// 
/// The buffer must be valid. Data is written to the virtio device.
pub fn bwrite(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    if guard.valid() {
        let dev = guard.dev();
        let blockno = guard.blockno();
        let data = guard.data().to_vec();
        drop(guard);
        drop(cache);
        write_block(dev, blockno, &data);
    }
}

/// Pin a buffer (increment refcount without using it).
/// 
/// Used by the log to keep blocks in cache during transactions.
pub fn bpin(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    guard.inc_ref();
}

/// Unpin a buffer (decrement refcount).
/// 
/// If refcount reaches zero, buffer becomes eligible for eviction.
pub fn bunpin(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    guard.dec_ref();
    let refcnt = guard.refcnt();
    drop(guard);
    if refcnt == 0 {
        cache.move_to_head(buf_ref.index());
        cache.wait_cond.wakeup();
    }
}

/// Read a 1024-byte block from disk (2x 512-byte sectors).
fn read_block(dev: u32, blockno: u32, dst: &mut [u8]) {
    // Virtio uses 512-byte sectors, we need 2 sectors for 1024-byte block
    for i in 0..2usize {
        let sector = (blockno as u64) * 2 + i as u64;
        let mut block = crate::drivers::virtio::Block {
            blockno: sector,
            data: [0u8; 512],
        };
        virtio_rw(&mut block, false); // false = read
        let start = i * 512;
        let end = (i + 1) * 512;
        dst[start..end].copy_from_slice(&block.data);
    }
}

/// Write a 1024-byte block to disk (2x 512-byte sectors).
fn write_block(dev: u32, blockno: u32, src: &[u8]) {
    for i in 0..2usize {
        let sector = (blockno as u64) * 2 + i as u64;
        let mut block = crate::drivers::virtio::Block {
            blockno: sector,
            data: [0u8; 512],
        };
        let start = i * 512;
        let end = (i + 1) * 512;
        block.data.copy_from_slice(&src[start..end]);
        virtio_rw(&mut block, true); // true = write
    }
}